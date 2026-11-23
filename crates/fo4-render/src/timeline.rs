//! The actors of a run timeline's frame as placeholder markers
//! (`fo4_scene::timeline`, `spec/timeline.md`). Presentation only: an
//! upright magenta box with a yellow pointed roof, standing on the place
//! the run puts the actor; its size and colours are this module's choice
//! and say nothing about the actor or the game. Skinned characters are not
//! drawn in a timeline frame.
use crate::{append, Alpha, Draw, DrawList, Mesh, Sidedness, Texture, Vertex};
use fo4_core::{Budget, Result};
use fo4_scene::transform::Transform;

/// Half the side of a marker's box, in world units (presentation).
pub const MARKER_HALF_SIDE: f64 = 20.0;
/// Height of a marker's box above the actor's position (presentation).
pub const MARKER_HEIGHT: f64 = 128.0;
/// Height of the roof above the box (presentation).
pub const MARKER_ROOF: f64 = 32.0;
/// The box's colour and the roof's (presentation: chosen to look
/// artificial).
pub const MARKER_BOX_RGBA: [f32; 4] = [1.0, 0.0, 1.0, 1.0];
pub const MARKER_ROOF_RGBA: [f32; 4] = [1.0, 1.0, 0.0, 1.0];

/// The marker's mesh: the box's four sides and bottom on texel 0, the
/// roof on texel 1 of a 2 x 1 texture.
fn marker_mesh() -> Mesh {
    let (s, h, r) = (MARKER_HALF_SIDE, MARKER_HEIGHT, MARKER_ROOF);
    let corners = [[-s, -s], [s, -s], [s, s], [-s, s]];
    let mut vertices = Vec::new();
    let v = |x: f64, y: f64, z: f64, u: f64| Vertex {
        position: [x, y, z],
        uv: [u, 0.5],
    };
    // 0..4 bottom, 4..8 top of the box (texel 0).
    for z in [0.0, h] {
        for c in corners {
            vertices.push(v(c[0], c[1], z, 0.25));
        }
    }
    // 8..12 the top again and 12 the apex (texel 1).
    for c in corners {
        vertices.push(v(c[0], c[1], h, 0.75));
    }
    vertices.push(v(0.0, 0.0, h + r, 0.75));
    let mut triangles: Vec<[u16; 3]> = vec![[0, 2, 1], [0, 3, 2]];
    for i in 0..4u16 {
        let j = (i + 1) % 4;
        triangles.push([i, j, j + 4]);
        triangles.push([i, j + 4, i + 4]);
        triangles.push([8 + i, 8 + j, 12]);
    }
    Mesh {
        vertex_colors: None,
        normals: None,
        vertices,
        triangles,
    }
}

/// Appends one marker per position, tagged `first_tag`, `first_tag + 1`,
/// ... in order, each drawn opaque and double-sided; returns the tags.
/// Nothing is appended for no position.
pub fn append_markers(
    list: &mut DrawList,
    at: &[[f64; 3]],
    first_tag: u32,
    b: &Budget,
) -> Result<Vec<u32>> {
    let mut tags = Vec::new();
    if at.is_empty() {
        return Ok(tags);
    }
    let mesh = list.meshes.len();
    append(&mut list.meshes, marker_mesh(), b)?;
    let texture = list.textures.len();
    append(
        &mut list.textures,
        Texture {
            mips: Vec::new(),
            width: 2,
            height: 1,
            rgba: [MARKER_BOX_RGBA, MARKER_ROOF_RGBA].concat(),
            mip: 0,
            srgb: false,
        },
        b,
    )?;
    for (k, p) in at.iter().enumerate() {
        let tag = u32::try_from(k)
            .ok()
            .and_then(|k| first_tag.checked_add(k))
            .ok_or_else(|| {
                fo4_core::Error::new("MarkerTagOverflow", "timeline", 0, "u32 tags", k)
            })?;
        append(
            &mut list.draws,
            Draw {
                sidedness: Sidedness {
                    material_two_sided: Some(1),
                    nif_shader_flags_2: None,
                },
                instance: tag,
                mesh,
                texture,
                world: Transform {
                    translation: *p,
                    ..Transform::IDENTITY
                },
                alpha: Alpha {
                    enabled: 0,
                    source: 0,
                    destination: 0,
                    test_enabled: 0,
                    test_ref: 0,
                    nif_test_function: Some(4),
                    opacity: 1.0,
                    nif_flags: None,
                },
                uv_offset: [0.0; 2],
                uv_scale: [1.0; 2],
                raw_tile_flags: None,
                falloff: None,
                palette: None,
                colour: None,
            },
            b,
        )?;
        tags.push(tag);
    }
    Ok(tags)
}

/// The first tag after every draw of the list (0 for an empty list).
pub fn next_tag(list: &DrawList) -> u32 {
    list.draws
        .iter()
        .map(|d| d.instance.saturating_add(1))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_are_drawn_where_the_frame_puts_them_with_their_own_tags() {
        let mut list = DrawList::default();
        let b = Budget::new(1 << 20);
        assert!(append_markers(&mut list, &[], 0, &b).unwrap().is_empty());
        assert!(list.draws.is_empty() && list.meshes.is_empty());
        let tags = append_markers(&mut list, &[[0., 0., 0.], [300., 0., 0.]], 7, &b).unwrap();
        assert_eq!(tags, vec![7, 8]);
        assert_eq!(next_tag(&list), 9);
        assert_eq!(list.meshes.len(), 1);
        assert_eq!(list.draws[1].world.translation, [300., 0., 0.]);
        list.validate_texture(0).unwrap();
        // Seen from the side at eye height: magenta box, yellow roof.
        let camera = fo4_scene::camera::look_at(
            [150., -600., 80.],
            [150., 0., 80.],
            fo4_scene::camera::reference_frustum([160, 90]),
            [160, 90],
        )
        .unwrap();
        let f = crate::render(
            &list,
            &camera,
            crate::RenderOptions::REFERENCE,
            &fo4_core::Limits::default(),
            &Budget::new(64 << 20),
        )
        .unwrap();
        let tagged: std::collections::BTreeSet<u32> = f.instances.iter().copied().collect();
        assert!(tagged.contains(&7) && tagged.contains(&8), "{tagged:?}");
        let magenta = f
            .color
            .iter()
            .filter(|c| c[0] > 0.9 && c[1] < 0.1 && c[2] > 0.9)
            .count();
        let yellow = f
            .color
            .iter()
            .filter(|c| c[0] > 0.9 && c[1] > 0.9 && c[2] < 0.1)
            .count();
        assert!(magenta > 100 && yellow > 10, "{magenta} {yellow}");
    }
}
