//! The actors of a run timeline's frame as placeholder markers
//! (`fo4_scene::timeline`, `spec/timeline.md`). Presentation only: an
//! upright magenta box with a yellow pointed roof, standing on the place
//! the run puts the actor; its size and colours are this module's choice
//! and say nothing about the actor or the game. With `--timeline-cast` the
//! actors are drawn as their characters (`fo4_scene::timeline::cast`,
//! skinned by `crate::draw::build_with_actors`) and a marker stands only
//! for an actor the build cannot draw ([`cast_markers`]).
use crate::{append, draw::Build, Alpha, Draw, DrawList, Mesh, Sidedness, Texture, Vertex};
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
    let posed: Vec<([f64; 3], bool)> = at.iter().map(|p| (*p, false)).collect();
    append_markers_posed(list, &posed, first_tag, b)
}

/// [`append_markers`] with, for each marker, whether it is drawn lying (a
/// dead actor under `timeline_dead_actor_lying_placeholder_v1`: the box
/// turned as `fo4_scene::timeline::lying_rotation` turns a character).
/// With no marker lying the draws are exactly [`append_markers`]'s.
pub fn append_markers_posed(
    list: &mut DrawList,
    at: &[([f64; 3], bool)],
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
    for (k, (p, lying)) in at.iter().enumerate() {
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
                world: if *lying {
                    let r = fo4_scene::timeline::lying_rotation([0.; 3]);
                    Transform {
                        translation: *p,
                        ..fo4_scene::transform::Transform::euler(
                            [0., 0., 0., r[0], r[1], r[2]],
                            1.,
                        )?
                    }
                } else {
                    Transform {
                        translation: *p,
                        ..Transform::IDENTITY
                    }
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

/// The first tag after every draw of the list and every instance of the
/// build (an actor the build left out keeps its instance index).
pub fn first_free_tag(build: &Build) -> u32 {
    let instances = u32::try_from(build.instances.len()).unwrap_or(u32::MAX);
    next_tag(&build.list).max(instances)
}

/// What a draw build did with a timeline cast, and the markers drawn for
/// the actors it could not draw.
#[derive(Debug, Default)]
pub struct CastDrawn {
    /// (cast index, tag) of the actors drawn as characters.
    pub characters: Vec<(usize, u32)>,
    /// (cast index, marker tag, why) of the actors drawn as markers.
    pub fallback: Vec<(usize, u32, String)>,
    /// (cast index, why) of drawn characters whose head was left out.
    pub heads: Vec<(usize, String)>,
    /// The markers drawn instead, in the order of `fallback`.
    pub markers: Vec<fo4_scene::timeline::Marker>,
    /// The first marker tag.
    pub first: u32,
    /// Every actor drawn (character or marker): key and place, for the
    /// camera's aim.
    pub drawn: Vec<(fo4_plugin::FormKey, [f64; 3])>,
    /// Cast indices of the characters drawn with a bone their skeleton
    /// lacks skinned at its bind transform
    /// (`actor_missing_bone_bind_pose_v1`).
    pub bind_bones: Vec<usize>,
}

/// After `crate::draw::build_with_actors` drew `cast.actors` (and anything
/// else appended since), a marker for every cast actor the build left out,
/// tagged from [`first_free_tag`], with the reason.
pub fn cast_markers(
    build: &mut Build,
    cast: &fo4_scene::timeline::cast::Cast,
    level: fo4_scene::RuleLevel,
    b: &Budget,
) -> Result<CastDrawn> {
    let mut out = CastDrawn::default();
    let mut refused: Vec<(usize, String)> = Vec::new();
    for (k, index) in build.actors.iter().enumerate() {
        let why = build
            .omitted
            .iter()
            .find(|o| o.instance == *index)
            .map(|o| {
                cast.why_not(k, level).unwrap_or_else(|| {
                    let causes: Vec<&fo4_core::Error> = o.causes.iter().collect();
                    match fo4_scene::timeline::cast::summary(&causes) {
                        s if s.is_empty() => "left out by the build".into(),
                        s => s,
                    }
                })
            });
        match why {
            Some(w) => refused.push((k, w)),
            None => {
                out.characters.push((k, *index));
                if build
                    .instances
                    .iter()
                    .find(|i| i.index == *index)
                    .is_some_and(|i| {
                        i.hypotheses
                            .contains(&fo4_scene::hypotheses::ACTOR_MISSING_BONE_BIND)
                    })
                {
                    out.bind_bones.push(k);
                }
                if let Some(h) = build
                    .actor_head_omissions
                    .iter()
                    .find(|o| o.instance == *index)
                {
                    let causes: Vec<&fo4_core::Error> = h.causes.iter().collect();
                    let w = fo4_scene::timeline::cast::summary(&causes);
                    out.heads.push((k, w));
                }
            }
        }
    }
    out.first = first_free_tag(build);
    let at: Vec<([f64; 3], bool)> = refused
        .iter()
        .map(|(k, _)| {
            let m = &cast.markers[*k];
            (
                m.pos,
                matches!(m.pose, fo4_scene::timeline::Pose::Lying { .. }),
            )
        })
        .collect();
    let tags = append_markers_posed(&mut build.list, &at, out.first, b)?;
    for ((k, why), tag) in refused.into_iter().zip(tags) {
        out.markers.push(cast.markers[k].clone());
        out.fallback.push((k, tag, why));
    }
    for (k, _) in &out.characters {
        let m = &cast.markers[*k];
        out.drawn.push((m.reference.clone(), m.pos));
    }
    for m in &out.markers {
        out.drawn.push((m.reference.clone(), m.pos));
    }
    Ok(out)
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

    /// A synthetic room (`crate::fixture::actor_room`): one actor at its
    /// stored place, one the run moved onto a rotated reference, one whose
    /// base is missing; the cast draws the first two as their characters
    /// where the run puts them and a marker for the third, with the reason.
    #[test]
    fn a_timeline_cast_draws_each_actor_as_its_character_where_the_run_puts_it() {
        use crate::fixture::{actor_room, Placed, ACTOR_BASE, ACTOR_ROOM, MARKER_BASE};
        use fo4_scene::timeline::{self as tl, cast, Event, Spot, Timeline, What};
        use fo4_scene::{hypotheses, Loader, RuleLevel};
        let temp = fo4_core::TestDir::new("timeline-cast");
        let root = temp.join("Data");
        let placed = |id: u32, base: u32, data: [f32; 6]| Placed {
            id,
            base,
            data,
            scale: None,
        };
        let m = actor_room(
            &root,
            &[
                placed(0x900, ACTOR_BASE, [100., 200., 0., 0., 0., 0.5]),
                placed(0x901, ACTOR_BASE, [0., 0., 0., 0., 0., 0.]),
                placed(0x902, 0x8ff, [0., 300., 0., 0., 0., 0.]),
            ],
            &[placed(0x910, MARKER_BASE, [500., 600., 0., 0., 0., 1.25])],
        );
        let b = Budget::new(64 << 20);
        let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
        let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Hypotheses, &b);
        let scene = loader.by_editor_id(ACTOR_ROOM, &b).unwrap();
        let key = |id: u32| fo4_plugin::FormKey {
            plugin: "base.esm".into(),
            object_id: id,
        };
        let room = key(0x800);
        let actor = |t: f64, id: u32, pos: [f64; 3], source: &str| Event {
            t,
            what: What::Actor {
                reference: key(id),
                at: Spot::At {
                    space: room.clone(),
                    pos,
                },
                source: source.into(),
            },
        };
        let t = Timeline {
            events: vec![
                Event {
                    t: 0.,
                    what: What::Player {
                        space: room.clone(),
                        cell: Some(room.clone()),
                        pos: [0., -500., 0.],
                        facing: None,
                    },
                },
                actor(
                    0.,
                    0x900,
                    [100., 200., 0.],
                    "world_actor_placed_position_v1",
                ),
                actor(0., 0x901, [0., 0., 0.], "world_actor_placed_position_v1"),
                actor(0., 0x902, [0., 300., 0.], "world_actor_placed_position_v1"),
                actor(1., 0x901, [500., 600., 0.], "pack_location_destination_v1"),
            ],
            ..Timeline::default()
        };
        let frame = t.at(2.);
        let space = tl::scene_space(&store, &scene);
        let (markers, left, _) = tl::markers(&store, &frame, &space, &b).unwrap();
        assert_eq!(markers.len(), 3, "{left:?}");
        // Every cast rule is pending.
        let refused = cast::resolve(
            &mut loader,
            &scene,
            &markers,
            cast::Options::default(),
            RuleLevel::Measured,
            &b,
        )
        .unwrap_err();
        assert_eq!(refused.code, "TimelineRuleLevel");
        for target_rotation in [false, true] {
            let c = cast::resolve(
                &mut loader,
                &scene,
                &markers,
                cast::Options {
                    target_rotation,
                    ..cast::Options::default()
                },
                RuleLevel::Hypotheses,
                &b,
            )
            .unwrap();
            assert_eq!(c.actors.actors.len(), 3);
            assert!(c.why_not(2, RuleLevel::Hypotheses).is_some());
            let options = crate::draw::BuildOptions::reference(RuleLevel::Hypotheses);
            let mut build =
                crate::draw::build_with_actors(&scene, &c.actors, &loader, &m.limits, options, &b)
                    .unwrap();
            let d = cast_markers(&mut build, &c, RuleLevel::Hypotheses, &b).unwrap();
            let drawn: Vec<usize> = d.characters.iter().map(|x| x.0).collect();
            assert_eq!(drawn, [0, 1]);
            assert_eq!(d.fallback.len(), 1);
            assert_eq!(d.fallback[0].0, 2);
            assert!(d.fallback[0].1 >= d.first);
            assert_eq!(d.drawn.len(), 3);
            // The marker stands where the run puts the third actor.
            let marker = build.list.draws.last().unwrap();
            assert_eq!(marker.world.translation, [0., 300., 0.]);
            assert_eq!(marker.instance, d.fallback[0].1);
            // Each character's draw: the run's place, the facing the
            // reading gives.
            let world = |tag: u32| {
                build
                    .list
                    .draws
                    .iter()
                    .find(|x| x.instance == tag)
                    .unwrap()
                    .world
            };
            let stored = world(d.characters[0].1);
            let expect =
                fo4_scene::transform::Transform::euler([100., 200., 0., 0., 0., 0.5], 1.).unwrap();
            assert_eq!(stored, expect);
            let moved = world(d.characters[1].1);
            let yaw = if target_rotation { 1.25 } else { 0. };
            let expect =
                fo4_scene::transform::Transform::euler([500., 600., 0., 0., 0., yaw], 1.).unwrap();
            assert_eq!(moved, expect);
            assert!(c.facing[0].contains("stored place"), "{}", c.facing[0]);
            let rule = if target_rotation {
                hypotheses::TIMELINE_ACTOR_TARGET_ROTATION
            } else {
                hypotheses::TIMELINE_ACTOR_ROTATION
            };
            assert!(c.rules.contains(rule), "{:?}", c.rules);
            assert_eq!(c.rules.len(), 1);
            assert!(c.facing[1].contains(rule), "{}", c.facing[1]);
            let text = cast::report(&c, &d.characters, &d.fallback, &d.heads, &d.bind_bones);
            assert!(text.contains("2 drawn as characters"), "{text}");
            assert!(text.contains("marker instead"), "{text}");
        }
    }

    /// A posed build draws an actor whose `Actor::pose` names one of the
    /// given poses in that pose (here the rest pose lifted 10 units) and
    /// the others in their rest pose; a key among none of the poses
    /// leaves the actor out with a named reason.
    #[test]
    fn a_pose_key_poses_its_actor_and_a_missing_key_is_named() {
        use crate::fixture::{actor_room, Placed, ACTOR_BASE, ACTOR_ROOM};
        use fo4_scene::{actor, DecodedAsset, Loader, RuleLevel};
        let temp = fo4_core::TestDir::new("pose-key");
        let root = temp.join("Data");
        let placed = |id: u32, x: f32| Placed {
            id,
            base: ACTOR_BASE,
            data: [x, 0., 0., 0., 0., 0.],
            scale: None,
        };
        let m = actor_room(&root, &[placed(0x900, 0.), placed(0x901, 300.)], &[]);
        let b = Budget::new(64 << 20);
        let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
        let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Hypotheses, &b);
        let scene = loader.by_editor_id(ACTOR_ROOM, &b).unwrap();
        let mut actors = actor::resolve(&scene, &mut loader, &b).unwrap();
        let skeleton = actors.actors[0].plan.as_ref().unwrap().skeleton_asset;
        let rest = loader
            .assets
            .with_decoded(skeleton, &b, |d| {
                let DecodedAsset::Model(n) = d else {
                    panic!("model")
                };
                actor::pose::Skeleton::read(&n, "skeleton", &m.limits, &b)
            })
            .unwrap();
        let mut lifted = rest.clone();
        for a in lifted.bones.values_mut() {
            a.translation[2] += 10.;
        }
        let options = crate::draw::BuildOptions::reference(RuleLevel::Hypotheses);
        let plain =
            crate::draw::build_with_actors(&scene, &actors, &loader, &m.limits, options, &b)
                .unwrap();
        let key = usize::MAX;
        actors.actors[1].pose = Some(key);
        let poses = std::collections::BTreeMap::from([(key, lifted)]);
        let (posed, _) = crate::draw::build_with_posed_actors(
            &scene, &actors, poses, &loader, &m.limits, options, &b,
        )
        .unwrap();
        let z = |build: &crate::draw::Build, x: f64| -> Vec<f64> {
            build
                .list
                .draws
                .iter()
                .filter(|d| d.world.translation[0] == x)
                .flat_map(|d| {
                    build.list.meshes[d.mesh]
                        .vertices
                        .iter()
                        .map(|v| v.position[2])
                })
                .collect()
        };
        assert!(!z(&plain, 0.).is_empty());
        assert_eq!(
            z(&posed, 0.),
            z(&plain, 0.),
            "the first actor keeps its rest pose"
        );
        let (a, bz) = (z(&plain, 300.), z(&posed, 300.));
        assert_eq!(a.len(), bz.len());
        assert!(
            a.iter().zip(&bz).all(|(p, q)| (q - p - 10.).abs() < 1e-3),
            "{a:?} {bz:?}"
        );
        // A key that is none of the given poses.
        actors.actors[1].pose = Some(key - 1);
        let (missing, _) = crate::draw::build_with_posed_actors(
            &scene,
            &actors,
            std::collections::BTreeMap::new(),
            &loader,
            &m.limits,
            options,
            &b,
        )
        .unwrap();
        assert!(missing
            .omitted
            .iter()
            .flat_map(|o| &o.causes)
            .any(|e| e.code == "ActorPoseMissing"));
    }

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

    /// The run's death and sit lines in a frame, each under its switch: a
    /// dead actor left out or drawn lying (its character turned, its marker
    /// box laid down), a sitting actor at its furniture's marker nearest it
    /// facing the marker's heading; with the readings off every marker and
    /// draw is as before.
    #[test]
    fn deaths_and_sits_move_lay_down_or_leave_out_the_cast() {
        use crate::fixture::{actor_room_with, rec, sub, Placed, ACTOR_BASE, ACTOR_ROOM};
        use fo4_scene::timeline::{
            self as tl, cast, Dead, Event, Pose, Readings, Spot, Timeline, What,
        };
        use fo4_scene::{hypotheses, Loader, RuleLevel};
        let temp = fo4_core::TestDir::new("timeline-dead-sit");
        let root = temp.join("Data");
        // A furniture base with two markers of 24 bytes: offsets (40, 0, 0)
        // heading 0.25 and (-40, 0, 0) heading 0.75.
        let mut snam = Vec::new();
        for (x, h) in [(40f32, 0.25f32), (-40., 0.75)] {
            for v in [x, 0., 0., h] {
                snam.extend(v.to_le_bytes());
            }
            snam.extend([0u8; 8]);
        }
        let furn = rec(
            b"FURN",
            0x940,
            0,
            &[sub(b"MNAM", &3u32.to_le_bytes()), sub(b"SNAM", &snam)].concat(),
        );
        let placed = |id: u32, base: u32, data: [f32; 6]| Placed {
            id,
            base,
            data,
            scale: None,
        };
        let m = actor_room_with(
            &root,
            &[
                placed(0x900, ACTOR_BASE, [100., 200., 0., 0., 0., 0.5]),
                placed(0x901, ACTOR_BASE, [0., 300., 0., 0., 0., 0.]),
            ],
            &[placed(0x950, 0x940, [500., 0., 0., 0., 0., 1.])],
            &furn,
        );
        let b = Budget::new(64 << 20);
        let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
        let vfs = fo4_vfs::Vfs::mount(&m, &b).unwrap();
        let mut loader = Loader::new(&store, &vfs, "english", RuleLevel::Hypotheses, &b);
        let scene = loader.by_editor_id(ACTOR_ROOM, &b).unwrap();
        let space = tl::scene_space(&store, &scene);
        let key = |id: u32| fo4_plugin::FormKey {
            plugin: "base.esm".into(),
            object_id: id,
        };
        let actor = |r: u32, pos: [f64; 3]| Event {
            t: 0.,
            what: What::Actor {
                reference: key(r),
                at: Spot::At {
                    space: space.clone(),
                    pos,
                },
                source: "world_actor_placed_position_v1".into(),
            },
        };
        let note = |t: f64, text: &str| Event {
            t,
            what: What::Note(text.into()),
        };
        let t = Timeline {
            events: vec![
                Event {
                    t: 0.,
                    what: What::Player {
                        space: space.clone(),
                        cell: None,
                        pos: [0., -500., 0.],
                        facing: None,
                    },
                },
                actor(0x900, [100., 200., 0.]),
                actor(0x901, [0., 300., 0.]),
                note(1., "sit base.esm:00000900 base.esm:00000950"),
                note(1., "death base.esm:00000901"),
            ],
            ..Timeline::default()
        };
        let frame = |r: Readings| t.at_with(2., r);
        // Off: as before.
        let (off, _, rules) = tl::markers(&store, &frame(Readings::default()), &space, &b).unwrap();
        assert_eq!(off.len(), 2);
        assert!(off.iter().all(|m| m.pose == Pose::Standing));
        assert!(!rules.contains(hypotheses::TIMELINE_SIT_MARKER));
        // Left out.
        let (left_out, left, rules) = tl::markers(
            &store,
            &frame(Readings {
                dead: Dead::LeftOut,
                ..Readings::default()
            }),
            &space,
            &b,
        )
        .unwrap();
        assert_eq!(left_out.len(), 1);
        assert!(left.iter().any(|l| l.contains("dead from 1 s")), "{left:?}");
        assert!(rules.contains(hypotheses::TIMELINE_DEAD_LEFT_OUT));
        // Lying and seated.
        let on = frame(Readings {
            dead: Dead::Lying,
            sit_marker: true,
            ..Readings::default()
        });
        let (marks, _, rules) = tl::markers(&store, &on, &space, &b).unwrap();
        assert!(rules.contains(hypotheses::TIMELINE_DEAD_LYING));
        assert!(rules.contains(hypotheses::TIMELINE_SIT_MARKER));
        assert!(rules.contains(hypotheses::FURNITURE_MARKER_SNAM));
        let seated = marks.iter().find(|m| m.reference == key(0x900)).unwrap();
        // The marker nearer the actor: the furniture turned by 1 radian
        // about the vertical, its two markers 40 units either side.
        let w = fo4_scene::transform::Transform::euler([500., 0., 0., 0., 0., 1.], 1.).unwrap();
        let (a, z) = (w.apply([40., 0., 0.]), w.apply([-40., 0., 0.]));
        let d = |p: [f64; 3]| (p[0] - 100.).hypot(p[1] - 200.);
        let (want, heading) = if d(a) <= d(z) { (a, 1.25) } else { (z, 1.75) };
        assert_eq!(seated.pos, want);
        match &seated.pose {
            Pose::Seated {
                heading: h, base, ..
            } => {
                assert!((h - heading).abs() < 1e-6, "{h}");
                assert_eq!(*base, key(0x940));
            }
            p => panic!("{p:?}"),
        }
        let dead = marks.iter().find(|m| m.reference == key(0x901)).unwrap();
        assert_eq!(dead.pose, Pose::Lying { since: 1. });
        // The cast: the dead actor laid down, the sitter at the marker's
        // heading.
        let c = cast::resolve(
            &mut loader,
            &scene,
            &marks,
            cast::Options::default(),
            RuleLevel::Hypotheses,
            &b,
        )
        .unwrap();
        let k = |r: u32| {
            c.markers
                .iter()
                .position(|m| m.reference == key(r))
                .unwrap()
        };
        let t0 = c.actors.actors[k(0x901)].transform.unwrap();
        assert_eq!([t0[3], t0[4], t0[5]], tl::lying_rotation([0., 0., 0.]));
        let t1 = c.actors.actors[k(0x900)].transform.unwrap();
        assert!((t1[5] - heading).abs() < 1e-6 && t1[3] == 0. && t1[4] == 0.);
        assert!(c.facing[k(0x900)].contains(hypotheses::TIMELINE_SIT_MARKER));
        // A lying marker box is laid down; upright ones are as before.
        let mut list = crate::DrawList::default();
        let tags =
            append_markers_posed(&mut list, &[([0.; 3], true), ([5.; 3], false)], 7, &b).unwrap();
        assert_eq!(tags, [7, 8]);
        assert_ne!(list.draws[0].world.rotation, Transform::IDENTITY.rotation);
        let mut plain = crate::DrawList::default();
        append_markers(&mut plain, &[[5.; 3]], 8, &b).unwrap();
        assert_eq!(plain.draws[0].world, list.draws[1].world);
    }
}
