//! Measurements only: raw and class-prefixed lookup, without changing either consumer.
use crate::Reports;
use fo4_core::{add, limit, Budget, Error, Manifest, Result};
use fo4_plugin::{Field, Value};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
#[derive(Default, Serialize)]
pub struct Counts {
    pub distinct: u64,
    pub citations: u64,
    pub as_is_only: u64,
    pub as_is_only_citations: u64,
    pub prefixed_only: u64,
    pub prefixed_only_citations: u64,
    pub both: u64,
    pub both_citations: u64,
    pub neither: u64,
    pub neither_citations: u64,
    pub starts_with_root: u64,
    pub starts_with_root_citations: u64,
    pub invalid: u64,
    pub empty: u64,
}
#[derive(Default, Serialize)]
pub struct Summary {
    pub classes: BTreeMap<String, Counts>,
    pub rejected_forms: BTreeMap<String, PathFormCounts>,
    pub empty_path_slots: u64,
    pub asset_attempted: u64,
    pub asset_read: u64,
    pub asset_failed: u64,
    pub records: u64,
}
#[derive(Default, Serialize)]
pub struct PathFormCounts {
    pub paths: u64,
    pub citations: u64,
    pub unique: u64,
    pub unique_citations: u64,
    pub none: u64,
    pub none_citations: u64,
    pub ambiguous: u64,
    pub ambiguous_citations: u64,
}
struct Path {
    class: &'static str,
    raw: Vec<u8>,
    count: u64,
}
struct Census {
    paths: Vec<Path>,
    index: HashMap<(&'static str, Vec<u8>), usize>,
    budget: Budget,
    max: u64,
}
impl Census {
    fn new(m: &Manifest) -> Self {
        Self {
            paths: Vec::new(),
            index: HashMap::new(),
            budget: Budget::new(m.limits.max_memory),
            max: m.limits.max_entries,
        }
    }
    fn cite(&mut self, class: &'static str, raw: &[u8]) -> Result<()> {
        // One bounded lookup allocation; no retention per occurrence.
        let work = self.budget.child();
        work.charge(raw.len() as u64, "root census", 0)?;
        let key = (class, raw.to_vec());
        if let Some(i) = self.index.get(&key) {
            self.paths[*i].count = add(self.paths[*i].count, 1, "root census", 0)?;
            return Ok(());
        }
        limit(self.paths.len() as u64 + 1, self.max, "root census", 0)?;
        self.budget
            .charge(raw.len() as u64 * 2 + 128, "root census", 0)?;
        self.index
            .try_reserve(1)
            .map_err(|e| Error::new("AllocationFailed", "root census", 0, "citation index", e))?;
        self.budget.reserve(&mut self.paths, 1, "root census", 0)?;
        self.index.insert(key, self.paths.len());
        self.paths.push(Path {
            class,
            raw: raw.to_vec(),
            count: 1,
        });
        Ok(())
    }
    fn finish(self, v: &Vfs, r: &mut Reports) -> Result<()> {
        for p in self.paths {
            let root = if p.class == "record_modl" {
                "meshes"
            } else if matches!(
                p.class,
                "txst_texture" | "material_texture" | "nif_texture_set" | "nif_effect_texture"
            ) {
                "textures"
            } else {
                "materials"
            };
            let normalized = fo4_core::path_key_bytes(&p.raw);
            let as_is = v.winner_bytes(&p.raw).is_ok();
            let mut prefixed = root.as_bytes().to_vec();
            prefixed.push(b'/');
            prefixed.extend_from_slice(&p.raw);
            let with_root = v.winner_bytes(&prefixed).is_ok();
            let starts = normalized
                .as_ref()
                .is_ok_and(|key| key.starts_with(format!("{root}/").as_bytes()));
            let c = r
                .summary
                .root_census
                .classes
                .entry(p.class.into())
                .or_default();
            c.distinct += 1;
            c.citations += p.count;
            if p.raw.is_empty() {
                c.empty += p.count;
            }
            if normalized.is_err() {
                c.invalid += p.count;
            }
            if starts {
                c.starts_with_root += 1;
                c.starts_with_root_citations += p.count;
            }
            let category = match (as_is, with_root) {
                (true, false) => {
                    c.as_is_only += 1;
                    c.as_is_only_citations += p.count;
                    "as_is_only"
                }
                (false, true) => {
                    c.prefixed_only += 1;
                    c.prefixed_only_citations += p.count;
                    "prefixed_only"
                }
                (true, true) => {
                    c.both += 1;
                    c.both_citations += p.count;
                    "both"
                }
                _ => {
                    c.neither += 1;
                    c.neither_citations += p.count;
                    "neither"
                }
            };
            r.item(&serde_json::json!({"kind":"root_citation","class":p.class,"path_hex":fo4_core::HexBytes(&p.raw).to_string(),"citations":p.count,"root":root,"resolution":category,"starts_with_root":starts}))?;
            use fo4_scene::paths::{candidate, resolve, Citation};
            let citation = match p.class {
                "record_modl" => Citation::RecordModel,
                "mswp_bnam" | "mswp_snam" => Citation::SwapMaterial,
                "txst_texture" => Citation::TextureSetTexture,
                "txst_material" => Citation::TextureSetMaterial,
                "material_texture" => Citation::MaterialTexture,
                "material_parent" => Citation::MaterialParent,
                "nif_material" => Citation::NifMaterial,
                "nif_root_material" => Citation::NifRootMaterial,
                _ => Citation::NifTexture,
            };
            if p.raw.is_empty() {
                r.summary.root_census.empty_path_slots += p.count;
                continue;
            }
            let work = self.budget.child();
            let strict = resolve(citation, &p.raw, &work);
            if strict.as_ref().is_ok_and(|key| v.winner_bytes(key).is_ok()) {
                continue;
            }
            let absolute = p.raw.len() >= 3
                && p.raw[0].is_ascii_alphabetic()
                && p.raw[1] == b':'
                && b"/\\".contains(&p.raw[2]);
            let proposal = match candidate(citation, &p.raw, &work) {
                Ok(v) => Some(v),
                Err(e)
                    if matches!(
                        e.code.as_str(),
                        "LimitExceeded" | "AllocationFailed" | "Overflow"
                    ) =>
                {
                    return Err(e)
                }
                Err(_) => None,
            };
            let form = if absolute {
                "absolute_drive"
            } else if proposal
                .as_ref()
                .is_some_and(|p| p.hypotheses.contains(&fo4_scene::hypotheses::PATH_ROOT))
            {
                "missing_root"
            } else if p.raw.contains(&b'\\') {
                "slashes"
            } else {
                "other"
            };
            let unique = proposal
                .as_ref()
                .is_some_and(|p| v.winner_bytes(&p.key).is_ok());
            let outcome = if unique { "unique" } else { "none" };
            let label = format!("{form}|{}|{}", citation.rule().0, p.class);
            let counts = r
                .summary
                .root_census
                .rejected_forms
                .entry(label)
                .or_default();
            counts.paths += 1;
            counts.citations += p.count;
            if unique {
                counts.unique += 1;
                counts.unique_citations += p.count;
            } else {
                counts.none += 1;
                counts.none_citations += p.count;
            }
            r.item(&serde_json::json!({"kind":"path_form","origin":p.class,"rule":citation.rule().0,"form":form,"raw":fo4_core::HexBytes(&p.raw).to_string(),"count":p.count,"strict_error":strict.err(),"candidate":proposal,"outcome":outcome,"candidate_keys":u8::from(unique)}))?;
        }
        Ok(())
    }
}
pub fn sweep(m: &Manifest, r: &mut Reports) -> Result<()> {
    r.items_enabled = true;
    let v = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    v.check_complete()?;
    r.summary.fingerprint = Some(v.fingerprint()?.to_owned());
    r.summary.fingerprint_scope = Some("assets".into());
    r.summary.vfs_memory = v.memory_used();
    let mut c = Census::new(m);
    for p in &m.plugins {
        let source = fo4_core::FileSource::open(&fo4_core::within(&m.root, &p.path)?)?;
        let work = Budget::new(m.limits.max_memory);
        let h = fo4_plugin::Header::read_for_game(m.game, &p.name, &source, &m.limits, &work)?;
        fo4_plugin::scan_bounded(&source, &h, &m.limits, &work, |record, _, budget| {
            r.summary.root_census.records += 1;
            let fields = record.fields(&m.limits, budget)?;
            for Field {
                signature, value, ..
            } in fields
            {
                if let Value::Text(path) = value {
                    let class = match (&record.signature, &signature) {
                        (_, b"MODL") => Some("record_modl"),
                        (b"MSWP", b"BNAM") => Some("mswp_bnam"),
                        (b"MSWP", b"SNAM") => Some("mswp_snam"),
                        (b"TXST", b"MNAM") => Some("txst_material"),
                        (
                            b"TXST",
                            b"TX00" | b"TX01" | b"TX02" | b"TX03" | b"TX04" | b"TX05" | b"TX06"
                            | b"TX07",
                        ) => Some("txst_texture"),
                        _ => None,
                    };
                    if let Some(class) = class {
                        c.cite(class, path.as_bytes())?;
                    }
                }
            }
            Ok(())
        })?;
    }
    for chain in v.candidates.chunk_by(|a, b| a.key == b.key) {
        let Some(w) = chain.last() else { continue };
        let Some(key) = &w.key else { continue };
        if !key.ends_with(b".nif") && !key.ends_with(b".bgsm") && !key.ends_with(b".bgem") {
            continue;
        }
        r.summary.root_census.asset_attempted += 1;
        let work = Budget::new(m.limits.max_memory);
        let result = (|| {
            let bytes = v.read_candidate(w, &work)?;
            if key.ends_with(b".nif") {
                let n = fo4_nif::Nif::read(&bytes, &w.original, m.game, &m.limits, &work)?;
                for block in &n.blocks {
                    let ty = &n.block_types[block.type_index as usize];
                    let fields = match &block.decoded {
                        Some(fo4_nif::Data::Fields(f)) => f,
                        _ => continue,
                    };
                    for field in fields {
                        match &field.value {
                            fo4_nif::Value::StringIndex(i)
                                if matches!(
                                    ty.as_str(),
                                    "BSLightingShaderProperty" | "BSEffectShaderProperty"
                                ) && matches!(field.name, "Name" | "Root Material") =>
                            {
                                if let Some(path) = n.strings.get(*i as usize) {
                                    c.cite(
                                        if field.name == "Name" {
                                            "nif_material"
                                        } else {
                                            "nif_root_material"
                                        },
                                        path,
                                    )?;
                                }
                            }
                            fo4_nif::Value::Bytes(path)
                                if ty == "BSShaderTextureSet" && field.name == "Texture" =>
                            {
                                c.cite("nif_texture_set", path)?
                            }
                            fo4_nif::Value::Bytes(path)
                                if ty == "BSEffectShaderProperty"
                                    && matches!(
                                        field.name,
                                        "Source Texture"
                                            | "Greyscale Texture"
                                            | "Environment Map Texture"
                                            | "Normal Texture"
                                            | "Environment Mask Texture"
                                    ) =>
                            {
                                c.cite("nif_effect_texture", path)?
                            }
                            _ => {}
                        }
                    }
                }
            } else {
                let mat = fo4_material::Material::read(&bytes, &w.original, &m.limits, &work)?;
                for (_, path) in mat.texture_paths() {
                    c.cite("material_texture", path.bytes())?;
                }
                if let fo4_material::Kind::Bgsm(b) = &mat.kind {
                    c.cite("material_parent", b.root_material_path.bytes())?;
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => r.summary.root_census.asset_read += 1,
            Err(e) => {
                r.summary.root_census.asset_failed += 1;
                r.error(&e)?;
            }
        }
    }
    c.finish(&v, r)
}
