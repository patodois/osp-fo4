#![forbid(unsafe_code)]
//! Bounded reader for the interface movies (`spec/ui.md`).
//!
//! A movie is a Flash file (SWF; the format is public: Adobe's SWF File
//! Format Specification, version 19). Parsing is done by the `swf` crate
//! published by the Ruffle project (MIT OR Apache-2.0, `THIRD_PARTY.md`);
//! this crate adds the bounds, the tolerant tag walk and the census view:
//! header, frame size and rate, the tag census, exported symbols and class
//! links, shapes, texts and fonts, sprites, and the ActionScript blocks as
//! opaque byte counts with their kind. Nothing here runs a movie.
//!
//! Malformed input gives a named [`fo4_core::Error`] or a per-tag entry in
//! [`Movie::tag_errors`], never a panic: every call into the parser runs
//! behind `catch_unwind`, and every size read from the file is capped by
//! [`Limits`].
use fo4_core::{Budget, Error, Result};
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

pub mod abc;
pub mod raster;

/// Caps for one movie.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Stored (possibly compressed) bytes.
    pub max_stored: u64,
    /// Declared uncompressed length (header's own eight bytes included).
    pub max_declared: u64,
    /// Tags in the whole movie, sprites' tags included.
    pub max_tags: u64,
    /// Nesting of sprites (a well-formed movie has one level).
    pub max_depth: u32,
    /// Bytes of one text or name kept as a string.
    pub max_string: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_stored: 64 << 20,
            max_declared: 128 << 20,
            max_tags: 2_000_000,
            max_depth: 4,
            max_string: 16_384,
        }
    }
}

/// How the body after the eight-byte header is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Compression {
    /// `FWS` or `GFX`.
    None,
    /// `CWS` or `CFX`: zlib.
    Zlib,
    /// `ZWS`: refused (`Unsupported`), the reader is built without LZMA.
    Lzma,
}

/// Count and body bytes of one tag code.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagStat {
    pub count: u64,
    pub bytes: u64,
}

/// [`TagError::code`] when the stream ends inside a tag header.
pub const NO_CODE: u16 = u16::MAX;

/// A tag the parser refused; the walk goes on after its stored length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagError {
    pub code: u16,
    /// Offset of the tag's header in the decompressed body.
    pub offset: u64,
    pub sprite: Option<u16>,
    pub message: String,
}

/// One shape definition (DefineShape 1-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeStat {
    pub id: u16,
    pub version: u8,
    /// Bounds in twips: x min, x max, y min, y max.
    pub bounds: [i32; 4],
    /// Fill styles by kind: solid, linear, radial, focal, bitmap.
    pub fills: [u32; 5],
    pub lines: u32,
    pub straight: u32,
    pub curved: u32,
    pub style_changes: u32,
}

/// Kind of a text definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextKind {
    /// DefineEditText: a text field.
    Field,
    /// DefineText or DefineText2: glyph runs.
    Static,
}

/// One text definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextStat {
    pub id: u16,
    pub kind: TextKind,
    pub font: Option<u16>,
    /// Field: characters of the initial text; static: glyphs.
    pub chars: u32,
    /// Field: a variable name is stored.
    pub has_variable: bool,
    /// Field: flag bits as stored.
    pub flags: u16,
    /// Field: the initial text and the variable name (notes only, never
    /// versioned).
    pub text: String,
    pub variable: String,
}

/// One font definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontStat {
    pub id: u16,
    /// 1, 2, 3 (DefineFont3) or 4.
    pub version: u8,
    pub name: String,
    pub glyphs: u32,
    pub has_layout: bool,
    /// DefineFont4: bytes of the embedded font data.
    pub data: u64,
}

/// One sprite (DefineSprite).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteStat {
    pub id: u16,
    pub frames: u16,
    pub tags: u32,
    pub places: u32,
    pub labels: u32,
}

/// Kind of an ActionScript block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionKind {
    /// DoAction: AVM1 bytes on a frame.
    Frame,
    /// DoInitAction: AVM1 bytes run once for a sprite.
    Init,
    /// DoAbc or DoAbc2: AVM2 bytecode.
    Abc,
    /// A button's condition actions (AVM1).
    Button,
    /// Clip event actions of a placement (AVM1).
    Clip,
}

/// One ActionScript block, kept opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionBlock {
    pub kind: ActionKind,
    /// Offset and length of the block's bytes in [`Movie::data`].
    pub offset: u64,
    pub len: u64,
    /// The sprite whose timeline holds it, `None` for the root.
    pub sprite: Option<u16>,
    /// DoAbc2: the stored name and the lazy-initialise flag.
    pub name: String,
    pub lazy: bool,
}

/// A movie read to its End tag.
#[derive(Debug, Clone)]
pub struct Movie {
    pub name: String,
    pub signature: [u8; 3],
    pub compression: Compression,
    pub version: u8,
    pub stored_len: u64,
    /// The header's uncompressed length (its own eight bytes included).
    pub declared_len: u32,
    /// Bytes after the eight-byte header, as decompressed.
    pub body_len: u64,
    /// Stage rectangle in twips: x min, x max, y min, y max.
    pub stage: [i32; 4],
    /// Frame rate as stored (8.8 fixed point).
    pub frame_rate_raw: i16,
    pub frames: u16,
    /// FileAttributes flag bits as stored (first tag only), `None` without one.
    pub attributes: Option<u32>,
    pub background: Option<[u8; 4]>,
    /// Census by tag code, the root's and every sprite's tags.
    pub tags: BTreeMap<u16, TagStat>,
    pub tag_errors: Vec<TagError>,
    /// Bytes after the root's End tag (or the whole rest when it is missing).
    pub trailing: u64,
    pub end_found: bool,
    /// ExportAssets: (character id, name).
    pub exports: Vec<(u16, String)>,
    /// SymbolClass: (character id, class name); id 0 is the root's class.
    pub classes: Vec<(u16, String)>,
    /// ImportAssets: (source, character id, name).
    pub imports: Vec<(String, u16, String)>,
    /// The source of every ImportAssets tag, entries or none.
    pub import_sources: Vec<String>,
    pub shapes: Vec<ShapeStat>,
    pub morph_shapes: u32,
    pub texts: Vec<TextStat>,
    pub fonts: Vec<FontStat>,
    pub sprites: Vec<SpriteStat>,
    pub actions: Vec<ActionBlock>,
    /// Frame labels (root and sprites), notes only.
    pub labels: Vec<String>,
    /// Placements and the instance names they store (notes only).
    pub places: u64,
    pub instance_names: Vec<String>,
    /// Named placements: the timeline (sprite id, `None` for the root), the
    /// character placed (`None` for a modification) and the name.
    pub named_places: Vec<(Option<u16>, Option<u16>, String)>,
    /// The decompressed tag stream (after the stage rectangle, rate and
    /// frame count).
    pub data: Vec<u8>,
}

fn err(
    code: &str,
    file: &str,
    offset: u64,
    expected: impl ToString,
    found: impl ToString,
) -> Error {
    Error::new(code, file, offset, expected, found)
}

/// Runs a parser call, turning a panic into a named error.
fn guarded<T>(file: &str, offset: u64, f: impl FnOnce() -> T) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| {
        err(
            "ParserPanic",
            file,
            offset,
            "no panic",
            "swf parser panicked",
        )
    })
}

fn rect(r: &swf::Rectangle<swf::Twips>) -> [i32; 4] {
    [r.x_min.get(), r.x_max.get(), r.y_min.get(), r.y_max.get()]
}

fn text(s: &swf::SwfStr, version: u8, max: usize) -> String {
    let b = s.as_bytes();
    let b = &b[..b.len().min(max)];
    swf::SwfStr::from_bytes(b)
        .to_str_lossy(swf::SwfStr::encoding_for_version(version))
        .into_owned()
}

/// Header of one tag in `rest`: (code, body length, header length).
fn tag_header(rest: &[u8]) -> Option<(u16, u64, u64)> {
    let w = u16::from_le_bytes([*rest.first()?, *rest.get(1)?]);
    let code = w >> 6;
    let short = u64::from(w & 0x3f);
    if short == 0x3f {
        let l = rest.get(2..6)?;
        Some((
            code,
            u64::from(u32::from_le_bytes([l[0], l[1], l[2], l[3]])),
            6,
        ))
    } else {
        Some((code, short, 2))
    }
}

/// Offset of `inner` inside `outer`, when it lies there.
fn offset_in(outer: &[u8], inner: &[u8]) -> Option<u64> {
    let o = outer.as_ptr() as usize;
    let i = inner.as_ptr() as usize;
    let off = i.checked_sub(o)?;
    (off.checked_add(inner.len())? <= outer.len()).then_some(off as u64)
}

/// A tag as the walk hands it to a visitor.
pub struct Visit<'a, 'b> {
    pub tag: &'b swf::Tag<'a>,
    pub code: u16,
    /// Offset of the tag's header in the tag stream.
    pub offset: u64,
    pub body: u64,
    pub sprite: Option<u16>,
}

/// Walks a tag stream tolerantly: each tag's stored length is checked
/// before it is parsed, a refused tag is reported and skipped, sprites are
/// walked with their own tags. Returns (end found, trailing bytes).
/// DefineSprite's tag code (SWF19).
pub const SPRITE: u16 = 39;

pub fn walk<'a>(
    data: &'a [u8],
    version: u8,
    name: &str,
    l: &Limits,
    count: &mut u64,
    visit: &mut dyn FnMut(Visit<'a, '_>),
    error: &mut dyn FnMut(TagError),
) -> Result<(bool, u64)> {
    walk_in(data, data, version, name, l, None, 0, count, visit, error)
}

#[allow(clippy::too_many_arguments)]
fn walk_in<'a>(
    all: &'a [u8],
    stream: &'a [u8],
    version: u8,
    name: &str,
    l: &Limits,
    sprite: Option<u16>,
    depth: u32,
    count: &mut u64,
    visit: &mut dyn FnMut(Visit<'a, '_>),
    error: &mut dyn FnMut(TagError),
) -> Result<(bool, u64)> {
    fo4_core::limit(u64::from(depth), u64::from(l.max_depth), name, 0)?;
    let mut rest = stream;
    loop {
        let offset = offset_in(all, rest).unwrap_or(0);
        if rest.is_empty() {
            return Ok((false, 0));
        }
        let Some((code, len, head)) = tag_header(rest) else {
            error(TagError {
                code: NO_CODE,
                offset,
                sprite,
                message: format!("{} bytes left, a tag header needs 2 or 6", rest.len()),
            });
            return Ok((false, rest.len() as u64));
        };
        let total = fo4_core::add(len, head, name, offset)?;
        if total > rest.len() as u64 {
            error(TagError {
                code,
                offset,
                sprite,
                message: format!(
                    "tag length {len} runs past the stream end ({} bytes left)",
                    rest.len() as u64 - head.min(rest.len() as u64)
                ),
            });
            return Ok((false, rest.len() as u64));
        }
        *count = fo4_core::add(*count, 1, name, offset)?;
        fo4_core::limit(*count, l.max_tags, name, offset)?;
        let here = &rest[..total as usize];
        let after = &rest[total as usize..];
        if code == 0 {
            return Ok((true, after.len() as u64));
        }
        if code == SPRITE {
            // DefineSprite (SWF19): sprite id u16, frame count u16, then its
            // own tag stream. The stream is walked here, once, so a refused
            // inner tag does not lose the rest of the sprite.
            let b = &here[head as usize..];
            if b.len() < 4 {
                error(TagError {
                    code,
                    offset,
                    sprite,
                    message: format!("sprite body of {} bytes, 4 needed", b.len()),
                });
            } else {
                let id = u16::from_le_bytes([b[0], b[1]]);
                let frames = u16::from_le_bytes([b[2], b[3]]);
                let tag = swf::Tag::DefineSprite(swf::Sprite {
                    id,
                    num_frames: frames,
                    tags: Vec::new(),
                });
                visit(Visit {
                    tag: &tag,
                    code,
                    offset,
                    body: len,
                    sprite,
                });
                let (end, trailing) = walk_in(
                    all,
                    &b[4..],
                    version,
                    name,
                    l,
                    Some(id),
                    depth + 1,
                    count,
                    visit,
                    error,
                )?;
                if !end || trailing != 0 {
                    error(TagError {
                        code,
                        offset,
                        sprite: Some(id),
                        message: format!(
                            "sprite stream: end tag found {end}, {trailing} bytes after it"
                        ),
                    });
                }
            }
        } else {
            let mut reader = swf::read::Reader::new(here, version);
            match guarded(name, offset, || reader.read_tag())? {
                Ok(tag) => visit(Visit {
                    tag: &tag,
                    code,
                    offset,
                    body: len,
                    sprite,
                }),
                Err(e) => error(TagError {
                    code,
                    offset,
                    sprite,
                    message: e.to_string(),
                }),
            }
        }
        rest = after;
    }
}

impl Movie {
    /// Reads a movie from its stored bytes.
    pub fn read(bytes: &[u8], name: &str, l: &Limits, b: &Budget) -> Result<Self> {
        fo4_core::limit(bytes.len() as u64, l.max_stored, name, 0)?;
        if bytes.len() < 8 {
            return Err(err(
                "Truncated",
                name,
                0,
                "8-byte movie header",
                bytes.len(),
            ));
        }
        let signature = [bytes[0], bytes[1], bytes[2]];
        let compression = match &signature {
            b"FWS" | b"GFX" => Compression::None,
            b"CWS" | b"CFX" => Compression::Zlib,
            b"ZWS" => Compression::Lzma,
            _ => {
                return Err(err(
                    "BadSignature",
                    name,
                    0,
                    "FWS, GFX, CWS, CFX or ZWS",
                    format!("{signature:02x?}"),
                ))
            }
        };
        let version = bytes[3];
        let declared_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        fo4_core::limit(u64::from(declared_len), l.max_declared, name, 4)?;
        let declared_body = u64::from(declared_len)
            .checked_sub(8)
            .ok_or_else(|| err("BadLength", name, 4, ">= 8", declared_len))?;
        let stored_body = &bytes[8..];
        // The body is normalised to an uncompressed stream before the
        // parser sees it, so the parser's own decompression is never used.
        let plain: Vec<u8> = match compression {
            Compression::None => {
                // Read no further than the declared length, as the format says.
                let n = (stored_body.len() as u64).min(declared_body);
                let mut v = b.bytes(n + 8, name, 8)?;
                v[..8].copy_from_slice(&bytes[..8]);
                v[8..].copy_from_slice(&stored_body[..n as usize]);
                v
            }
            Compression::Zlib => {
                let limits = fo4_core::Limits {
                    max_payload: l.max_declared,
                    ..fo4_core::Limits::default()
                };
                let body = fo4_core::zlib(stored_body, declared_body, &limits, b, name, 8)?;
                let mut v = b.bytes(0, name, 8)?;
                b.reserve(&mut v, body.len() + 8, name, 8)?;
                v.extend_from_slice(b"FWS");
                v.push(version);
                v.extend_from_slice(&declared_len.to_le_bytes());
                v.extend_from_slice(&body);
                v
            }
            Compression::Lzma => {
                return Err(err(
                    "Unsupported",
                    name,
                    0,
                    "an uncompressed or zlib movie",
                    "LZMA movie",
                ))
            }
        };
        let body_len = (plain.len() - 8) as u64;
        if version == 0 {
            return Err(err("BadVersion", name, 3, "version >= 1", 0));
        }
        let buf = guarded(name, 8, || swf::decompress_swf(&plain[..]))?
            .map_err(|e| err("BadHeader", name, 8, "stage rectangle, rate, frames", e))?;
        drop(plain);
        b.charge(buf.data.len() as u64, name, 8)?;
        let h = buf.header.swf_header();
        let mut m = Movie {
            name: name.to_string(),
            signature,
            compression,
            version,
            stored_len: bytes.len() as u64,
            declared_len,
            body_len,
            stage: rect(&h.stage_size),
            frame_rate_raw: h.frame_rate.get(),
            frames: h.num_frames,
            attributes: None,
            background: buf.header.background_color().map(|c| [c.r, c.g, c.b, c.a]),
            tags: BTreeMap::new(),
            tag_errors: Vec::new(),
            trailing: 0,
            end_found: false,
            exports: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            import_sources: Vec::new(),
            shapes: Vec::new(),
            morph_shapes: 0,
            texts: Vec::new(),
            fonts: Vec::new(),
            sprites: Vec::new(),
            actions: Vec::new(),
            labels: Vec::new(),
            places: 0,
            instance_names: Vec::new(),
            named_places: Vec::new(),
            data: Vec::new(),
        };
        let data = buf.data;
        let mut errors = Vec::new();
        let mut count = 0u64;
        let mut first = true;
        let (end, trailing) = {
            let ms = &mut m;
            let max = l.max_string;
            let d: &[u8] = &data;
            walk(
                d,
                version,
                name,
                l,
                &mut count,
                &mut |v| {
                    if v.sprite.is_none() && first {
                        if let swf::Tag::FileAttributes(a) = v.tag {
                            ms.attributes = Some(u32::from(a.bits()));
                        }
                        first = false;
                    }
                    ms.take(d, &v, max);
                },
                &mut |e| errors.push(e),
            )?
        };
        m.tag_errors = errors;
        m.end_found = end;
        m.trailing = trailing;
        m.data = data;
        Ok(m)
    }

    /// Reads the winning copy of `path` from the mounted archives and parses it.
    pub fn open(vfs: &fo4_vfs::Vfs, path: &str, l: &Limits, b: &Budget) -> Result<Self> {
        let bytes = vfs.read(path, b)?;
        Self::read(&bytes, path, l, b)
    }

    /// The bytes of an action block.
    pub fn action_bytes(&self, a: &ActionBlock) -> Option<&[u8]> {
        let s = usize::try_from(a.offset).ok()?;
        let e = s.checked_add(usize::try_from(a.len).ok()?)?;
        self.data.get(s..e)
    }

    /// The chains of named placements from the root timeline down to the
    /// placements of `target` (a character id): each chain the instance
    /// names in order. Breadth first, each timeline visited once.
    pub fn paths_to(&self, target: u16) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        let mut queue: std::collections::VecDeque<(Option<u16>, Vec<String>)> =
            std::collections::VecDeque::from([(None, Vec::new())]);
        let mut seen = std::collections::BTreeSet::new();
        while let Some((timeline, path)) = queue.pop_front() {
            if !seen.insert(timeline) {
                continue;
            }
            for (t, ch, name) in &self.named_places {
                if *t != timeline {
                    continue;
                }
                let Some(ch) = ch else { continue };
                let mut p = path.clone();
                p.push(name.clone());
                if *ch == target {
                    out.push(p.clone());
                }
                if self.sprites.iter().any(|s| s.id == *ch) {
                    queue.push_back((Some(*ch), p));
                }
            }
        }
        out
    }

    /// The root class's name (SymbolClass id 0), when linked.
    pub fn root_class(&self) -> Option<&str> {
        self.classes
            .iter()
            .find(|(id, _)| *id == 0)
            .map(|(_, n)| n.as_str())
    }

    fn block(
        &mut self,
        data: &[u8],
        kind: ActionKind,
        bytes: &[u8],
        sprite: Option<u16>,
        name: String,
        lazy: bool,
    ) {
        self.actions.push(ActionBlock {
            kind,
            offset: offset_in(data, bytes).unwrap_or(u64::MAX),
            len: bytes.len() as u64,
            sprite,
            name,
            lazy,
        });
    }

    fn take(&mut self, data: &[u8], v: &Visit<'_, '_>, max: usize) {
        use swf::Tag as T;
        let s = self.tags.entry(v.code).or_default();
        s.count += 1;
        s.bytes += v.body;
        if let Some(id) = v.sprite {
            if let Some(sp) = self.sprites.iter_mut().rev().find(|x| x.id == id) {
                sp.tags += 1;
                sp.places += u32::from(matches!(v.tag, T::PlaceObject(_)));
                sp.labels += u32::from(matches!(v.tag, T::FrameLabel(_)));
            }
        }
        let ver = self.version;
        let str = |x: &swf::SwfStr| text(x, ver, max);
        match v.tag {
            T::ExportAssets(a) => {
                for e in a {
                    self.exports.push((e.id, str(e.name)));
                }
            }
            T::SymbolClass(links) => {
                for k in links {
                    self.classes.push((k.id, str(k.class_name)));
                }
            }
            T::ImportAssets { url, imports } => {
                self.import_sources.push(str(url));
                for e in imports {
                    self.imports.push((str(url), e.id, str(e.name)));
                }
            }
            T::DefineShape(sh) => self.shapes.push(shape_stat(sh)),
            T::DefineMorphShape(_) => self.morph_shapes += 1,
            T::DefineEditText(t) => {
                let initial = t.initial_text().map(str).unwrap_or_default();
                self.texts.push(TextStat {
                    id: t.id(),
                    kind: TextKind::Field,
                    font: t.font_id(),
                    chars: initial.chars().count() as u32,
                    has_variable: !t.variable_name().is_empty(),
                    flags: t.flags().bits(),
                    text: initial,
                    variable: str(t.variable_name()),
                });
            }
            T::DefineText(t) | T::DefineText2(t) => self.texts.push(TextStat {
                id: t.id,
                kind: TextKind::Static,
                font: t.records.iter().find_map(|r| r.font_id),
                chars: t.records.iter().map(|r| r.glyphs.len() as u32).sum(),
                has_variable: false,
                flags: 0,
                text: String::new(),
                variable: String::new(),
            }),
            T::DefineFont(f) => self.fonts.push(FontStat {
                id: f.id,
                version: 1,
                name: String::new(),
                glyphs: f.glyphs.len() as u32,
                has_layout: false,
                data: 0,
            }),
            T::DefineFont2(f) => self.fonts.push(FontStat {
                id: f.id,
                version: f.version,
                name: str(f.name),
                glyphs: f.glyphs.len() as u32,
                has_layout: f.layout.is_some(),
                data: 0,
            }),
            T::DefineFont4(f) => self.fonts.push(FontStat {
                id: f.id,
                version: 4,
                name: str(f.name),
                glyphs: 0,
                has_layout: false,
                data: f.data.map_or(0, |d| d.len() as u64),
            }),
            T::DefineSprite(sp) => self.sprites.push(SpriteStat {
                id: sp.id,
                frames: sp.num_frames,
                tags: 0,
                places: 0,
                labels: 0,
            }),
            T::DoAction(a) => {
                self.block(data, ActionKind::Frame, a, v.sprite, String::new(), false)
            }
            T::DoInitAction { action_data, .. } => self.block(
                data,
                ActionKind::Init,
                action_data,
                v.sprite,
                String::new(),
                false,
            ),
            T::DoAbc(a) => self.block(data, ActionKind::Abc, a, v.sprite, String::new(), false),
            T::DoAbc2(a) => self.block(
                data,
                ActionKind::Abc,
                a.data,
                v.sprite,
                str(a.name),
                a.flags.contains(swf::DoAbc2Flag::LAZY_INITIALIZE),
            ),
            T::DefineButton(bt) | T::DefineButton2(bt) => {
                for a in &bt.actions {
                    if !a.action_data.is_empty() {
                        self.block(
                            data,
                            ActionKind::Button,
                            a.action_data,
                            v.sprite,
                            String::new(),
                            false,
                        );
                    }
                }
            }
            T::PlaceObject(p) => {
                self.places += 1;
                if let Some(n) = p.name {
                    self.instance_names.push(str(n));
                    let ch = match p.action {
                        swf::PlaceObjectAction::Place(id) | swf::PlaceObjectAction::Replace(id) => {
                            Some(id)
                        }
                        _ => None,
                    };
                    self.named_places.push((v.sprite, ch, str(n)));
                }
                if let Some(c) = &p.clip_actions {
                    for a in &c.records {
                        self.block(
                            data,
                            ActionKind::Clip,
                            a.action_data,
                            v.sprite,
                            String::new(),
                            false,
                        );
                    }
                }
            }
            T::FrameLabel(f) => self.labels.push(str(f.label)),
            _ => {}
        }
    }
}

fn shape_stat(sh: &swf::Shape) -> ShapeStat {
    let mut s = ShapeStat {
        id: sh.id,
        version: sh.version,
        bounds: rect(&sh.shape_bounds),
        fills: [0; 5],
        lines: 0,
        straight: 0,
        curved: 0,
        style_changes: 0,
    };
    let mut styles = |st: &swf::ShapeStyles| {
        for f in &st.fill_styles {
            s.fills[fill_kind(f)] += 1;
        }
        s.lines += st.line_styles.len() as u32;
    };
    styles(&sh.styles);
    for r in &sh.shape {
        match r {
            swf::ShapeRecord::StyleChange(c) => {
                if let Some(n) = &c.new_styles {
                    styles(n);
                }
                s.style_changes += 1;
            }
            swf::ShapeRecord::StraightEdge { .. } => s.straight += 1,
            swf::ShapeRecord::CurvedEdge { .. } => s.curved += 1,
        }
    }
    s
}

/// Index of a fill kind: solid, linear, radial, focal, bitmap.
pub fn fill_kind(f: &swf::FillStyle) -> usize {
    match f {
        swf::FillStyle::Color(_) => 0,
        swf::FillStyle::LinearGradient(_) => 1,
        swf::FillStyle::RadialGradient(_) => 2,
        swf::FillStyle::FocalGradient { .. } => 3,
        swf::FillStyle::Bitmap { .. } => 4,
    }
}

/// The tag's name as the parser's tag table spells it, or `unknown`.
pub fn tag_name(code: u16) -> String {
    match swf::TagCode::from_u16(code) {
        Some(c) => format!("{c:?}"),
        None => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests;
