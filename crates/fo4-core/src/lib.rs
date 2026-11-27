#![forbid(unsafe_code)]
//! Shared checked I/O, diagnostics and explicit installation contract.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    fs::{File, Metadata},
    io::{Read, Seek},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Mutex,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceSpan {
    pub file: String,
    pub offset: u64,
    pub length: u64,
    pub archive_entry: Option<String>,
    pub version: Option<u32>,
    pub sha256: Option<String>,
    pub decompressed_offset: Option<u64>,
}
impl SourceSpan {
    pub fn new(file: &str, offset: u64, length: u64) -> Self {
        Self {
            file: file.into(),
            offset,
            length,
            archive_entry: None,
            version: None,
            sha256: None,
            decompressed_offset: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Error {
    pub code: String,
    pub source: Box<SourceSpan>,
    pub expected: String,
    pub found: String,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(
        code: &str,
        file: &str,
        offset: u64,
        expected: impl ToString,
        found: impl ToString,
    ) -> Self {
        Self {
            code: code.into(),
            source: Box::new(SourceSpan::new(file, offset, 0)),
            expected: expected.to_string(),
            found: found.to_string(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}@{}: expected {}; found {}",
            self.code, self.source.file, self.source.offset, self.expected, self.found
        )
    }
}
impl std::error::Error for Error {}
pub fn io_error(file: &str, offset: u64, e: std::io::Error) -> Error {
    Error::new("Io", file, offset, "readable stable file", e)
}
pub fn add(a: u64, b: u64, file: &str, offset: u64) -> Result<u64> {
    a.checked_add(b)
        .ok_or_else(|| Error::new("Overflow", file, offset, "u64 addition", format!("{a}+{b}")))
}
pub fn mul(a: u64, b: u64, file: &str, offset: u64) -> Result<u64> {
    a.checked_mul(b).ok_or_else(|| {
        Error::new(
            "Overflow",
            file,
            offset,
            "u64 multiplication",
            format!("{a}*{b}"),
        )
    })
}
pub fn limit(value: u64, max: u64, file: &str, offset: u64) -> Result<()> {
    if value > max {
        Err(Error::new(
            "LimitExceeded",
            file,
            offset,
            format!("<= {max}"),
            value,
        ))
    } else {
        Ok(())
    }
}
pub fn range(offset: u64, size: u64, end: u64, file: &str) -> Result<()> {
    let actual = add(offset, size, file, offset)?;
    if actual > end {
        Err(Error::new(
            "Truncated",
            file,
            offset,
            format!("range end <= {end}"),
            actual,
        ))
    } else {
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_vfs_memory: u64,
    pub max_override_memory: u64,
    pub max_report_bytes: u64,
    pub max_payload: u64,
    pub max_memory: u64,
    pub max_entries: u64,
    pub max_records: u64,
    pub max_subrecords: u64,
    pub max_string: u64,
    pub max_depth: u64,
    pub max_chunks: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_vfs_memory: 512 << 20,
            max_override_memory: 512 << 20,
            max_report_bytes: 64 << 20,
            max_payload: 256 << 20,
            max_memory: 512 << 20,
            max_entries: 1_000_000,
            max_records: 1_000_000,
            max_subrecords: 2_000_000,
            max_string: 16_384,
            max_depth: 64,
            max_chunks: 256,
        }
    }
}
impl Limits {
    pub fn validate(&self, file: &str) -> Result<()> {
        for (n, v, m) in [
            ("payload", self.max_payload, 2 << 30),
            ("memory", self.max_memory, 16 << 30),
            ("VFS memory", self.max_vfs_memory, 16 << 30),
            ("override memory", self.max_override_memory, 16 << 30),
            ("report bytes", self.max_report_bytes, 1 << 30),
            ("depth", self.max_depth, 256),
            ("entries", self.max_entries, 50_000_000),
            ("records", self.max_records, 100_000_000),
            ("subrecords", self.max_subrecords, 1_000_000_000),
            ("string", self.max_string, 16 << 20),
            ("chunks", self.max_chunks, 256),
        ] {
            if v == 0 || v > m {
                return Err(Error::new(
                    "LimitExceeded",
                    file,
                    0,
                    format!("{n}: 1..={m}"),
                    v,
                ));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy)]
#[repr(usize)]
pub enum AllocationKind {
    General,
    CellContext,
    Children,
    Reference,
    BaseAssets,
    AssetGraph,
    Diagnostics,
    Publication,
}
impl AllocationKind {
    pub const NAMES: [&'static str; 8] = [
        "general",
        "cell_context",
        "children",
        "reference",
        "base_assets",
        "asset_graph",
        "diagnostics",
        "publication",
    ];
}
struct Meter {
    current: Cell<usize>,
    live: [Cell<u64>; 8],
    charged: [Cell<u64>; 8],
    peak: Cell<u64>,
    rejected: Cell<Option<(usize, u64)>>,
}
#[derive(Debug, Serialize)]
pub struct AllocationStats {
    pub used: u64,
    pub peak: u64,
    pub categories: [AllocationCategory; 8],
    pub rejected_category: Option<&'static str>,
    pub rejected_requested_bytes: Option<u64>,
}
#[derive(Debug, Serialize)]
pub struct AllocationCategory {
    pub name: &'static str,
    pub live_credits: u64,
    pub cumulative_charged: u64,
}
struct MeterScope<'a> {
    meter: &'a Meter,
    previous: usize,
}
impl Drop for MeterScope<'_> {
    fn drop(&mut self) {
        self.meter.current.set(self.previous);
    }
}
/// Conservative allocation credits. Child credits are released when the child drops,
/// unless the child is [`Budget::commit`]ted to its parent.
/// Within one budget, credits remain cumulative; use children for temporary work.
///
/// Every account of one tree shares the root's ceiling. A root made with
/// [`Budget::with_scope_limit`] also limits each child, with all its own
/// children, to a smaller ceiling: the root then bounds what a whole scene
/// keeps and the scope limit what one read or one cell may take. A root made
/// with [`Budget::new`] has a scope limit equal to its ceiling, which changes
/// nothing.
pub struct Budget {
    max: u64,
    shared: Rc<Cell<u64>>,
    local: Cell<u64>,
    meter: Option<Rc<Meter>>,
    categories: [Cell<u64>; 8],
    /// Ceiling of each child scope (the child and its descendants).
    scope_max: u64,
    /// Credits of each enclosing child scope, outermost first; this
    /// account's own scope last. Empty for a root.
    scopes: Vec<Rc<Cell<u64>>>,
    /// Named in the error when the root ceiling, not a scope, is exceeded.
    name: Option<&'static str>,
}
impl Budget {
    pub fn new(max: u64) -> Self {
        Self {
            max,
            shared: Rc::new(Cell::new(0)),
            local: Cell::new(0),
            meter: None,
            categories: std::array::from_fn(|_| Cell::new(0)),
            scope_max: max,
            scopes: Vec::new(),
            name: None,
        }
    }
    /// Limits every child scope of this root to `scope_max` credits (at
    /// most the root's ceiling), while the root keeps its own ceiling for
    /// the sum. Only a root can set it.
    pub fn with_scope_limit(mut self, scope_max: u64) -> Self {
        if self.scopes.is_empty() {
            self.scope_max = scope_max.min(self.max);
        }
        self
    }
    /// Names the root ceiling in the error that exceeding it returns, such
    /// as the option that sets it.
    pub fn named(mut self, name: &'static str) -> Self {
        self.name = Some(name);
        self
    }
    /// The root ceiling.
    pub fn max(&self) -> u64 {
        self.max
    }
    /// The ceiling of each child scope.
    pub fn scope_max(&self) -> u64 {
        self.scope_max
    }
    /// Whether `e` is a refusal a later charge with more room could pass:
    /// a charge past this budget's ceiling (the bound it states, as
    /// [`Budget::charge`] words it) or an allocation that failed; not a
    /// limit of the data read, nor of one scope's work.
    pub fn refused_at_ceiling(&self, e: &Error) -> bool {
        match e.code.as_str() {
            "AllocationFailed" => true,
            "LimitExceeded" => {
                let ceiling = format!("<= {}", self.max);
                e.expected == ceiling || e.expected.starts_with(&format!("{ceiling} ("))
            }
            _ => false,
        }
    }
    /// Optional telemetry of conservative credits; this is not resident memory.
    pub fn metered(max: u64) -> Self {
        let mut b = Self::new(max);
        b.meter = Some(Rc::new(Meter {
            current: Cell::new(0),
            live: std::array::from_fn(|_| Cell::new(0)),
            charged: std::array::from_fn(|_| Cell::new(0)),
            peak: Cell::new(0),
            rejected: Cell::new(None),
        }));
        b
    }
    pub fn measure<T>(&self, kind: AllocationKind, f: impl FnOnce() -> T) -> T {
        let _scope = self.meter.as_ref().map(|meter| {
            let previous = meter.current.replace(kind as usize);
            MeterScope { meter, previous }
        });
        f()
    }
    pub fn measurements(&self) -> Option<AllocationStats> {
        self.meter.as_ref().map(|m| AllocationStats {
            used: self.used(),
            peak: m.peak.get(),
            categories: std::array::from_fn(|i| AllocationCategory {
                name: AllocationKind::NAMES[i],
                live_credits: m.live[i].get(),
                cumulative_charged: m.charged[i].get(),
            }),
            rejected_category: m.rejected.get().map(|(i, _)| AllocationKind::NAMES[i]),
            rejected_requested_bytes: m.rejected.get().map(|(_, n)| n),
        })
    }
    /// A scoped allocation account sharing the aggregate ceiling. Drop releases its charges.
    /// Its allocated data must be dropped before this account is dropped.
    /// The child is a scope: its charges and its own children's together
    /// stay within the root's scope limit.
    pub fn child(&self) -> Self {
        let mut scopes = Vec::with_capacity(self.scopes.len() + 1);
        scopes.extend(self.scopes.iter().cloned());
        scopes.push(Rc::new(Cell::new(0)));
        Self {
            max: self.max,
            shared: Rc::clone(&self.shared),
            local: Cell::new(0),
            meter: self.meter.clone(),
            categories: std::array::from_fn(|_| Cell::new(0)),
            scope_max: self.scope_max,
            scopes,
            name: self.name,
        }
    }
    /// A long-lived account released when it drops, like [`Budget::child`],
    /// but inside this account's scope instead of a new one: for a cache
    /// that grows with the whole scene, bounded by the enclosing scope (the
    /// root's ceiling for an account of the root) rather than by the scope
    /// limit of one read.
    pub fn account(&self) -> Self {
        Self {
            max: self.max,
            shared: Rc::clone(&self.shared),
            local: Cell::new(0),
            meter: self.meter.clone(),
            categories: std::array::from_fn(|_| Cell::new(0)),
            scope_max: self.scope_max,
            scopes: self.scopes.clone(),
            name: self.name,
        }
    }
    /// Keeps the charges of `self`, a child of `parent`, as `parent`'s:
    /// for data made within the child's scope limit that outlives it.
    /// Fails unless `self` is a direct child of `parent`; its charges are
    /// then released as when it drops.
    pub fn commit(self, parent: &Budget) -> Result<()> {
        let direct = Rc::ptr_eq(&self.shared, &parent.shared)
            && self.scopes.len() == parent.scopes.len() + 1
            && self
                .scopes
                .iter()
                .zip(&parent.scopes)
                .all(|(a, b)| Rc::ptr_eq(a, b));
        if !direct {
            return Err(Error::new(
                "BudgetNotAChild",
                "budget",
                0,
                "a direct child of the parent",
                "another account",
            ));
        }
        let local = add(parent.local.get(), self.local.get(), "budget", 0)?;
        parent.local.set(local);
        self.local.set(0);
        for (own, theirs) in self.categories.iter().zip(&parent.categories) {
            theirs.set(add(theirs.get(), own.replace(0), "budget", 0)?);
        }
        Ok(())
    }
    pub fn used(&self) -> u64 {
        self.shared.get()
    }
    pub fn remaining(&self) -> u64 {
        self.max.saturating_sub(self.shared.get())
    }
    pub fn charge(&self, size: u64, file: &str, offset: u64) -> Result<()> {
        let next = add(self.shared.get(), size, file, offset)?;
        let reject = || {
            if let Some(m) = &self.meter {
                m.rejected.set(Some((m.current.get(), size)));
            }
        };
        if next > self.max {
            reject();
            return Err(match self.name {
                Some(name) => Error::new(
                    "LimitExceeded",
                    file,
                    offset,
                    format!("<= {} ({name})", self.max),
                    next,
                ),
                None => Error::new(
                    "LimitExceeded",
                    file,
                    offset,
                    format!("<= {}", self.max),
                    next,
                ),
            });
        }
        for scope in &self.scopes {
            let n = add(scope.get(), size, file, offset)?;
            if n > self.scope_max {
                reject();
            }
            limit(n, self.scope_max, file, offset)?;
        }
        let local = add(self.local.get(), size, file, offset)?;
        if let Some(m) = &self.meter {
            let i = m.current.get();
            let own = add(self.categories[i].get(), size, file, offset)?;
            let live = add(m.live[i].get(), size, file, offset)?;
            let charged = add(m.charged[i].get(), size, file, offset)?;
            self.categories[i].set(own);
            m.live[i].set(live);
            m.charged[i].set(charged);
            m.peak.set(m.peak.get().max(next));
        }
        self.shared.set(next);
        for scope in &self.scopes {
            scope.set(scope.get() + size);
        }
        self.local.set(local);
        Ok(())
    }
    pub fn reserve<T>(&self, v: &mut Vec<T>, n: usize, file: &str, offset: u64) -> Result<()> {
        self.reserve_capped(v, n, usize::MAX, file, offset)
    }
    pub fn reserve_capped<T>(
        &self,
        v: &mut Vec<T>,
        n: usize,
        ceiling: usize,
        file: &str,
        offset: u64,
    ) -> Result<()> {
        let desired = v
            .len()
            .checked_add(n)
            .ok_or_else(|| Error::new("Overflow", file, offset, "vector length + count", n))?;
        limit(desired as u64, ceiling as u64, file, offset)?;
        if desired <= v.capacity() {
            return Ok(());
        }
        let growth = v.capacity().checked_mul(2).ok_or_else(|| {
            Error::new(
                "Overflow",
                file,
                offset,
                "vector capacity * 2",
                v.capacity(),
            )
        })?;
        let capacity = desired.max(growth.min(ceiling));
        let size = mul(
            capacity as u64,
            std::mem::size_of::<T>() as u64,
            file,
            offset,
        )?;
        // Charge the entire new allocation and retain the old charge. This covers
        // the transient copy during realloc and keeps growth amortized linear.
        self.charge(size, file, offset)?;
        v.try_reserve_exact(capacity - v.len())
            .map_err(|e| Error::new("AllocationFailed", file, offset, size, e))
    }
    pub fn bytes(&self, n: u64, file: &str, offset: u64) -> Result<Vec<u8>> {
        let n = usize::try_from(n)
            .map_err(|e| Error::new("LimitExceeded", file, offset, "usize", e))?;
        let mut v = Vec::new();
        self.reserve(&mut v, n, file, offset)?;
        v.resize(n, 0);
        Ok(v)
    }
}
impl Drop for Budget {
    fn drop(&mut self) {
        if let Some(m) = &self.meter {
            for (i, n) in self.categories.iter().enumerate() {
                m.live[i].set(m.live[i].get().saturating_sub(n.get()));
            }
        }
        self.shared
            .set(self.shared.get().saturating_sub(self.local.get()));
        for scope in &self.scopes {
            scope.set(scope.get().saturating_sub(self.local.get()));
        }
    }
}
pub trait ReadAt {
    fn check_stable(&self) -> Result<()> {
        Ok(())
    }
    fn name(&self) -> &str;
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<()>;
}
pub struct SliceSource<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}
impl ReadAt for SliceSource<'_> {
    fn name(&self) -> &str {
        self.name
    }
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<()> {
        range(offset, out.len() as u64, self.len(), self.name)?;
        let start = offset as usize;
        out.copy_from_slice(&self.bytes[start..start + out.len()]);
        Ok(())
    }
}
struct PositionedReader<R> {
    reader: R,
    position: Option<u64>,
}
impl<R: Read + Seek> PositionedReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            position: None,
        }
    }
    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> std::io::Result<()> {
        let prior = self.position.take();
        match prior {
            Some(current) => {
                let delta = i128::from(offset) - i128::from(current);
                if let Ok(delta) = i64::try_from(delta) {
                    self.reader.seek_relative(delta)?;
                } else {
                    self.reader.seek(std::io::SeekFrom::Start(offset))?;
                }
            }
            None => {
                self.reader.seek(std::io::SeekFrom::Start(offset))?;
            }
        }
        self.reader.read_exact(out)?;
        self.position = Some(
            offset
                .checked_add(out.len() as u64)
                .ok_or_else(|| std::io::Error::other("position overflow"))?,
        );
        Ok(())
    }
}
pub struct FileSource {
    name: String,
    path: PathBuf,
    file: Mutex<PositionedReader<std::io::BufReader<File>>>,
    metadata_checks: std::sync::atomic::AtomicU64,
    metadata: Metadata,
}
impl FileSource {
    pub fn open(path: &Path) -> Result<Self> {
        let name = path.to_string_lossy().into_owned();
        let file = File::open(path).map_err(|e| io_error(&name, 0, e))?;
        let metadata = file.metadata().map_err(|e| io_error(&name, 0, e))?;
        if !metadata.is_file() {
            return Err(Error::new("InvalidFile", &name, 0, "regular file", "other"));
        }
        Ok(Self {
            name,
            path: path.into(),
            file: Mutex::new(PositionedReader::new(std::io::BufReader::with_capacity(
                64 * 1024,
                file,
            ))),
            metadata_checks: std::sync::atomic::AtomicU64::new(0),
            metadata,
        })
    }
    pub fn metadata_checks(&self) -> u64 {
        self.metadata_checks
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<()> {
        self.metadata_checks
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let now = std::fs::metadata(&self.path).map_err(|e| io_error(&self.name, 0, e))?;
        let same = self.metadata.len() == now.len()
            && self.metadata.modified().ok() == now.modified().ok();
        #[cfg(unix)]
        let same = {
            use std::os::unix::fs::MetadataExt;
            same && self.metadata.ino() == now.ino()
                && self.metadata.dev() == now.dev()
                && self.metadata.ctime() == now.ctime()
                && self.metadata.ctime_nsec() == now.ctime_nsec()
        };
        if !same {
            return Err(Error::new(
                "FileChanged",
                &self.name,
                0,
                "original file metadata",
                "changed",
            ));
        }
        Ok(())
    }
}
impl ReadAt for FileSource {
    fn check_stable(&self) -> Result<()> {
        self.check()
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn len(&self) -> u64 {
        self.metadata.len()
    }
    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<()> {
        range(offset, out.len() as u64, self.len(), &self.name)?;
        let mut state = self.file.lock().map_err(|_| {
            Error::new("Io", &self.name, offset, "unpoisoned file lock", "poisoned")
        })?;
        state
            .read_at(offset, out)
            .map_err(|e| io_error(&self.name, offset, e))
    }
}
pub fn read_bytes(
    source: &dyn ReadAt,
    offset: u64,
    size: u64,
    limits: &Limits,
    budget: &Budget,
) -> Result<Vec<u8>> {
    source.check_stable()?;
    let bytes = read_bytes_inside_operation(source, offset, size, limits, budget)?;
    source.check_stable()?;
    Ok(bytes)
}
/// `read_bytes` without the stability checks. The caller brackets its whole
/// operation with `ReadAt::check_stable`, once before the first read and once
/// after the last, instead of paying two metadata lookups per read.
pub fn read_bytes_inside_operation(
    source: &dyn ReadAt,
    offset: u64,
    size: u64,
    limits: &Limits,
    budget: &Budget,
) -> Result<Vec<u8>> {
    range(offset, size, source.len(), source.name())?;
    limit(size, limits.max_payload, source.name(), offset)?;
    let mut b = budget.bytes(size, source.name(), offset)?;
    source.read_exact_at(offset, &mut b)?;
    Ok(b)
}
pub struct Cursor<'a> {
    pub source: &'a dyn ReadAt,
    pub pos: u64,
    pub end: u64,
}
impl<'a> Cursor<'a> {
    pub fn new(source: &'a dyn ReadAt) -> Self {
        Self {
            source,
            pos: 0,
            end: source.len(),
        }
    }
    pub fn at(source: &'a dyn ReadAt, pos: u64, end: u64) -> Result<Self> {
        range(pos, 0, end, source.name())?;
        range(end, 0, source.len(), source.name())?;
        Ok(Self { source, pos, end })
    }
    pub fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        range(self.pos, N as u64, self.end, self.source.name())?;
        let mut b = [0; N];
        self.source.read_exact_at(self.pos, &mut b)?;
        self.pos = add(self.pos, N as u64, self.source.name(), self.pos)?;
        Ok(b)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take::<1>()?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    /// Preserve all IEEE encodings. Numeric policies belong to consumers.
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub fn bytes(&mut self, n: u64, l: &Limits, b: &Budget) -> Result<Vec<u8>> {
        range(self.pos, n, self.end, self.source.name())?;
        let v = read_bytes_inside_operation(self.source, self.pos, n, l, b)?;
        self.pos = add(self.pos, n, self.source.name(), self.pos)?;
        Ok(v)
    }
}
/// Exact, bounded zlib stream. Rejects short/long output, missing checksum and trailing input.
pub fn zlib(
    input: &[u8],
    declared: u64,
    l: &Limits,
    b: &Budget,
    file: &str,
    offset: u64,
) -> Result<Vec<u8>> {
    use flate2::{Decompress, FlushDecompress, Status};
    limit(declared, l.max_payload, file, offset)?;
    let mut out = Vec::new();
    let mut decoder = Decompress::new(true);
    let mut input_pos = 0usize;
    loop {
        let mut chunk = [0u8; 32 * 1024];
        let before_out = decoder.total_out();
        let before_in = decoder.total_in();
        let room = add(declared, 1, file, offset)?
            .saturating_sub(before_out)
            .min(chunk.len() as u64) as usize;
        let status = decoder
            .decompress(
                &input[input_pos..],
                &mut chunk[..room],
                FlushDecompress::None,
            )
            .map_err(|e| {
                Error::new(
                    "InvalidCompression",
                    file,
                    offset,
                    "complete zlib stream",
                    e,
                )
            })?;
        let n = (decoder.total_out() - before_out) as usize;
        input_pos = decoder.total_in() as usize;
        if decoder.total_out() > declared {
            return Err(Error::new(
                "DecompressedSizeMismatch",
                file,
                offset,
                declared,
                decoder.total_out(),
            ));
        }
        b.reserve_capped(&mut out, n, declared as usize, file, offset)?;
        out.extend_from_slice(&chunk[..n]);
        if status == Status::StreamEnd {
            if decoder.total_out() != declared || input_pos != input.len() {
                return Err(Error::new(
                    "DecompressedSizeMismatch",
                    file,
                    offset,
                    format!("{declared} output bytes and {} input bytes", input.len()),
                    format!("{} output; {input_pos} input", decoder.total_out()),
                ));
            }
            break;
        }
        if decoder.total_out() == before_out && decoder.total_in() == before_in {
            return Err(Error::new(
                "DecompressedSizeMismatch",
                file,
                offset,
                "complete stream with progress",
                "truncated or stalled stream",
            ));
        }
    }
    Ok(out)
}
pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn digest(source: &dyn ReadAt) -> Result<String> {
    source.check_stable()?;
    let mut hash = Sha256::new();
    let mut b = [0; 65536];
    let mut pos = 0;
    while pos < source.len() {
        let n = (source.len() - pos).min(b.len() as u64) as usize;
        source.read_exact_at(pos, &mut b[..n])?;
        hash.update(&b[..n]);
        pos = add(pos, n as u64, source.name(), pos)?;
    }
    source.check_stable()?;
    Ok(format!("{:x}", hash.finalize()))
}
/// Policy: ASCII fold and slash normalization, never collapse dot components.
pub fn path_key(path: &str) -> Result<String> {
    if path.is_empty() || !path.is_ascii() || path.contains(['\0', ':']) {
        return Err(Error::new(
            "InvalidPath",
            path,
            0,
            "nonempty ASCII relative path",
            path,
        ));
    }
    if path
        .split(['/', '\\'])
        .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(Error::new(
            "PathTraversal",
            path,
            0,
            "relative normal components",
            path,
        ));
    }
    let mut key = String::new();
    key.try_reserve_exact(path.len())
        .map_err(|e| Error::new("AllocationFailed", path, 0, path.len(), e))?;
    for c in path.bytes() {
        key.push(if c == b'\\' {
            '/'
        } else {
            c.to_ascii_lowercase() as char
        });
    }
    Ok(key)
}
pub fn within(root: &Path, relative: &str) -> Result<PathBuf> {
    path_key(relative)?;
    let root = root
        .canonicalize()
        .map_err(|e| io_error(&root.to_string_lossy(), 0, e))?;
    let p = root.join(relative.replace('\\', "/"));
    let resolved = p.canonicalize().map_err(|e| io_error(relative, 0, e))?;
    if !resolved.starts_with(&root) {
        return Err(Error::new(
            "PathTraversal",
            relative,
            0,
            "path inside root",
            resolved.display(),
        ));
    }
    Ok(resolved)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSpec {
    pub name: String,
    pub path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Mount {
    Archive {
        path: String,
        plugin: Option<String>,
    },
    Loose {
        base: Option<String>,
        files: Vec<String>,
        plugin: Option<String>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvalidationOrigin {
    ExplicitProfile,
    Ini { file: String, line: u32 },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveInvalidation {
    pub enabled: bool,
    pub origin: InvalidationOrigin,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Game {
    #[default]
    #[serde(rename = "fallout4")]
    Fallout4,
    #[serde(rename = "skyrim-se")]
    SkyrimSe,
}
impl Game {
    pub fn fixed_masters(self) -> &'static [&'static str] {
        match self {
            Self::Fallout4 => &[
                "Fallout4.esm",
                "DLCRobot.esm",
                "DLCworkshop01.esm",
                "DLCCoast.esm",
                "DLCworkshop02.esm",
                "DLCworkshop03.esm",
                "DLCNukaWorld.esm",
            ],
            Self::SkyrimSe => &[
                "Skyrim.esm",
                "Update.esm",
                "Dawnguard.esm",
                "HearthFires.esm",
                "Dragonborn.esm",
            ],
        }
    }
    pub fn creation_list(self) -> &'static str {
        match self {
            Self::Fallout4 => "Fallout4.ccc",
            Self::SkyrimSe => "Skyrim.ccc",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default)]
    pub game: Game,
    #[serde(default)]
    pub archive_invalidation: Option<ArchiveInvalidation>,
    pub root: PathBuf,
    pub language: String,
    pub plugins: Vec<PluginSpec>,
    pub mounts: Vec<Mount>,
    pub required_masters: Vec<String>,
    pub excluded: Vec<String>,
    #[serde(default)]
    pub limits: Limits,
    pub build: Option<String>,
}
impl Manifest {
    pub fn load(path: &Path) -> Result<Self> {
        let s = FileSource::open(path)?;
        limit(s.len(), 1 << 20, s.name(), 0)?;
        let data = read_bytes(&s, 0, s.len(), &Limits::default(), &Budget::new(2 << 20))?;
        let mut m: Self = serde_json::from_slice(&data)
            .map_err(|e| Error::new("InvalidManifest", s.name(), 0, "explicit JSON manifest", e))?;
        if m.root.is_relative() {
            m.root = path.parent().unwrap_or(Path::new(".")).join(&m.root);
        }
        m.validate(s.name())?;
        Ok(m)
    }
    pub fn validate(&self, file: &str) -> Result<()> {
        self.limits.validate(file)?;
        if let Some(ArchiveInvalidation {
            origin: InvalidationOrigin::Ini { file: ini, line },
            ..
        }) = &self.archive_invalidation
        {
            if ini.is_empty() || ini.contains('\0') || *line == 0 {
                return Err(Error::new(
                    "InvalidConfigOrigin",
                    file,
                    0,
                    "INI path and 1-based line",
                    format!("{ini}:{line}"),
                ));
            }
        }
        if self.plugins.is_empty() {
            return Err(Error::new(
                "LoadOrderRequired",
                file,
                0,
                "explicit nonempty plugins",
                0,
            ));
        }
        limit(self.plugins.len() as u64, 4350, file, 0)?;
        limit(self.mounts.len() as u64, self.limits.max_entries, file, 0)?;
        if self.language.is_empty()
            || !self
                .language
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(Error::new(
                "InvalidLanguage",
                file,
                0,
                "language token",
                &self.language,
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        for p in &self.plugins {
            path_key(&p.path)?;
            let k = path_key(&p.name)?;
            if k.contains('/') || !names.insert(k) {
                return Err(Error::new(
                    "AmbiguousPath",
                    file,
                    0,
                    "unique plugin basename",
                    &p.name,
                ));
            }
        }
        for mount in &self.mounts {
            match mount {
                Mount::Archive { path, .. } => {
                    path_key(path)?;
                }
                Mount::Loose { base, files, .. } => {
                    if let Some(base) = base {
                        path_key(base)?;
                    }
                    limit(files.len() as u64, self.limits.max_entries, file, 0)?;
                    for p in files {
                        path_key(p)?;
                    }
                }
            }
        }
        Ok(())
    }
    pub fn active(&self, plugin: &Option<String>) -> bool {
        plugin
            .as_ref()
            .is_none_or(|n| self.plugins.iter().any(|p| p.name.eq_ignore_ascii_case(n)))
    }
}
#[cfg(test)]
mod tests;

/// Synthetic test directory, removed on unwind as well as normal completion.
#[doc(hidden)]
pub struct TestDir(std::path::PathBuf);
impl TestDir {
    pub fn new(prefix: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = base.join("synthetic").join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create synthetic tempdir");
        Self(path.canonicalize().expect("canonical tempdir"))
    }
}
impl std::ops::Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("synthetic cleanup {}: {e}", self.0.display());
            }
        }
    }
}

/// Strict display/English decoder using the public Microsoft/Unicode mapping.
pub fn decode_cp1252(bytes: &[u8], file: &str, offset: u64, b: &Budget) -> Result<String> {
    let capacity = bytes
        .iter()
        .enumerate()
        .try_fold(0usize, |size, (i, &byte)| {
            let ch = if byte < 128 {
                byte as char
            } else {
                CP1252[(byte - 128) as usize].ok_or_else(|| {
                    Error::new(
                        "InvalidStringEncoding",
                        file,
                        offset + i as u64,
                        "defined Windows-1252 byte",
                        format!("0x{byte:02X}"),
                    )
                })?
            };
            size.checked_add(ch.len_utf8()).ok_or_else(|| {
                Error::new("Overflow", file, offset, "CP1252 UTF-8 size", bytes.len())
            })
        })?;
    b.charge(capacity as u64, file, offset)?;
    let mut text = String::new();
    text.try_reserve_exact(capacity)
        .map_err(|e| Error::new("AllocationFailed", file, offset, "decoded CP1252", e))?;
    for (i, &byte) in bytes.iter().enumerate() {
        let ch = if byte < 128 {
            byte as char
        } else {
            CP1252[(byte - 128) as usize].ok_or_else(|| {
                Error::new(
                    "InvalidStringEncoding",
                    file,
                    offset + i as u64,
                    "defined Windows-1252 byte",
                    format!("0x{byte:02X}"),
                )
            })?
        };
        text.push(ch);
    }
    Ok(text)
}
// Data mapping only: Unicode/Microsoft CP1252.TXT, no third-party implementation.
const CP1252: [Option<char>; 128] = [
    Some('\u{20ac}'),
    None,
    Some('\u{201a}'),
    Some('\u{192}'),
    Some('\u{201e}'),
    Some('\u{2026}'),
    Some('\u{2020}'),
    Some('\u{2021}'),
    Some('\u{2c6}'),
    Some('\u{2030}'),
    Some('\u{160}'),
    Some('\u{2039}'),
    Some('\u{152}'),
    None,
    Some('\u{17d}'),
    None,
    None,
    Some('\u{2018}'),
    Some('\u{2019}'),
    Some('\u{201c}'),
    Some('\u{201d}'),
    Some('\u{2022}'),
    Some('\u{2013}'),
    Some('\u{2014}'),
    Some('\u{2dc}'),
    Some('\u{2122}'),
    Some('\u{161}'),
    Some('\u{203a}'),
    Some('\u{153}'),
    None,
    Some('\u{17e}'),
    Some('\u{178}'),
    Some('\u{a0}'),
    Some('\u{a1}'),
    Some('\u{a2}'),
    Some('\u{a3}'),
    Some('\u{a4}'),
    Some('\u{a5}'),
    Some('\u{a6}'),
    Some('\u{a7}'),
    Some('\u{a8}'),
    Some('\u{a9}'),
    Some('\u{aa}'),
    Some('\u{ab}'),
    Some('\u{ac}'),
    Some('\u{ad}'),
    Some('\u{ae}'),
    Some('\u{af}'),
    Some('\u{b0}'),
    Some('\u{b1}'),
    Some('\u{b2}'),
    Some('\u{b3}'),
    Some('\u{b4}'),
    Some('\u{b5}'),
    Some('\u{b6}'),
    Some('\u{b7}'),
    Some('\u{b8}'),
    Some('\u{b9}'),
    Some('\u{ba}'),
    Some('\u{bb}'),
    Some('\u{bc}'),
    Some('\u{bd}'),
    Some('\u{be}'),
    Some('\u{bf}'),
    Some('\u{c0}'),
    Some('\u{c1}'),
    Some('\u{c2}'),
    Some('\u{c3}'),
    Some('\u{c4}'),
    Some('\u{c5}'),
    Some('\u{c6}'),
    Some('\u{c7}'),
    Some('\u{c8}'),
    Some('\u{c9}'),
    Some('\u{ca}'),
    Some('\u{cb}'),
    Some('\u{cc}'),
    Some('\u{cd}'),
    Some('\u{ce}'),
    Some('\u{cf}'),
    Some('\u{d0}'),
    Some('\u{d1}'),
    Some('\u{d2}'),
    Some('\u{d3}'),
    Some('\u{d4}'),
    Some('\u{d5}'),
    Some('\u{d6}'),
    Some('\u{d7}'),
    Some('\u{d8}'),
    Some('\u{d9}'),
    Some('\u{da}'),
    Some('\u{db}'),
    Some('\u{dc}'),
    Some('\u{dd}'),
    Some('\u{de}'),
    Some('\u{df}'),
    Some('\u{e0}'),
    Some('\u{e1}'),
    Some('\u{e2}'),
    Some('\u{e3}'),
    Some('\u{e4}'),
    Some('\u{e5}'),
    Some('\u{e6}'),
    Some('\u{e7}'),
    Some('\u{e8}'),
    Some('\u{e9}'),
    Some('\u{ea}'),
    Some('\u{eb}'),
    Some('\u{ec}'),
    Some('\u{ed}'),
    Some('\u{ee}'),
    Some('\u{ef}'),
    Some('\u{f0}'),
    Some('\u{f1}'),
    Some('\u{f2}'),
    Some('\u{f3}'),
    Some('\u{f4}'),
    Some('\u{f5}'),
    Some('\u{f6}'),
    Some('\u{f7}'),
    Some('\u{f8}'),
    Some('\u{f9}'),
    Some('\u{fa}'),
    Some('\u{fb}'),
    Some('\u{fc}'),
    Some('\u{fd}'),
    Some('\u{fe}'),
    Some('\u{ff}'),
];

/// Opaque archive key: only ASCII case/slashes change; high bytes stay distinct.
pub fn path_key_bytes(path: &[u8]) -> Result<Vec<u8>> {
    if path.is_empty() || path.contains(&0) || path.contains(&b':') {
        return Err(Error::new(
            "InvalidPath",
            "archive path",
            0,
            "nonempty relative bytes without NUL or colon",
            HexBytes(path),
        ));
    }
    if path
        .split(|b| *b == b'/' || *b == b'\\')
        .any(|p| p.is_empty() || p == b"." || p == b"..")
    {
        return Err(Error::new(
            "PathTraversal",
            "archive path",
            0,
            "nonempty normal components",
            HexBytes(path),
        ));
    }
    let mut key = Vec::new();
    key.try_reserve_exact(path.len())
        .map_err(|e| Error::new("AllocationFailed", "archive path", 0, path.len(), e))?;
    key.extend(path.iter().map(|b| {
        if *b == b'\\' {
            b'/'
        } else {
            b.to_ascii_lowercase()
        }
    }));
    Ok(key)
}
pub struct HexBytes<'a>(pub &'a [u8]);
impl std::fmt::Display for HexBytes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
pub fn serialize_hex<S: serde::Serializer>(
    bytes: &[u8],
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    s.collect_str(&HexBytes(bytes))
}
pub fn serialize_optional_hex<S: serde::Serializer>(
    bytes: &Option<Vec<u8>>,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    match bytes {
        Some(b) => s.serialize_some(&HexDisplay(b)),
        None => s.serialize_none(),
    }
}
struct HexDisplay<'a>(&'a [u8]);
impl serde::Serialize for HexDisplay<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        serialize_hex(self.0, s)
    }
}

/// IEEE 754 binary16 conversion, including signed zero, subnormals, infinity and NaN.
pub fn half_to_f32(v: u16) -> f32 {
    let sign = ((v as u32) & 0x8000) << 16;
    let exponent = ((v >> 10) & 31) as i32;
    let fraction = (v & 1023) as u32;
    if exponent == 0 {
        if fraction == 0 {
            f32::from_bits(sign)
        } else {
            let shift = fraction.leading_zeros() - 21;
            f32::from_bits(sign | ((113 - shift) << 23) | ((fraction << (shift + 13)) & 0x7fffff))
        }
    } else if exponent == 31 {
        f32::from_bits(sign | 0x7f800000 | (fraction << 13))
    } else {
        f32::from_bits(sign | (((exponent + 112) as u32) << 23) | (fraction << 13))
    }
}

pub mod ieee;
