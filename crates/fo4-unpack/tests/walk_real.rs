//! Optional walk on a user's own installation; the default tests stay synthetic.
use fo4_core::{Budget, Manifest};
use fo4_scene::collision;
use fo4_scene::{Loader, RuleLevel};
use fo4_walk::{Character, Input, Params, Settings, World};

#[test]
#[ignore = "requires FO4_MANIFEST, FO4_CELL and FO4_WALK_FROM (x,y,z above a floor) for a user's own installation; game data is not distributed"]
fn real_cell_walk_settles_and_is_deterministic() {
    let manifest = std::env::var_os("FO4_MANIFEST")
        .expect("RequiredInstallation: set FO4_MANIFEST when explicitly running this ignored test");
    let cell = std::env::var("FO4_CELL").expect("RequiredCell: set FO4_CELL to a cell EditorID");
    let from: Vec<f64> = std::env::var("FO4_WALK_FROM")
        .expect("RequiredStart: set FO4_WALK_FROM to x,y,z above a floor of the cell")
        .split(',')
        .map(|v| v.parse().unwrap())
        .collect();
    let from = [from[0], from[1], from[2]];
    let m = Manifest::load(std::path::Path::new(&manifest)).unwrap();
    let store =
        fo4_plugin::typed::Store::open(&m, &Budget::new(m.limits.max_override_memory)).unwrap();
    let vfs = fo4_vfs::Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory)).unwrap();
    let b = Budget::new(m.limits.max_memory);
    let mut loader = Loader::new(&store, &vfs, &m.language, RuleLevel::Hypotheses, &b);
    let scene = loader.by_editor_id(&cell, &b).unwrap();
    let options = collision::Options {
        level: RuleLevel::Hypotheses,
        body_placement: None,
    };
    let c = collision::cell(&scene, &loader, &m.limits, options, &b).unwrap();
    let w = World::build(&c, c.body_placement.unwrap(), &b).unwrap();
    assert!(w.stats.triangles + w.stats.hulls > 0);
    let run = || {
        let mut ch = Character::new(Params::PROVISIONAL, Settings::DEFAULT, from).unwrap();
        ch.settle(&w, 600);
        let settled = ch.state();
        let mut states = vec![settled];
        for k in 0..600u32 {
            let input = Input {
                forward: 1.0,
                heading: 0.01 * f64::from(k),
                ..Input::default()
            };
            states.push(ch.tick(&w, input));
        }
        states
    };
    let a = run();
    assert!(a[0].grounded, "{:?}", a[0]);
    let surface = w
        .ray_down(from[0], from[1], from[2])
        .expect("a surface under the start");
    assert!(a[0].feet[2] >= surface.z - 1e-6, "{:?} {surface:?}", a[0]);
    assert_eq!(a, run());
}
