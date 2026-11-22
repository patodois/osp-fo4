//! Shared loading for the `.pex` examples: the manifest's mounts and every
//! `.pex` entry in them, winners and overridden copies.
use fo4_core::{Budget, Manifest};
use fo4_vfs::{Location, Vfs};
use std::path::Path;

#[allow(dead_code)]
pub struct Entry {
    /// Index into `vfs.candidates`.
    pub candidate: usize,
    /// Highest-priority candidate of its path.
    pub winner: bool,
    /// Mount group from the archive file name: base, dlc or cc (other).
    pub group: &'static str,
}

pub fn open(manifest: &str) -> Result<(Manifest, Vfs), Box<dyn std::error::Error>> {
    let m = Manifest::load(Path::new(manifest))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    Ok((m, vfs))
}

/// Every `.pex` candidate in key order.
#[allow(dead_code)]
pub fn entries(vfs: &Vfs) -> Vec<Entry> {
    let c = &vfs.candidates;
    let mut out = Vec::new();
    for (i, x) in c.iter().enumerate() {
        let Some(k) = &x.key else { continue };
        if !k.ends_with(b".pex") {
            continue;
        }
        let winner = c.get(i + 1).and_then(|n| n.key.as_ref()) != Some(k);
        let group = match &x.location {
            Location::Archive { archive, .. } => {
                let p = vfs.archives[*archive].path.to_ascii_lowercase();
                if p.starts_with("fallout4 - ") {
                    "base"
                } else if p.starts_with("dlc") {
                    "dlc"
                } else {
                    "other"
                }
            }
            Location::Loose { .. } => "loose",
        };
        out.push(Entry {
            candidate: i,
            winner,
            group,
        });
    }
    out
}
