//! Optional run on a user's own installation; the default tests stay synthetic.
use fo4_core::{Budget, Manifest};
use fo4_scene::collision::{self, BodyPlacement, Shape};
use fo4_scene::{Loader, RuleLevel};

#[test]
#[ignore = "requires FO4_MANIFEST and FO4_CELL naming a user's own installation and cell; game data is not distributed"]
fn real_cell_collision_places_bodies_under_hypotheses() {
    let manifest = std::env::var_os("FO4_MANIFEST")
        .expect("RequiredInstallation: set FO4_MANIFEST when explicitly running this ignored test");
    let cell = std::env::var("FO4_CELL")
        .expect("RequiredCell: set FO4_CELL to a cell EditorID of the installation");
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
    assert_eq!(c.body_placement, Some(BodyPlacement::Ignored));
    assert!(!c.instances.is_empty(), "{:?}", c.unplaced.first());
    assert!(c.models.iter().all(|m| m.error.is_none()));
    for i in &c.instances {
        for v in BodyPlacement::ALL {
            let bx = c.world_bounds(i, v).expect("finite world bounds");
            assert!(bx.min.iter().chain(&bx.max).all(|x| x.is_finite()));
        }
        if let Shape::Mesh { triangles, .. } = &c.leaf(i).shape {
            assert_eq!(
                c.world_triangles(i, BodyPlacement::Ignored).count(),
                triangles.len()
            );
        }
    }
}
