use super::*;
fn u32s(out: &mut Vec<u8>, values: &[u32]) {
    for n in values {
        out.extend(n.to_le_bytes());
    }
}
fn text(out: &mut Vec<u8>, s: &[u8]) {
    u32s(out, &[s.len() as u32]);
    out.extend(s);
}
fn nif(bs: u32, blocks: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    u32s(&mut out, &[0x14020007]);
    out.push(1);
    u32s(&mut out, &[12, blocks.len() as u32, bs]);
    out.extend(vec![0; if bs == 130 { 4 } else { 3 }]);
    out.extend((blocks.len() as u16).to_le_bytes());
    for (name, _) in blocks {
        text(&mut out, name.as_bytes());
    }
    for i in 0..blocks.len() {
        out.extend((i as u16).to_le_bytes());
    }
    for (_, data) in blocks {
        u32s(&mut out, &[data.len() as u32]);
    }
    u32s(&mut out, &[1, 4]);
    text(&mut out, b"name");
    u32s(&mut out, &[0]);
    for (_, data) in blocks {
        out.extend(data);
    }
    u32s(&mut out, &[if blocks.is_empty() { 0 } else { 1 }]);
    if !blocks.is_empty() {
        u32s(&mut out, &[0]);
    }
    out
}
fn av() -> Vec<u8> {
    let mut v = Vec::new();
    u32s(&mut v, &[0, 0, u32::MAX, 0]);
    for f in [0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.] {
        u32s(&mut v, &[f32::to_bits(f)]);
    }
    u32s(&mut v, &[u32::MAX]);
    v
}
fn shape(bs: u32) -> Vec<u8> {
    let mut v = av();
    for f in [0., 0., 0., 1.] {
        u32s(&mut v, &[f32::to_bits(f)]);
    }
    u32s(&mut v, &[u32::MAX; 3]);
    let desc = (1u64 << 44) | if bs == 130 { 2 } else { 4 };
    v.extend(desc.to_le_bytes());
    if bs == 130 {
        u32s(&mut v, &[1]);
    } else {
        v.extend(1u16.to_le_bytes());
    }
    v.extend(3u16.to_le_bytes());
    u32s(&mut v, &[if bs == 130 { 30 } else { 54 }]);
    for _ in 0..3 {
        if bs == 130 {
            for value in [0x3c00u16, 0, 0, 0] {
                v.extend(value.to_le_bytes());
            }
        } else {
            for value in [1f32, 0., 0., 0.] {
                v.extend(value.to_le_bytes());
            }
        }
    }
    for i in [0u16, 1, 2] {
        v.extend(i.to_le_bytes());
    }
    if bs == 100 {
        u32s(&mut v, &[0]);
    }
    v
}
fn parse<'a>(bytes: &'a [u8], bs: u32, b: &Budget) -> Result<Nif<'a>> {
    Nif::read(
        bytes,
        "synthetic.nif",
        if bs == 130 {
            Game::Fallout4
        } else {
            Game::SkyrimSe
        },
        &Limits::default(),
        b,
    )
}
#[test]
fn static_nodes_shapes_both_games_and_raw_blocks() {
    for bs in [100, 130] {
        let mut node = av();
        u32s(&mut node, &[1, 1]);
        if bs == 100 {
            u32s(&mut node, &[0]);
        }
        let bytes = nif(
            bs,
            &[
                ("BSFadeNode", node),
                ("BSTriShape", shape(bs)),
                ("bhkFuture", vec![5; 17]),
                ("BSSkin::Instance", vec![7; 5]),
                ("UnknownFuture", vec![9; 3]),
            ],
        );
        let n = parse(&bytes, bs, &Budget::new(1 << 20)).unwrap();
        assert!(n.blocks.iter().all(|b| b.diagnostics.is_empty()));
        assert_eq!(n.raw_block(2).unwrap(), &[5; 17]);
        assert!(n.blocks[2].decoded.is_none());
        if let Some(Data::Mesh(m)) = &n.blocks[1].decoded {
            assert_eq!(m.vertices.as_ref().unwrap()[0].position, Some([1., 0., 0.]));
            assert_eq!(m.triangles.as_ref().unwrap(), &[[0, 1, 2]]);
        } else {
            panic!("mesh not decoded");
        }
    }
}
#[test]
fn header_every_truncation_corruption_and_budget() {
    let bytes = nif(130, &[("BSTriShape", shape(130))]);
    for end in 0..bytes.len() {
        assert!(
            parse(&bytes[..end], 130, &Budget::new(16384)).is_err(),
            "{end}"
        );
    }
    for offset in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[offset] ^= 0x80;
        let b = Budget::new(16384);
        let _ = parse(&bad, 130, &b);
        assert!(b.used() <= 16384);
    }
    for max in [0, 16, 128, 512, 1024] {
        let b = Budget::new(max);
        let _ = parse(&bytes, 130, &b);
        assert!(b.used() <= max);
    }
    assert_eq!(
        parse(&bytes, 100, &Budget::new(16384)).unwrap_err().code,
        "UnsupportedNifBsVersion"
    );
}
#[test]
fn bad_geometry_stays_raw_with_diagnostic_and_exact_block_bound() {
    let mut data = shape(130);
    let len = data.len();
    data[len - 2..].copy_from_slice(&9u16.to_le_bytes());
    let bytes = nif(130, &[("BSTriShape", data), ("Unknown", vec![1, 2, 3])]);
    let n = parse(&bytes, 130, &Budget::new(16384)).unwrap();
    assert!(n.blocks[0].decoded.is_none());
    assert_eq!(n.blocks[0].diagnostics[0].code, "InvalidNifTriangleIndex");
    assert_eq!(n.raw_block(1).unwrap(), [1, 2, 3]);
}
#[test]
fn cycle_and_depth_have_named_errors() {
    let mut node = av();
    u32s(&mut node, &[1, 0]);
    let bytes = nif(130, &[("NiNode", node)]);
    assert_eq!(
        parse(&bytes, 130, &Budget::new(16384)).unwrap_err().code,
        "CyclicNifScene"
    );
}

#[test]
fn common_extra_and_controller_blocks_are_interpreted() {
    let mut packed = Vec::new();
    u32s(&mut packed, &[0]);
    packed.extend(0u64.to_le_bytes());
    u32s(&mut packed, &[0, 0, 0, 0, 0]);
    let mut points = Vec::new();
    u32s(&mut points, &[0, 1]);
    text(&mut points, b"parent");
    text(&mut points, b"attach");
    for f in [1f32, 0., 0., 0., 1., 2., 3., 1.] {
        points.extend(f.to_le_bytes());
    }
    let mut controller = Vec::new();
    u32s(&mut controller, &[u32::MAX]);
    controller.extend(76u16.to_le_bytes());
    for v in [1f32, 0., 0., 1.] {
        controller.extend(v.to_le_bytes());
    }
    u32s(&mut controller, &[u32::MAX]);
    controller.push(0);
    u32s(&mut controller, &[0, u32::MAX]);
    let mut interp = Vec::new();
    for f in [0f32, 0., 0., 1., 0., 0., 0., 1.] {
        interp.extend(f.to_le_bytes());
    }
    u32s(&mut interp, &[u32::MAX]);
    let bytes = nif(
        130,
        &[
            ("BSPackedCombinedSharedGeomDataExtra", packed),
            ("BSConnectPoint::Parents", points),
            ("NiControllerManager", controller),
            ("NiTransformInterpolator", interp),
        ],
    );
    let n = parse(&bytes, 130, &Budget::new(1 << 20)).unwrap();
    for block in &n.blocks {
        assert!(
            block.decoded.is_some(),
            "type {}",
            n.block_types[block.type_index as usize]
        );
        assert!(block.raw_tail.is_empty());
        assert!(block.diagnostics.is_empty());
    }
}

#[test]
fn documented_skyrim_legacy_stream_is_selected_by_sse_profile() {
    let mut node = av();
    u32s(&mut node, &[0, 0]);
    let bytes = nif(83, &[("NiNode", node)]);
    let n = parse(&bytes, 83, &Budget::new(16384)).unwrap();
    assert_eq!(n.bs_version, 83);
    assert!(n.blocks[0].decoded.is_some());
    assert!(n.blocks[0].raw_tail.is_empty());
}

#[test]
fn geometry_variants_keep_lod_segments_dynamic_and_compact_attributes() {
    for bs in [100, 130] {
        for name in [
            "BSTriShape",
            "BSSubIndexTriShape",
            "BSMeshLODTriShape",
            "BSDynamicTriShape",
        ] {
            if (name == "BSMeshLODTriShape" && bs == 100)
                || (name == "BSDynamicTriShape" && bs == 130)
            {
                continue;
            }
            let mut body = shape(bs);
            match name {
                "BSSubIndexTriShape" => {
                    if bs == 100 {
                        u32s(&mut body, &[0]);
                    } else {
                        u32s(&mut body, &[1, 0, 0]);
                    }
                }
                "BSMeshLODTriShape" => u32s(&mut body, &[1, 0, 0]),
                "BSDynamicTriShape" => {
                    u32s(&mut body, &[48]);
                    for _ in 0..3 {
                        for x in [1f32, 2., 3., 1.] {
                            body.extend(x.to_le_bytes());
                        }
                    }
                }
                _ => {}
            }
            let bytes = nif(bs, &[(name, body)]);
            let n = parse(&bytes, bs, &Budget::new(1 << 20)).unwrap();
            assert!(
                n.blocks[0].decoded.is_some(),
                "{name} {} {:?}",
                bs,
                n.blocks[0].diagnostics
            );
            assert!(n.blocks[0].raw_tail.is_empty());
        }
    }
    let mut node = av();
    u32s(&mut node, &[1, 1]);
    let mut end = av();
    u32s(&mut end, &[0]);
    let bytes = nif(130, &[("NiNode", node), ("NiNode", end)]);
    let limits = Limits {
        max_depth: 1,
        ..Limits::default()
    };
    assert_eq!(
        Nif::read(
            &bytes,
            "depth",
            Game::Fallout4,
            &limits,
            &Budget::new(16384)
        )
        .unwrap_err()
        .code,
        "LimitExceeded"
    );
}
#[test]
fn shader_texture_metadata_and_unknown_tail_are_retained() {
    let mut set = Vec::new();
    u32s(&mut set, &[2]);
    text(&mut set, b"textures/d.dds");
    text(&mut set, b"textures/n.dds");
    let mut extra = Vec::new();
    u32s(&mut extra, &[0, 1]);
    text(&mut extra, b"raw\x81string");
    extra.push(0xa5);
    let bytes = nif(
        130,
        &[("BSShaderTextureSet", set), ("NiStringsExtraData", extra)],
    );
    let n = parse(&bytes, 130, &Budget::new(16384)).unwrap();
    assert!(n.blocks[0].decoded.is_some());
    assert!(n.blocks[1].decoded.is_some());
    assert_eq!(&bytes[n.blocks[1].raw_tail.clone()], &[0xa5]);
}

#[test]
fn stage10_nonfinite_half_position_preserves_mesh() {
    let mut shape = shape(130);
    let at = shape.len() - 6 - 24;
    shape[at..at + 2].copy_from_slice(&0x7fffu16.to_le_bytes());
    let bytes = nif(130, &[("BSTriShape", shape)]);
    let n = parse(&bytes, 130, &Budget::new(16384)).unwrap();
    let Some(Data::Mesh(m)) = &n.blocks[0].decoded else {
        panic!("nonfinite position discarded the mesh")
    };
    assert!(m.vertices.as_ref().unwrap()[0].position.unwrap()[0].is_nan());
    assert_eq!(m.non_finite.position, 1);
    assert_eq!(n.non_finite().total(), 1);
    assert!(n.blocks[0].diagnostics.is_empty());
}
#[test]
fn stage10_nonfinite_full_position_preserves_mesh() {
    let mut shape = shape(100);
    let at = shape.len() - 4 - 6 - 48;
    shape[at..at + 4].copy_from_slice(&0xff800000u32.to_le_bytes());
    let bytes = nif(100, &[("BSTriShape", shape)]);
    let n = parse(&bytes, 100, &Budget::new(16384)).unwrap();
    let Some(Data::Mesh(m)) = &n.blocks[0].decoded else {
        panic!("infinite position discarded the mesh")
    };
    assert_eq!(
        m.vertices.as_ref().unwrap()[0].position.unwrap()[0].to_bits(),
        0xff800000
    );
}
#[test]
fn stage10_nonfinite_uv_preserves_mesh() {
    let mut v = av();
    for f in [0f32, 0., 0., 1.] {
        u32s(&mut v, &[f.to_bits()]);
    }
    u32s(&mut v, &[u32::MAX; 3]);
    let desc = (3u64 << 44) | (2 << 8) | 3;
    v.extend(desc.to_le_bytes());
    u32s(&mut v, &[1]);
    v.extend(3u16.to_le_bytes());
    u32s(&mut v, &[42]);
    for _ in 0..3 {
        for h in [0x3c00u16, 0, 0, 0, 0xfc00, 0x7fff] {
            v.extend(h.to_le_bytes());
        }
    }
    for i in [0u16, 1, 2] {
        v.extend(i.to_le_bytes());
    }
    let bytes = nif(130, &[("BSTriShape", v)]);
    let n = parse(&bytes, 130, &Budget::new(16384)).unwrap();
    let Some(Data::Mesh(m)) = &n.blocks[0].decoded else {
        panic!("nonfinite UV discarded the mesh")
    };
    let uv = m.vertices.as_ref().unwrap()[0].uv.unwrap();
    assert_eq!(m.non_finite.uv, 6);
    assert_eq!(n.non_finite().total(), 6);
    assert_eq!(uv[0], f32::NEG_INFINITY);
    assert!(uv[1].is_nan());
}

#[test]
fn scene_depth_is_checked_even_when_children_precede_parents() {
    let mut leaf = av();
    u32s(&mut leaf, &[0]);
    let mut child = av();
    u32s(&mut child, &[1, 0]);
    let mut root = av();
    u32s(&mut root, &[1, 1]);
    let bytes = nif(
        130,
        &[("NiNode", leaf), ("NiNode", child), ("NiNode", root)],
    );
    let limits = Limits {
        max_depth: 2,
        ..Limits::default()
    };
    assert_eq!(
        Nif::read(
            &bytes,
            "reverse order",
            Game::Fallout4,
            &limits,
            &Budget::new(16384)
        )
        .unwrap_err()
        .code,
        "LimitExceeded"
    );
}

#[test]
fn light_color_controller_consumes_documented_target_color() {
    let mut data = Vec::new();
    u32s(&mut data, &[u32::MAX]);
    data.extend(76u16.to_le_bytes());
    for f in [1f32, 0., 0., 1.] {
        data.extend(f.to_le_bytes());
    }
    u32s(&mut data, &[u32::MAX, u32::MAX]);
    data.extend(1u16.to_le_bytes());
    let bytes = nif(130, &[("NiLightColorController", data)]);
    let n = parse(&bytes, 130, &Budget::new(16384)).unwrap();
    assert!(n.blocks[0].decoded.is_some());
    assert!(n.blocks[0].raw_tail.is_empty());
}
#[test]
fn legacy_geometry_keeps_float_color_and_triangle_presence() {
    let mut data = Vec::new();
    u32s(&mut data, &[0]);
    data.extend(1u16.to_le_bytes());
    data.extend([0, 0, 1]);
    for f in [1f32, 2., 3.] {
        data.extend(f.to_le_bytes());
    }
    data.extend(0u16.to_le_bytes());
    u32s(&mut data, &[0]);
    data.push(0);
    for f in [0f32, 0., 0., 1.] {
        data.extend(f.to_le_bytes());
    }
    data.push(1);
    for f in [0.25f32, 0.5, 0.75, 1.] {
        data.extend(f.to_le_bytes());
    }
    data.extend(0u16.to_le_bytes());
    u32s(&mut data, &[u32::MAX]);
    data.extend(0u16.to_le_bytes());
    u32s(&mut data, &[0]);
    data.push(0);
    data.extend(0u16.to_le_bytes());
    let bytes = nif(100, &[("NiTriShapeData", data)]);
    let n = parse(&bytes, 100, &Budget::new(16384)).unwrap();
    match &n.blocks[0].decoded {
        Some(Data::Mesh(m)) => {
            assert_eq!(
                m.vertices.as_ref().unwrap()[0].color_float,
                Some([0.25, 0.5, 0.75, 1.])
            );
            assert!(m.triangles.is_none());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn shader_layouts_are_bounded_and_keep_exact_float_bits() {
    for bs in [100, 130] {
        for effect in [false, true] {
            let mut data = Vec::new();
            if !effect {
                u32s(&mut data, &[1]);
            }
            u32s(&mut data, &[0, 0, u32::MAX, 0, 0]);
            for f in [0f32, 0., 1., 1.] {
                data.extend(f.to_le_bytes());
            }
            if effect {
                text(&mut data, b"textures/e.dds");
                data.extend([3, 255, 0, 0]);
                for f in [1f32, 0., 0., 1., 1., 2., 3., 4., 1., 100.] {
                    data.extend(f.to_le_bytes());
                }
                text(&mut data, b"");
                if bs == 130 {
                    for _ in 0..3 {
                        text(&mut data, b"");
                    }
                    data.extend(1f32.to_le_bytes());
                }
            } else {
                u32s(&mut data, &[u32::MAX]);
                for f in [0f32, 0., 0., 1.] {
                    data.extend(f.to_le_bytes());
                }
                if bs == 130 {
                    u32s(&mut data, &[0]);
                }
                u32s(&mut data, &[3]);
                for f in [1f32, 0., 1., 1., 1., 1., 1.] {
                    data.extend(f.to_le_bytes());
                }
                if bs == 100 {
                    for f in [0.3f32, 2.] {
                        data.extend(f.to_le_bytes());
                    }
                } else {
                    for f in [0f32, f32::MAX, 0., 1., 5., -1., -1., -1., -1., -1., -1.] {
                        data.extend(f.to_le_bytes());
                    }
                }
                data.extend(1f32.to_le_bytes());
                if bs == 130 {
                    data.extend([0, 0]);
                }
            }
            let ty = if effect {
                "BSEffectShaderProperty"
            } else {
                "BSLightingShaderProperty"
            };
            let bytes = nif(bs, &[(ty, data.clone())]);
            let n = parse(&bytes, bs, &Budget::new(1 << 20)).unwrap();
            assert!(
                n.blocks[0].decoded.is_some(),
                "{ty}/{bs}: {:?}",
                n.blocks[0].diagnostics
            );
            assert!(n.blocks[0].raw_tail.is_empty(), "{ty}/{bs}");
            for end in 0..data.len() {
                let bytes = nif(bs, &[(ty, data[..end].to_vec()), ("Future", vec![1, 2, 3])]);
                let n = parse(&bytes, bs, &Budget::new(1 << 20)).unwrap();
                assert!(!n.blocks[0].diagnostics.is_empty(), "{ty}/{bs}/{end}");
                assert_eq!(n.raw_block(1).unwrap(), [1, 2, 3]);
            }
        }
    }
}

#[test]
fn retained_diagnostic_strings_are_charged_to_the_asset_budget() {
    let bytes = nif(130, &[("BSShaderTextureSet", Vec::new())]);
    let file = "f".repeat(8192);
    let budget = Budget::new(4096);
    assert!(Nif::read(&bytes, &file, Game::Fallout4, &Limits::default(), &budget).is_err());
    assert!(budget.used() <= 4096);
}

#[test]
fn stage10_dynamic_and_metadata_ieee_are_counted_without_hiding_index_errors() {
    let mut body = shape(100);
    u32s(&mut body, &[48]);
    for i in 0..12 {
        u32s(
            &mut body,
            &[match i {
                0 => 0x7fc12345,
                1 => 0xff800000,
                _ => 0,
            }],
        );
    }
    let mut node = av();
    node[16..20].copy_from_slice(&0x7f800000u32.to_le_bytes());
    u32s(&mut node, &[0, 0]);
    let bytes = nif(100, &[("NiNode", node), ("BSDynamicTriShape", body)]);
    let n = parse(&bytes, 100, &Budget::new(65536)).unwrap();
    assert!(n.blocks.iter().all(|b| b.diagnostics.is_empty()));
    let counts = n.non_finite();
    assert_eq!(counts.dynamic_position, 2);
    assert_eq!(counts.fields, 1);
    assert_eq!(counts.total(), 3);
    let Some(Data::Mesh(m)) = &n.blocks[1].decoded else {
        panic!()
    };
    assert_eq!(
        m.dynamic_vertices.as_ref().unwrap()[0][0].to_bits(),
        0x7fc12345
    );
    let mut body = shape(130);
    let at = body.len() - 6 - 24;
    body[at..at + 2].copy_from_slice(&0x7fffu16.to_le_bytes());
    let end = body.len();
    body[end - 2..].copy_from_slice(&9u16.to_le_bytes());
    let bytes = nif(130, &[("BSTriShape", body)]);
    let n = parse(&bytes, 130, &Budget::new(65536)).unwrap();
    assert!(n.blocks[0].decoded.is_none());
    assert_eq!(n.blocks[0].diagnostics[0].code, "InvalidNifTriangleIndex");
}

#[test]
fn stage18_animation_key_layouts_are_bounded_without_pose_evaluation() {
    let mut specimens = vec![];
    for (name, width) in [("NiFloatData", 4), ("NiPosData", 12), ("NiBoolData", 1)] {
        specimens.push((name, 0u32.to_le_bytes().to_vec()));
        for kind in [1u32, 2, 3, 5] {
            let mut v = Vec::new();
            u32s(&mut v, &[2, kind]);
            for _ in 0..2 {
                v.extend(0.25f32.to_le_bytes());
                v.extend(vec![0x35; width]);
                if kind == 2 {
                    v.extend(vec![0x27; 2 * width]);
                }
                if kind == 3 {
                    v.extend([0; 12]);
                }
            }
            specimens.push((name, v));
        }
    }
    specimens.push(("NiTransformData", vec![0; 12]));
    let mut q = Vec::new();
    u32s(&mut q, &[1, 3]);
    q.extend([0; 32]);
    q.extend([0; 8]);
    specimens.push(("NiTransformData", q));
    let mut xyz = Vec::new();
    u32s(&mut xyz, &[1, 4, 0, 0, 0, 0, 0]);
    specimens.push(("NiTransformData", xyz));
    for (name, width) in [
        ("NiBlendFloatInterpolator", 4),
        ("NiBlendPoint3Interpolator", 12),
    ] {
        let mut manager = vec![1, 2];
        manager.extend(0.5f32.to_le_bytes());
        manager.extend(vec![0; width]);
        specimens.push((name, manager));
        let mut items = vec![0, 1];
        items.extend(0.5f32.to_le_bytes());
        items.extend([1, 0, 0x80, 0x80]);
        items.extend([0; 16]);
        items.extend((-1i32).to_le_bytes());
        items.extend([0; 8]);
        items.push(255);
        items.extend([0; 4]);
        items.extend(vec![0; width]);
        specimens.push((name, items));
    }
    for (name, body) in specimens {
        let bytes = nif(130, &[(name, body.clone())]);
        let n = parse(&bytes, 130, &Budget::new(2 << 20)).unwrap();
        assert!(
            n.blocks[0].diagnostics.is_empty(),
            "{name} {:?}",
            n.blocks[0].diagnostics
        );
        assert!(n.blocks[0].decoded.is_some());
        for cut in 0..body.len() {
            let bytes = nif(130, &[(name, body[..cut].to_vec())]);
            let b = Budget::new(2 << 20);
            let n = parse(&bytes, 130, &b).unwrap();
            assert!(!n.blocks[0].diagnostics.is_empty(), "{name} {cut}");
            assert!(b.used() <= 2 << 20);
        }
        assert!(parse(&bytes, 130, &Budget::new(0)).is_err());
    }
    let mut nan = Vec::new();
    u32s(&mut nan, &[1, 1, 0, 0x7fc01234]);
    let bytes = nif(130, &[("NiFloatData", nan)]);
    let parsed = parse(&bytes, 130, &Budget::new(2 << 20)).unwrap();
    let Some(Data::Fields(fields)) = &parsed.blocks[0].decoded else {
        panic!("key fields")
    };
    assert!(fields
        .iter()
        .any(|f| f.name == "Key Value" && matches!(f.value, Value::FloatBits(0x7fc01234))));
    for body in [
        vec![255; 4],
        [1u32.to_le_bytes(), 99u32.to_le_bytes()].concat(),
    ] {
        let bytes = nif(130, &[("NiFloatData", body)]);
        assert!(
            !parse(&bytes, 130, &Budget::new(2 << 20)).unwrap().blocks[0]
                .diagnostics
                .is_empty()
        );
    }
    let bytes = nif(130, &[("NiBlendFloatInterpolator", vec![0x80; 10])]);
    assert!(parse(&bytes, 130, &Budget::new(2 << 20)).unwrap().blocks[0]
        .diagnostics
        .iter()
        .any(|e| e.code == "UnsupportedBlendFlags"));
}

#[test]
fn stage18_bethesda_particle_envelopes_and_modifier_controllers_keep_runtime_data() {
    let mut particle = av();
    particle.extend([0; 16]);
    u32s(&mut particle, &[u32::MAX, u32::MAX, u32::MAX]);
    particle.extend(0u64.to_le_bytes());
    particle.extend([0; 8]);
    u32s(&mut particle, &[u32::MAX]);
    particle.push(1);
    u32s(&mut particle, &[1, u32::MAX]);
    let mut timing = Vec::new();
    u32s(&mut timing, &[u32::MAX]);
    timing.extend(0u16.to_le_bytes());
    timing.extend([0; 16]);
    u32s(&mut timing, &[u32::MAX, u32::MAX, 0]);
    let mut emitter = timing.clone();
    u32s(&mut emitter, &[u32::MAX]);
    for bs in [100, 130] {
        for (name, body) in [
            ("NiParticleSystem", &particle),
            ("NiPSysEmitterCtlr", &emitter),
            ("NiPSysModifierActiveCtlr", &timing),
        ] {
            let bytes = nif(bs, &[(name, body.clone())]);
            let n = parse(&bytes, bs, &Budget::new(2 << 20)).unwrap();
            assert!(
                n.blocks[0].diagnostics.is_empty(),
                "{name}: {:?}",
                n.blocks[0].diagnostics
            );
            assert!(matches!(n.blocks[0].decoded, Some(Data::Fields(_))));
            for cut in 0..body.len() {
                let bytes = nif(bs, &[(name, body[..cut].to_vec())]);
                let budget = Budget::new(2 << 20);
                let n = parse(&bytes, bs, &budget).unwrap();
                assert!(!n.blocks[0].diagnostics.is_empty());
                assert!(budget.used() <= 2 << 20);
            }
            assert!(parse(&bytes, bs, &Budget::new(0)).is_err());
        }
    }
    let mut bad = particle;
    let len = bad.len();
    bad[len - 8..len - 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let bytes = nif(130, &[("NiParticleSystem", bad)]);
    assert_eq!(
        parse(&bytes, 130, &Budget::new(2 << 20)).unwrap().blocks[0].diagnostics[0].code,
        "LimitExceeded"
    );
}
