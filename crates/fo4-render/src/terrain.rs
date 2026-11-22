//! Terrain of a cell in a draw list (`spec/land.md`).
//!
//! [`append_terrain`] draws plain geometry: each triangle gets one neutral
//! grey whose brightness follows the angle between its geometric normal and
//! a fixed direction, so that slopes read in an unlit frame.
//!
//! [`append_textured_terrain`] draws the ground textures under the rules of
//! `fo4_scene::terrain::layers`: per quadrant one opaque pass and one
//! additive overlay pass per further texture, each pass carrying its
//! texture's per-vertex weight, so the frame holds the weighted sum. A
//! texture no record names, or one that is refused, is drawn as that same
//! grey with its weight. There is no game lighting.
use crate::draw::{block_texture, decode_texture, BuildOptions};
use crate::{append, Alpha, Draw, DrawList, Mesh, Sidedness, Texture, Vertex};
use fo4_core::{Budget, Limits, Result};
use fo4_plugin::typed::Store;
use fo4_plugin::FormKey;
use fo4_scene::collision::{rules_state, triangle_normal};
use fo4_scene::paths::Citation;
use fo4_scene::terrain::layers::{
    self, quad_index, quadrant_quads, texture_uv, Layers, Outcome, Quadrant, Source, QUADRANT_SIDE,
};
use fo4_scene::terrain::Terrain;
use fo4_scene::transform::Transform;
use fo4_scene::{hypotheses, AssetKind, DecodedAsset, Loader, RuleLevel};
use serde::Serialize;
use std::collections::BTreeMap;

/// Grey of a triangle facing [`LIGHT`] squarely, and of one at right angles.
pub const BRIGHT: f32 = 0.85;
pub const DARK: f32 = 0.30;
/// Fixed shading direction (unnormalized): from the south-west, above.
pub const LIGHT: [f64; 3] = [-1.0, -1.0, 2.0];

/// Shade in [DARK, BRIGHT] for a unit normal; degenerate triangles get DARK.
pub fn shade(n: [f64; 3]) -> f32 {
    let len = LIGHT.iter().map(|x| x * x).sum::<f64>().sqrt();
    let d = (0..3).map(|k| n[k] * LIGHT[k] / len).sum::<f64>();
    DARK + (BRIGHT - DARK) * d.clamp(0.0, 1.0) as f32
}

/// Appends the terrain as one opaque, double-sided draw tagged `instance`,
/// with its own 1 x 1 white texture. Vertices are not shared between
/// triangles so each triangle has a flat shade.
pub fn append_terrain(
    list: &mut DrawList,
    terrain: &Terrain,
    instance: u32,
    b: &Budget,
) -> Result<()> {
    let triangles: Vec<[[f64; 3]; 3]> = {
        let mut v = Vec::new();
        b.reserve(&mut v, 2 * 32 * 32, "terrain", 0)?;
        v.extend(terrain.world_triangles());
        v
    };
    let n = triangles.len();
    let mut vertices = Vec::new();
    b.reserve(&mut vertices, 3 * n, "terrain", 0)?;
    let mut colors = Vec::new();
    b.reserve(&mut colors, 3 * n, "terrain", 0)?;
    let mut indices = Vec::new();
    b.reserve(&mut indices, n, "terrain", 0)?;
    for (k, t) in triangles.iter().enumerate() {
        let g = shade(triangle_normal(*t));
        for p in t {
            vertices.push(Vertex {
                position: *p,
                uv: [0.0, 0.0],
            });
            colors.push([g, g, g, 1.0]);
        }
        let base = u16::try_from(3 * k).map_err(|e| {
            fo4_core::Error::new("TerrainVertexIndex", "terrain", 0, "u16 index", e)
        })?;
        indices.push([base, base + 1, base + 2]);
    }
    let texture = list.textures.len();
    let mut rgba = Vec::new();
    b.reserve(&mut rgba, 4, "terrain", 0)?;
    rgba.extend([1.0; 4]);
    append(
        &mut list.textures,
        Texture {
            mips: Vec::new(),
            width: 1,
            height: 1,
            rgba,
            mip: 0,
            srgb: false,
        },
        b,
    )?;
    let mesh = list.meshes.len();
    append(
        &mut list.meshes,
        Mesh {
            vertex_colors: Some(colors),
            vertices,
            triangles: indices,
        },
        b,
    )?;
    append(
        &mut list.draws,
        Draw {
            sidedness: Sidedness {
                material_two_sided: Some(1),
                nif_shader_flags_2: None,
            },
            instance,
            mesh,
            texture,
            world: Transform::IDENTITY,
            alpha: Alpha {
                enabled: 0,
                source: 6,
                destination: 7,
                test_enabled: 0,
                test_ref: 0,
                nif_test_function: None,
                opacity: 1.0,
                nif_flags: None,
            },
            uv_offset: [0.0, 0.0],
            uv_scale: [1.0, 1.0],
            raw_tile_flags: None,
        },
        b,
    )
}

const TWO_SIDED: Sidedness = Sidedness {
    material_two_sided: Some(1),
    nif_shader_flags_2: None,
};
const OPAQUE: Alpha = Alpha {
    enabled: 0,
    source: 6,
    destination: 7,
    test_enabled: 0,
    test_ref: 0,
    nif_test_function: None,
    opacity: 1.0,
    nif_flags: None,
};
/// Source alpha times source plus destination: codes 6 and 0 of the CPU
/// `blend`, a sum of the passes whatever their order.
const ADDITIVE: Alpha = Alpha {
    enabled: 1,
    source: 6,
    destination: 0,
    ..OPAQUE
};

/// Landscape colour textures decoded once per draw list, with the budget of
/// the build options it was made with.
pub struct TextureCache {
    options: BuildOptions,
    texture_bytes: u64,
    block_bytes: u64,
    by_ltex: BTreeMap<FormKey, std::result::Result<usize, String>>,
    /// The default texture of each world (`land_default_texture_world_named_v1`).
    by_world: BTreeMap<Option<FormKey>, std::result::Result<usize, String>>,
    /// Draw-list textures by resolved path, shared by every record naming it.
    by_path: BTreeMap<Vec<u8>, usize>,
    white: Option<usize>,
    /// Path rules used by a loaded texture.
    pub path_rules: Vec<&'static str>,
    /// Optional ground texture rules a drawn texture relied on.
    pub rules: Vec<&'static str>,
}
impl TextureCache {
    pub fn new(options: BuildOptions) -> Self {
        Self {
            options,
            texture_bytes: 0,
            block_bytes: 0,
            by_ltex: BTreeMap::new(),
            by_world: BTreeMap::new(),
            by_path: BTreeMap::new(),
            white: None,
            path_rules: Vec::new(),
            rules: Vec::new(),
        }
    }
    /// Landscape textures by outcome: `drawn` or the refusal code.
    pub fn outcomes(&self) -> BTreeMap<String, u64> {
        tally(self.by_ltex.values())
    }
    /// Default textures of the worlds met, by outcome.
    pub fn default_outcomes(&self) -> BTreeMap<String, u64> {
        tally(self.by_world.values())
    }
    fn white(&mut self, list: &mut DrawList, b: &Budget) -> Result<usize> {
        if let Some(i) = self.white {
            return Ok(i);
        }
        let mut rgba = Vec::new();
        b.reserve(&mut rgba, 4, "terrain", 0)?;
        rgba.extend([1.0; 4]);
        let i = list.textures.len();
        append(
            &mut list.textures,
            Texture {
                mips: Vec::new(),
                width: 1,
                height: 1,
                rgba,
                mip: 0,
                srgb: false,
            },
            b,
        )?;
        self.white = Some(i);
        Ok(i)
    }
    /// The draw-list texture of a landscape texture record, or why not.
    #[allow(clippy::too_many_arguments)]
    fn ltex(
        &mut self,
        list: &mut DrawList,
        store: &Store,
        loader: &mut Loader<'_>,
        ltex: &FormKey,
        level: RuleLevel,
        limits: &Limits,
        b: &Budget,
    ) -> Result<std::result::Result<usize, String>> {
        if let Some(v) = self.by_ltex.get(ltex) {
            return Ok(v.clone());
        }
        let outcome = match layers::colour_texture(store, ltex, b)? {
            Ok(colour) => self.colour(list, loader, colour, level, limits, b)?,
            Err(code) => Err(code.to_owned()),
        };
        b.charge(ltex.plugin.len() as u64 + 64, "terrain textures", 0)?;
        self.by_ltex.insert(ltex.clone(), outcome.clone());
        Ok(outcome)
    }
    /// The draw-list texture of the default ground of `world` under
    /// `land_default_texture_world_named_v1`, or why not.
    #[allow(clippy::too_many_arguments)]
    fn default(
        &mut self,
        list: &mut DrawList,
        store: &Store,
        loader: &mut Loader<'_>,
        world: Option<&FormKey>,
        level: RuleLevel,
        limits: &Limits,
        b: &Budget,
    ) -> Result<std::result::Result<usize, String>> {
        let key = world.cloned();
        if let Some(v) = self.by_world.get(&key) {
            return Ok(v.clone());
        }
        let outcome = if !accepted(hypotheses::LAND_DEFAULT_NAMED, level) {
            Err("DefaultTextureRuleNotAccepted".to_owned())
        } else if let Some(world) = world {
            match layers::default_texture_set(store, world, b)? {
                Ok(txst) => match layers::texture_set_colour(store, &txst, b)? {
                    Ok(colour) => {
                        let drawn = self.colour(list, loader, colour, level, limits, b)?;
                        if drawn.is_ok() {
                            self.rule(hypotheses::LAND_DEFAULT_NAMED, b)?;
                        }
                        drawn
                    }
                    Err(code) => Err(code.to_owned()),
                },
                Err(code) => Err(code.to_owned()),
            }
        } else {
            Err("TerrainWithoutWorld".to_owned())
        };
        b.charge(128, "terrain textures", 0)?;
        self.by_world.insert(key, outcome.clone());
        Ok(outcome)
    }
    fn rule(&mut self, rule: &'static str, b: &Budget) -> Result<()> {
        if !self.rules.contains(&rule) {
            b.reserve(&mut self.rules, 1, "terrain textures", 0)?;
            self.rules.push(rule);
            self.rules.sort_unstable();
        }
        Ok(())
    }
    /// The draw-list texture of a texture set's colour: its TX00, or the
    /// diffuse texture of its material under
    /// `ltex_texture_set_material_diffuse_v1`.
    fn colour(
        &mut self,
        list: &mut DrawList,
        loader: &mut Loader<'_>,
        colour: layers::ColourTexture,
        level: RuleLevel,
        limits: &Limits,
        b: &Budget,
    ) -> Result<std::result::Result<usize, String>> {
        let path = match colour {
            layers::ColourTexture::TextureSet(path) => path.into_bytes(),
            layers::ColourTexture::Material(material) => {
                if !accepted(hypotheses::LTEX_MATERIAL, level) {
                    return Ok(Err("TextureSetWithoutTx00".to_owned()));
                }
                match self.material_diffuse(loader, &material, level, b)? {
                    Ok(path) => {
                        let drawn = self.texture(
                            list,
                            loader,
                            &path,
                            Citation::MaterialTexture,
                            level,
                            limits,
                            b,
                        )?;
                        if drawn.is_ok() {
                            self.rule(hypotheses::LTEX_MATERIAL, b)?;
                        }
                        return Ok(drawn);
                    }
                    Err(code) => return Ok(Err(code)),
                }
            }
        };
        self.texture(
            list,
            loader,
            &path,
            Citation::TextureSetTexture,
            level,
            limits,
            b,
        )
    }
    /// The diffuse path of a lighting material (BGSM), or why not.
    fn material_diffuse(
        &mut self,
        loader: &mut Loader<'_>,
        material: &str,
        level: RuleLevel,
        b: &Budget,
    ) -> Result<std::result::Result<Vec<u8>, String>> {
        let key = match self.resolve(
            loader,
            material.as_bytes(),
            Citation::TextureSetMaterial,
            level,
            b,
        )? {
            Ok(k) => k,
            Err(code) => return Ok(Err(code)),
        };
        let id = match loader.assets.load(AssetKind::Material, &key, b) {
            Ok(id) => id,
            Err(e) => return Ok(Err(e.code)),
        };
        let scratch = b.child();
        let found = loader.assets.with_decoded(id, &scratch, |d| {
            let DecodedAsset::Material(m) = d else {
                return Ok(Err("RenderAssetKind".to_owned()));
            };
            if !matches!(m.kind, fo4_material::Kind::Bgsm(_)) {
                return Ok(Err("LandscapeMaterialNotLighting".to_owned()));
            }
            let diffuse = m
                .texture_paths()
                .find(|(name, _)| *name == "diffuse")
                .map(|(_, p)| p.bytes());
            let found = match diffuse {
                Some(p) if !p.is_empty() => {
                    let mut v = Vec::new();
                    b.reserve(&mut v, p.len(), "terrain textures", 0)?;
                    v.extend_from_slice(p);
                    Ok(v)
                }
                _ => Err("LandscapeMaterialWithoutDiffuse".to_owned()),
            };
            Ok(found)
        });
        match found {
            Ok(v) => Ok(v),
            Err(e) if e.code == "AllocationFailed" => Err(e),
            Err(e) => Ok(Err(e.code)),
        }
    }
    /// The VFS key of a citation whose path rules are accepted at `level`.
    fn resolve(
        &mut self,
        loader: &mut Loader<'_>,
        raw: &[u8],
        citation: Citation,
        level: RuleLevel,
        b: &Budget,
    ) -> Result<std::result::Result<Vec<u8>, String>> {
        let resolved = match loader.assets.resolve_path(citation, raw, b) {
            Ok(r) => r,
            Err(e) => return Ok(Err(e.code)),
        };
        if !rules_state(&resolved.hypotheses, fo4_scene::hypotheses::status).ready_at(level) {
            return Ok(Err("UnverifiedRenderPathNormalization".to_owned()));
        }
        for h in resolved.hypotheses {
            if !self.path_rules.contains(&h) {
                b.reserve(&mut self.path_rules, 1, "terrain textures", 0)?;
                self.path_rules.push(h);
            }
        }
        Ok(Ok(resolved.key))
    }
    /// The draw-list texture of texture citation `raw`, loaded once per
    /// resolved path: kept as blocks when the options say so, else decoded
    /// under them; its alpha is 1 either way.
    #[allow(clippy::too_many_arguments)]
    fn texture(
        &mut self,
        list: &mut DrawList,
        loader: &mut Loader<'_>,
        raw: &[u8],
        citation: Citation,
        level: RuleLevel,
        limits: &Limits,
        b: &Budget,
    ) -> Result<std::result::Result<usize, String>> {
        let key = match self.resolve(loader, raw, citation, level, b)? {
            Ok(k) => k,
            Err(code) => return Ok(Err(code)),
        };
        if let Some(i) = self.by_path.get(&key) {
            return Ok(Ok(*i));
        }
        let id = match loader.assets.load(AssetKind::Texture, &key, b) {
            Ok(id) => id,
            Err(e) => return Ok(Err(e.code)),
        };
        let kept = match self.options.blocks {
            Some(o) => match block_texture(
                loader,
                id,
                o.max_dimension,
                o.max_bytes,
                &mut self.block_bytes,
                b,
            ) {
                Ok(t) => t,
                Err(e) if e.code == "AllocationFailed" => return Err(e),
                Err(e) => return Ok(Err(e.code)),
            },
            None => None,
        };
        let loaded = match kept {
            Some(t) => t,
            None => {
                match decode_texture(loader, id, self.options, &mut self.texture_bytes, limits, b) {
                    Ok(t) => t,
                    // A failed allocation is the process's, not this texture's; any
                    // other error (limits included) refuses this texture only.
                    Err(e) if e.code == "AllocationFailed" => return Err(e),
                    Err(e) => return Ok(Err(e.code)),
                }
            }
        };
        let (mut texture, mut blocks) = (loaded.texture, loaded.blocks);
        // `land_layer_weights_residual_v1`: texture alpha is not used; the
        // weight rides in the vertex alpha of the overlay passes.
        if let Some(blocks) = &mut blocks {
            blocks.opaque = true;
        }
        for px in texture.rgba.as_chunks_mut::<4>().0 {
            px[3] = 1.0;
        }
        for m in &mut texture.mips {
            for px in m.rgba.as_chunks_mut::<4>().0 {
                px[3] = 1.0;
            }
        }
        match &blocks {
            Some(k) => texture.validate_blocks(k)?,
            None => texture.validate()?,
        }
        let i = list.textures.len();
        append(&mut list.textures, texture, b)?;
        if let Some(k) = blocks {
            b.charge(64, "block texture", 0)?;
            list.blocks.insert(i, k);
        }
        b.charge(key.len() as u64 + 64, "terrain textures", 0)?;
        self.by_path.insert(key, i);
        Ok(Ok(i))
    }
}

fn tally<'a>(
    values: impl Iterator<Item = &'a std::result::Result<usize, String>>,
) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for v in values {
        *m.entry(match v {
            Ok(_) => "drawn".to_owned(),
            Err(code) => code.clone(),
        })
        .or_default() += 1;
    }
    m
}

/// Whether optional rule `rule` is accepted at `level`.
fn accepted(rule: &'static str, level: RuleLevel) -> bool {
    rules_state(&[rule], fo4_scene::hypotheses::status).ready_at(level)
}

/// What one terrain piece drew.
#[derive(Debug, Default, Serialize)]
pub struct TexturedReport {
    pub grid: [i32; 2],
    /// Why no weights were built; the piece is then drawn plain grey.
    pub refusal: Option<layers::Refusal>,
    pub rules: Vec<&'static str>,
    pub quadrants_textured: u32,
    pub quadrants_untextured: u32,
    pub quadrant_refusals: BTreeMap<String, u32>,
    /// Alpha layers of the textured quadrants.
    pub layers: u32,
    pub passes: u32,
    pub overlay_triangles: u64,
    /// Sum of the weights at every quadrant vertex, by what draws them:
    /// `texture`, `default_texture` (the default drawn with the texture
    /// its world names), `default` (the default drawn grey), `refused` (a
    /// named texture that could not be drawn); divided by the vertex count.
    pub weight_share: BTreeMap<&'static str, f64>,
    /// The default texture: `drawn` or why not; absent when no quadrant
    /// has a default weight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_texture: Option<String>,
    pub texture_refusals: BTreeMap<String, u32>,
    pub over_unity_vertices: u32,
    pub vertex_colors: bool,
}

enum Pass {
    Texture(usize),
    Grey,
}

/// Appends `terrain` with its ground textures, tagged `instance`. Refused
/// weights fall back to [`append_terrain`]; refused quadrants and textures
/// are drawn grey. `cache` holds the decoded textures across pieces.
#[allow(clippy::too_many_arguments)]
pub fn append_textured_terrain(
    list: &mut DrawList,
    terrain: &Terrain,
    store: &Store,
    loader: &mut Loader<'_>,
    cache: &mut TextureCache,
    instance: u32,
    level: RuleLevel,
    limits: &Limits,
    b: &Budget,
) -> Result<TexturedReport> {
    let mut report = TexturedReport {
        grid: terrain.grid,
        ..TexturedReport::default()
    };
    let layers = match layers::load(store, terrain, level, b)? {
        Outcome::Built(l) => l,
        Outcome::Refused(r) => {
            report.refusal = Some(r);
            append_terrain(list, terrain, instance, b)?;
            return Ok(report);
        }
    };
    // Every named texture of the untouched quadrants, decoded once.
    let mut resolved = BTreeMap::new();
    for q in layers.quadrants.iter().filter(|q| q.refusal.is_none()) {
        for t in &q.textures {
            if let Source::Ltex(key) = &t.source {
                if !resolved.contains_key(key) {
                    let r = cache.ltex(list, store, loader, key, level, limits, b)?;
                    b.charge(key.plugin.len() as u64 + 64, "terrain textures", 0)?;
                    resolved.insert(key.clone(), r);
                }
            }
        }
    }
    let has_default = layers
        .quadrants
        .iter()
        .any(|q| q.refusal.is_none() && q.textures.iter().any(|t| t.source == Source::Default));
    let default = if has_default {
        let d = cache.default(
            list,
            store,
            loader,
            terrain.world.as_ref(),
            level,
            limits,
            b,
        )?;
        report.default_texture = Some(match &d {
            Ok(_) => "drawn".to_owned(),
            Err(code) => code.clone(),
        });
        d.ok()
    } else {
        None
    };
    let white = cache.white(list, b)?;
    append_layers(
        list,
        terrain,
        &layers,
        &resolved,
        default,
        white,
        instance,
        &mut report,
        b,
    )?;
    Ok(report)
}

/// Appends the passes of `layers` over `terrain`: `textures` gives the
/// draw-list texture of each named landscape texture or why it is refused,
/// `default` the texture of the default when its world names one, `white`
/// a 1 x 1 white texture for the grey passes.
#[allow(clippy::too_many_arguments)]
pub fn append_layers(
    list: &mut DrawList,
    terrain: &Terrain,
    layers: &Layers,
    textures: &BTreeMap<FormKey, std::result::Result<usize, String>>,
    default: Option<usize>,
    white: usize,
    instance: u32,
    report: &mut TexturedReport,
    b: &Budget,
) -> Result<()> {
    report.rules = layers.rules.clone();
    report.vertex_colors = layers.colors.is_some();
    let triangles: Vec<[usize; 3]> = {
        let mut v = Vec::new();
        b.reserve(&mut v, 2 * 32 * 32, "terrain", 0)?;
        v.extend(terrain.triangle_indices());
        v
    };
    let mut shares: BTreeMap<&'static str, f64> = BTreeMap::new();
    let mut vertices = 0u64;
    for q in &layers.quadrants {
        if let Some(code) = q.refusal {
            report.quadrants_untextured += 1;
            *report.quadrant_refusals.entry(code.to_owned()).or_default() += 1;
            let mut weights = Vec::new();
            b.reserve(&mut weights, QUADRANT_SIDE * QUADRANT_SIDE, "terrain", 0)?;
            weights.resize(QUADRANT_SIDE * QUADRANT_SIDE, 1.0f32);
            pass(
                list, terrain, layers, q, &triangles, &weights, white, true, true, instance, b,
            )?;
            report.passes += 1;
            continue;
        }
        report.quadrants_textured += 1;
        report.layers += q.layers as u32;
        report.over_unity_vertices += q.over_unity_vertices;
        vertices += (QUADRANT_SIDE * QUADRANT_SIDE) as u64;
        // Passes in source order; every source without a texture joins one
        // grey pass at the place of the first of them.
        let mut passes: Vec<(Pass, Vec<f32>)> = Vec::new();
        b.reserve(&mut passes, q.textures.len(), "terrain", 0)?;
        for t in &q.textures {
            let sum: f64 = t.weights.iter().map(|w| f64::from(*w)).sum();
            let drawn = match &t.source {
                Source::Default => match default {
                    Some(i) => {
                        *shares.entry("default_texture").or_default() += sum;
                        Some(i)
                    }
                    None => {
                        *shares.entry("default").or_default() += sum;
                        None
                    }
                },
                Source::Ltex(key) => match textures.get(key) {
                    Some(Ok(i)) => {
                        *shares.entry("texture").or_default() += sum;
                        Some(*i)
                    }
                    other => {
                        let code = match other {
                            Some(Err(code)) => code.clone(),
                            _ => "TextureNotResolved".to_owned(),
                        };
                        *shares.entry("refused").or_default() += sum;
                        *report.texture_refusals.entry(code).or_default() += 1;
                        None
                    }
                },
            };
            match drawn {
                Some(i) => passes.push((Pass::Texture(i), t.weights.clone())),
                None => match passes.iter_mut().find(|p| matches!(p.0, Pass::Grey)) {
                    Some(p) => {
                        for (a, x) in p.1.iter_mut().zip(&t.weights) {
                            *a += x;
                        }
                    }
                    None => passes.push((Pass::Grey, t.weights.clone())),
                },
            }
        }
        for (k, (p, weights)) in passes.iter().enumerate() {
            let first = k == 0;
            if !first && weights.iter().all(|w| *w == 0.0) {
                continue;
            }
            let (texture, grey) = match p {
                Pass::Texture(i) => (*i, false),
                Pass::Grey => (white, true),
            };
            report.overlay_triangles += pass(
                list, terrain, layers, q, &triangles, weights, texture, grey, first, instance, b,
            )?;
            report.passes += 1;
        }
    }
    if vertices > 0 {
        for (k, v) in shares {
            report.weight_share.insert(k, v / vertices as f64);
        }
    }
    Ok(())
}

/// One pass over the 512 triangles of quadrant `q`: opaque when `first`
/// (colour times weight), else an additive overlay carrying the weight in
/// vertex alpha, without the triangles whose three weights are zero.
/// Returns the overlay triangles drawn.
#[allow(clippy::too_many_arguments)]
fn pass(
    list: &mut DrawList,
    terrain: &Terrain,
    layers: &Layers,
    q: &Quadrant,
    triangles: &[[usize; 3]],
    weights: &[f32],
    texture: usize,
    grey: bool,
    first: bool,
    instance: u32,
    b: &Budget,
) -> Result<u64> {
    const SIDE: usize = fo4_plugin::land::SIDE;
    let n = 2 * layers::QUADRANT_QUADS * layers::QUADRANT_QUADS;
    let mut vertices = Vec::new();
    b.reserve(&mut vertices, 3 * n, "terrain", 0)?;
    let mut colors = Vec::new();
    b.reserve(&mut colors, 3 * n, "terrain", 0)?;
    let mut indices = Vec::new();
    b.reserve(&mut indices, n, "terrain", 0)?;
    let weight = |i: usize| -> f32 {
        let (r, c) = (i / SIDE - q.origin[0], i % SIDE - q.origin[1]);
        weights[r * QUADRANT_SIDE + c]
    };
    for [r, c] in quadrant_quads(q) {
        let k = quad_index(r, c);
        for t in &triangles[2 * k..2 * k + 2] {
            let w = t.map(weight);
            if !first && w.iter().all(|x| *x == 0.0) {
                continue;
            }
            let g = if grey {
                shade(triangle_normal(
                    t.map(|i| terrain.vertex(i / SIDE, i % SIDE)),
                ))
            } else {
                1.0
            };
            let base = u16::try_from(vertices.len()).map_err(|e| {
                fo4_core::Error::new("TerrainVertexIndex", "terrain", 0, "u16 index", e)
            })?;
            for (corner, i) in t.iter().enumerate() {
                let (r, c) = (i / SIDE, i % SIDE);
                vertices.push(Vertex {
                    position: terrain.vertex(r, c),
                    uv: texture_uv(r, c),
                });
                let tint = layers.colors.as_ref().map_or([1.0; 3], |v| v[*i]);
                let rgb = tint.map(|x| x * g);
                colors.push(if first {
                    [
                        rgb[0] * w[corner],
                        rgb[1] * w[corner],
                        rgb[2] * w[corner],
                        1.0,
                    ]
                } else {
                    [rgb[0], rgb[1], rgb[2], w[corner]]
                });
            }
            indices.push([base, base + 1, base + 2]);
        }
    }
    let drawn = indices.len() as u64;
    let mesh = list.meshes.len();
    append(
        &mut list.meshes,
        Mesh {
            vertex_colors: Some(colors),
            vertices,
            triangles: indices,
        },
        b,
    )?;
    append(
        &mut list.draws,
        Draw {
            sidedness: TWO_SIDED,
            instance,
            mesh,
            texture,
            world: Transform::IDENTITY,
            alpha: if first { OPAQUE } else { ADDITIVE },
            uv_offset: [0.0, 0.0],
            uv_scale: [1.0, 1.0],
            raw_tile_flags: None,
        },
        b,
    )?;
    if !first {
        append(&mut list.overlays, list.draws.len() - 1, b)?;
    }
    Ok(if first { 0 } else { drawn })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_plugin::land::{Heights, POINTS};
    use fo4_scene::camera;
    use fo4_scene::terrain::Diagonal;

    fn terrain() -> Terrain {
        // A slope rising to the east by one unit (8 world units) per vertex.
        let mut gradient = vec![1i8; POINTS];
        for row in gradient.as_chunks_mut::<33>().0 {
            row[0] = 0;
        }
        let key = |id| FormKey {
            plugin: "synthetic.esm".into(),
            object_id: id,
        };
        Terrain::from_heights(
            key(1),
            key(2),
            None,
            [0, 0],
            0,
            &Heights {
                subrecord: 0,
                offset: 0.0,
                gradient,
                trailing: [0; 3],
            },
            Diagonal::SouthwestNortheast,
            fo4_scene::State::Blocked,
            &Budget::new(1 << 16),
        )
        .unwrap()
    }

    #[test]
    fn terrain_draws_one_flat_shade_per_triangle_under_its_tag() {
        let t = terrain();
        let b = Budget::new(64 << 20);
        let mut list = DrawList::default();
        append_terrain(&mut list, &t, 7, &b).unwrap();
        assert_eq!(
            (list.draws.len(), list.meshes.len(), list.textures.len()),
            (1, 1, 1)
        );
        let mesh = &list.meshes[0];
        assert_eq!(mesh.triangles.len(), 2048);
        assert_eq!(mesh.vertices.len(), 3 * 2048);
        // Every triangle of a plane faces the same way: one shade.
        let colors = mesh.vertex_colors.as_ref().unwrap();
        let g = shade(triangle_normal(t.world_triangles().next().unwrap()));
        assert!(colors.iter().all(|c| *c == [g, g, g, 1.0]));
        assert!(g > DARK && g < BRIGHT);
        let cam = camera::look_at(
            [2048.0, -3000.0, 3000.0],
            [2048.0, 2048.0, 128.0],
            camera::reference_frustum([64, 36]),
            [64, 36],
        )
        .unwrap();
        let frame = crate::render(
            &list,
            &cam,
            crate::RenderOptions::REFERENCE,
            &fo4_core::Limits::default(),
            &b,
        )
        .unwrap();
        let tagged = frame.instances.iter().filter(|i| **i == 7).count();
        assert!(tagged > 64 * 36 / 8, "{tagged}");
        let at = frame.instances.iter().position(|i| *i == 7).unwrap();
        assert!((frame.color[at][0] - g).abs() < 1e-6);
    }

    fn solid(rgba: [f32; 4]) -> Texture {
        Texture {
            mips: Vec::new(),
            width: 1,
            height: 1,
            rgba: rgba.to_vec(),
            mip: 0,
            srgb: false,
        }
    }

    /// Quadrant 0: base A and layer B at opacity 0.5 everywhere; quadrant 1:
    /// base A; quadrant 2: no base (the default); quadrant 3: base C, whose
    /// texture is refused.
    fn layered() -> (Terrain, fo4_scene::terrain::layers::Layers) {
        use fo4_plugin::land::{AlphaHeader, AlphaLayer, AlphaPoint, BaseLayer, Land};
        use fo4_plugin::typed::FormRef;
        let form = |id: u32| {
            Some(FormRef {
                raw: id,
                key: Some(FormKey {
                    plugin: "synthetic.esm".into(),
                    object_id: id,
                }),
                subrecord: 0,
            })
        };
        let base = |quadrant, id| BaseLayer {
            subrecord: 0,
            texture: form(id),
            quadrant,
            raw: [0; 3],
        };
        let land = Land {
            data: None,
            heights: None,
            normals: None,
            colors: None,
            base_layers: vec![base(0, 0xa), base(1, 0xa), base(3, 0xc)],
            alpha_layers: vec![AlphaLayer {
                header: AlphaHeader {
                    subrecord: 0,
                    texture: form(0xb),
                    quadrant: 0,
                    unknown: 0,
                    layer: 0,
                },
                point_subrecords: vec![0],
                points: (0..289)
                    .map(|position| AlphaPoint {
                        position,
                        unknown: [0; 2],
                        opacity: 0.5,
                    })
                    .collect(),
            }],
            textures: Vec::new(),
            uninterpreted: Vec::new(),
            non_finite_values: 0,
        };
        let t = terrain();
        let l = fo4_scene::terrain::layers::from_land(
            t.land.clone(),
            &land,
            fo4_scene::State::Blocked,
            &Budget::new(1 << 20),
        )
        .unwrap();
        (t, l)
    }

    #[test]
    fn texture_passes_sum_their_weights_over_one_opaque_pass() {
        let (t, layers) = layered();
        let b = Budget::new(64 << 20);
        let mut list = DrawList::default();
        list.textures.push(solid([1.0, 0.0, 0.0, 1.0]));
        list.textures.push(solid([0.0, 1.0, 0.0, 1.0]));
        list.textures.push(solid([1.0; 4]));
        let key = |id| FormKey {
            plugin: "synthetic.esm".into(),
            object_id: id,
        };
        let textures = BTreeMap::from([
            (key(0xa), Ok(0)),
            (key(0xb), Ok(1)),
            (key(0xc), Err("TextureSetWithoutTx00".to_owned())),
        ]);
        let mut report = TexturedReport::default();
        append_layers(
            &mut list,
            &t,
            &layers,
            &textures,
            None,
            2,
            7,
            &mut report,
            &b,
        )
        .unwrap();
        // Quadrant 0: opaque A then the overlay B; one opaque pass each for
        // the others, grey for the default and the refused texture.
        assert_eq!(list.draws.len(), 5);
        assert_eq!(list.overlays, [1]);
        assert_eq!(
            list.draws.iter().map(|d| d.texture).collect::<Vec<_>>(),
            [0, 1, 0, 2, 2]
        );
        assert_eq!(list.draws[1].alpha.enabled, 1);
        assert!(list.draws.iter().all(|d| d.instance == 7));
        assert_eq!(
            (report.quadrants_textured, report.passes, report.layers),
            (4, 5, 1)
        );
        assert_eq!(report.overlay_triangles, 512);
        assert_eq!(report.texture_refusals["TextureSetWithoutTx00"], 1);
        assert_eq!(
            report.weight_share,
            BTreeMap::from([("default", 0.25), ("refused", 0.25), ("texture", 0.5)])
        );
        let cam = camera::look_at(
            [2048.0, 1500.0, 9000.0],
            [2048.0, 2048.0, 128.0],
            camera::reference_frustum([96, 96]),
            [96, 96],
        )
        .unwrap();
        let at = |frame: &crate::Frame, x: f64, y: f64| {
            let p = cam
                .project_coordinates(cam.coordinates([x, y, t.height_at(x, y).unwrap()]))
                .unwrap()
                .pixel;
            let i = p[1] as usize * 96 + p[0] as usize;
            (frame.color[i], frame.instances[i])
        };
        let draw = |list: &DrawList| {
            crate::render(
                list,
                &cam,
                crate::RenderOptions::REFERENCE,
                &fo4_core::Limits::default(),
                &b,
            )
            .unwrap()
        };
        let frame = draw(&list);
        let (c, tag) = at(&frame, 1024.0, 1024.0);
        assert_eq!(tag, 7);
        assert!((c[0] - 0.5).abs() < 1e-5 && (c[1] - 0.5).abs() < 1e-5 && c[2] == 0.0);
        assert_eq!(at(&frame, 3072.0, 1024.0).0[..3], [1.0, 0.0, 0.0]);
        let g = shade(triangle_normal(t.world_triangles().next().unwrap()));
        for (x, y) in [(1024.0, 3072.0), (3072.0, 3072.0)] {
            let c = at(&frame, x, y).0;
            assert!(c[..3].iter().all(|v| (v - g).abs() < 1e-5), "{c:?}");
        }
        // Without the overlay mark the B pass ties with the opaque depth and
        // is dropped: only A's half weight remains.
        list.overlays.clear();
        let c = at(&draw(&list), 1024.0, 1024.0).0;
        assert!((c[0] - 0.5).abs() < 1e-5 && c[1] == 0.0);
        // A malformed overlay set is refused, and snapshots carry none.
        list.overlays = vec![1, 1];
        assert!(crate::render(
            &list,
            &cam,
            crate::RenderOptions::REFERENCE,
            &fo4_core::Limits::default(),
            &b
        )
        .is_err());
        list.overlays = vec![1];
        let e = crate::snapshot::write(&list, &mut std::io::sink(), &fo4_core::Limits::default())
            .unwrap_err();
        assert_eq!(e.code, "SnapshotOverlayDraws");
    }

    #[test]
    fn a_named_default_draws_the_default_weight_with_its_texture() {
        let (t, layers) = layered();
        let b = Budget::new(64 << 20);
        let mut list = DrawList::default();
        list.textures.push(solid([1.0, 0.0, 0.0, 1.0]));
        list.textures.push(solid([0.0, 1.0, 0.0, 1.0]));
        list.textures.push(solid([1.0; 4]));
        list.textures.push(solid([0.0, 0.0, 1.0, 1.0]));
        let key = |id| FormKey {
            plugin: "synthetic.esm".into(),
            object_id: id,
        };
        let textures = BTreeMap::from([(key(0xa), Ok(0)), (key(0xb), Ok(1)), (key(0xc), Ok(0))]);
        let mut report = TexturedReport::default();
        append_layers(
            &mut list,
            &t,
            &layers,
            &textures,
            Some(3),
            2,
            7,
            &mut report,
            &b,
        )
        .unwrap();
        // Quadrant 2 (no base) is one opaque pass of the default's texture.
        assert_eq!(
            list.draws.iter().map(|d| d.texture).collect::<Vec<_>>(),
            [0, 1, 0, 3, 0]
        );
        assert_eq!(
            report.weight_share,
            BTreeMap::from([("default_texture", 0.25), ("texture", 0.75)])
        );
        let colors = list.meshes[list.draws[3].mesh]
            .vertex_colors
            .as_ref()
            .unwrap();
        assert!(colors.iter().all(|c| *c == [1.0, 1.0, 1.0, 1.0]));
    }

    #[test]
    fn vertex_colours_multiply_every_pass() {
        let (t, mut layers) = layered();
        layers.colors = Some(vec![[0.5, 1.0, 0.25]; fo4_plugin::land::POINTS]);
        let b = Budget::new(64 << 20);
        let mut list = DrawList::default();
        list.textures.push(solid([1.0, 0.0, 0.0, 1.0]));
        list.textures.push(solid([0.0, 1.0, 0.0, 1.0]));
        list.textures.push(solid([1.0; 4]));
        let key = |id| FormKey {
            plugin: "synthetic.esm".into(),
            object_id: id,
        };
        let textures = BTreeMap::from([(key(0xa), Ok(0)), (key(0xb), Ok(1))]);
        let mut report = TexturedReport::default();
        append_layers(
            &mut list,
            &t,
            &layers,
            &textures,
            None,
            2,
            7,
            &mut report,
            &b,
        )
        .unwrap();
        assert!(report.vertex_colors);
        // An unresolved name is refused, not drawn.
        assert_eq!(report.texture_refusals["TextureNotResolved"], 1);
        let opaque = list.meshes[list.draws[0].mesh]
            .vertex_colors
            .as_ref()
            .unwrap();
        assert!(opaque.iter().all(|c| *c == [0.25, 0.5, 0.125, 1.0]));
        let overlay = list.meshes[list.draws[1].mesh]
            .vertex_colors
            .as_ref()
            .unwrap();
        assert!(overlay.iter().all(|c| *c == [0.5, 1.0, 0.25, 0.5]));
        for v in &list.meshes[list.draws[0].mesh].vertices {
            let [x, y] = [v.position[0] / 256.0, -v.position[1] / 256.0];
            assert_eq!(v.uv, [x, y]);
        }
    }

    #[test]
    fn shade_is_bounded_and_monotone_in_facing() {
        assert_eq!(shade([0.0, 0.0, 0.0]), DARK);
        let l = LIGHT.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((shade(LIGHT.map(|x| x / l)) - BRIGHT).abs() < 1e-6);
        assert_eq!(shade([1.0, 1.0, 0.0].map(|x: f64| x / 2f64.sqrt())), DARK);
        assert!(shade([0.0, 0.0, 1.0]) > shade([1.0, 0.0, 0.0]));
    }
}
