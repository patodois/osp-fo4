#![forbid(unsafe_code)]
//! Census of the materials whose opacity (the common `alpha` field) is above
//! 1 while blending is on, for `blend_alpha_clamped_before_blend_v1`
//! (`spec/render.md`, Opacity above 1 with blending): counts only, written
//! to `OUT.txt`; one line per material with its path to `OUT.txt.tsv`,
//! which is never committed.
//!
//! `blend_opacity_census MANIFEST OUT.txt`
//!
//! Every winning BGSM and BGEM of the archives is read. For each material
//! with blending enabled and an opacity above 1: the kind, the blend tuple
//! and alpha test, the opacity, and the texture the draw build samples (the
//! diffuse or base slot) decoded at the level the reference draws (the
//! first with both sides at most 256): its alpha range, and the share of
//! texels whose alpha times the opacity exceeds 1 (where a clamp before the
//! blend and no clamp differ). Every winning NIF is then read for the
//! shapes whose shader names such a material.
use fo4_core::{Budget, Game, Manifest};
use fo4_nif::{Nif, Value};
use fo4_scene::paths;
use fo4_vfs::Vfs;
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn bucket(x: f32) -> String {
    for (v, k) in [
        (1.1, "1.1"),
        (1.2, "1.2"),
        (1.5, "1.5"),
        (2., "2"),
        (3., "3"),
    ] {
        if x <= v + 1e-4 {
            return format!("at most {k}");
        }
    }
    "above 3".into()
}
fn share(x: f64) -> &'static str {
    if x == 0. {
        "none"
    } else if x < 0.1 {
        "under 10 %"
    } else if x < 0.5 {
        "10 to 50 %"
    } else if x < 1. {
        "50 % or more"
    } else {
        "all"
    }
}

/// Alpha range and the share of texels whose alpha times `opacity`
/// exceeds 1, on the level the reference draws.
fn texture_alpha(vfs: &Vfs, m: &Manifest, path: &[u8], opacity: f32) -> String {
    let b = Budget::new(m.limits.max_memory);
    let Ok(bytes) = vfs.read_path_bytes(path, &b) else {
        return "texture missing".into();
    };
    let Ok(t) = fo4_texture::Texture::read(&bytes, "t", &m.limits, &b) else {
        return "texture unreadable".into();
    };
    let Some((i, _)) = t
        .surfaces
        .iter()
        .enumerate()
        .filter(|(_, s)| s.array == 0 && s.face == 0 && s.depth == 1)
        .find(|(_, s)| s.width <= 256 && s.height <= 256)
    else {
        return "no small level".into();
    };
    let Ok(rgba) = t.decode(i, &m.limits, &b) else {
        return "level not decoded".into();
    };
    let px = rgba.as_chunks::<4>().0;
    let lo = px.iter().map(|p| p[3]).fold(f32::MAX, f32::min);
    let hi = px.iter().map(|p| p[3]).fold(f32::MIN, f32::max);
    let above = px.iter().filter(|p| p[3] * opacity > 1.).count();
    format!(
        "{}, alpha {:.2} to {:.2}, alpha x opacity above 1 on {} of the texels",
        t.format.name(),
        (lo * 4.).floor() / 4.,
        (hi * 4.).ceil() / 4.,
        share(above as f64 / px.len().max(1) as f64)
    )
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [manifest, out] = args.as_slice() else {
        return Err("blend_opacity_census MANIFEST OUT.txt".into());
    };
    let started = std::time::Instant::now();
    let m = Manifest::load(Path::new(manifest))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut keys: Vec<Vec<u8>> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.clone())
        .collect();
    keys.sort();
    keys.dedup();
    let mut c = Counts::new();
    let mut rows = String::from("material\tkind\topacity\tblend\ttexture\n");
    // Materials in scope: path -> facts key.
    let mut scope: HashMap<Vec<u8>, String> = HashMap::new();
    for path in keys
        .iter()
        .filter(|k| k.ends_with(b".bgsm") || k.ends_with(b".bgem"))
    {
        let b = Budget::new(m.limits.max_memory);
        let Ok(bytes) = vfs.read_path_bytes(path, &b) else {
            bump(&mut c, "materials: unreadable");
            continue;
        };
        let Ok(mat) = fo4_material::Material::read(&bytes, "m", &m.limits, &b) else {
            bump(&mut c, "materials: unreadable");
            continue;
        };
        let cm = &mat.common;
        bump(&mut c, format!("materials: {}", mat.magic()));
        if cm.alpha_blend.enabled == 0
            || cm.alpha.partial_cmp(&1.) != Some(std::cmp::Ordering::Greater)
        {
            continue;
        }
        let blend = format!(
            "src {} dst {} test {}",
            cm.alpha_blend.source, cm.alpha_blend.destination, cm.alpha_test.0
        );
        let slot = mat
            .texture_paths()
            .find(|(name, _)| matches!(*name, "diffuse" | "base"))
            .map(|(_, p)| p.bytes().to_vec())
            .unwrap_or_default();
        let citation = paths::Citation::MaterialTexture;
        let tex = if slot.is_empty() {
            "no texture".to_string()
        } else {
            match paths::resolve(citation, &slot, &b) {
                Ok(p) => texture_alpha(&vfs, &m, &p, cm.alpha),
                Err(e) => format!("path not resolved ({})", e.code),
            }
        };
        let kind = mat.magic();
        bump(&mut c, format!("in scope: {kind}"));
        bump(&mut c, format!("in scope: {kind}: blend {blend}"));
        bump(
            &mut c,
            format!("in scope: {kind}: opacity {}", bucket(cm.alpha)),
        );
        bump(&mut c, format!("in scope: {kind}: texture: {tex}"));
        bump(
            &mut c,
            format!(
                "in scope: {kind}: opacity {}: {}",
                bucket(cm.alpha),
                tex.split(", ").last().unwrap_or("")
            ),
        );
        let _ = writeln!(
            rows,
            "{}\t{kind}\t{}\t{blend}\t{tex}",
            String::from_utf8_lossy(path),
            cm.alpha
        );
        scope.insert(path.clone(), tex);
    }
    // Shapes and models that name a material in scope.
    let mut models = 0u64;
    let mut files = 0u64;
    for path in keys.iter().filter(|k| k.ends_with(b".nif")) {
        let b = Budget::new(m.limits.max_memory);
        let Ok(nb) = vfs.read_path_bytes(path, &b) else {
            continue;
        };
        let Ok(n) = Nif::read(&nb, "m", Game::Fallout4, &m.limits, &b) else {
            continue;
        };
        files += 1;
        let mut cites = false;
        for block in &n.blocks {
            let ty = &n.block_types[block.type_index as usize];
            if ty != "BSLightingShaderProperty" && ty != "BSEffectShaderProperty" {
                continue;
            }
            let Some(d) = &block.decoded else {
                continue;
            };
            let f = fo4_scene::model_policy::fields(d);
            let Some(name) = f.iter().find_map(|x| match x.value {
                Value::StringIndex(i) if x.name == "Name" => n.strings.get(i as usize),
                _ => None,
            }) else {
                continue;
            };
            let Ok(resolved) = paths::resolve(paths::Citation::NifMaterial, name, &b) else {
                continue;
            };
            if let Some(tex) = scope.get(&resolved) {
                cites = true;
                bump(
                    &mut c,
                    format!(
                        "shapes naming a material in scope: {ty}: {}",
                        tex.split(", ").last().unwrap_or("")
                    ),
                );
            }
        }
        if cites {
            models += 1;
        }
    }
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# Blend opacity census (`blend_opacity_census`): {} s; {files} NIF read, {models} name a material in scope\n",
        started.elapsed().as_secs()
    );
    for (k, n) in &c {
        let _ = writeln!(text, "{n}\t{k}");
    }
    std::fs::write(out, text)?;
    std::fs::write(format!("{out}.tsv"), rows)?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
