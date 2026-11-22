//! Ground textures of a terrain: the LAND base and alpha layers turned into
//! per-vertex texture weights under registered rules (`spec/land.md`,
//! "Ground textures").
//!
//! `fo4_plugin::land` keeps BTXT, ATXT and VTXT as stored. How they become
//! a surface is not settled by any source for Fallout 4: which part of the
//! 33 x 33 grid a quadrant covers, how a VTXT position maps to a vertex,
//! how the opacities combine with the base, what a missing base means, which
//! file draws a landscape texture, how often it repeats and what VCLR does.
//! Each is a pending rule in `spec/hypotheses.tsv`; [`load`] builds weights
//! only at a level that accepts them all, and reports the rest.
//!
//! The weights are linear: at every vertex each texture of a quadrant has a
//! weight, the colour is the sum of weight times texture colour, and the
//! weights are interpolated across each triangle like any vertex attribute.
use super::{Terrain, QUADS};
use crate::collision::rules_state;
use crate::hypotheses::{self, Status};
use crate::{RuleLevel, State};
use fo4_core::{Budget, Result};
use fo4_plugin::land::{Land, POINTS, SIDE};
use fo4_plugin::typed::{Store, Txst};
use fo4_plugin::FormKey;
use serde::Serialize;

/// Vertices per side of a quadrant under `land_quadrant_layout_v1`.
pub const QUADRANT_SIDE: usize = 17;
/// Vertices of a quadrant.
pub const QUADRANT_POINTS: usize = QUADRANT_SIDE * QUADRANT_SIDE;
/// Quads per side of a quadrant.
pub const QUADRANT_QUADS: usize = QUADRANT_SIDE - 1;
/// World units of one texture repeat under `land_texture_repeat_256_v1`.
pub const TEXTURE_REPEAT: f64 = 256.0;

/// Rules the ground textures rely on, besides the terrain's own.
pub const RULES: [&str; 7] = [
    hypotheses::LAND_QUADRANTS,
    hypotheses::LAND_VTXT_POSITION,
    hypotheses::LAND_LAYER_WEIGHTS,
    hypotheses::LAND_MISSING_BASE,
    hypotheses::LTEX_TEXTURE_SET,
    hypotheses::LAND_TEXTURE_REPEAT,
    hypotheses::LAND_VERTEX_COLORS,
];

/// What draws one weight of a quadrant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A landscape texture record (LTEX) named by BTXT or ATXT.
    Ltex(FormKey),
    /// The texture of a quadrant without BTXT, or of an ATXT whose FormID
    /// is zero (`land_missing_base_default_v1`). No record names it.
    Default,
}

/// One texture of a quadrant and its weight at the quadrant's 289 vertices
/// (index `17 r + c`, row 0 south, column 0 west).
#[derive(Debug, Serialize)]
pub struct Weighted {
    pub source: Source,
    pub weights: Vec<f32>,
}

/// A quadrant: its textures with their weights, or why it has none.
#[derive(Debug, Serialize)]
pub struct Quadrant {
    pub index: u8,
    /// First row and column of the quadrant in the cell's 33 x 33 grid.
    pub origin: [usize; 2],
    pub base: Option<Source>,
    pub layers: usize,
    /// Distinct sources, the base first, then layers in ascending layer
    /// number; a source named twice is one entry with summed weights.
    pub textures: Vec<Weighted>,
    /// Vertices whose layer opacities sum to more than one; the base weight
    /// is zero there and the sum is kept as stored.
    pub over_unity_vertices: u32,
    /// Why the quadrant has no weights; it is then drawn as untextured.
    pub refusal: Option<&'static str>,
}

/// The ground textures of one terrain.
#[derive(Debug, Serialize)]
pub struct Layers {
    pub land: FormKey,
    pub quadrants: Vec<Quadrant>,
    /// VCLR as multipliers in [0, 1] at index `33 r + c`, when present.
    pub colors: Option<Vec<[f32; 3]>>,
    pub rules: Vec<&'static str>,
    pub state: State,
}

/// Why a terrain has no texture weights.
#[derive(Debug, Clone, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub enum Outcome {
    Built(Box<Layers>),
    Refused(Refusal),
}

/// Grid row and column of quadrant `q` under `land_quadrant_layout_v1`:
/// 0 south-west, 1 south-east, 2 north-west, 3 north-east.
pub fn quadrant_origin(q: u8) -> Option<[usize; 2]> {
    (q < 4).then(|| {
        let q = usize::from(q);
        [QUADRANT_QUADS * (q / 2), QUADRANT_QUADS * (q % 2)]
    })
}

/// Quadrant-local vertex of a VTXT position under `land_vtxt_position_grid_v1`.
pub fn position_vertex(position: u16) -> Option<[usize; 2]> {
    let p = usize::from(position);
    (p < QUADRANT_POINTS).then_some([p / QUADRANT_SIDE, p % QUADRANT_SIDE])
}

/// Texture coordinates of grid vertex (row, column) under
/// `land_texture_repeat_256_v1`: u grows east, v grows south, one repeat
/// per 256 world units, cell-local (a cell holds 16 whole repeats).
pub fn texture_uv(row: usize, column: usize) -> [f64; 2] {
    let step = super::SPACING / TEXTURE_REPEAT;
    [column as f64 * step, -(row as f64) * step]
}

fn quadrant(land: &Land, q: u8, b: &Budget) -> Result<Quadrant> {
    let origin = quadrant_origin(q).unwrap_or_default();
    let mut out = Quadrant {
        index: q,
        origin,
        base: None,
        layers: 0,
        textures: Vec::new(),
        over_unity_vertices: 0,
        refusal: None,
    };
    let source = |t: &Option<fo4_plugin::typed::FormRef>| -> std::result::Result<Source, ()> {
        match t {
            None => Ok(Source::Default),
            Some(f) => f.key.clone().map(Source::Ltex).ok_or(()),
        }
    };
    let bases: Vec<_> = land
        .base_layers
        .iter()
        .filter(|l| l.quadrant == q)
        .collect();
    let base = match bases.as_slice() {
        [] => Source::Default,
        [one] => match source(&one.texture) {
            Ok(s) => s,
            Err(()) => {
                out.refusal = Some("UnresolvedBaseTexture");
                return Ok(out);
            }
        },
        _ => {
            out.refusal = Some("MultipleBaseLayers");
            return Ok(out);
        }
    };
    out.base = Some(base.clone());
    let mut layers: Vec<_> = land
        .alpha_layers
        .iter()
        .filter(|l| l.header.quadrant == q)
        .collect();
    layers.sort_by_key(|l| l.header.layer);
    out.layers = layers.len();
    let mut entries: Vec<(Source, Vec<f32>)> = Vec::new();
    b.reserve(&mut entries, layers.len() + 1, "terrain layers", 0)?;
    let mut total = Vec::new();
    b.reserve(&mut total, QUADRANT_POINTS, "terrain layers", 0)?;
    total.resize(QUADRANT_POINTS, 0f32);
    for layer in &layers {
        let Ok(s) = source(&layer.header.texture) else {
            out.refusal = Some("UnresolvedLayerTexture");
            return Ok(out);
        };
        let mut w = Vec::new();
        b.reserve(&mut w, QUADRANT_POINTS, "terrain layers", 0)?;
        w.resize(QUADRANT_POINTS, 0f32);
        let mut seen = [false; QUADRANT_POINTS];
        for p in &layer.points {
            let Some([r, c]) = position_vertex(p.position) else {
                out.refusal = Some("LayerPositionOutsideQuadrant");
                return Ok(out);
            };
            if !p.opacity.is_finite() || !(0.0..=1.0).contains(&p.opacity) {
                out.refusal = Some("LayerOpacityOutsideUnitRange");
                return Ok(out);
            }
            let i = r * QUADRANT_SIDE + c;
            if std::mem::replace(&mut seen[i], true) {
                out.refusal = Some("DuplicateLayerPosition");
                return Ok(out);
            }
            w[i] = p.opacity;
            total[i] += p.opacity;
        }
        entries.push((s, w));
    }
    let mut base_weights = Vec::new();
    b.reserve(&mut base_weights, QUADRANT_POINTS, "terrain layers", 0)?;
    for t in &total {
        if *t > 1.0 {
            out.over_unity_vertices += 1;
        }
        base_weights.push((1.0 - t).max(0.0));
    }
    entries.insert(0, (base, base_weights));
    b.reserve(&mut out.textures, entries.len(), "terrain layers", 0)?;
    for (s, w) in entries {
        match out.textures.iter_mut().find(|t| t.source == s) {
            Some(t) => {
                for (a, x) in t.weights.iter_mut().zip(&w) {
                    *a += x;
                }
            }
            None => out.textures.push(Weighted {
                source: s,
                weights: w,
            }),
        }
    }
    Ok(out)
}

/// Texture weights of `land` and its VCLR multipliers, under the candidate
/// rules, without consulting the registry: `rules` and `state` are recorded
/// as given. [`load`] is the gated entry point.
pub fn from_land(land_key: FormKey, land: &Land, state: State, b: &Budget) -> Result<Layers> {
    let mut quadrants = Vec::new();
    b.reserve(&mut quadrants, 4, "terrain layers", 0)?;
    for q in 0..4 {
        quadrants.push(quadrant(land, q, b)?);
    }
    let colors = match &land.colors {
        Some(c) if c.values.len() == POINTS => {
            let mut v = Vec::new();
            b.reserve(&mut v, POINTS, "terrain colours", 0)?;
            v.extend(c.values.iter().map(|p| p.map(|x| f32::from(x) / 255.0)));
            Some(v)
        }
        _ => None,
    };
    let mut rules = Vec::new();
    b.reserve(&mut rules, RULES.len(), "terrain layers", 0)?;
    rules.extend(RULES);
    rules.sort_unstable();
    Ok(Layers {
        land: land_key,
        quadrants,
        colors,
        rules,
        state,
    })
}

/// Texture weights of a built terrain at `level` with the embedded registry.
pub fn load(store: &Store, terrain: &Terrain, level: RuleLevel, b: &Budget) -> Result<Outcome> {
    load_with(store, terrain, level, &hypotheses::status, b)
}

/// [`load`] against an explicit status table.
pub fn load_with(
    store: &Store,
    terrain: &Terrain,
    level: RuleLevel,
    status: &dyn Fn(&str) -> Option<Status>,
    b: &Budget,
) -> Result<Outcome> {
    let state = rules_state(&RULES, status);
    if !state.ready_at(level) {
        return Ok(Outcome::Refused(Refusal {
            code: "TextureRulesNotAccepted",
            detail: format!("{state:?} at {level:?}"),
        }));
    }
    let Some(land) = store.get_land(&terrain.land, b)? else {
        return Ok(Outcome::Refused(Refusal {
            code: "NoLand",
            detail: "LAND winner absent".into(),
        }));
    };
    Ok(Outcome::Built(Box::new(from_land(
        terrain.land.clone(),
        &land.data,
        state,
        b,
    )?)))
}

/// Optional rules of the ground textures: each applies only where the
/// level accepts it, and otherwise leaves what it covers refused.
pub const OPTIONAL_RULES: [&str; 2] = [hypotheses::LTEX_MATERIAL, hypotheses::LAND_DEFAULT_NAMED];

/// What draws a texture set's colour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourTexture {
    /// Its TX00 path (`ltex_tnam_texture_set_tx00_v1`).
    TextureSet(String),
    /// The MNAM material path of a texture set without TX00, whose
    /// diffuse texture draws it (`ltex_texture_set_material_diffuse_v1`).
    Material(String),
}

/// The colour texture of a landscape texture record under
/// `ltex_tnam_texture_set_v1`: the TX00 path of the TXST its TNAM names.
/// `Ok(Err(code))` is a refusal of that texture only.
pub fn colour_texture_path(
    store: &Store,
    ltex: &FormKey,
    b: &Budget,
) -> Result<std::result::Result<String, &'static str>> {
    Ok(match colour_texture(store, ltex, b)? {
        Ok(ColourTexture::TextureSet(path)) => Ok(path),
        Ok(ColourTexture::Material(_)) => Err("TextureSetWithoutTx00"),
        Err(code) => Err(code),
    })
}

/// [`colour_texture_path`], with a texture set that has no TX00 but an
/// MNAM material answered by that material
/// (`ltex_texture_set_material_diffuse_v1`; the caller checks the rule).
pub fn colour_texture(
    store: &Store,
    ltex: &FormKey,
    b: &Budget,
) -> Result<std::result::Result<ColourTexture, &'static str>> {
    match texture_set_of(store, ltex, b)? {
        Ok(txst) => texture_set_colour(store, &txst, b),
        Err(code) => Ok(Err(code)),
    }
}

/// The colour of texture set `txst`: TX00, else the MNAM material.
pub fn texture_set_colour(
    store: &Store,
    txst: &FormKey,
    b: &Budget,
) -> Result<std::result::Result<ColourTexture, &'static str>> {
    match store.overrides().winner(txst) {
        Some(w) if w.signature == *b"TXST" => {}
        _ => return Ok(Err("LandscapeTextureTnamNotTextureSet")),
    }
    let Some(set) = store.get::<Txst>(txst, b)? else {
        return Ok(Err("LandscapeTextureTnamNotTextureSet"));
    };
    Ok(match (&set.data.textures[0], &set.data.material) {
        (Some(path), _) => Ok(ColourTexture::TextureSet(path.clone())),
        (None, Some(material)) => Ok(ColourTexture::Material(material.clone())),
        (None, None) => Err("TextureSetWithoutColour"),
    })
}

/// The texture set of `world`'s default ground under
/// `land_default_texture_world_named_v1`: among the texture sets that the
/// TNAM of some landscape texture record names, the single one whose editor
/// ID contains, ignoring ASCII case, both the world's editor ID and
/// `default`. `Ok(Err(code))` when there is none or more than one.
pub fn default_texture_set(
    store: &Store,
    world: &FormKey,
    b: &Budget,
) -> Result<std::result::Result<FormKey, &'static str>> {
    match store.overrides().winner(world) {
        Some(w) if w.signature == *b"WRLD" => {}
        Some(_) => return Ok(Err("NotAWorld")),
        None => return Ok(Err("MissingWorld")),
    }
    let Some(w) = store.raw(world, b)? else {
        return Ok(Err("MissingWorld"));
    };
    let Some(name) = editor_id(&w) else {
        return Ok(Err("WorldWithoutEditorId"));
    };
    let order = store.order();
    let keys: Vec<FormKey> = store
        .winners()
        .filter(|w| w.signature == *b"LTEX" && !w.deleted)
        .map(|w| store.overrides().form_key(w.key, order))
        .collect();
    let mut found: Vec<FormKey> = Vec::new();
    for ltex in &keys {
        let Ok(txst) = texture_set_of(store, ltex, b)? else {
            continue;
        };
        if found.contains(&txst) {
            continue;
        }
        let Some(set) = store.raw(&txst, b)? else {
            continue;
        };
        let Some(id) = editor_id(&set) else {
            continue;
        };
        if id.contains(&name) && id.contains("default") {
            b.reserve(&mut found, 1, "terrain default texture", 0)?;
            found.push(txst);
        }
    }
    Ok(match found.len() {
        0 => Err("NoNamedDefaultTexture"),
        1 => Ok(found.remove(0)),
        _ => Err("SeveralNamedDefaultTextures"),
    })
}

/// A record's editor ID in ASCII lower case, without its terminator.
fn editor_id(record: &fo4_plugin::Record) -> Option<String> {
    let s = record.subrecords.iter().find(|s| s.signature == *b"EDID")?;
    let d = record.data(s);
    let d = d.strip_suffix(&[0]).unwrap_or(d);
    (!d.is_empty()).then(|| String::from_utf8_lossy(d).to_ascii_lowercase())
}

/// The TXST that the TNAM of landscape texture record `ltex` names.
fn texture_set_of(
    store: &Store,
    ltex: &FormKey,
    b: &Budget,
) -> Result<std::result::Result<FormKey, &'static str>> {
    let Some(winner) = store.overrides().winner(ltex) else {
        return Ok(Err("MissingLandscapeTexture"));
    };
    if winner.signature != *b"LTEX" {
        return Ok(Err("NotALandscapeTexture"));
    }
    let plugin = winner.plugin;
    let Some(record) = store.raw(ltex, b)? else {
        return Ok(Err("MissingLandscapeTexture"));
    };
    let tnam: Vec<_> = record
        .subrecords
        .iter()
        .filter(|s| s.signature == *b"TNAM")
        .collect();
    let [s] = tnam.as_slice() else {
        return Ok(Err("LandscapeTextureWithoutSingleTnam"));
    };
    let d = record.data(s);
    let Ok(raw) = <[u8; 4]>::try_from(d) else {
        return Ok(Err("LandscapeTextureTnamSize"));
    };
    let Some(txst) = store.order().remap(
        plugin,
        u32::from_le_bytes(raw),
        &record.source.file,
        s.offset,
    )?
    else {
        return Ok(Err("LandscapeTextureNullTnam"));
    };
    match store.overrides().winner(&txst) {
        Some(w) if w.signature == *b"TXST" => Ok(Ok(txst)),
        _ => Ok(Err("LandscapeTextureTnamNotTextureSet")),
    }
}

/// The quads of a quadrant, as (grid row, grid column) of their south-west
/// corners, row by row from the south-west.
pub fn quadrant_quads(q: &Quadrant) -> impl Iterator<Item = [usize; 2]> + '_ {
    (0..QUADRANT_QUADS * QUADRANT_QUADS).map(move |k| {
        [
            q.origin[0] + k / QUADRANT_QUADS,
            q.origin[1] + k % QUADRANT_QUADS,
        ]
    })
}

/// Index of quad (row, column) in `Terrain::triangle_indices` order: its
/// two triangles are `2 i` and `2 i + 1`.
pub fn quad_index(row: usize, column: usize) -> usize {
    row * QUADS + column
}

const _: () = assert!(2 * QUADRANT_QUADS == SIDE - 1);

#[cfg(test)]
mod tests;
