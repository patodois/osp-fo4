use super::Reports;
use crate::references::{Census, Target};
use fo4_core::{add, Budget, Error, Manifest, Result};
use fo4_vfs::{Location, Vfs};
pub(super) fn sweep(m: &Manifest, r: &mut Reports, scope: &str) -> Result<()> {
    let vfs_budget = Budget::new(m.limits.max_vfs_memory);
    let v = Vfs::mount(m, &vfs_budget)?;
    for e in &v.diagnostics {
        r.error(e)?;
    }
    v.check_complete()?;
    r.summary.vfs_memory = v.memory_used();
    r.summary.fingerprint = Some(v.fingerprint.clone());
    r.summary.fingerprint_scope = Some("assets".into());
    let mut references = Census::new(m.limits.max_entries, m.limits.max_memory / 4);
    let mut pos = 0;
    while pos < v.candidates.len() {
        let Some(key) = v.candidates[pos].key.as_deref() else {
            pos += 1;
            continue;
        };
        let end = pos
            + v.candidates[pos..]
                .partition_point(|candidate| candidate.key.as_deref() == Some(key));
        let wanted = match scope {
            "nif" => key.ends_with(b".nif"),
            "materials" => key.ends_with(b".bgsm") || key.ends_with(b".bgem"),
            "textures" => key.ends_with(b".dds"),
            _ => false,
        };
        if !wanted {
            pos = end;
            continue;
        }
        r.summary.shadowed_assets += (end - pos - 1) as u64;
        r.summary.files_enumerated += 1;
        r.summary.files_attempted += 1;
        let work = Budget::new(m.limits.max_memory);
        let result = (|| -> Result<bool> {
            let winner = v.winner_bytes(key)?;
            let bytes = v.read_candidate(winner, &work)?;
            r.summary.payload_bytes = add(
                r.summary.payload_bytes,
                bytes.len() as u64,
                &winner.original,
                0,
            )?;
            let mut valid = true;
            match scope {
                "nif" => {
                    let mut n =
                        fo4_nif::Nif::read(&bytes, &winner.original, m.game, &m.limits, &work)?;
                    let numeric = n.non_finite();
                    r.numeric(&numeric.attributes(), true)?;
                    if numeric.total() > 0 {
                        let mut notice = Error::new(
                            "NonFiniteNifValues",
                            &winner.original,
                            0,
                            "IEEE values preserved; consumer decides",
                            serde_json::to_string(&numeric).map_err(|e| {
                                Error::new("Serialize", &winner.original, 0, "numeric counts", e)
                            })?,
                        );
                        notice.source.version = Some(n.bs_version);
                        report_error(r, &v, winner, notice, true)?;
                    }
                    r.hist(format!("nif.version.{}", n.version))?;
                    r.hist(format!("nif.bs_version.{}", n.bs_version))?;
                    r.summary.blocks += n.blocks.len() as u64;
                    r.summary.uninterpreted_fields += n.groups.len() as u64;
                    for block in &mut n.blocks {
                        let name = &n.block_types[block.type_index as usize];
                        r.hist(format!("nif.block.{name}"))?;
                        if block.decoded.is_none() {
                            r.summary.raw_blocks += 1;
                            r.hist(format!("nif.raw.{name}"))?;
                        } else if !block.raw_tail.is_empty() {
                            r.summary.partially_read_blocks += 1;
                            r.hist(format!("nif.tail.{name}"))?;
                            r.summary.uninterpreted_fields += 1;
                        }
                        if let Some(data) = &block.decoded {
                            let fields = match data {
                                fo4_nif::Data::Node { fields, .. }
                                | fo4_nif::Data::Fields(fields) => fields,
                                fo4_nif::Data::Mesh(mesh) => &mesh.fields,
                            };
                            if m.game == fo4_core::Game::Fallout4
                                && matches!(
                                    name.as_str(),
                                    "BSLightingShaderProperty" | "BSEffectShaderProperty"
                                )
                            {
                                for field in fields.iter().filter(|f| f.name == "Name") {
                                    if let fo4_nif::Value::StringIndex(i) = field.value {
                                        if let Some(path) = n.strings.get(i as usize) {
                                            let tail = &path[path.len().saturating_sub(5)..];
                                            if tail.eq_ignore_ascii_case(b".bgsm")
                                                || tail.eq_ignore_ascii_case(b".bgem")
                                            {
                                                references.check(
                                                    &v,
                                                    Target::Material,
                                                    path,
                                                    r,
                                                    &work,
                                                )?;
                                            }
                                        }
                                    }
                                }
                            }
                            r.summary.uninterpreted_fields += fields
                                .iter()
                                .filter(|f| {
                                    matches!(f.value, fo4_nif::Value::Raw(_))
                                        || f.name.contains("Unused")
                                        || f.name.contains("Unknown")
                                })
                                .count()
                                as u64;
                        }
                        if let Some(fo4_nif::Data::Mesh(mesh)) = &block.decoded {
                            r.summary.vertices += mesh.declared_vertices as u64;
                            r.summary.triangles += mesh.declared_triangles as u64;
                            r.summary.uninterpreted_fields += mesh.raw_vertex_aux.len() as u64;
                            if let Some(vertices) = &mesh.vertices {
                                r.summary.uninterpreted_fields +=
                                    vertices.iter().filter(|v| v.raw_skin.is_some()).count() as u64;
                            } else {
                                r.summary.uninterpreted_fields += 1;
                            }
                        }
                        for error in block.diagnostics.drain(..) {
                            valid = false;
                            report_error(r, &v, winner, error, false)?;
                        }
                    }
                }
                "materials" => {
                    let mat =
                        fo4_material::Material::read(&bytes, &winner.original, &m.limits, &work)?;
                    r.hist(format!("material.magic.{}", mat.magic()))?;
                    r.hist(format!("material.version.{}", mat.version))?;
                    r.hist(format!("material.{}.version.{}", mat.magic(), mat.version))?;
                    r.summary.uninterpreted_fields += mat.uninterpreted_fields();
                    let count = mat.non_finite_values();
                    r.numeric(&[("material_fields", count)], true)?;
                    if count > 0 {
                        let notice = Error::new(
                            "NonFiniteMaterialValues",
                            &winner.original,
                            0,
                            "IEEE values preserved; consumer decides",
                            count,
                        );
                        report_error(r, &v, winner, notice, true)?;
                    }
                    for (slot, path) in mat.texture_paths() {
                        r.hist(format!(
                            "material.texture_slot.{}.{}.{}",
                            mat.magic(),
                            slot,
                            if path.bytes().is_empty() {
                                "empty"
                            } else {
                                "nonempty"
                            }
                        ))?;
                        references.check(&v, Target::Texture, path.bytes(), r, &work)?;
                    }
                }
                "textures" => {
                    let t = fo4_texture::Texture::read(&bytes, &winner.original, &m.limits, &work)?;
                    r.summary.uninterpreted_fields += t.uninterpreted_header_fields;
                    r.hist(format!("dds.format.{}", t.format.name()))?;
                    r.hist(format!(
                        "dds.header.{}",
                        if t.dx10.is_some() { "DX10" } else { "legacy" }
                    ))?;
                    r.hist(format!("dds.cubemap.{}", t.cubemap))?;
                    r.summary.texture_surfaces += t.surfaces.len() as u64;
                    r.summary.decoded_texture_bytes = add(
                        r.summary.decoded_texture_bytes,
                        t.verify_decode(&m.limits, &work)?,
                        &winner.original,
                        0,
                    )?;
                }
                _ => {
                    return Err(Error::new(
                        "UnsupportedAssetScope",
                        "CLI",
                        0,
                        "nif/materials/textures",
                        scope,
                    ))
                }
            }
            r.item(&serde_json::json!({"kind":"asset","scope":scope,"path":winner.original,"valid":valid,"bytes":bytes.len()}))?;
            Ok(valid)
        })();
        match result {
            Ok(true) => r.summary.files_valid += 1,
            Ok(false) => {
                r.summary.files_failed += 1;
                if scope == "nif" {
                    r.summary.material_references.files_not_fully_checked += 1;
                }
            }
            Err(error) => {
                r.summary.files_failed += 1;
                if scope == "nif" {
                    r.summary.material_references.files_not_fully_checked += 1;
                } else if scope == "materials" {
                    r.summary.texture_references.files_not_fully_checked += 1;
                }
                let winner = v.winner_bytes(key);
                match winner {
                    Ok(winner) => report_error(r, &v, winner, error, false)?,
                    Err(_) => r.error(&error)?,
                }
            }
        }
        pos = end;
    }
    references.finish(r)?;
    Ok(())
}
fn report_error(
    r: &mut Reports,
    v: &Vfs,
    c: &fo4_vfs::Candidate,
    mut e: Error,
    notice: bool,
) -> Result<()> {
    match c.location {
        Location::Archive { archive, entry } => {
            let a = &v.archives[archive];
            let source = a.index.entry(entry)?;
            e.source.decompressed_offset = Some(e.source.offset);
            e.source.offset = source.source.offset;
            e.source.length = source.source.length;
            e.source.file = a.path.clone();
            e.source.archive_entry = Some(c.original.clone());
        }
        Location::Loose { ref path } => e.source.file = path.to_string_lossy().into_owned(),
    }
    if notice {
        r.notice(&e)
    } else {
        r.error(&e)
    }
}
