//! Shared checks for the typed object views: open an object of a given class,
//! read its fields with checked slicing, follow its table entries and read
//! the array slots it owns. Every error carries the file name and the
//! absolute file offset of the field at fault.
use crate::{Container, ObjectSpan};
use fo4_core::{Budget, Error, Result};

/// Flag set in the u32 after the element count of every measured array slot.
pub const ARRAY_FLAG: u32 = 0x8000_0000;

/// One array slot of an object, as checked by [`View::array`]: the element
/// count stored at `field + 8`, and where its storage starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Array {
    /// Offset of the slot from the object's offset.
    pub field: u32,
    pub count: u32,
    /// Offset of the storage from the object's offset (0 when `count` is 0).
    pub storage: u32,
    /// Element size measured for this slot.
    pub element: u32,
}
impl Array {
    /// Offset of element `i` from the object's offset.
    pub fn element_at(&self, i: usize) -> Option<usize> {
        if i >= self.count as usize {
            return None;
        }
        (self.storage as usize).checked_add(i.checked_mul(self.element as usize)?)
    }
}

/// An owned copy of a text read from the file, charged to `b`: many
/// table entries may point at one text, so each copy counts (the lossy
/// conversion's replacement characters included).
pub(crate) fn owned_text(t: &[u8], file: &str, offset: u64, b: &Budget) -> Result<String> {
    b.charge(t.len() as u64, file, offset)?;
    let s = String::from_utf8_lossy(t).into_owned();
    if s.len() > t.len() {
        b.charge((s.len() - t.len()) as u64, file, offset)?;
    }
    Ok(s)
}

pub(crate) struct View<'a, 'c> {
    pub c: &'c Container<'a>,
    pub span: ObjectSpan<'a>,
    pub bytes: &'a [u8],
    file: &'c str,
    at0: u64,
    code: &'static str,
}

impl<'a, 'c> View<'a, 'c> {
    /// Object `index` of `c`, which must have one of `classes`.
    pub fn open(
        c: &'c Container<'a>,
        index: usize,
        file: &'c str,
        base: u64,
        classes: &[&str],
        code: &'static str,
    ) -> Result<Self> {
        let data_start = c.data().map_or(0, |d| d.start as u64);
        let span = c.object_span(index).ok_or_else(|| {
            Error::new(
                "InvalidHavokObjectSpan",
                file,
                base.saturating_add(data_start),
                format!("object index < {}", c.objects.len()),
                index,
            )
        })?;
        let at0 = base
            .saturating_add(data_start)
            .saturating_add(span.offset as u64);
        if !classes.contains(&span.class) {
            return Err(Error::new(
                "UnexpectedHavokClass",
                file,
                at0,
                classes.join(" or "),
                span.class,
            ));
        }
        Ok(Self {
            c,
            span,
            bytes: c.span_bytes(&span),
            file,
            at0,
            code,
        })
    }
    /// Absolute file offset of `field`.
    pub fn abs(&self, field: usize) -> u64 {
        self.at0.saturating_add(field as u64)
    }
    pub fn err(&self, field: usize, expected: impl ToString, found: impl ToString) -> Error {
        Error::new(
            self.code,
            self.file,
            self.at0.saturating_add(field as u64),
            expected,
            found,
        )
    }
    fn get(&self, field: usize, len: usize) -> Result<&'a [u8]> {
        field
            .checked_add(len)
            .and_then(|end| self.bytes.get(field..end))
            .ok_or_else(|| {
                self.err(
                    field,
                    format!(
                        "{len} bytes inside the {}-byte object span",
                        self.bytes.len()
                    ),
                    "end of span",
                )
            })
    }
    pub fn u8(&self, field: usize) -> Result<u8> {
        Ok(self.get(field, 1)?[0])
    }
    pub fn u16(&self, field: usize) -> Result<u16> {
        let b = self.get(field, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn u32(&self, field: usize) -> Result<u32> {
        let b = self.get(field, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// `N` consecutive f32, each required to be finite.
    pub fn finite<const N: usize>(&self, field: usize, what: &str) -> Result<[f32; N]> {
        let mut out = [0f32; N];
        for (k, v) in out.iter_mut().enumerate() {
            let at = field + 4 * k;
            *v = f32::from_bits(self.u32(at)?);
            if !v.is_finite() {
                return Err(self.err(at, format!("finite {what}"), *v));
            }
        }
        Ok(out)
    }
    /// The span must be exactly `len` bytes.
    pub fn span_len(&self, len: usize) -> Result<()> {
        if self.bytes.len() != len {
            return Err(self.err(0, format!("object span of {len} bytes"), self.bytes.len()));
        }
        Ok(())
    }
    /// Object index named by the second-table entry whose source is `field`.
    pub fn link(&self, field: usize) -> Option<usize> {
        let source = (self.span.offset as usize).checked_add(field)?;
        let l = self
            .c
            .object_pointers
            .iter()
            .find(|l| l.source as usize == source)?;
        self.c
            .objects
            .binary_search_by_key(&l.destination, |o| o.offset)
            .ok()
    }
    /// Like [`View::link`], but a missing entry is an error.
    pub fn required_link(&self, field: usize, what: &str) -> Result<usize> {
        self.link(field).ok_or_else(|| {
            self.err(
                field,
                format!("second-table entry for the {what} link"),
                "none",
            )
        })
    }
    /// Destination, relative to the object's offset, of the first-table
    /// entry whose source is `field`.
    pub fn pointer(&self, field: usize) -> Option<usize> {
        let source = (self.span.offset as usize).checked_add(field)?;
        let p = self
            .c
            .pointers
            .iter()
            .find(|p| p.source as usize == source)?;
        (p.destination as usize).checked_sub(self.span.offset as usize)
    }
    /// The NUL-terminated bytes that the first-table entry with source
    /// `field` points to; the NUL must lie inside the span.
    pub fn string(&self, field: usize, what: &str) -> Result<&'a [u8]> {
        let at = self.pointer(field).ok_or_else(|| {
            self.err(
                field,
                format!("first-table entry for the {what} text"),
                "none",
            )
        })?;
        let rest = self.bytes.get(at..).unwrap_or(&[]);
        let nul = rest.iter().position(|c| *c == 0).ok_or_else(|| {
            self.err(
                at,
                format!("NUL-terminated {what} inside the span"),
                "no NUL",
            )
        })?;
        Ok(&rest[..nul])
    }
    /// Checks the array slot at `field` with elements of `element` bytes:
    /// the u32 at +12 is [`ARRAY_FLAG`] OR the count at +8; a first-table
    /// entry has the slot as source exactly when the count is not zero; and
    /// the storage, `count * element` bytes from its destination, ends inside
    /// the span before any other first-table destination of this object.
    pub fn array(&self, field: usize, element: u32) -> Result<Array> {
        let count = self.u32(field + 8)?;
        let flag = self.u32(field + 12)?;
        if flag != ARRAY_FLAG | count {
            return Err(self.err(
                field + 12,
                format!("{:08x} (flag | count)", ARRAY_FLAG | count),
                format!("{flag:08x}"),
            ));
        }
        let lo = self.span.offset;
        let hi = self.span.end;
        let source = lo as usize + field;
        let dest = self
            .c
            .pointers
            .iter()
            .find(|p| p.source as usize == source)
            .map(|p| p.destination);
        let storage = match (count, dest) {
            (0, None) => {
                return Ok(Array {
                    field: field as u32,
                    count,
                    storage: 0,
                    element,
                })
            }
            (0, Some(_)) => {
                return Err(self.err(field, "no storage pointer for an empty array", "one"))
            }
            (_, None) => {
                return Err(self.err(
                    field,
                    format!("a first-table entry for {count} elements"),
                    "none",
                ))
            }
            (_, Some(d)) => d,
        };
        let end = (count as u64)
            .checked_mul(element as u64)
            .and_then(|n| n.checked_add(storage as u64));
        let fits = storage >= lo && end.is_some_and(|e| e <= hi as u64);
        let end = end.unwrap_or(u64::MAX);
        if !fits {
            return Err(self.err(
                field + 8,
                format!("{count} elements of {element} bytes inside the span [{lo}, {hi})"),
                format!("storage [{storage}, {end})"),
            ));
        }
        // A destination inside [storage, end) lies in this span, so the
        // container guarantees that this object owns it.
        if let Some(p) = self
            .c
            .pointers
            .iter()
            .find(|p| p.destination > storage && (p.destination as u64) < end)
        {
            return Err(self.err(
                field + 8,
                format!("{count} elements of {element} bytes before the next storage block"),
                format!(
                    "storage block at {} inside [{storage}, {end})",
                    p.destination
                ),
            ));
        }
        Ok(Array {
            field: field as u32,
            count,
            storage: storage - lo,
            element,
        })
    }
}

/// Euclidean norm in f64.
pub(crate) fn norm(v: &[f32]) -> f64 {
    v.iter()
        .map(|x| f64::from(*x) * f64::from(*x))
        .sum::<f64>()
        .sqrt()
}
