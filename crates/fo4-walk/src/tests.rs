//! Synthetic geometry only: every primitive is written here.
use super::*;
use crate::geometry::V3;
use fo4_core::Budget;
use fo4_scene::hypotheses::{self, Status};

const P: Params = Params::PROVISIONAL;

fn budget() -> Budget {
    Budget::new(256 << 20)
}
fn quad(a: V3, b: V3, c: V3, d: V3) -> [[V3; 3]; 2] {
    [[a, b, c], [a, c, d]]
}
/// Horizontal rectangle at height z over [x0, x1] x [y0, y1].
fn rect(x0: f64, x1: f64, y0: f64, y1: f64, z: f64) -> [[V3; 3]; 2] {
    quad([x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z])
}
fn floor(w: &mut World, b: &Budget) -> u32 {
    w.add_triangles(&rect(-1000.0, 1000.0, -1000.0, 1000.0, 0.0), "floor", b)
        .unwrap()
}
fn cuboid(min: V3, max: V3) -> Vec<V3> {
    (0..8)
        .map(|i| {
            [
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            ]
        })
        .collect()
}
fn character(feet: V3) -> Character {
    Character::new(P, Settings::DEFAULT, feet).unwrap()
}
fn ahead(heading: f64) -> Input {
    Input {
        forward: 1.0,
        heading,
        ..Input::default()
    }
}
/// Runs `ticks` ticks of `input`; returns every state.
fn run(c: &mut Character, w: &World, input: Input, ticks: usize) -> Vec<State> {
    (0..ticks).map(|_| c.tick(w, input)).collect()
}
fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn settles_on_a_flat_floor_and_walks_at_the_set_speeds() {
    let b = budget();
    let mut w = World::new();
    let f = floor(&mut w, &b);
    let mut c = character([0.0, 0.0, 50.0]);
    let ticks = c.settle(&w, 600);
    assert!(ticks < 60, "{ticks}");
    assert!(c.grounded);
    assert!(near(c.feet[2], 0.0, 1e-6), "{:?}", c.feet);
    assert_eq!(c.ground.map(|g| g.source), Some(f));
    assert!(near(c.eye()[2], P.eye_height, 1e-6));
    let states = run(&mut c, &w, ahead(0.0), 60);
    assert!(states
        .iter()
        .all(|s| s.grounded && !s.blocked && !s.stepped));
    assert!(near(c.feet[0], P.walk_speed, 1e-6), "{:?}", c.feet);
    assert!(near(c.feet[2], 0.0, 1e-6));
    let input = Input {
        run: true,
        ..ahead(std::f64::consts::FRAC_PI_2)
    };
    let start = c.feet;
    run(&mut c, &w, input, 60);
    assert!(near(c.feet[1] - start[1], P.run_speed, 1e-6));
    // D (right) while facing +X moves towards -Y.
    let start = c.feet;
    let right = Input {
        right: 1.0,
        ..Input::default()
    };
    run(&mut c, &w, right, 30);
    assert!(near(c.feet[1] - start[1], -P.walk_speed / 2.0, 1e-6));
}

#[test]
fn climbs_a_ramp_below_the_slope_limit_and_not_one_above() {
    let b = budget();
    let ramp = |degrees: f64| {
        let rise = 200.0 * degrees.to_radians().tan();
        let mut w = World::new();
        w.add_triangles(&rect(-500.0, 0.0, -300.0, 300.0, 0.0), "low", &b)
            .unwrap();
        w.add_triangles(
            &quad(
                [0.0, -300.0, 0.0],
                [200.0, -300.0, rise],
                [200.0, 300.0, rise],
                [0.0, 300.0, 0.0],
            ),
            "ramp",
            &b,
        )
        .unwrap();
        w.add_triangles(&rect(200.0, 900.0, -300.0, 300.0, rise), "top", &b)
            .unwrap();
        (w, rise)
    };
    let (w, rise) = ramp(30.0);
    let mut c = character([-100.0, 0.0, 0.0]);
    c.settle(&w, 600);
    let states = run(&mut c, &w, ahead(0.0), 300);
    assert!(c.grounded);
    assert!(c.feet[0] > 300.0, "{:?}", c.feet);
    assert!(near(c.feet[2], rise, 1e-6), "{:?}", c.feet);
    assert!(states.iter().all(|s| !s.blocked));
    // Walking back down, ground snapping keeps the character on the ramp.
    let states = run(&mut c, &w, ahead(std::f64::consts::PI), 300);
    assert!(states.iter().all(|s| s.grounded));
    assert!(
        c.feet[0] < 0.0 && near(c.feet[2], 0.0, 1e-6),
        "{:?}",
        c.feet
    );
    let (w, _) = ramp(60.0);
    let mut c = character([-100.0, 0.0, 0.0]);
    c.settle(&w, 600);
    let states = run(&mut c, &w, ahead(0.0), 300);
    assert!(c.feet[2] < 1.0, "{:?}", c.feet);
    assert!(c.feet[0] < 10.0, "{:?}", c.feet);
    assert!(states.last().is_some_and(|s| s.blocked && s.grounded));
}

#[test]
fn a_wall_blocks_at_the_radius_and_the_character_slides_along_it() {
    let b = budget();
    for mesh in [false, true] {
        let mut w = World::new();
        floor(&mut w, &b);
        let wall = if mesh {
            let t = quad(
                [100.0, -500.0, 0.0],
                [100.0, 500.0, 0.0],
                [100.0, 500.0, 300.0],
                [100.0, -500.0, 300.0],
            );
            w.add_triangles(&t, "wall", &b).unwrap()
        } else {
            w.add_hull(
                &cuboid([100.0, -500.0, 0.0], [120.0, 500.0, 300.0]),
                0.0,
                "wall",
                &b,
            )
            .unwrap()
        };
        let mut c = character([0.0, 0.0, 0.0]);
        c.settle(&w, 600);
        let states = run(&mut c, &w, ahead(0.0), 120);
        assert!(near(c.feet[0], 100.0 - P.radius, 1e-5), "{:?}", c.feet);
        assert!(states.last().is_some_and(|s| s.blocked && s.grounded));
        assert_eq!(
            states.last().and_then(|s| s.wall).map(|g| g.source),
            Some(wall)
        );
        assert!(states[0].wall.is_none());
        let start = c.feet;
        run(&mut c, &w, ahead(std::f64::consts::FRAC_PI_4), 60);
        assert!(near(c.feet[0], 100.0 - P.radius, 1e-5), "{:?}", c.feet);
        let along = P.walk_speed * std::f64::consts::FRAC_PI_4.sin();
        assert!(near(c.feet[1] - start[1], along, 1e-3), "{:?}", c.feet);
    }
}

#[test]
fn steps_onto_a_ledge_lower_than_the_step_height_only() {
    let b = budget();
    for (height, climbs) in [(10.0, true), (P.step_height - 1.0, true), (30.0, false)] {
        let mut w = World::new();
        floor(&mut w, &b);
        w.add_hull(
            &cuboid([50.0, -300.0, 0.0], [400.0, 300.0, height]),
            0.0,
            "ledge",
            &b,
        )
        .unwrap();
        let mut c = character([0.0, 0.0, 0.0]);
        c.settle(&w, 600);
        let states = run(&mut c, &w, ahead(0.0), 120);
        if climbs {
            assert!(states.iter().any(|s| s.stepped), "{height}");
            assert!(near(c.feet[2], height, 1e-6), "{height} {:?}", c.feet);
            assert!(c.feet[0] > 150.0, "{height} {:?}", c.feet);
        } else {
            assert!(near(c.feet[2], 0.0, 1e-6), "{height} {:?}", c.feet);
            assert!(near(c.feet[0], 50.0 - P.radius, 1e-5), "{:?}", c.feet);
            assert!(states.last().is_some_and(|s| s.blocked));
        }
    }
}

#[test]
fn stands_on_a_box_and_falls_off_its_edge_to_the_floor() {
    let b = budget();
    let mut w = World::new();
    floor(&mut w, &b);
    let bx = w
        .add_hull(
            &cuboid([-50.0, -50.0, 0.0], [50.0, 50.0, 40.0]),
            0.0,
            "box",
            &b,
        )
        .unwrap();
    let mut c = character([0.0, 0.0, 100.0]);
    c.settle(&w, 600);
    assert!(near(c.feet[2], 40.0, 1e-6), "{:?}", c.feet);
    assert_eq!(c.ground.map(|g| g.source), Some(bx));
    let states = run(&mut c, &w, ahead(0.0), 120);
    assert!(states.iter().any(|s| !s.grounded));
    assert!(c.grounded && near(c.feet[2], 0.0, 1e-6), "{:?}", c.feet);
    // No air control: the fall keeps the walking velocity.
    assert!(c.feet[0] > 100.0);
}

#[test]
fn falls_through_a_gap_onto_the_floor_below() {
    let b = budget();
    let mut w = World::new();
    w.add_triangles(&rect(-500.0, -50.0, -500.0, 500.0, 0.0), "left", &b)
        .unwrap();
    w.add_triangles(&rect(50.0, 500.0, -500.0, 500.0, 0.0), "right", &b)
        .unwrap();
    let below = w
        .add_triangles(&rect(-500.0, 500.0, -500.0, 500.0, -200.0), "below", &b)
        .unwrap();
    let mut c = character([0.0, 0.0, 10.0]);
    c.settle(&w, 600);
    assert!(near(c.feet[2], -200.0, 1e-6), "{:?}", c.feet);
    assert_eq!(c.ground.map(|g| g.source), Some(below));
    // Walking off the edge of the upper floor ends on the lower one too.
    let mut c = character([-100.0, 0.0, 0.0]);
    c.settle(&w, 600);
    assert!(near(c.feet[2], 0.0, 1e-6));
    run(&mut c, &w, ahead(0.0), 180);
    assert!(c.grounded && near(c.feet[2], -200.0, 1e-6), "{:?}", c.feet);
}

#[test]
fn spheres_carry_and_block_the_character() {
    let b = budget();
    let mut w = World::new();
    floor(&mut w, &b);
    let s = w.add_sphere([0.0, 0.0, 0.0], 50.0, "dome", &b).unwrap();
    let mut c = character([0.0, 0.0, 200.0]);
    c.settle(&w, 600);
    assert!(near(c.feet[2], 50.0, 1e-6), "{:?}", c.feet);
    assert_eq!(c.ground.map(|g| g.source), Some(s));
    let mut w = World::new();
    floor(&mut w, &b);
    w.add_sphere([100.0, 0.0, 50.0], 50.0, "ball", &b).unwrap();
    let mut c = character([0.0, 0.0, 0.0]);
    c.settle(&w, 600);
    run(&mut c, &w, ahead(0.0), 120);
    assert!(
        near(c.feet[0], 100.0 - 50.0 - P.radius, 1e-4),
        "{:?}",
        c.feet
    );
    assert!(near(c.feet[2], 0.0, 1e-6));
}

#[test]
fn capsules_block_as_pillars_and_low_logs_are_stepped_over() {
    let b = budget();
    let mut w = World::new();
    floor(&mut w, &b);
    w.add_capsule([[100.0, 0.0, 0.0], [100.0, 0.0, 200.0]], 10.0, "pillar", &b)
        .unwrap();
    let mut c = character([0.0, 0.0, 0.0]);
    c.settle(&w, 600);
    run(&mut c, &w, ahead(0.0), 120);
    assert!(
        near(c.feet[0], 100.0 - 10.0 - P.radius, 1e-4),
        "{:?}",
        c.feet
    );
    let mut w = World::new();
    floor(&mut w, &b);
    w.add_capsule([[100.0, -300.0, 6.0], [100.0, 300.0, 6.0]], 6.0, "log", &b)
        .unwrap();
    let mut c = character([0.0, 0.0, 0.0]);
    c.settle(&w, 600);
    run(&mut c, &w, ahead(0.0), 180);
    assert!(c.feet[0] > 200.0, "{:?}", c.feet);
    assert!(c.grounded && near(c.feet[2], 0.0, 1e-6), "{:?}", c.feet);
}

fn mixed_world(b: &Budget) -> World {
    let mut w = World::new();
    floor(&mut w, b);
    w.add_hull(
        &cuboid([80.0, -40.0, 0.0], [160.0, 40.0, 12.0]),
        2.0,
        "slab",
        b,
    )
    .unwrap();
    w.add_sphere([250.0, 30.0, 0.0], 40.0, "ball", b).unwrap();
    w.add_capsule([[300.0, -200.0, 0.0], [300.0, 200.0, 0.0]], 5.0, "rail", b)
        .unwrap();
    w.add_triangles(
        &quad(
            [400.0, -500.0, 0.0],
            [400.0, 500.0, 0.0],
            [400.0, 500.0, 300.0],
            [400.0, -500.0, 300.0],
        ),
        "wall",
        b,
    )
    .unwrap();
    w
}

#[test]
fn the_trace_is_deterministic() {
    let b = budget();
    let trace = || {
        let w = mixed_world(&b);
        let mut c = character([0.0, 0.0, 30.0]);
        let mut out = Vec::new();
        for k in 0..400u32 {
            let input = Input {
                forward: 1.0,
                right: if k % 97 < 30 { 0.5 } else { 0.0 },
                heading: 0.002 * f64::from(k),
                run: k % 150 > 100,
            };
            out.push(c.tick(&w, input));
        }
        out
    };
    let a = trace();
    let bb = trace();
    assert_eq!(a, bb);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&bb).unwrap()
    );
    assert!(a.iter().any(|s| s.stepped));
    assert!(a.iter().any(|s| s.blocked));
}

#[test]
fn ray_down_finds_the_highest_surface_below_a_point() {
    let b = budget();
    let mut w = World::new();
    let f = floor(&mut w, &b);
    let bx = w
        .add_hull(
            &cuboid([-50.0, -50.0, 0.0], [50.0, 50.0, 40.0]),
            0.0,
            "box",
            &b,
        )
        .unwrap();
    let s = w.add_sphere([300.0, 0.0, 0.0], 50.0, "ball", &b).unwrap();
    let cap = w
        .add_capsule(
            [[-300.0, -50.0, 20.0], [-300.0, 50.0, 20.0]],
            10.0,
            "log",
            &b,
        )
        .unwrap();
    let h = w.ray_down(0.0, 0.0, 1000.0).unwrap();
    assert_eq!(h.source, bx);
    assert!(near(h.z, 40.0, 1e-5), "{h:?}");
    let h = w.ray_down(0.0, 0.0, 30.0).unwrap();
    assert_eq!(h.source, f);
    assert_eq!((h.z, h.normal), (0.0, [0.0, 0.0, 1.0]));
    let h = w.ray_down(330.0, 0.0, 1000.0).unwrap();
    assert_eq!(h.source, s);
    assert!(near(h.z, 40.0, 1e-5), "{h:?}");
    assert!(near(h.normal[0], 0.6, 1e-4) && near(h.normal[2], 0.8, 1e-4));
    let h = w.ray_down(-300.0, 10.0, 1000.0).unwrap();
    assert_eq!(h.source, cap);
    assert!(near(h.z, 30.0, 1e-5), "{h:?}");
    assert!(w.ray_down(5000.0, 0.0, 1000.0).is_none());
    // A triangle wound downwards is still a top surface.
    let mut w = World::new();
    w.add_triangles(
        &[[[0.0, 0.0, 5.0], [0.0, 10.0, 5.0], [10.0, 0.0, 5.0]]],
        "down",
        &b,
    )
    .unwrap();
    let h = w.ray_down(1.0, 1.0, 10.0).unwrap();
    assert_eq!((h.z, h.normal), (5.0, [0.0, 0.0, 1.0]));
}

#[test]
fn surfaces_lists_every_primitive_on_the_line_highest_first() {
    let b = budget();
    let mut w = World::new();
    let f = floor(&mut w, &b);
    let bx = w
        .add_hull(
            &cuboid([-50.0, -50.0, 10.0], [50.0, 50.0, 40.0]),
            2.0,
            "box",
            &b,
        )
        .unwrap();
    let up = w
        .add_triangles(&rect(-20.0, 20.0, -20.0, 20.0, 100.0), "slab", &b)
        .unwrap();
    let all = w.surfaces(0.0, 0.0);
    let order: Vec<u32> = all.iter().map(|h| h.source).collect();
    // The slab is two triangles; the line crosses their shared edge, so
    // both report it.
    assert_eq!(order, vec![up, up, bx, f, f]);
    assert!(all.windows(2).all(|p| p[0].z >= p[1].z));
    assert!(near(all[2].z, 42.0, 1e-5), "{:?}", all[2]);
    assert_eq!(all[3].z, 0.0);
    // ray_down is the first surface at or below its start.
    for z in [1000.0, 99.0, 41.0, 5.0] {
        let first = all.iter().find(|h| h.z <= z + 1e-9).copied();
        assert_eq!(w.ray_down(0.0, 0.0, z), first, "{z}");
    }
    assert_eq!(
        w.surfaces(30.0, 0.0)
            .iter()
            .map(|h| h.source)
            .collect::<Vec<_>>(),
        vec![bx, f]
    );
    assert!(w.surfaces(5000.0, 0.0).is_empty());
}

#[test]
fn the_world_is_built_from_a_placed_cell() {
    use fo4_scene::collision::{
        BodyCollision, BodyPlacement, CellCollision, CollisionInstance, Leaf, ModelCollision,
        ModelPlacement, Shape, ShapeClass, UNITS,
    };
    use fo4_scene::transform::Transform;
    let key = fo4_plugin::FormKey {
        plugin: "Synthetic.esp".into(),
        object_id: 0x800,
    };
    let leaf = |shape: Shape, class: ShapeClass| Leaf {
        object: 0,
        path: vec![class],
        steps: Vec::new(),
        shape,
    };
    // A 2 x 2 collision-unit floor at z = 0 and a sphere of radius 0.5.
    let mesh = Shape::Mesh {
        vertices: vec![
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        records: Vec::new(),
        record_vertices: Vec::new(),
        placeholders: 0,
    };
    let sphere = Shape::Sphere {
        center: [0.5, 0.5, 0.0],
        radius: 0.25,
    };
    let body = BodyCollision {
        collision_object: 1,
        target: 0,
        target_name: None,
        flags: 0,
        system: 2,
        body_id: 0,
        body: fo4_havok::Body {
            index: 0,
            shape: 0,
            shape_slot: 0,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        },
        target_local: Transform::IDENTITY,
        chain: vec![0],
        hidden_chain: false,
        leaves: vec![
            leaf(mesh, ShapeClass::CompressedMesh),
            leaf(sphere, ShapeClass::Sphere),
        ],
        render: Vec::new(),
    };
    let frame = Transform {
        translation: [1000.0, 2000.0, 300.0],
        ..Transform::IDENTITY
    };
    let rules = vec![hypotheses::COLLISION_UNITS, hypotheses::MESH_TRIANGLES];
    let c = CellCollision {
        level: RuleLevel::Hypotheses,
        body_placement: Some(BodyPlacement::Ignored),
        models: vec![ModelCollision {
            asset: 0,
            path: "synthetic.nif".into(),
            collision_objects: 1,
            systems: 1,
            bodies: vec![body],
            refusals: Vec::new(),
            error: None,
            render_points: Vec::new(),
            render: Vec::new(),
            render_error: None,
        }],
        placements: vec![ModelPlacement {
            reference: key.clone(),
            base: key,
            model: 0,
            frame,
            scene_rules: Vec::new(),
        }],
        instances: [ShapeClass::CompressedMesh, ShapeClass::Sphere]
            .into_iter()
            .enumerate()
            .map(|(k, kind)| CollisionInstance {
                placement: 0,
                body: 0,
                leaf: k,
                kind,
                world: frame,
                rules: rules.clone(),
            })
            .collect(),
        unplaced: Vec::new(),
        without_collision: Vec::new(),
    };
    let b = budget();
    let w = World::build(&c, BodyPlacement::Ignored, &b).unwrap();
    assert_eq!(w.stats.instances, 2);
    assert_eq!(w.stats.triangles, 2);
    assert_eq!(w.stats.spheres, 1);
    assert_eq!(w.sources[0].instance, Some(0));
    assert_eq!(w.sources[1].class, Some(ShapeClass::Sphere));
    assert_eq!(
        w.rules,
        [
            hypotheses::COLLISION_UNITS,
            hypotheses::MESH_TRIANGLES,
            rules::TRIANGLE_SIDES
        ]
    );
    let h = w.ray_down(1000.0 - 0.9 * UNITS, 2000.0, 1000.0).unwrap();
    assert_eq!((h.source, h.z), (0, 300.0));
    let h = w
        .ray_down(1000.0 + 0.5 * UNITS, 2000.0 + 0.5 * UNITS, 1000.0)
        .unwrap();
    assert_eq!(h.source, 1);
    assert!(near(h.z, 300.0 + 0.25 * UNITS, 1e-5), "{h:?}");
    let mut ch = character([1000.0 - 0.5 * UNITS, 2000.0 - 0.5 * UNITS, 400.0]);
    ch.settle(&w, 600);
    assert!(near(ch.feet[2], 300.0, 1e-6), "{:?}", ch.feet);
    assert_eq!(ch.ground.map(|g| g.source), Some(0));
    // Later triangle sets (terrain) join the same broad phase.
    let mut w = w;
    let t = w
        .add_triangles(&rect(500.0, 600.0, 2000.0, 2100.0, 250.0), "terrain", &b)
        .unwrap();
    assert_eq!(w.ray_down(550.0, 2050.0, 1000.0).map(|h| h.source), Some(t));
    // A budget too small for the world is an error, not a partial world.
    assert!(World::build(&c, BodyPlacement::Ignored, &Budget::new(64)).is_err());
}

#[test]
fn step_runs_whole_fixed_ticks() {
    let b = budget();
    let mut w = World::new();
    floor(&mut w, &b);
    let mut c = character([0.0, 0.0, 0.0]);
    let s = c.step(&w, Input::default(), 0.05);
    assert_eq!(s.tick, 3);
    let s = c.step(&w, Input::default(), 0.01);
    assert_eq!(s.tick, 3);
    let s = c.step(&w, Input::default(), 0.01);
    assert_eq!(s.tick, 4);
    let s = c.step(&w, Input::default(), 10.0);
    assert_eq!(s.tick, 4 + u64::from(controller::MAX_TICKS_PER_STEP));
    assert_eq!(c.step(&w, Input::default(), f64::NAN).tick, s.tick);
}

#[test]
fn every_controller_rule_is_registered_and_pending() {
    for r in rules::PARAMS.iter().chain([&rules::TRIANGLE_SIDES]) {
        assert_eq!(hypotheses::status(r), Some(Status::Pending), "{r}");
    }
    let mut all: Vec<&str> = rules::PARAMS.to_vec();
    all.push(rules::TRIANGLE_SIDES);
    assert!(accept(&all, RuleLevel::Hypotheses, hypotheses::status).is_ok());
    let e = accept(&all, RuleLevel::Measured, hypotheses::status).unwrap_err();
    assert_eq!(e.code, "WalkRuleNotAccepted");
    assert!(e.found.contains(rules::STEP_HEIGHT));
    let refuted = |name: &str| {
        if name == rules::GRAVITY {
            Some(Status::Refuted)
        } else {
            hypotheses::status(name)
        }
    };
    let e = accept(&all, RuleLevel::Hypotheses, refuted).unwrap_err();
    assert_eq!(e.found, rules::GRAVITY);
    assert_eq!(P.provenance().len(), rules::PARAMS.len());
    assert!(P
        .provenance()
        .iter()
        .all(|(_, _, _, o)| *o == Origin::Provisional));
}

#[test]
fn parameters_and_settings_are_validated() {
    assert!(P.validate().is_ok());
    for bad in [
        Params { radius: 0.0, ..P },
        Params { height: 20.0, ..P },
        Params {
            eye_height: 500.0,
            ..P
        },
        Params {
            step_height: 120.0,
            ..P
        },
        Params {
            max_slope_degrees: 90.0,
            ..P
        },
        Params {
            gravity: f64::NAN,
            ..P
        },
    ] {
        assert!(Character::new(bad, Settings::DEFAULT, [0.0; 3]).is_err());
    }
    let s = Settings {
        tick: 0.0,
        ..Settings::DEFAULT
    };
    assert!(Character::new(P, s, [0.0; 3]).is_err());
    assert!(Character::new(P, Settings::DEFAULT, [f64::INFINITY, 0.0, 0.0]).is_err());
    let custom = Params {
        walk_speed: 90.0,
        ..P
    };
    assert!(custom
        .provenance()
        .iter()
        .any(|(n, _, _, o)| *n == "walk_speed" && *o == Origin::Caller));
}

/// A box 100 x 100 x 40 on the floor, as one convex hull or as triangles
/// (top and the four sides).
fn box_world(triangles: bool, b: &Budget) -> (World, u32) {
    let mut w = World::new();
    floor(&mut w, b);
    let source = if triangles {
        let (lo, hi, top) = (-50.0, 50.0, 40.0);
        let mut t = rect(lo, hi, lo, hi, top).to_vec();
        for (a, c) in [
            ([lo, lo], [hi, lo]),
            ([hi, lo], [hi, hi]),
            ([hi, hi], [lo, hi]),
            ([lo, hi], [lo, lo]),
        ] {
            t.extend(quad(
                [a[0], a[1], 0.0],
                [c[0], c[1], 0.0],
                [c[0], c[1], top],
                [a[0], a[1], top],
            ));
        }
        w.add_triangles(&t, "box", b).unwrap()
    } else {
        w.add_hull(
            &cuboid([-50.0, -50.0, 0.0], [50.0, 50.0, 40.0]),
            0.0,
            "box",
            b,
        )
        .unwrap()
    };
    (w, source)
}

#[test]
fn rests_on_an_edge_within_the_slope_limit_and_stays_grounded() {
    let b = budget();
    for triangles in [false, true] {
        let (w, bx) = box_world(triangles, &b);
        // The axis 10 units outside the edge: the lower sphere touches the
        // edge with a normal 38.7 degrees from vertical, inside the limit.
        let mut c = character([60.0, 0.0, 200.0]);
        let ticks = c.settle(&w, 600);
        assert!(ticks < 600, "{triangles} {ticks}");
        let on_edge = 40.0 - P.radius + (P.radius * P.radius - 10.0 * 10.0).sqrt();
        assert!(near(c.feet[2], on_edge, 1e-6), "{triangles} {:?}", c.feet);
        assert!(near(c.feet[0], 60.0, 1e-9));
        assert_eq!(c.ground.map(|g| g.source), Some(bx));
        let rest = c.feet;
        let states = run(&mut c, &w, Input::default(), 300);
        assert!(
            states.iter().all(|s| s.grounded && s.feet == rest),
            "{triangles}"
        );
        // From the edge it walks off to the floor...
        let mut off = c.clone();
        run(&mut off, &w, ahead(0.0), 120);
        assert!(
            off.grounded && near(off.feet[2], 0.0, 1e-6),
            "{:?}",
            off.feet
        );
        // ...or onto the top.
        run(&mut c, &w, ahead(std::f64::consts::PI), 30);
        assert!(c.grounded && near(c.feet[2], 40.0, 1e-6), "{:?}", c.feet);
        // 12 units out the touching normal is steeper than the limit: it
        // slides off the edge to the floor.
        let mut c = character([62.0, 0.0, 200.0]);
        c.settle(&w, 600);
        assert!(c.grounded && near(c.feet[2], 0.0, 1e-6), "{:?}", c.feet);
        assert!(c.feet[0] >= 50.0 + P.radius - 1e-6);
    }
}

#[test]
fn a_set_of_one_world_walks_exactly_as_the_world() {
    let b = budget();
    let w = mixed_world(&b);
    let mut set = WorldSet::new();
    set.insert(7, mixed_world(&b)).unwrap();
    let inputs: Vec<Input> = (0..400u32)
        .map(|k| Input {
            forward: 1.0,
            right: if k % 97 < 30 { 0.5 } else { 0.0 },
            heading: 0.002 * f64::from(k),
            run: k % 150 > 100,
        })
        .collect();
    let mut a = character([0.0, 0.0, 30.0]);
    let mut c = character([0.0, 0.0, 30.0]);
    for input in &inputs {
        assert_eq!(a.tick(&w, *input), c.tick(&set, *input));
    }
    assert_eq!(Space::bounds(&set), w.bounds);
}

/// Two 2,000-unit floors side by side along x, as two keyed worlds.
fn two_floors(b: &Budget) -> WorldSet<[i32; 2]> {
    let mut set = WorldSet::new();
    for k in [0, 1] {
        let x0 = 2000.0 * f64::from(k);
        let mut w = World::new();
        w.add_triangles(&rect(x0, x0 + 2000.0, 0.0, 2000.0, 0.0), "floor", b)
            .unwrap();
        w.add_sphere([x0 + 1000.0, 1500.0, 0.0], 40.0, "ball", b)
            .unwrap();
        set.insert([k, 0], w).unwrap();
    }
    set
}

#[test]
fn a_set_numbers_entries_across_its_worlds_in_key_order() {
    let b = budget();
    let set = two_floors(&b);
    // Inserted in the other order: the same numbering.
    let mut other = WorldSet::new();
    let mut pieces: Vec<_> = set.keys().copied().collect();
    pieces.reverse();
    for k in pieces {
        other.insert(k, set.get(&k).unwrap().clone()).unwrap();
    }
    assert_eq!(set.entries(), 6);
    let (mut scratch, mut a, mut c) = (Vec::new(), Vec::new(), Vec::new());
    // A capsule standing across the shared edge touches both floors.
    let seg = [[2000.0, 500.0, 15.0], [2000.0, 500.0, 111.0]];
    Space::contacts(&set, seg, 16.0, 0.0, &mut scratch, &mut a);
    Space::contacts(&other, seg, 16.0, 0.0, &mut scratch, &mut c);
    assert_eq!(a, c);
    let entries: Vec<u32> = a.iter().map(|x| x.entry).collect();
    assert!(entries.iter().any(|e| *e < 3) && entries.iter().any(|e| *e >= 3));
    // Entry 3 is the second floor's first triangle, of source 2 (its first).
    assert_eq!(set.source_of(3), 2);
    assert_eq!(
        set.source(2).map(|(k, s)| (*k, s.label.clone())),
        Some(([1, 0], Some("floor".into())))
    );
    assert_eq!(set.source(3).map(|(k, _)| *k), Some([1, 0]));
    assert!(set.source(4).is_none());
    let hit = set.ray_down(3000.0, 1500.0, 500.0).unwrap();
    assert!(near(hit.z, 40.0, 1e-6), "{hit:?}");
    assert_eq!(
        set.source(hit.source).map(|(k, s)| (*k, s.label.clone())),
        Some(([1, 0], Some("ball".into())))
    );
}

#[test]
fn a_walker_crosses_from_one_world_of_a_set_to_the_next_and_falls_when_one_is_removed() {
    let b = budget();
    let mut set = two_floors(&b);
    let mut c = character([1900.0, 500.0, 10.0]);
    c.settle(&set, 600);
    assert!(c.grounded);
    let states: Vec<State> = (0..120).map(|_| c.tick(&set, ahead(0.0))).collect();
    assert!(states.iter().all(|s| s.grounded), "{:?}", c.feet);
    assert!(c.feet[0] > 2000.0 + 90.0, "{:?}", c.feet);
    let ground = c.ground.unwrap();
    assert_eq!(set.source(ground.source).map(|(k, _)| *k), Some([1, 0]));
    // The world under the walker goes: it falls; back, it stands again.
    let floor = set.remove(&[1, 0]).unwrap();
    assert!(!set.contains(&[1, 0]) && set.len() == 1);
    assert!(!c.tick(&set, Input::default()).grounded);
    set.insert([1, 0], floor).unwrap();
    let mut d = character([2500.0, 500.0, 10.0]);
    d.settle(&set, 600);
    assert!(d.grounded && near(d.feet[2], 0.0, 1e-6));
}
