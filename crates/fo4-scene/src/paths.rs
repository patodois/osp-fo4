//! Citation-specific roots. Sources and corpus measurements: spec/citation-roots.md.
use fo4_core::{path_key_bytes, Budget, Error, Result};
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Citation {
    RecordModel,
    SwapMaterial,
    TextureSetTexture,
    TextureSetMaterial,
    MaterialTexture,
    MaterialParent,
    NifMaterial,
    NifRootMaterial,
    NifTexture,
}
impl Citation {
    pub fn asset_kind(self) -> crate::AssetKind {
        match self {
            Self::RecordModel => crate::AssetKind::Model,
            Self::TextureSetTexture | Self::MaterialTexture | Self::NifTexture => {
                crate::AssetKind::Texture
            }
            _ => crate::AssetKind::Material,
        }
    }
    pub fn rule(self) -> (&'static str, &'static [u8], bool) {
        match self {
            Self::RecordModel => ("fo4_modl_meshes_relative_v1", b"meshes", true),
            Self::SwapMaterial => ("fo4_mswp_materials_relative_v1", b"materials", true),
            Self::TextureSetTexture => ("fo4_txst_textures_relative_v1", b"textures", true),
            Self::TextureSetMaterial => ("fo4_txst_materials_relative_v1", b"materials", true),
            Self::MaterialTexture => ("fo4_material_textures_relative_v1", b"textures", true),
            Self::MaterialParent => ("fo4_parent_materials_relative_v1", b"materials", true),
            Self::NifRootMaterial => ("fo4_nif_root_materials_relative_v1", b"materials", true),
            Self::NifMaterial => ("fo4_nif_material_data_relative_v1", b"materials", false),
            Self::NifTexture => ("fo4_nif_texture_data_relative_v1", b"textures", false),
        }
    }
}
pub fn resolve(citation: Citation, raw: &[u8], b: &Budget) -> Result<Vec<u8>> {
    let (rule, root, relative) = citation.rule();
    b.charge(raw.len() as u64, "citation path", 0)?;
    let normalized = path_key_bytes(raw).map_err(|e| {
        Error::new(
            "RejectedVfsPath",
            "citation path",
            0,
            format!("{rule}; {}", e.code),
            fo4_core::HexBytes(raw),
        )
    })?;
    let first = normalized.split(|v| *v == b'/').next().unwrap_or(&[]);
    if first == b"data" || relative && first == root || !relative && first != root {
        return Err(Error::new(
            "RejectedVfsPath",
            "citation path",
            0,
            rule,
            fo4_core::HexBytes(raw),
        ));
    }
    let mut key = Vec::new();
    let capacity = normalized
        .len()
        .checked_add(if relative { root.len() + 1 } else { 0 })
        .ok_or_else(|| {
            Error::new(
                "Overflow",
                "citation path",
                0,
                "root + path",
                normalized.len(),
            )
        })?;
    b.reserve(&mut key, capacity, "citation path", 0)?;
    if relative {
        key.extend_from_slice(root);
        key.push(b'/');
    }
    key.extend_from_slice(&normalized);
    Ok(key)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn class_roots_are_explicit_and_never_strip_authored_prefixes() {
        let b = Budget::new(1 << 20);
        for c in [
            Citation::RecordModel,
            Citation::SwapMaterial,
            Citation::TextureSetTexture,
            Citation::TextureSetMaterial,
            Citation::MaterialTexture,
            Citation::MaterialParent,
            Citation::NifRootMaterial,
        ] {
            let (_, root, _) = c.rule();
            let mut expected = root.to_vec();
            expected.extend_from_slice(b"/folder/a.nif");
            assert_eq!(resolve(c, b"Folder\\A.NIF", &b).unwrap(), expected);
            let mut wrong = root.to_vec();
            wrong.extend_from_slice(b"/a.nif");
            assert_eq!(resolve(c, &wrong, &b).unwrap_err().code, "RejectedVfsPath");
        }
        assert_eq!(
            resolve(Citation::NifMaterial, b"Materials\\A.BGSM", &b).unwrap(),
            b"materials/a.bgsm"
        );
        assert!(resolve(Citation::NifMaterial, b"a.bgsm", &b).is_err());
        for p in [
            b"data/meshes/a.nif".as_slice(),
            b"C:/dev/a.nif",
            b"/meshes/a.nif",
            b"../a.nif",
        ] {
            assert!(resolve(Citation::RecordModel, p, &b).is_err());
        }
        assert!(resolve(Citation::RecordModel, b"a.nif", &Budget::new(1)).is_err());
    }
}

/// Prospective transformations only; existence and explicit VFS priority are checked separately.
#[derive(Debug, serde::Serialize)]
pub struct CandidatePath {
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub key: Vec<u8>,
    pub hypotheses: Vec<&'static str>,
}
pub fn candidate(citation: Citation, raw: &[u8], b: &Budget) -> Result<CandidatePath> {
    let (_, root, _) = citation.rule();
    let mut path = Vec::new();
    b.reserve(&mut path, raw.len(), "path hypothesis", 0)?;
    path.extend(raw.iter().map(|v| {
        if *v == b'\\' {
            b'/'
        } else {
            v.to_ascii_lowercase()
        }
    }));
    let mut hypotheses = Vec::new();
    let absolute = path.len() >= 3 && path[0].is_ascii_alphabetic() && path[1..3] == *b":/";
    let tail = if absolute {
        let mut end = None;
        let mut at = 0;
        for part in path.split(|v| *v == b'/') {
            if part == b"data" {
                end = Some(at + part.len() + 1);
            }
            at += part.len() + 1;
        }
        let start = end.filter(|n| *n < path.len()).ok_or_else(|| {
            Error::new(
                "PathCandidateUnavailable",
                "path hypothesis",
                0,
                "absolute prefix with complete data component",
                fo4_core::HexBytes(raw),
            )
        })?;
        b.reserve(&mut hypotheses, 1, "path hypothesis", 0)?;
        hypotheses.push(crate::hypotheses::PATH_STRIP);
        &path[start..]
    } else if path.starts_with(b"data/") {
        b.reserve(&mut hypotheses, 1, "path hypothesis", 0)?;
        hypotheses.push(crate::hypotheses::PATH_DATA_RELATIVE);
        &path[5..]
    } else {
        path.as_slice()
    };
    let tail = path_key_bytes(tail)?;
    let first = tail.split(|v| *v == b'/').next().unwrap_or(&[]);
    let missing = first != root;
    if missing && [b"data".as_slice(), b"meshes", b"materials", b"textures"].contains(&first) {
        return Err(Error::new(
            "PathCandidateWrongRoot",
            "path hypothesis",
            0,
            String::from_utf8_lossy(root),
            fo4_core::HexBytes(raw),
        ));
    }
    let mut key = Vec::new();
    b.reserve(
        &mut key,
        tail.len()
            .checked_add(root.len() + 1)
            .ok_or_else(|| Error::new("Overflow", "path hypothesis", 0, "root+path", tail.len()))?,
        "path hypothesis",
        0,
    )?;
    if missing {
        b.reserve(&mut hypotheses, 1, "path hypothesis", 0)?;
        hypotheses.push(crate::hypotheses::PATH_ROOT);
        key.extend_from_slice(root);
        key.push(b'/');
    }
    key.extend_from_slice(&tail);
    if hypotheses.is_empty() {
        return Err(Error::new(
            "PathCandidateUnavailable",
            "path hypothesis",
            0,
            "one authorized transformation",
            fo4_core::HexBytes(raw),
        ));
    }
    Ok(CandidatePath { key, hypotheses })
}
#[derive(Debug, serde::Serialize)]
pub struct ResolvedPath {
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub key: Vec<u8>,
    pub hypotheses: Vec<&'static str>,
    pub original_rejection: Option<Error>,
}
pub fn resolve_with(
    c: Citation,
    raw: &[u8],
    v: &fo4_vfs::Vfs,
    policy: crate::PathPolicy,
    b: &Budget,
) -> Result<ResolvedPath> {
    fo4_core::limit(raw.len() as u64, v.limits().max_string, "citation path", 0)?;
    let original = match resolve(c, raw, b) {
        Ok(key) => {
            return Ok(ResolvedPath {
                key,
                hypotheses: Vec::new(),
                original_rejection: None,
            })
        }
        Err(e) if e.code == "RejectedVfsPath" => e,
        Err(e) => return Err(e),
    };
    if policy == crate::PathPolicy::Strict {
        return Err(original);
    }
    let proposal = match candidate(c, raw, b) {
        Ok(p) => p,
        Err(e)
            if matches!(
                e.code.as_str(),
                "AllocationFailed" | "LimitExceeded" | "Overflow"
            ) =>
        {
            return Err(e)
        }
        Err(_) => return Err(original),
    };
    // No basename search, arbitrary disk access, or fallback to a shadowed candidate.
    if v.winner_bytes(&proposal.key).is_err() {
        return Err(original);
    }
    Ok(ResolvedPath {
        key: proposal.key,
        hypotheses: proposal.hypotheses,
        original_rejection: Some(original),
    })
}
impl ResolvedPath {
    pub fn diagnostics(&self, file: &str, offset: u64, b: &Budget) -> Result<Vec<Error>> {
        let mut out = Vec::new();
        for h in &self.hypotheses {
            b.reserve(&mut out, 1, file, offset)?;
            out.push(Error::new(
                if *h == crate::hypotheses::PATH_STRIP {
                    "HypothesisAssetDataPrefixStrip"
                } else if *h == crate::hypotheses::PATH_DATA_RELATIVE {
                    "HypothesisAssetDataRelativeStrip"
                } else {
                    "HypothesisAssetMissingRoot"
                },
                file,
                offset,
                "unique existing VFS winner; runtime path rule unverified",
                format!(
                    "original={}; normalized={}",
                    self.original_rejection
                        .as_ref()
                        .map_or("", |e| e.found.as_str()),
                    fo4_core::HexBytes(&self.key)
                ),
            ));
        }
        Ok(out)
    }
}
