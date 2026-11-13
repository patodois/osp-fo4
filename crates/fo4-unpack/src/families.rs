//! Original NIF citations, including superseded lists, without repairing paths.
use crate::Reports;
use fo4_core::{add, path_key_bytes, Budget, Game, Manifest, Result};
use fo4_scene::model_policy::{MaterialStatus, ModelPolicy};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default, Serialize)]
pub struct Counts {
    pub citations: u64,
    pub distinct_paths: u64,
    pub suppressed: u64,
}
#[derive(Default, Serialize)]
pub struct Summary {
    pub attempted: u64,
    pub read: u64,
    pub failed: u64,
    pub bare_citations: u64,
    pub bare_distinct_paths: u64,
    pub bare_suppressed: u64,
    pub bare_textures: BTreeMap<String, Counts>,
    pub shapes_by_material: BTreeMap<String, u64>,
}
pub fn bare(raw: &[u8]) -> bool {
    path_key_bytes(raw)
        .is_ok_and(|key| key.split(|c| *c == b'/').next() != Some(b"textures".as_slice()))
}
pub fn absolute(raw: &[u8]) -> bool {
    raw.starts_with(b"/") || raw.starts_with(b"\\") || raw.get(1) == Some(&b':')
}
pub fn sweep(m: &Manifest, r: &mut Reports) -> Result<()> {
    let v = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    for e in &v.diagnostics {
        r.error(e)?;
    }
    v.check_complete()?;
    r.summary.vfs_memory = v.memory_used();
    r.summary.fingerprint = Some(v.fingerprint.clone());
    let aggregate = Budget::new(m.limits.max_memory);
    let mut distinct: BTreeMap<String, BTreeSet<Vec<u8>>> = BTreeMap::new();
    for chain in v.candidates.chunk_by(|a, b| a.key == b.key) {
        let Some(c) = chain.last() else { continue };
        let Some(key) = &c.key else { continue };
        if !key.ends_with(b".nif") {
            continue;
        }
        r.summary.shader_citations.attempted += 1;
        let b = Budget::new(m.limits.max_memory);
        let attempt = (|| {
            let bytes = v.read_candidate(c, &b)?;
            let n = fo4_nif::Nif::read(&bytes, &c.original, Game::Fallout4, &m.limits, &b)?;
            let p = ModelPolicy::inspect(&n, &v, &b)?;
            let citations = p.texture_citations(&n, &b)?;
            for citation in citations.iter().filter(|c| bare(&c.raw)) {
                let owner = format!("{:?}", citation.ownership);
                if !distinct.contains_key(&owner) {
                    aggregate.charge(owner.len() as u64 + 192, "shader family", citation.offset)?;
                }
                let paths = distinct.entry(owner.clone()).or_default();
                if !paths.contains(&citation.raw) {
                    aggregate.charge(
                        citation.raw.len() as u64 + 128,
                        "shader family paths",
                        citation.offset,
                    )?;
                    paths.insert(citation.raw.clone());
                }
                let stat = r
                    .summary
                    .shader_citations
                    .bare_textures
                    .entry(owner)
                    .or_default();
                stat.citations = add(stat.citations, 1, "shader family", citation.offset)?;
                stat.suppressed = add(
                    stat.suppressed,
                    u64::from(citation.suppressed),
                    "shader family",
                    citation.offset,
                )?;
            }
            for block in &n.blocks {
                let Some(fo4_nif::Data::Mesh(mesh)) = &block.decoded else {
                    continue;
                };
                let material = mesh.fields.iter().find_map(|f| {
                    if f.name == "Shader Property" {
                        if let fo4_nif::Value::Ref(i) = f.value {
                            Some(i)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });
                let status = match material {
                    None => MaterialStatus::Unknown, // No decoded association on this mesh representation.
                    Some(i) if i < 0 => MaterialStatus::Empty,
                    Some(i) => p.material[i as usize],
                };
                let label = format!("{status:?}");
                if !r
                    .summary
                    .shader_citations
                    .shapes_by_material
                    .contains_key(&label)
                {
                    aggregate.charge(
                        label.len() as u64 + 128,
                        "shape family",
                        block.data.start as u64,
                    )?;
                }
                let stat = r
                    .summary
                    .shader_citations
                    .shapes_by_material
                    .entry(label)
                    .or_default();
                *stat = add(*stat, 1, "shape family", block.data.start as u64)?;
            }
            Ok(())
        })();
        match attempt {
            Ok(()) => r.summary.shader_citations.read += 1,
            Err(e) => {
                r.summary.shader_citations.failed += 1;
                r.error(&e)?;
            }
        }
    }
    let mut union = BTreeSet::new();
    for (owner, paths) in distinct {
        if let Some(c) = r.summary.shader_citations.bare_textures.get_mut(&owner) {
            c.distinct_paths = paths.len() as u64;
            r.summary.shader_citations.bare_citations = add(
                r.summary.shader_citations.bare_citations,
                c.citations,
                "bare citations",
                0,
            )?;
            r.summary.shader_citations.bare_suppressed = add(
                r.summary.shader_citations.bare_suppressed,
                c.suppressed,
                "bare citations",
                0,
            )?;
        }
        for path in paths {
            if !union.contains(&path) {
                aggregate.charge(path.len() as u64 + 128, "bare path union", 0)?;
                union.insert(path);
            }
        }
    }
    r.summary.shader_citations.bare_distinct_paths = union.len() as u64;
    r.summary.other_memory = aggregate.used();
    Ok(())
}

pub fn instance_causes(
    nodes: &fo4_scene::Assets<'_>,
    ids: &[usize],
    errors: &[fo4_core::Error],
    b: &Budget,
) -> Result<BTreeMap<String, crate::causes::Counts>> {
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for id in ids {
        for rejected in &nodes.nodes[*id].rejections {
            let family = match rejected.class {
                fo4_scene::paths::Citation::NifMaterial if absolute(&rejected.raw) => {
                    "absolute_shader_name"
                }
                fo4_scene::paths::Citation::NifTexture if bare(&rejected.raw) => "bare_nif_texture",
                _ => continue,
            };
            let label = fo4_core::HexBytes(&rejected.raw).to_string();
            b.charge(
                (family.len() + label.len()) as u64 + 256,
                "refused family",
                rejected.offset,
            )?;
            groups.entry(family.into()).or_default().insert(label);
        }
    }
    let mut out = BTreeMap::new();
    for (family, paths) in groups {
        let sole = !errors.is_empty()
            && errors
                .iter()
                .all(|e| e.code == "RejectedVfsPath" && paths.contains(&e.found));
        b.charge(family.len() as u64 + 192, "refused family counts", 0)?;
        out.insert(
            family,
            crate::causes::Counts {
                instances: 1,
                sole_cause_instances: u64::from(sole),
                citations: paths.len() as u64,
                ..Default::default()
            },
        );
    }
    Ok(out)
}
