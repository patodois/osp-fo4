#![forbid(unsafe_code)]
//! Explicit mount order, normalized lookup, winner/loser provenance.
/// This thread's archive read and inflate time (measurement only).
pub use fo4_archive::clock as archive_clock;
use fo4_archive::GameArchive;
use fo4_core::{
    digest, digest_bytes, limit, path_key_bytes, read_bytes, within, Budget, Error, FileSource,
    Limits, Manifest, Mount, ReadAt, Result,
};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};

pub struct MountedArchive {
    _allocation: Budget,
    pub path: String,
    pub source: Arc<FileSource>,
    pub index: GameArchive,
}
/// A content digest the mount stamp refers to but has not computed yet.
enum PendingDigest {
    Archive(Arc<FileSource>),
    Loose(PathBuf),
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Location {
    Archive { archive: usize, entry: usize },
    Loose { path: PathBuf },
}
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    #[serde(
        rename = "key_hex",
        serialize_with = "fo4_core::serialize_optional_hex"
    )]
    pub key: Option<Vec<u8>>,
    pub blocked: bool,
    pub original: String,
    pub original_encoding: &'static str,
    pub layer: usize,
    pub location: Location,
}
pub struct Vfs {
    allocation: Budget,
    pub diagnostics: Vec<Error>,
    pub incomplete: bool,
    incomplete_source: Option<fo4_core::SourceSpan>,
    archive_invalidation: bool,
    pub archives: Vec<MountedArchive>,
    pub candidates: Vec<Candidate>,
    pub inactive_mounts: Vec<usize>,
    /// Mount stamp with zeroed 64-byte slots for the content digests in `pending`.
    stamp: Vec<u8>,
    pending: Vec<(usize, PendingDigest)>,
    fingerprint: std::cell::OnceCell<String>,
    limits: Limits,
}
impl Vfs {
    pub fn mount(m: &Manifest, b: &Budget) -> Result<Self> {
        m.validate("manifest")?;
        let allocation = b.child();
        let b = &allocation;
        let l = &m.limits;
        let mut v = Self {
            diagnostics: Vec::new(),
            incomplete: false,
            incomplete_source: None,
            archive_invalidation: m.archive_invalidation.as_ref().is_some_and(|c| c.enabled),
            archives: Vec::new(),
            candidates: Vec::new(),
            inactive_mounts: Vec::new(),
            stamp: Vec::new(),
            pending: Vec::new(),
            fingerprint: std::cell::OnceCell::new(),
            limits: l.clone(),
            allocation: b.child(),
        };
        let mut stamp = Vec::new();
        b.reserve(&mut stamp, 64, "manifest", 0)?;
        stamp.extend_from_slice(b"bethesda-vfs:assets:8\0");
        append(
            &mut stamp,
            match m.game {
                fo4_core::Game::Fallout4 => b"fallout4",
                fo4_core::Game::SkyrimSe => b"skyrim-se",
            },
            b,
        )?;
        append(&mut stamp, &[v.archive_invalidation as u8], b)?;
        append(&mut stamp, m.language.as_bytes(), b)?;
        for p in &m.plugins {
            append(&mut stamp, p.name.as_bytes(), b)?;
        }
        for (layer, mount) in m.mounts.iter().enumerate() {
            let plugin = match mount {
                Mount::Archive { plugin, .. } | Mount::Loose { plugin, .. } => plugin,
            };
            if !m.active(plugin) {
                b.reserve(&mut v.inactive_mounts, 1, "manifest", 0)?;
                v.inactive_mounts.push(layer);
                continue;
            }
            append(&mut stamp, &layer.to_le_bytes(), b)?;
            let before = v.candidates.len();
            match mount {
                Mount::Archive { path, .. } => {
                    let archive_allocation = b.child();
                    let attempt = (|| -> Result<_> {
                        let source = Arc::new(FileSource::open(&within(&m.root, path)?)?);
                        append(&mut stamp, path.as_bytes(), b)?;
                        let slot = append_digest_slot(&mut stamp, b)?;
                        b.reserve(&mut v.pending, 1, path, 0)?;
                        v.pending
                            .push((slot, PendingDigest::Archive(Arc::clone(&source))));
                        let index =
                            GameArchive::read(m.game, source.as_ref(), l, &archive_allocation)?;
                        Ok((source, index))
                    })();
                    let (source, index) = match attempt {
                        Ok(value) => value,
                        Err(error) => {
                            drop(archive_allocation);
                            b.reserve(&mut v.diagnostics, 1, path, 0)?;
                            b.charge(512 + error.found.len() as u64, path, 0)?;
                            v.incomplete = true;
                            v.incomplete_source
                                .get_or_insert_with(|| error.source.as_ref().clone());
                            v.diagnostics.push(error);
                            continue;
                        }
                    };
                    let a = v.archives.len();
                    limit(
                        (v.candidates.len() + index.len()) as u64,
                        l.max_entries,
                        path,
                        0,
                    )?;
                    b.reserve(&mut v.candidates, index.len(), path, 0)?;
                    for entry in 0..index.len() {
                        let e = index.entry(entry)?;
                        for diagnostic in e.diagnostics {
                            b.reserve(&mut v.diagnostics, 1, path, e.source.offset)?;
                            b.charge(512 + diagnostic.found.len() as u64, path, e.source.offset)?;
                            v.diagnostics.push(diagnostic.clone());
                        }

                        if e.key.is_empty() && !e.diagnostics.is_empty() {
                            v.incomplete = true;
                            v.incomplete_source.get_or_insert_with(|| {
                                let mut source = (*e.source).clone();
                                source.archive_entry = Some(e.name.clone());
                                source
                            });
                        }
                        b.charge((e.key.len() + e.name.len()) as u64, path, e.source.offset)?;
                        v.candidates.push(Candidate {
                            key: if e.key.is_empty() {
                                None
                            } else {
                                Some(e.key.clone())
                            },
                            blocked: !e.diagnostics.is_empty(),
                            original: e.name.clone(),
                            original_encoding: e.name_encoding,
                            layer,
                            location: Location::Archive { archive: a, entry },
                        });
                    }
                    b.reserve(&mut v.archives, 1, path, 0)?;
                    v.archives.push(MountedArchive {
                        _allocation: archive_allocation,
                        path: path.clone(),
                        source,
                        index,
                    });
                }
                Mount::Loose { base, files, .. } => {
                    let layer_root = if let Some(base) = base {
                        append(&mut stamp, base.as_bytes(), b)?;
                        within(&m.root, base)?
                    } else {
                        m.root.clone()
                    };
                    limit(
                        (v.candidates.len() + files.len()) as u64,
                        l.max_entries,
                        "manifest",
                        0,
                    )?;
                    b.reserve(&mut v.candidates, files.len(), "manifest", 0)?;
                    for path in files {
                        let resolved = within(&layer_root, path)?;
                        // Opening validates the declared file now; its bytes are hashed on demand.
                        FileSource::open(&resolved)?;
                        append(&mut stamp, path.as_bytes(), b)?;
                        let slot = append_digest_slot(&mut stamp, b)?;
                        b.charge(
                            (path.len() * 2 + resolved.as_os_str().len() * 2) as u64,
                            path,
                            0,
                        )?;
                        b.reserve(&mut v.pending, 1, path, 0)?;
                        v.pending
                            .push((slot, PendingDigest::Loose(resolved.clone())));
                        v.candidates.push(Candidate {
                            key: Some(path_key_bytes(path.as_bytes())?),
                            blocked: false,
                            original: path.clone(),
                            original_encoding: "ASCII",
                            layer,
                            location: Location::Loose { path: resolved },
                        });
                    }
                }
            }
            // Same layer has no implied ordering between equivalent names.
            v.candidates[before..].sort_by(|a, b| a.key.cmp(&b.key));
            for pair in v.candidates[before..].windows(2) {
                if pair[0].key.is_some()
                    && !pair[0].blocked
                    && !pair[1].blocked
                    && pair[0].key == pair[1].key
                    && v.hash_indexed_candidate(&pair[0])? != v.hash_indexed_candidate(&pair[1])?
                {
                    return Err(Error::new(
                        "AmbiguousPath",
                        &pair[1].original,
                        0,
                        "same bytes for equivalent names in one layer",
                        &pair[0].original,
                    ));
                }
            }
        }
        v.candidates.sort_by(|a, b| {
            a.key
                .cmp(&b.key)
                .then_with(|| {
                    if v.archive_invalidation {
                        matches!(a.location, Location::Loose { .. })
                            .cmp(&matches!(b.location, Location::Loose { .. }))
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .then(a.layer.cmp(&b.layer))
                .then(a.original.cmp(&b.original))
        });
        append(&mut stamp, &[v.incomplete as u8], b)?;
        if let Some(source) = &v.incomplete_source {
            append(&mut stamp, source.file.as_bytes(), b)?;
        }
        v.stamp = stamp;
        v.allocation = allocation;
        Ok(v)
    }
    /// SHA-256 over the mount stamp: game, invalidation, language, plugin names, mount
    /// order and the content digest of every mounted archive and loose file.
    ///
    /// Hashing reads every mounted byte, so it runs on first use, not at mount time.
    pub fn fingerprint(&self) -> Result<&str> {
        if let Some(value) = self.fingerprint.get() {
            return Ok(value);
        }
        let work = self.allocation.child();
        let mut stamp = Vec::new();
        work.reserve(&mut stamp, self.stamp.len(), "manifest", 0)?;
        stamp.extend_from_slice(&self.stamp);
        for (slot, pending) in &self.pending {
            let hex = match pending {
                PendingDigest::Archive(source) => digest(source.as_ref())?,
                PendingDigest::Loose(path) => digest(&FileSource::open(path)?)?,
            };
            stamp[*slot..*slot + DIGEST_HEX].copy_from_slice(hex.as_bytes());
        }
        Ok(self.fingerprint.get_or_init(|| digest_bytes(&stamp)))
    }
    pub fn check_complete(&self) -> Result<()> {
        if self.incomplete {
            let source = self.incomplete_source.clone();
            let mut error = Error::new(
                "VfsIncomplete",
                "VFS",
                0,
                "all declared archives indexed with usable names",
                "archive unavailable or blocked entry without usable name",
            );
            if let Some(source) = source {
                error.source = Box::new(source);
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn chain(&self, path: &str) -> Result<&[Candidate]> {
        self.chain_bytes(path.as_bytes())
    }
    pub fn chain_bytes(&self, path: &[u8]) -> Result<&[Candidate]> {
        self.check_complete()?;
        let key = path_key_bytes(path)?;
        let first = self
            .candidates
            .partition_point(|c| c.key.as_deref() < Some(key.as_slice()));
        let last = self
            .candidates
            .partition_point(|c| c.key.as_deref() <= Some(key.as_slice()));
        let chain = &self.candidates[first..last];
        if !self.archive_invalidation
            && chain
                .iter()
                .any(|c| matches!(c.location, Location::Loose { .. }))
            && chain
                .iter()
                .any(|c| matches!(c.location, Location::Archive { .. }))
        {
            return Err(Error::new(
                "UnsupportedMountRule",
                &fo4_core::HexBytes(path).to_string(),
                0,
                "explicit enabled archive invalidation for loose/BA2 collision",
                "absent or disabled",
            ));
        }
        Ok(chain)
    }
    fn supported(&self, c: &Candidate) -> Result<()> {
        if c.blocked {
            if let Location::Archive { archive, entry } = c.location {
                let a = self.archives.get(archive).ok_or_else(|| {
                    Error::new("InvalidMount", &c.original, 0, "archive index", archive)
                })?;
                let e = a.index.entry(entry)?;
                let mut error = Error::new(
                    "WinnerUnsupported",
                    &a.path,
                    e.source.offset,
                    "supported highest-priority entry",
                    e.diagnostics.first().map_or("blocked", |e| e.code.as_str()),
                );
                error.source = Box::new((*e.source).clone());
                error.source.archive_entry = Some(e.name.clone());
                return Err(error);
            }
            return Err(Error::new(
                "WinnerUnsupported",
                &c.original,
                0,
                "supported candidate",
                "blocked",
            ));
        }
        Ok(())
    }
    pub fn winner(&self, path: &str) -> Result<&Candidate> {
        self.winner_bytes(path.as_bytes())
    }
    pub fn winner_bytes(&self, path: &[u8]) -> Result<&Candidate> {
        let winner = self.chain_bytes(path)?.last().ok_or_else(|| {
            Error::new(
                "MissingPath",
                &fo4_core::HexBytes(path).to_string(),
                0,
                "mounted asset",
                "absent",
            )
        })?;
        self.supported(winner)?;
        Ok(winner)
    }
    pub fn read_path_bytes(&self, path: &[u8], b: &Budget) -> Result<Vec<u8>> {
        self.read_candidate(self.winner_bytes(path)?, b)
    }
    /// Names/order plus archive/loose bytes only. This method never opens plugins.
    pub fn records_fingerprint(&self, plugins: &[(String, String)], b: &Budget) -> Result<String> {
        let mut stamp = Vec::new();
        append(&mut stamp, b"assets+plugins:8", b)?;
        append(&mut stamp, self.fingerprint()?.as_bytes(), b)?;
        for (name, hash) in plugins {
            append(&mut stamp, name.as_bytes(), b)?;
            append(&mut stamp, hash.as_bytes(), b)?;
        }
        Ok(digest_bytes(&stamp))
    }
    pub fn read(&self, path: &str, b: &Budget) -> Result<Vec<u8>> {
        self.read_candidate(self.winner(path)?, b)
    }
    pub fn read_candidate(&self, c: &Candidate, b: &Budget) -> Result<Vec<u8>> {
        self.check_complete()?;
        self.supported(c)?;
        self.read_indexed_candidate(c, b)
    }
    fn read_indexed_candidate(&self, c: &Candidate, b: &Budget) -> Result<Vec<u8>> {
        match &c.location {
            Location::Archive { archive, entry } => {
                let a = self.archives.get(*archive).ok_or_else(|| {
                    Error::new("InvalidMount", &c.original, 0, "archive index", archive)
                })?;
                a.index
                    .read_entry(a.source.as_ref(), *entry, &self.limits, b)
            }
            Location::Loose { path } => {
                let s = FileSource::open(path)?;
                read_bytes(&s, 0, s.len(), &self.limits, b)
            }
        }
    }
    /// Declared logical bytes; DX10 includes the reconstructed DDS header.
    pub fn payload_size(&self, c: &Candidate) -> Result<u64> {
        self.check_complete()?;
        match &c.location {
            Location::Archive { archive, entry } => self
                .archives
                .get(*archive)
                .ok_or_else(|| Error::new("InvalidMount", &c.original, 0, "archive", archive))?
                .index
                .payload_size(*entry),
            Location::Loose { path } => Ok(FileSource::open(path)?.len()),
        }
    }
    /// At most 4096 decoded bytes, with explicit whole-stream validation state.
    pub fn read_prefix(&self, c: &Candidate, max: u64, b: &Budget) -> Result<fo4_archive::Prefix> {
        self.check_complete()?;
        self.supported(c)?;
        limit(max, 4096, &c.original, 0)?;
        match &c.location {
            Location::Archive { archive, entry } => {
                let a = self.archives.get(*archive).ok_or_else(|| {
                    Error::new("InvalidMount", &c.original, 0, "archive", archive)
                })?;
                a.index
                    .read_prefix(a.source.as_ref(), *entry, max, &self.limits, b)
            }
            Location::Loose { path } => {
                let source = FileSource::open(path)?;
                let n = max.min(source.len());
                Ok(fo4_archive::Prefix {
                    bytes: read_bytes(&source, 0, n, &self.limits, b)?,
                    declared_size: source.len(),
                    full_stream_validated: n == source.len(),
                })
            }
        }
    }
    pub fn provenance(&self, c: &Candidate) -> Result<fo4_core::SourceSpan> {
        match &c.location {
            Location::Archive { archive, entry } => {
                let a = self.archives.get(*archive).ok_or_else(|| {
                    Error::new("InvalidMount", &c.original, 0, "archive", archive)
                })?;
                let mut span = a.index.entry(*entry)?.source.clone();
                span.archive_entry = Some(c.original.clone());
                Ok(span)
            }
            Location::Loose { path } => {
                let source = FileSource::open(path)?;
                Ok(fo4_core::SourceSpan::new(source.name(), 0, source.len()))
            }
        }
    }
    pub fn hash(&self, c: &Candidate) -> Result<String> {
        self.check_complete()?;
        self.supported(c)?;
        self.hash_indexed_candidate(c)
    }
    // Mount-time integrity check of known entries, even if other names are unavailable.
    fn hash_indexed_candidate(&self, c: &Candidate) -> Result<String> {
        match &c.location {
            Location::Loose { path } => digest(&FileSource::open(path)?),
            Location::Archive { .. } => Ok(digest_bytes(
                &self.read_indexed_candidate(c, &Budget::new(self.limits.max_memory))?,
            )),
        }
    }
    pub fn original_bytes<'a>(&'a self, c: &'a Candidate) -> Result<&'a [u8]> {
        match c.location {
            Location::Archive { archive, entry } => self
                .archives
                .get(archive)
                .ok_or_else(|| Error::new("InvalidMount", &c.original, 0, "archive index", archive))
                .and_then(|a| a.index.entry(entry).map(|e| e.original_name.as_slice())),
            Location::Loose { .. } => Ok(c.original.as_bytes()),
        }
    }
    pub fn memory_used(&self) -> u64 {
        self.allocation.used()
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
}
const DIGEST_HEX: usize = 64;
/// Reserves a framed slot for a hex SHA-256 and returns the offset of its 64 bytes.
fn append_digest_slot(v: &mut Vec<u8>, b: &Budget) -> Result<usize> {
    append(v, &[0; DIGEST_HEX], b)?;
    Ok(v.len() - DIGEST_HEX)
}
fn append(v: &mut Vec<u8>, bytes: &[u8], b: &Budget) -> Result<()> {
    let n = bytes
        .len()
        .checked_add(8)
        .ok_or_else(|| Error::new("Overflow", "manifest", 0, "stamp size", bytes.len()))?;
    b.reserve(v, n, "manifest", 0)?;
    v.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    v.extend_from_slice(bytes);
    Ok(())
}
#[cfg(test)]
mod tests;
