//! Sub-reasons of four omission reasons for `coverage_census`
//! (`spec/materials.md`): missing materials, missing textures, swap scopes
//! and texture transform controllers. Every label is a class of the data (a role, a
//! path shape, a field value bucket), never a path or a name.
use fo4_core::{Budget, Manifest, Mount};
use fo4_nif::Value;
use fo4_scene::{
    missing::{self, Read},
    paths::Citation,
    AssetKind, DecodedAsset, Instance, Loader,
};
use fo4_vfs::Vfs;
use std::collections::{BTreeMap, HashMap, HashSet};

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Where a material path that the mounted archives lack could be found
/// under another spelling.
pub struct Spelling {
    /// Every mounted key under `materials/`.
    keys: HashSet<Vec<u8>>,
    /// File name (last component) -> how many mounted keys end with it.
    names: HashMap<Vec<u8>, u32>,
    /// Keys of archives in the data folder that the manifest does not mount.
    unmounted: HashSet<Vec<u8>>,
    /// Keys of loose files in the data folder.
    loose: HashSet<Vec<u8>>,
    pub unmounted_archives: u64,
    pub loose_files: u64,
    /// The same three sets for keys under `textures/`, and the mounted
    /// texture keys without their extension.
    texture_keys: HashSet<Vec<u8>>,
    texture_names: HashMap<Vec<u8>, u32>,
    texture_stems: HashSet<Vec<u8>>,
    texture_unmounted: HashSet<Vec<u8>>,
    texture_loose: HashSet<Vec<u8>>,
}
fn stem(key: &[u8]) -> &[u8] {
    match key.iter().rposition(|c| *c == b'.') {
        Some(k) if !key[k..].contains(&b'/') => &key[..k],
        _ => key,
    }
}

fn file_name(key: &[u8]) -> &[u8] {
    key.rsplit(|c| *c == b'/').next().unwrap_or(key)
}
fn lower(path: &str) -> Vec<u8> {
    path.bytes()
        .map(|c| {
            if c == b'\\' {
                b'/'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

impl Spelling {
    pub fn build(m: &Manifest, vfs: &Vfs) -> AnyResult<Self> {
        let mut s = Spelling {
            keys: HashSet::new(),
            names: HashMap::new(),
            unmounted: HashSet::new(),
            loose: HashSet::new(),
            unmounted_archives: 0,
            loose_files: 0,
            texture_keys: HashSet::new(),
            texture_names: HashMap::new(),
            texture_stems: HashSet::new(),
            texture_unmounted: HashSet::new(),
            texture_loose: HashSet::new(),
        };
        for c in &vfs.candidates {
            if let Some(k) = &c.key {
                if k.starts_with(b"materials/") && s.keys.insert(k.clone()) {
                    *s.names.entry(file_name(k).to_vec()).or_default() += 1;
                }
                if k.starts_with(b"textures/") && s.texture_keys.insert(k.clone()) {
                    *s.texture_names.entry(file_name(k).to_vec()).or_default() += 1;
                    s.texture_stems.insert(stem(k).to_vec());
                }
            }
        }
        let mounted: HashSet<Vec<u8>> = m
            .mounts
            .iter()
            .filter_map(|x| match x {
                Mount::Archive { path, .. } => Some(lower(path)),
                Mount::Loose { .. } => None,
            })
            .collect();
        let root = Path::new(&m.root);
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in read.flatten() {
                let p = e.path();
                let Ok(t) = e.file_type() else {
                    continue;
                };
                if t.is_dir() {
                    stack.push(p);
                    continue;
                }
                let Ok(rel) = p.strip_prefix(root) else {
                    continue;
                };
                let rel = lower(&rel.to_string_lossy());
                let archive = rel.ends_with(b".ba2") || rel.ends_with(b".bsa");
                if archive && dir == root {
                    if mounted.contains(&rel) {
                        continue;
                    }
                    s.unmounted_archives += 1;
                    let src = fo4_core::FileSource::open(&p)?;
                    let a = fo4_archive::GameArchive::read(
                        m.game,
                        &src,
                        &m.limits,
                        &Budget::new(m.limits.max_vfs_memory),
                    )?;
                    if let fo4_archive::GameArchive::Fallout4(a) = a {
                        for e in a.entries {
                            if e.key.starts_with(b"materials/") {
                                s.unmounted.insert(e.key);
                            } else if e.key.starts_with(b"textures/") {
                                s.texture_unmounted.insert(e.key);
                            }
                        }
                    }
                } else if !archive {
                    s.loose_files += 1;
                    if rel.ends_with(b".bgsm") || rel.ends_with(b".bgem") {
                        s.loose.insert(rel);
                    } else if rel.starts_with(b"textures/") {
                        s.texture_loose.insert(rel);
                    }
                }
            }
        }
        Ok(s)
    }
    /// The class of a missing material key.
    pub fn class(&self, key: &[u8]) -> &'static str {
        if self.loose.contains(key) {
            return "in a loose folder";
        }
        if self.unmounted.contains(key) {
            return "in an archive the manifest does not mount";
        }
        if !key.starts_with(b"materials/") {
            return "outside the materials root";
        }
        let mut other = key.to_vec();
        if other.ends_with(b".bgsm") || other.ends_with(b".bgem") {
            let n = other.len();
            other[n - 2] = if key.ends_with(b"sm") { b'e' } else { b's' };
            if self.keys.contains(&other) {
                return "present with the other material extension";
            }
        }
        if let Some(rest) = key.strip_prefix(b"materials/materials/") {
            let mut k = b"materials/".to_vec();
            k.extend_from_slice(rest);
            if self.keys.contains(&k) {
                return "present without a doubled root folder";
            }
        }
        match self.names.get(file_name(key)).copied().unwrap_or(0) {
            0 => "absent from every mounted archive and loose folder; file name nowhere",
            1 => "absent at its path; file name under one other folder",
            _ => "absent at its path; file name under several other folders",
        }
    }
    /// The class of a missing texture key.
    pub fn texture_class(&self, key: &[u8]) -> &'static str {
        if self.texture_loose.contains(key) {
            return "in a loose folder";
        }
        if self.texture_unmounted.contains(key) {
            return "in an archive the manifest does not mount";
        }
        if !key.starts_with(b"textures/") {
            return "outside the textures root";
        }
        if self.texture_stems.contains(stem(key)) {
            return "present with another extension (or none)";
        }
        if let Some(rest) = key.strip_prefix(b"textures/textures/") {
            let mut k = b"textures/".to_vec();
            k.extend_from_slice(rest);
            if self.texture_keys.contains(&k) {
                return "present without a doubled root folder";
            }
        }
        match self.texture_names.get(file_name(key)).copied().unwrap_or(0) {
            0 => "absent from every mounted archive and loose folder; file name nowhere",
            1 => "absent at its path; file name under one other folder",
            _ => "absent at its path; file name under several other folders",
        }
    }
}
use std::path::Path;

/// The slot words of a texture citation.
fn slot_label(slot: Option<fo4_scene::TextureSlot>) -> String {
    use fo4_scene::TextureSlot as S;
    match slot {
        Some(S::Material { slot, .. }) => format!("material {slot}"),
        Some(S::TextureSet(k)) => format!("texture set slot {k}"),
        Some(S::Effect { field, .. }) => format!("effect {field}"),
        None => "other citation".into(),
    }
}

/// Labels of the missing textures of `i`: one per file (whether a slot the
/// frame samples cites it, the citing slots, the spelling class), and the
/// placement's set of the first two.
pub fn missing_texture_labels(
    i: &Instance,
    loader: &Loader<'_>,
    spelling: &Spelling,
    b: &Budget,
) -> fo4_core::Result<(String, Vec<String>)> {
    let unread = missing::unread_textures(&loader.assets, &i.assets, b)?;
    let mut files = Vec::new();
    let mut set: Vec<String> = Vec::new();
    for t in &i.assets {
        let node = &loader.assets.nodes[*t];
        if node.kind != AssetKind::Texture
            || !node.blockers.iter().any(|e| e.code == "MissingTexture")
        {
            continue;
        }
        let mut slots: Vec<String> = Vec::new();
        for n in &i.assets {
            let citer = &loader.assets.nodes[*n];
            for (d, c) in citer.dependencies.iter().zip(&citer.dependency_citations) {
                if d == t {
                    let l = slot_label(c.slot);
                    if !slots.contains(&l) {
                        slots.push(l);
                    }
                }
            }
        }
        slots.sort();
        let read = if unread.contains(t) {
            "unsampled slots only"
        } else {
            "a sampled slot"
        };
        let class = spelling.texture_class(&node.path);
        files.push(format!("{read}; {}; {class}", slots.join(", ")));
        let l = format!("{read}; {}", slots.join(", "));
        if !set.contains(&l) {
            set.push(l);
        }
    }
    set.sort();
    Ok((set.join(" + "), files))
}

fn roles(r: &missing::Roles) -> String {
    let mut v = Vec::new();
    for (n, name) in [
        (r.name_lighting_textured, "name (lighting, own texture)"),
        (r.name_lighting_bare, "name (lighting, no own texture)"),
        (r.name_effect, "name (effect)"),
        (r.root_material, "root material"),
        (r.parent, "material parent"),
        (r.swap_from, "swap source"),
        (r.swap_to_cited, "swap target (source cited)"),
        (r.swap_to_uncited, "swap target (source not cited)"),
        (r.other, "other"),
    ] {
        if n > 0 {
            v.push(name);
        }
    }
    v.join(", ")
}

/// Labels of the missing materials of `i`: one per file (read decision,
/// citing roles, spelling class), and the placement's set of read decisions.
pub fn missing_labels(
    i: &Instance,
    loader: &Loader<'_>,
    spelling: &Spelling,
    b: &Budget,
) -> fo4_core::Result<(String, Vec<String>, Vec<&'static str>)> {
    let swaps = missing::instance_swaps(i);
    let found = missing::classify(&loader.assets, &i.assets, &swaps, b)?;
    let mut files = Vec::new();
    let mut spell = Vec::new();
    let mut set: Vec<String> = Vec::new();
    for m in &found {
        let read = match m.read {
            Read::NotRead(w) => format!("not read: {w}"),
            Read::EmbeddedShader => "read; every citing lighting shader has its own texture".into(),
            Read::Needed(w) => format!("read: {w}"),
        };
        let class = spelling.class(&loader.assets.nodes[m.node].path);
        files.push(format!("{read}; cited as {}; {class}", roles(&m.roles)));
        spell.push(class);
        if !set.contains(&read) {
            set.push(read);
        }
    }
    set.sort();
    if set.is_empty() {
        set.push("no missing material in the closure".into());
    }
    Ok((set.join(" + "), files, spell))
}

/// Which records carry the swaps of `i`, whether they are one record, and
/// their substitution lists.
pub fn scopes_label(i: &Instance, found: &str) -> String {
    let mut scopes = Vec::new();
    let mut keys = Vec::new();
    let mut fnam = 0;
    let mut pairs = Vec::new();
    let mut note = |s: &fo4_plugin::typed::Loaded<fo4_plugin::typed::Mswp>, scope: &'static str| {
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
        keys.push(s.key.clone());
        fnam += s
            .data
            .entries
            .iter()
            .filter(|e| e.signature == *b"FNAM")
            .count();
        pairs.push(
            s.data
                .entries
                .iter()
                .filter(|e| e.signature == *b"BNAM")
                .count(),
        );
    };
    if let Some(s) = &i.material_swap {
        note(s, "reference");
    }
    fn parts<'a>(
        v: &'a fo4_scene::Base,
        depth: u32,
        out: &mut Vec<(&'a fo4_plugin::typed::Loaded<fo4_plugin::typed::Mswp>, u32)>,
    ) {
        if let Some(s) = &v.material_swap {
            out.push((s, depth));
        }
        for p in &v.parts {
            if let Some(r) = &p.resolved {
                parts(r, depth + 1, out);
            }
        }
    }
    let mut based = Vec::new();
    if let Some(v) = &i.base {
        parts(v, 0, &mut based);
    }
    for (s, d) in based {
        note(s, if d == 0 { "base" } else { "SCOL part base" });
    }
    let same = keys.windows(2).all(|w| w[0] == w[1]);
    let pairs = if pairs.iter().all(|n| *n == 1) {
        "one pair each"
    } else if pairs.iter().all(|n| *n <= 4) {
        "two to four pairs"
    } else {
        "more than four pairs in one"
    };
    format!(
        "{}; scopes {}; {}; {pairs}; FNAM {}",
        found.trim(),
        scopes.join("+"),
        if same {
            "one MSWP record"
        } else {
            "different MSWP records"
        },
        if fnam > 0 { "present" } else { "absent" }
    )
}

const UV_VARIABLES: [&str; 4] = ["U offset", "U scale", "V offset", "V scale"];

fn bucket(v: f32) -> &'static str {
    if v == 0. {
        "0"
    } else if v == 1. {
        "1"
    } else if v.abs() < 1. {
        "between -1 and 1"
    } else if v.abs() <= 10. {
        "1 to 10 in magnitude"
    } else {
        "above 10 in magnitude"
    }
}

/// What the census reads of one model's texture-transform controllers:
/// (controller labels, shader labels, model label).
#[derive(Clone, Default)]
pub struct TextureModel {
    pub controllers: Vec<String>,
    pub shaders: Vec<String>,
    pub label: String,
}

fn texture_model(
    loader: &Loader<'_>,
    vfs: &Vfs,
    id: usize,
    b: &Budget,
) -> fo4_core::Result<TextureModel> {
    loader.assets.with_decoded(id, b, |d| {
        let DecodedAsset::Model(n) = d else {
            return Ok(TextureModel::default());
        };
        let tags = fo4_scene::rest::tags(&n, b)?;
        let mut out = TextureModel::default();
        let mut classes: Vec<&'static str> = Vec::new();
        let mut seen_shaders = Vec::new();
        for (k, block) in n.blocks.iter().enumerate() {
            if !tags
                .get(k)
                .is_some_and(|t| t.contains(&fo4_scene::rest::TEXTURE_TRANSFORM))
            {
                continue;
            }
            let ty = n.block_types[block.type_index as usize].as_str();
            if !ty.ends_with("Controller") {
                continue;
            }
            if let Some(t) = fo4_scene::rest::texture_controller(&n, k) {
                let var = t
                    .variable
                    .checked_sub(20)
                    .and_then(|v| UV_VARIABLES.get(v as usize))
                    .copied()
                    .unwrap_or("?");
                let class = if t.first_key_is_stored {
                    "lighting, first key = stored"
                } else {
                    "lighting, first key differs from stored"
                };
                if !classes.contains(&class) {
                    classes.push(class);
                }
                out.controllers.push(format!(
                    "{class}; {var}; stored {}; manager flag {}; in a sequence {}",
                    bucket(t.stored),
                    t.manager_flag,
                    t.in_sequence
                ));
                if !seen_shaders.contains(&t.shader) {
                    seen_shaders.push(t.shader);
                    out.shaders
                        .push(shader_label(&n, t.shader, loader, vfs, b)?);
                }
            } else {
                let var = block.decoded.as_ref().and_then(|d| {
                    fo4_scene::model_policy::fields(d)
                        .iter()
                        .find(|f| f.name == "Controlled Variable")
                        .and_then(|f| match f.value {
                            Value::U32(v) => Some(v),
                            _ => None,
                        })
                });
                let class = if ty.starts_with("BSEffectShaderProperty") {
                    "effect, not settled by the effect first-key rule"
                } else {
                    "other controller type"
                };
                if !classes.contains(&class) {
                    classes.push(class);
                }
                let var = match (ty, var) {
                    ("BSEffectShaderPropertyFloatController", Some(v @ 6..=9)) => {
                        UV_VARIABLES[(v - 6) as usize]
                    }
                    _ => "-",
                };
                out.controllers.push(format!("{class}; {ty}; {var}"));
            }
        }
        classes.sort_unstable();
        out.label = classes.join(" + ");
        Ok(out)
    })
}

fn shader_label(
    n: &fo4_nif::Nif<'_>,
    shader: usize,
    loader: &Loader<'_>,
    vfs: &Vfs,
    b: &Budget,
) -> fo4_core::Result<String> {
    let Some(f) = n.blocks[shader]
        .decoded
        .as_ref()
        .map(fo4_scene::model_policy::fields)
    else {
        return Ok("shader not decoded".into());
    };
    let floats = |name: &str| -> Vec<f32> {
        f.iter()
            .filter(|x| x.name == name)
            .filter_map(|x| match x.value {
                Value::FloatBits(v) => Some(f32::from_bits(v)),
                _ => None,
            })
            .collect()
    };
    let (o, s) = (floats("UV Offset"), floats("UV Scale"));
    let nif_uv = [
        o.first().copied().unwrap_or(f32::NAN),
        o.get(1).copied().unwrap_or(f32::NAN),
        s.first().copied().unwrap_or(f32::NAN),
        s.get(1).copied().unwrap_or(f32::NAN),
    ];
    let shape = match (
        nif_uv[0] == 0. && nif_uv[1] == 0.,
        nif_uv[2] == 1. && nif_uv[3] == 1.,
    ) {
        (true, true) => "NIF offset 0 and scale 1",
        (false, true) => "NIF offset not 0, scale 1",
        (true, false) => "NIF offset 0, scale not 1",
        (false, false) => "NIF offset not 0 and scale not 1",
    };
    let name = f.iter().find_map(|x| match x.value {
        Value::StringIndex(i) if x.name == "Name" => n.strings.get(i as usize),
        _ => None,
    });
    let material = match name.filter(|p| !p.is_empty()) {
        None => "no material".to_string(),
        Some(raw) => match loader.assets.resolve_path(Citation::NifMaterial, raw, b) {
            Err(_) => "material path not resolved".into(),
            Ok(r) => match vfs.read_path_bytes(&r.key, b) {
                Err(_) => "material missing".into(),
                Ok(bytes) => {
                    match fo4_material::Material::read(&bytes, "material", vfs.limits(), b) {
                        Err(_) => "material unreadable".into(),
                        Ok(m) => {
                            let c = &m.common;
                            let mat = [c.u_offset, c.v_offset, c.u_scale, c.v_scale];
                            if mat == nif_uv {
                                "material UV fields equal the NIF's".into()
                            } else {
                                "material UV fields differ from the NIF's".into()
                            }
                        }
                    }
                }
            },
        },
    };
    Ok(format!("{shape}; {material}"))
}

/// Labels of the texture-transform controllers of `i`'s models, cached by
/// model path.
pub fn texture_labels(
    i: &Instance,
    loader: &Loader<'_>,
    vfs: &Vfs,
    cache: &mut BTreeMap<Vec<u8>, TextureModel>,
    b: &Budget,
) -> fo4_core::Result<TextureModel> {
    let mut out = TextureModel::default();
    let mut labels = Vec::new();
    for id in &i.assets {
        let node = &loader.assets.nodes[*id];
        if node.kind != AssetKind::Model
            || !node.blockers.iter().any(|e| {
                e.code == "UnsupportedAnimationRuntime"
                    && e.found.ends_with(";rest=texture-transform")
            })
        {
            continue;
        }
        let t = match cache.get(&node.path) {
            Some(t) => t.clone(),
            None => {
                let t = texture_model(loader, vfs, *id, b)?;
                cache.insert(node.path.clone(), t.clone());
                t
            }
        };
        out.controllers.extend(t.controllers);
        out.shaders.extend(t.shaders);
        if !labels.contains(&t.label) {
            labels.push(t.label);
        }
    }
    labels.sort();
    out.label = labels.join(" | ");
    Ok(out)
}
