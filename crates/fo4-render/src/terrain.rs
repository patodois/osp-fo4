//! Terrain of a cell as plain geometry in a draw list (`spec/land.md`).
//!
//! A diagnostic drawing, not a landscape renderer: no landscape textures,
//! no vertex colours and no game lighting. Each triangle gets one neutral
//! grey whose brightness follows the angle between its geometric normal and
//! a fixed direction, so that slopes read in an unlit frame.
use crate::{append, Alpha, Draw, DrawList, Mesh, Sidedness, Texture, Vertex};
use fo4_core::{Budget, Result};
use fo4_scene::collision::triangle_normal;
use fo4_scene::terrain::Terrain;
use fo4_scene::transform::Transform;

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

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_plugin::land::{Heights, POINTS};
    use fo4_plugin::FormKey;
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

    #[test]
    fn shade_is_bounded_and_monotone_in_facing() {
        assert_eq!(shade([0.0, 0.0, 0.0]), DARK);
        let l = LIGHT.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((shade(LIGHT.map(|x| x / l)) - BRIGHT).abs() < 1e-6);
        assert_eq!(shade([1.0, 1.0, 0.0].map(|x: f64| x / 2f64.sqrt())), DARK);
        assert!(shade([0.0, 0.0, 1.0]) > shade([1.0, 0.0, 0.0]));
    }
}
