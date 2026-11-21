use super::*;
use fo4_core::{Budget, Limits};
use fo4_scene::{camera::Camera, transform::Transform};
fn camera() -> Camera {
    Camera {
        rotation: Transform::IDENTITY.rotation,
        translation: [0.; 3],
        scale: 1.,
        frustum: [-1., 1., 1., -1., 0.1, 100.],
        viewport: [0., 1., 1., 0.],
        size: [4, 4],
    }
}
fn alpha() -> Alpha {
    Alpha {
        enabled: 0,
        source: 0,
        destination: 0,
        test_enabled: 0,
        test_ref: 0,
        nif_test_function: None,
        opacity: 1.,
        nif_flags: None,
    }
}
fn vertex(x: f64, y: f64, z: f64, u: f64) -> Vertex {
    Vertex {
        position: [z, z * (1. - y / 2.), z * (x / 2. - 1.)],
        uv: [u, 0.],
    }
}
fn list() -> DrawList {
    DrawList {
        meshes: vec![Mesh {
            vertex_colors: None,
            vertices: vec![
                vertex(0., 0., 1., 0.),
                vertex(4., 0., 1., 1.),
                vertex(0., 4., 1., 0.),
                vertex(4., 4., 1., 1.),
            ],
            triangles: vec![[0, 1, 2], [1, 3, 2]],
        }],
        textures: vec![Texture {
            mips: Vec::new(),
            width: 1,
            height: 1,
            rgba: vec![1., 0., 0., 1.],
            mip: 0,
            srgb: false,
        }],
        draws: vec![Draw {
            sidedness: crate::Sidedness::default(),
            instance: 7,
            mesh: 0,
            texture: 0,
            world: Transform::IDENTITY,
            alpha: alpha(),
            uv_offset: [0.; 2],
            uv_scale: [1.; 2],
            raw_tile_flags: None,
        }],
        overlays: Vec::new(),
    }
}
fn frame(l: &DrawList, c: &Camera, workers: usize) -> Frame {
    render(
        l,
        c,
        RenderOptions {
            bilinear: false,
            backface_cull: false,
            workers,
            clear: [0., 0., 0., 1.],
        },
        &Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap()
}
#[test]
fn exact_top_left_coverage_shared_edge_and_winding() {
    let mut l = list();
    l.meshes[0].triangles.truncate(1);
    let f = frame(&l, &camera(), 1);
    assert_eq!(
        f.instances,
        [
            7,
            7,
            7,
            u32::MAX,
            7,
            7,
            u32::MAX,
            u32::MAX,
            7,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            u32::MAX
        ]
    );
    assert_eq!(f.statistics.contributing_samples, 6);
    l.meshes[0].triangles.push([1, 3, 2]);
    let f = frame(&l, &camera(), 1);
    assert_eq!(f.instances, [7; 16]);
    assert_eq!(f.statistics.contributing_samples, 16);
    for t in &mut l.meshes[0].triangles {
        t.swap(1, 2);
    }
    assert_eq!(frame(&l, &camera(), 1).instances, f.instances);
}
#[test]
fn perspective_uv_uses_reciprocal_depth() {
    let mut l = list();
    l.meshes[0].triangles.truncate(1);
    l.meshes[0].vertices[1] = vertex(4., 0., 4., 1.);
    l.textures[0] = Texture {
        mips: Vec::new(),
        width: 4,
        height: 1,
        rgba: vec![
            1., 0., 0., 1., 0., 1., 0., 1., 0., 0., 1., 1., 1., 1., 1., 1.,
        ],
        mip: 0,
        srgb: false,
    };
    // At (1.5,.5), screen barycentrics=(.5,.375,.125).
    // u=(.375/4)/(.5+.375/4+.125)=.130434..., not affine .375.
    let f = frame(&l, &camera(), 1);
    assert_eq!(f.color[1], [1., 0., 0., 1.]);
    assert!((f.depth[1] - 1. / 0.71875).abs() < 1e-6);
}
#[test]
fn depth_wins_independent_of_draw_order_and_clips_near_plane() {
    let mut l = list();
    l.textures.push(Texture {
        mips: Vec::new(),
        width: 1,
        height: 1,
        rgba: vec![0., 1., 0., 1.],
        mip: 0,
        srgb: false,
    });
    l.draws.push(Draw {
        sidedness: crate::Sidedness::default(),
        instance: 9,
        texture: 1,
        world: Transform {
            scale: 2.,
            ..Transform::IDENTITY
        },
        ..l.draws[0]
    });
    assert_eq!(frame(&l, &camera(), 1).instances, [7; 16]);
    l.draws.swap(0, 1);
    assert_eq!(frame(&l, &camera(), 1).instances, [7; 16]);
    let mut c = camera();
    c.frustum[4] = 1.5;
    assert_eq!(frame(&l, &c, 1).instances, [9; 16]);
    c.frustum[5] = 1.75;
    assert_eq!(frame(&l, &c, 1).instances, [u32::MAX; 16]);
}
#[test]
fn raw_alpha_threshold_blend_and_back_to_front_order() {
    let mut l = list();
    l.textures[0].rgba = vec![0., 0., 1., 1.];
    l.textures.push(Texture {
        mips: Vec::new(),
        width: 1,
        height: 1,
        rgba: vec![1., 0., 0., 0.5],
        mip: 0,
        srgb: false,
    });
    l.draws[0].world.scale = 3.;
    l.draws.push(Draw {
        sidedness: crate::Sidedness::default(),
        instance: 8,
        texture: 1,
        world: Transform {
            scale: 1.,
            ..Transform::IDENTITY
        },
        alpha: Alpha {
            enabled: 1,
            source: 6,
            destination: 7,
            ..alpha()
        },
        ..l.draws[0]
    });
    let f = frame(&l, &camera(), 1);
    assert_eq!(f.color[0], [0.5, 0., 0.5, 0.75]);
    assert_eq!(f.instances[0], 8);
    assert_eq!(f.depth[0], 1.);
    l.draws[1].alpha.test_enabled = 1;
    l.draws[1].alpha.test_ref = 128;
    assert_eq!(frame(&l, &camera(), 1).color[0], [0., 0., 1., 1.]);
    l.draws[1].alpha.test_ref = 127;
    assert_eq!(frame(&l, &camera(), 1).color[0], f.color[0]);
    // Add far green translucent layer after the near red in input: sorting must reverse them.
    l.textures.push(Texture {
        mips: Vec::new(),
        width: 1,
        height: 1,
        rgba: vec![0., 1., 0., 0.5],
        mip: 0,
        srgb: false,
    });
    l.draws.push(Draw {
        sidedness: crate::Sidedness::default(),
        instance: 10,
        texture: 2,
        world: Transform {
            scale: 2.,
            ..Transform::IDENTITY
        },
        ..l.draws[1]
    });
    assert_eq!(frame(&l, &camera(), 1).color[0][..3], [0.5, 0.25, 0.25]);
}
#[test]
fn deterministic_png_indices_depth_and_corrupt_inputs_are_bounded() {
    let l = list();
    let c = camera();
    let a = frame(&l, &c, 1);
    let b = frame(&l, &c, 4);
    let budget = Budget::new(8 << 20);
    assert_eq!(a.png(&budget).unwrap(), b.png(&budget).unwrap());
    assert_eq!(
        a.instance_bytes(&budget).unwrap(),
        b.instance_bytes(&budget).unwrap()
    );
    assert_eq!(
        a.depth_bytes(&budget).unwrap(),
        b.depth_bytes(&budget).unwrap()
    );
    let png = a.png(&budget).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let opts = RenderOptions {
        bilinear: false,
        backface_cull: false,
        workers: 1,
        clear: [0.; 4],
    };
    assert!(render(&l, &c, opts, &Limits::default(), &Budget::new(1)).is_err());
    let mut bad = list();
    bad.meshes[0].triangles[0][0] = 65535;
    assert_eq!(
        render(&bad, &c, opts, &Limits::default(), &budget)
            .unwrap_err()
            .code,
        "RenderVertexIndex"
    );
    bad = list();
    bad.textures[0].rgba.pop();
    assert_eq!(
        render(&bad, &c, opts, &Limits::default(), &budget)
            .unwrap_err()
            .code,
        "InvalidRenderTexture"
    );
    bad = list();
    bad.draws[0].alpha.enabled = 1;
    bad.draws[0].alpha.source = 999;
    assert_eq!(
        render(&bad, &c, opts, &Limits::default(), &budget)
            .unwrap_err()
            .code,
        "UnsupportedRenderAlpha"
    );
    let mut c = c;
    c.size = [u32::MAX, u32::MAX];
    assert!(render(&l, &c, opts, &Limits::default(), &budget).is_err());
}
#[test]
fn snapshot_round_trip_every_truncation_and_corrupt_counts() {
    let list = list();
    let l = Limits::default();
    let mut bytes = Vec::new();
    snapshot::write(&list, &mut bytes, &l).unwrap();
    let b = Budget::new(8 << 20);
    let parsed = snapshot::read(&bytes, &l, &b).unwrap();
    assert_eq!(
        frame(&list, &camera(), 1).png(&b).unwrap(),
        frame(&parsed, &camera(), 4).png(&b).unwrap()
    );
    for n in 0..bytes.len() {
        assert!(
            snapshot::read(&bytes[..n], &l, &Budget::new(8 << 20)).is_err(),
            "prefix {n}"
        );
    }
    for at in [8, 12, 16, 20, 24] {
        let mut corrupt = bytes.clone();
        corrupt[at..at + 4].fill(255);
        assert!(
            snapshot::read(&corrupt, &l, &Budget::new(8 << 20)).is_err(),
            "count {at}"
        );
    }
    assert!(snapshot::read(&bytes, &l, &Budget::new(1)).is_err());
    bytes.push(0);
    assert_eq!(
        snapshot::read(&bytes, &l, &b).unwrap_err().code,
        "UnexpectedRenderSnapshotTail"
    );
}
#[test]
fn measured_world_point_lands_on_the_predicted_pixel_in_the_raster() {
    let c = Camera {
        rotation: [[0., 1., 0.], [0., 0., 1.], [1., 0., 0.]],
        translation: [10., 20., 30.],
        scale: 2.,
        frustum: [-1., 1., 1., -1., 1., 100.],
        viewport: [0., 1., 1., 0.],
        size: [100, 100],
    };
    // P=(14,28,32) => a=(4,1,2) => pixel=(75,37.5).
    let predicted = c.project([14., 28., 32.]).unwrap().unwrap();
    assert_eq!(predicted.pixel, [75., 37.5]);
    let mut l = list();
    l.meshes[0].vertices = vec![[74., 36.], [77., 36.], [74., 40.], [77., 40.]]
        .into_iter()
        .map(|p| {
            let a = [4., 4. * (1. - p[1] / 50.), 4. * (p[0] / 50. - 1.)];
            Vertex {
                position: std::array::from_fn(|i| {
                    c.translation[i]
                        + c.scale * (0..3).map(|j| c.rotation[j][i] * a[j]).sum::<f64>()
                }),
                uv: [0.; 2],
            }
        })
        .collect();
    let f = frame(&l, &c, 1);
    assert_eq!(f.instances[37 * 100 + 75], 7);
}
#[test]
fn each_registered_blend_tuple_uses_its_raw_numbers() {
    let mut l = list();
    l.textures[0].rgba = vec![0.2, 0.4, 0.8, 1.];
    l.draws[0].world.scale = 3.;
    l.textures.push(Texture {
        mips: Vec::new(),
        width: 1,
        height: 1,
        rgba: vec![0.5, 0.25, 0.75, 0.5],
        mip: 0,
        srgb: false,
    });
    l.draws.push(Draw {
        sidedness: crate::Sidedness::default(),
        instance: 8,
        texture: 1,
        world: Transform::IDENTITY,
        alpha: Alpha {
            enabled: 1,
            source: 6,
            destination: 7,
            ..alpha()
        },
        ..l.draws[0]
    });
    for ((source, destination), expected) in [
        ((6, 7), [0.35, 0.325, 0.775]),
        ((6, 0), [0.45, 0.525, 1.]),
        ((4, 1), [0.1, 0.1, 0.6]),
    ] {
        l.draws[1].alpha.source = source;
        l.draws[1].alpha.destination = destination;
        let f = frame(&l, &camera(), 1);
        for (actual, expected) in f.color[0][..3].iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }
}

#[test]
fn bilinear_centers_area_lod_and_raw_sidedness_are_deterministic() {
    let mut l = list();
    let t = &mut l.textures[0];
    t.width = 2;
    t.height = 2;
    t.rgba = vec![
        1., 0., 0., 1., 0., 1., 0., 1., 0., 0., 1., 1., 1., 1., 1., 1.,
    ];
    let (sample, level) = t.sample_lod([0.5, 0.5], true, 0.);
    assert_eq!(sample, [0.5, 0.5, 0.5, 1.]);
    assert_eq!(level, 0);
    t.mips.push(Mip {
        width: 1,
        height: 1,
        mip: 1,
        rgba: vec![0., 1., 0., 1.],
    });
    for v in &mut l.meshes[0].vertices {
        let q = camera().project(v.position).unwrap().unwrap();
        v.uv = [q.pixel[0] / 4., q.pixel[1] / 4.];
    }
    l.draws[0].uv_scale = [16.; 2];
    l.draws[0].sidedness = Sidedness {
        material_two_sided: Some(0),
        nif_shader_flags_2: Some(16),
    };
    let options = RenderOptions {
        workers: 4,
        clear: [0.; 4],
        bilinear: true,
        backface_cull: false,
    };
    let b = Budget::new(8 << 20);
    let f = render(&l, &camera(), options, &Limits::default(), &b).unwrap();
    assert_eq!(f.color, vec![[0., 1., 0., 1.]; 16]);
    assert_eq!(f.mip_usage[0], [0, 16]);
    let c = render(
        &l,
        &camera(),
        RenderOptions {
            backface_cull: true,
            ..options
        },
        &Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    assert_eq!(c.statistics.culled_triangles, 2);
    assert_eq!(c.instances, [u32::MAX; 16]);
    l.draws[0].sidedness.material_two_sided = Some(1);
    let c = render(
        &l,
        &camera(),
        RenderOptions {
            backface_cull: true,
            ..options
        },
        &Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap();
    assert_eq!(c.color, f.color);
    let mut bytes = Vec::new();
    snapshot::write(&l, &mut bytes, &Limits::default()).unwrap();
    let copy = snapshot::read(&bytes, &Limits::default(), &Budget::new(8 << 20)).unwrap();
    assert_eq!(copy.textures[0].mips[0].mip, 1);
    assert_eq!(copy.draws[0].sidedness.material_two_sided, Some(1));
}

#[test]
fn compact_snapshot_is_exact_and_rejects_wrong_declared_lengths() {
    let l = list();
    let limits = Limits::default();
    let budget = Budget::new(8 << 20);
    let mut compact = Vec::new();
    let decoded = snapshot::write_compact(&l, &mut compact, &limits, &budget).unwrap();
    let mut raw = Vec::new();
    assert_eq!(snapshot::write(&l, &mut raw, &limits).unwrap(), decoded);
    let got = snapshot::read(&compact, &limits, &budget).unwrap();
    assert_eq!(
        frame(&l, &camera(), 1).color,
        frame(&got, &camera(), 1).color
    );
    for n in 0..compact.len() {
        assert!(snapshot::read(&compact[..n], &limits, &Budget::new(8 << 20)).is_err());
    }
    for n in [decoded - 1, decoded + 1] {
        let mut bad = compact.clone();
        bad[8..16].copy_from_slice(&n.to_le_bytes());
        assert!(snapshot::read(&bad, &limits, &Budget::new(8 << 20)).is_err());
    }
    compact.push(0);
    assert!(snapshot::read(&compact, &limits, &Budget::new(8 << 20)).is_err());
}

#[test]
fn effect_vertex_channels_interpolate_and_survive_clipping_and_snapshot() {
    let mut l = list();
    l.meshes[0].vertex_colors = Some(vec![[0.2, 0.4, 0.6, 0.5]; l.meshes[0].vertices.len()]);
    l.textures[0].rgba = vec![2., 1., 0.5, 1.];
    l.draws[0].alpha.opacity = 0.5;
    let f = frame(&l, &camera(), 1);
    for pixel in &f.color {
        for (actual, expected) in pixel.iter().zip([0.4, 0.4, 0.3, 0.25]) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }
    let limits = Limits::default();
    let mut bytes = Vec::new();
    snapshot::write(&l, &mut bytes, &limits).unwrap();
    for n in bytes.len() - l.meshes[0].vertices.len() * 16 - 1..bytes.len() {
        assert!(snapshot::read(&bytes[..n], &limits, &Budget::new(8 << 20)).is_err());
    }
    let copy = snapshot::read(&bytes, &limits, &Budget::new(8 << 20)).unwrap();
    assert_eq!(copy.meshes[0].vertex_colors, l.meshes[0].vertex_colors);
    assert_eq!(frame(&copy, &camera(), 1).color, f.color);
    l.meshes[0].vertex_colors.as_mut().unwrap().pop();
    assert!(render(
        &l,
        &camera(),
        RenderOptions {
            workers: 1,
            clear: [0.; 4],
            bilinear: false,
            backface_cull: false
        },
        &limits,
        &Budget::new(8 << 20)
    )
    .is_err());
    assert!(snapshot::write(&l, &mut Vec::new(), &limits).is_err());
    l.meshes[0].vertex_colors = Some(vec![[f32::NAN; 4]; l.meshes[0].vertices.len()]);
    assert!(render(
        &l,
        &camera(),
        RenderOptions {
            workers: 1,
            clear: [0.; 4],
            bilinear: false,
            backface_cull: false
        },
        &limits,
        &Budget::new(8 << 20)
    )
    .is_err());
}

#[test]
fn effect_color_is_perspective_correct_across_near_clipping() {
    let mut l = list();
    l.meshes[0].triangles.truncate(1);
    l.meshes[0].vertices[1] = vertex(4., 0., 4., 1.);
    l.meshes[0].vertex_colors = Some(vec![
        [0., 0., 0., 1.],
        [1., 0., 0., 1.],
        [0., 0., 0., 1.],
        [0., 0., 0., 1.],
    ]);
    let full = frame(&l, &camera(), 1);
    assert!((full.color[1][0] - (0.09375 / 0.71875)).abs() < 1e-6);
    let mut clipped_camera = camera();
    clipped_camera.frustum[4] = 1.5;
    let clipped = frame(&l, &clipped_camera, 1);
    assert_eq!(clipped.instances[2], 7);
    assert!((clipped.color[2][0] - full.color[2][0]).abs() < 1e-6);
}

#[test]
fn snapshot_writer_refuses_what_the_reader_would_reject() {
    let limits = Limits::default();
    let budget = Budget::new(8 << 20);
    for two_sided in [None, Some(0), Some(1)] {
        let mut ok = list();
        ok.draws[0].sidedness.material_two_sided = two_sided;
        let mut bytes = Vec::new();
        snapshot::write(&ok, &mut bytes, &limits).unwrap();
        let parsed = snapshot::read(&bytes, &limits, &budget).unwrap();
        assert_eq!(parsed.draws[0].sidedness.material_two_sided, two_sided);
    }
    for raw in [2u8, 254, 255] {
        let mut bad = list();
        bad.draws[0].sidedness.material_two_sided = Some(raw);
        let mut bytes = Vec::new();
        assert_eq!(
            snapshot::write(&bad, &mut bytes, &limits).unwrap_err().code,
            "SnapshotSidedness"
        );
    }
    let mut bad = list();
    bad.draws[0].uv_scale[1] = f64::NAN;
    let mut bytes = Vec::new();
    assert_eq!(
        snapshot::write(&bad, &mut bytes, &limits).unwrap_err().code,
        "SnapshotNonFinite"
    );
}
