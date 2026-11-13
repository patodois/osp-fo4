use crate::Reports;
use fo4_core::{add, limit, mul, path_key_bytes, Budget, Result};
use fo4_vfs::Vfs;
use serde::Serialize;
#[derive(Default, Serialize)]
pub struct Counts {
    pub cited: u64,
    pub unchecked: u64,
    pub unresolved_details_omitted: u64,
    pub empty: u64,
    pub present: u64,
    pub present_with_class_prefix: u64,
    pub missing: u64,
    pub invalid: u64,
    pub blocked: u64,
    pub unique_unresolved: u64,
    pub files_not_fully_checked: u64,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    Material,
    Texture,
}
impl Target {
    fn root(self) -> &'static [u8] {
        match self {
            Self::Material => b"materials/",
            Self::Texture => b"textures/",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Material => "material",
            Self::Texture => "texture",
        }
    }
}
struct Missing {
    target: Target,
    code: &'static str,
    path: Vec<u8>,
    count: u64,
}
pub struct Census {
    missing: Vec<Missing>,
    budget: Budget,
    max_entries: u64,
}
impl Census {
    pub fn new(max_entries: u64, max_memory: u64) -> Self {
        Self {
            missing: Vec::new(),
            budget: Budget::new(max_memory),
            max_entries,
        }
    }
    pub fn check(
        &mut self,
        v: &Vfs,
        target: Target,
        raw: &[u8],
        r: &mut Reports,
        work: &Budget,
    ) -> Result<()> {
        let counts = match target {
            Target::Material => &mut r.summary.material_references,
            Target::Texture => &mut r.summary.texture_references,
        };
        if raw.is_empty() {
            counts.empty += 1;
            return Ok(());
        }
        counts.cited += 1;
        counts.unchecked += 1;
        work.charge(
            add(
                512,
                mul(raw.len() as u64, 8, "reference", 0)?,
                "reference",
                0,
            )?,
            "reference",
            0,
        )?;
        let normalized = path_key_bytes(raw);
        let mut key = Vec::new();
        let capacity = raw.len().checked_add(target.root().len()).ok_or_else(|| {
            fo4_core::Error::new(
                "Overflow",
                "reference",
                0,
                "path length addition",
                raw.len(),
            )
        })?;
        work.reserve(&mut key, capacity, "reference", 0)?;
        let mut code = "MissingReferencedAsset";
        if let Ok(normalized) = normalized {
            key.extend_from_slice(&normalized);
            work.charge(
                add(
                    512,
                    mul(key.len() as u64, 8, "reference", 0)?,
                    "reference",
                    0,
                )?,
                "reference",
                0,
            )?;
            match v.winner_bytes(&key) {
                Ok(_) => {
                    counts.present += 1;
                    counts.unchecked -= 1;
                    return Ok(());
                }
                Err(e) if e.code == "MissingPath" => {}
                Err(e) if e.code == "WinnerUnsupported" => {
                    code = "BlockedReferencedAsset";
                }
                Err(_) => {
                    code = "InvalidReferencedAsset";
                }
            }
            // Explicit census policy: try a resource-class relative path only after exact lookup misses.
            // This does not repair traversal/absolute paths or select a shadowed fallback winner.
            if code == "MissingReferencedAsset" && !normalized.starts_with(target.root()) {
                key.clear();
                key.extend_from_slice(target.root());
                key.extend_from_slice(&normalized);
                work.charge(
                    add(
                        512,
                        mul(key.len() as u64, 8, "reference", 0)?,
                        "reference",
                        0,
                    )?,
                    "reference",
                    0,
                )?;
                match v.winner_bytes(&key) {
                    Ok(_) => {
                        counts.present += 1;
                        counts.unchecked -= 1;
                        counts.present_with_class_prefix += 1;
                        return Ok(());
                    }
                    Err(e) if e.code == "MissingPath" => {}
                    Err(e) if e.code == "WinnerUnsupported" => code = "BlockedReferencedAsset",
                    Err(_) => code = "InvalidReferencedAsset",
                }
            }
        } else {
            key.extend_from_slice(raw);
            code = "InvalidReferencedAsset";
        }
        counts.unchecked -= 1;
        match code {
            "MissingReferencedAsset" => counts.missing += 1,
            "BlockedReferencedAsset" => counts.blocked += 1,
            _ => counts.invalid += 1,
        }
        match self.missing.binary_search_by(|v| {
            (v.target, v.code, v.path.as_slice()).cmp(&(target, code, key.as_slice()))
        }) {
            Ok(i) => self.missing[i].count = add(self.missing[i].count, 1, "references", 0)?,
            Err(i) => {
                let stored = (|| -> Result<()> {
                    limit(
                        self.missing.len() as u64 + 1,
                        self.max_entries,
                        "references",
                        0,
                    )?;
                    self.budget.reserve(&mut self.missing, 1, "references", 0)?;
                    self.budget.charge(key.capacity() as u64, "references", 0)
                })();
                if let Err(error) = stored {
                    counts.unresolved_details_omitted += 1;
                    return Err(error);
                }
                self.missing.insert(
                    i,
                    Missing {
                        target,
                        code,
                        path: key,
                        count: 1,
                    },
                );
                counts.unique_unresolved += 1;
            }
        }
        Ok(())
    }
    pub fn finish(self, r: &mut Reports) -> Result<()> {
        for m in &self.missing {
            r.reference_notice(m.target.name(), &m.path, m.count, m.code)?;
        }
        r.summary.other_memory = add(r.summary.other_memory, self.budget.used(), "references", 0)?;
        Ok(())
    }
}
