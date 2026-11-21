//! Optional run on a user's own installation; the default tests stay synthetic.
use fo4_core::{Budget, Manifest};
use fo4_scene::terrain::{self, Outcome, QUADS};
use fo4_scene::RuleLevel;

#[test]
#[ignore = "requires FO4_MANIFEST and FO4_EXTERIOR_CELL naming a user's own installation and an exterior cell; game data is not distributed"]
fn real_exterior_cell_terrain_is_built_under_hypotheses_only() {
    let manifest = std::env::var_os("FO4_MANIFEST")
        .expect("RequiredInstallation: set FO4_MANIFEST when explicitly running this ignored test");
    let cell = std::env::var("FO4_EXTERIOR_CELL")
        .expect("RequiredCell: set FO4_EXTERIOR_CELL to an exterior cell EditorID");
    let m = Manifest::load(std::path::Path::new(&manifest)).unwrap();
    let store =
        fo4_plugin::typed::Store::open(&m, &Budget::new(m.limits.max_override_memory)).unwrap();
    let b = Budget::new(m.limits.max_memory);
    let key = store.find_cell(&cell, &b).unwrap().expect("cell EditorID");
    assert!(store.world(&key).is_some());
    let at = |level| terrain::Options {
        level,
        diagonal: None,
    };
    for level in [RuleLevel::Facts, RuleLevel::Measured] {
        assert!(matches!(
            terrain::load(&store, &key, at(level), &b).unwrap(),
            Outcome::Refused(_)
        ));
    }
    let Outcome::Built(t) = terrain::load(&store, &key, at(RuleLevel::Hypotheses), &b).unwrap()
    else {
        panic!("terrain expected under hypotheses")
    };
    assert_eq!(t.world_triangles().count(), 2 * QUADS * QUADS);
    assert!(t.heights.iter().all(|z| z.is_finite()));
    let [x, y] = t.origin();
    assert!(t.height_at(x + 2048.0, y + 2048.0).is_some());
}
