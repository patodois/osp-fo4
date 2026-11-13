//! Optional measurement fixture replay; the default workspace tests remain synthetic/offline.
use fo4_scene::transform::{euler_zyx, Transform};
#[test]
#[ignore = "requires FO4_TRANSFORM_FIXTURE; optional measurement data is not distributed"]
fn optional_measured_transform_fixture_replays_its_recorded_scope() {
    let path = std::env::var_os("FO4_TRANSFORM_FIXTURE")
        .expect("RequiredMeasurementFixture: set FO4_TRANSFORM_FIXTURE when explicitly running this ignored replay");
    let path = std::path::Path::new(&path);
    let size = std::fs::metadata(path).unwrap().len();
    assert!(size <= 64 << 20, "MeasurementSizeLimit");
    let data: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let rows = data["measurements"].as_array().unwrap();
    let available: Vec<_> = rows
        .iter()
        .filter(|row| row["root_available"] == true)
        .collect();
    assert!(
        !available.is_empty(),
        "MeasurementFixtureHasNoAvailableRoots"
    );
    fn bits(v: &serde_json::Value) -> u32 {
        u32::from_str_radix(v["bits"].as_str().unwrap(), 16).unwrap()
    }
    for (index, row) in available.iter().enumerate() {
        assert_eq!(row["root_available"], true);
        let placement = std::array::from_fn(|i| {
            row["file"][if i < 3 { "position" } else { "rotation" }][i % 3]
                .as_f64()
                .unwrap() as f32
        });
        let scale = row["file"]["scale"].as_f64().unwrap() as f32;
        let transform = Transform::euler(placement, scale).unwrap();
        for associated in [false, true] {
            let actual = euler_zyx([placement[3], placement[4], placement[5]], associated);
            for (i, r) in actual.iter().enumerate() {
                for (j, v) in r.iter().enumerate() {
                    assert_eq!(
                        v.to_bits(),
                        bits(&row["root_world"]["rotation"][i][j]),
                        "row {index}, association {associated}, R[{i}][{j}]"
                    );
                    if !associated {
                        assert_eq!((transform.rotation[i][j] as f32).to_bits(), v.to_bits());
                    }
                }
            }
        }
        for i in 0..3 {
            assert_eq!(
                (transform.translation[i] as f32).to_bits(),
                bits(&row["root_world"]["translation"][i]),
                "row {index} translation {i}"
            );
        }
        assert_eq!(
            (transform.scale as f32).to_bits(),
            bits(&row["root_world"]["scale"]),
            "row {index} scale"
        );
    }
    eprintln!("MeasurementFixture: recorded fixture scope replayed; this does not establish universal bit-exact arithmetic");
}
