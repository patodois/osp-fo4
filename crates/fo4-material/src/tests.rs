use super::*;
#[path = "test_builder.rs"]
mod builder;
use builder::fixture;
fn read(bytes: &[u8]) -> Result<Material<'_>> {
    Material::read(
        bytes,
        "generated",
        &Limits::default(),
        &Budget::new(1 << 20),
    )
}
#[test]
fn stage10_documented_v2_bgem_is_interpreted() {
    let w = fixture(b"BGEM", 2, false, b"");
    assert_eq!(w.v.len(), 135);
    let m = read(&w.v).unwrap();
    assert_eq!(m.version, 2);
    assert_eq!(m.common.alpha.to_bits(), 1.25f32.to_bits());
    assert_eq!(m.common.alpha_blend.mode, Some(BlendMode::Standard));
    let Kind::Bgem(e) = m.kind else { panic!() };
    assert_eq!(e.base_color, [1.25; 3]);
    assert_eq!(e.soft_depth, 1.25);
    assert!(e.glass.is_none());
    assert!(e.glow_texture.is_none());
}
#[test]
fn all_reference_version_gates_and_conditional_fields() {
    for v in 0..=MAX_VERSION {
        for kind in [b"BGSM", b"BGEM"] {
            for on in [false, true] {
                let w = fixture(kind, v, on, b"textures/a.dds");
                let m = read(&w.v).unwrap_or_else(|e| panic!("{kind:?}/{v}/{on}: {e}"));
                assert_eq!(m.common.depth_bias.is_some(), v >= 10);
                assert_eq!(m.common.environment_mapping.is_some(), v < 10);
                assert_eq!(m.common.mask_writes.is_some(), v >= 6);
                assert_eq!(m.non_finite_values(), 0);
                assert!(m
                    .texture_paths()
                    .all(|(_, p)| p.bytes() == b"textures/a.dds"));
                match &m.kind {
                    Kind::Bgsm(s) => {
                        assert_eq!(s.pbr.is_some(), v > 2);
                        assert_eq!(s.rim_power.is_some(), v < 8);
                        assert_eq!(s.translucency.is_some(), v >= 8);
                        assert_eq!(s.custom_porosity.is_some(), v >= 9);
                        assert_eq!(s.lum_emittance.is_some(), v >= 12);
                        assert_eq!(s.use_adaptative_emissive.is_some(), v >= 13);
                        assert_eq!(s.distance_field_alpha_texture.is_some(), v >= 17);
                        assert_eq!(s.emittance_color.is_some(), on);
                        assert_eq!(s.terrain_data.is_some(), v >= 3 && on);
                        assert_eq!(s.skew_specular_alpha.is_some(), v >= 1);
                        assert_eq!(
                            s.terrain_data.as_ref().and_then(|t| t.unknown_v3).is_some(),
                            v == 3 && on
                        );
                    }
                    Kind::Bgem(e) => {
                        assert_eq!(e.specular_texture.is_some(), v >= 11);
                        assert_eq!(e.emittance_color.is_some(), v >= 11);
                        assert_eq!(e.glass.is_some(), v >= 21 && on);
                        assert_eq!(e.adaptative_emissive_exposure_offset.is_some(), v >= 15);
                        assert_eq!(e.glowmap.is_some(), v >= 16);
                        assert_eq!(e.effect_pbr_specular.is_some(), v >= 20);
                        if let Some(g) = &e.glass {
                            assert_eq!(g.middle_v22.is_some(), v >= 22);
                            assert_eq!(g.raw.len(), if v >= 22 { 24 } else { 20 });
                        }
                    }
                }
            }
        }
    }
    let w = fixture(b"BGSM", 2, false, b"");
    assert_eq!(w.v.len(), 236);
    read(&w.v).unwrap();
}
#[test]
fn all_truncations_fields_mutations_and_bounded_strings() {
    for kind in [b"BGSM", b"BGEM"] {
        for v in 0..=MAX_VERSION {
            for on in [false, true] {
                let w = fixture(kind, v, on, b"textures/a.dds");
                for n in 0..w.v.len() {
                    assert!(read(&w.v[..n]).is_err(), "{kind:?}/{v}/{on}/{n}");
                }
                for (start, end) in w.ranges {
                    for at in start..end {
                        let mut bad = w.v.clone();
                        bad[at] ^= 0xff;
                        let budget = Budget::new(64);
                        let _ = Material::read(&bad, "corrupt", &Limits::default(), &budget);
                        assert_eq!(budget.used(), 0);
                    }
                }
            }
        }
    }
    let mut w = fixture(b"BGEM", 2, false, b"").v;
    w[63..67].copy_from_slice(&u32::MAX.to_le_bytes());
    let e = read(&w).unwrap_err();
    assert_eq!(e.code, "LimitExceeded");
    assert_eq!(e.source.offset, 63);
    let mut w = fixture(b"BGSM", 2, false, b"").v;
    w.push(0);
    assert_eq!(read(&w).unwrap_err().code, "TrailingMaterialData");
    for v in [MAX_VERSION + 1, u32::MAX] {
        w[4..8].copy_from_slice(&v.to_le_bytes());
        assert_eq!(read(&w).unwrap_err().code, "UnsupportedMaterialVersion");
    }
}
#[test]
fn raw_paths_cp1252_unknown_flags_and_nonfinite_colors() {
    let mut w = fixture(b"BGSM", 2, true, b"textures/caf\xe9.dds").v;
    w[8..12].copy_from_slice(&0x80000003u32.to_le_bytes());
    w[28..32].copy_from_slice(&0x7fc12345u32.to_le_bytes());
    let m = read(&w).unwrap();
    assert_eq!(m.raw, w);
    assert_eq!(m.non_finite_values(), 1);
    assert_eq!(m.common.alpha.to_bits(), 0x7fc12345);
    let (_, p) = m.texture_paths().next().unwrap();
    assert_eq!(p.bytes(), b"textures/caf\xe9.dds");
    assert_eq!(
        p.display_cp1252("generated", &Budget::new(100)).unwrap(),
        "textures/caf\u{e9}.dds"
    );
    assert_eq!(m.uninterpreted_fields(), 2);
    let short = Budget::new(1);
    assert!(p.display_cp1252("generated", &short).is_err());
}

#[test]
fn golden_offsets_preserve_color_floats_and_unknown_blend() {
    let mut w = fixture(b"BGSM", 2, false, b"").v;
    for (o, v) in [
        (124usize, 0.125f32),
        (128, 1.5),
        (132, -2.),
        (136, 2.5),
        (140, 3.5),
        (144, 4.5),
        (148, 5.5),
        (160, 6.5),
        (168, 7.5),
    ] {
        w[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    w[32] = 3;
    let m = read(&w).unwrap();
    assert!(m.common.alpha_blend.mode.is_none());
    assert_eq!(m.uninterpreted_fields(), 1);
    let Kind::Bgsm(v) = m.kind else { panic!() };
    assert_eq!(v.specular_color, [0.125, 1.5, -2.]);
    assert_eq!(v.specular_mult, 2.5);
    assert_eq!(v.smoothness, 3.5);
    assert_eq!(v.fresnel_power, 4.5);
    assert_eq!(v.wetness_control_spec_scale, 5.5);
    assert_eq!(v.wetness_control_env_map_scale, Some(6.5));
    assert_eq!(v.wetness_control_metalness, 7.5);
    let mut w = fixture(b"BGEM", 2, false, b"").v;
    w[94..98].copy_from_slice(&0xffc12345u32.to_le_bytes());
    let m = read(&w).unwrap();
    assert_eq!(m.non_finite_values(), 1);
    let Kind::Bgem(e) = m.kind else { panic!() };
    assert_eq!(e.base_color[0].to_bits(), 0xffc12345);
}

#[test]
fn disabled_blend_tuple_is_retained_without_requiring_factors() {
    for kind in [b"BGSM", b"BGEM"] {
        let mut bytes = fixture(kind, 2, false, b"").v;
        bytes[32] = 0; // MaterialLib maps (0,6,7) explicitly to Unknown.
        let material = read(&bytes).unwrap();
        assert_eq!(material.common.alpha_blend.mode, Some(BlendMode::Unknown));
        assert_eq!(
            (
                material.common.alpha_blend.enabled,
                material.common.alpha_blend.source,
                material.common.alpha_blend.destination
            ),
            (0, 6, 7)
        );
        assert_eq!(material.uninterpreted_fields(), 0);
    }
}
#[test]
fn stage14_mode_label_never_controls_alpha_readiness() {
    for enabled in [0, 1, 2, 255] {
        for (source, destination) in [(0, 0), (6, 7), (u32::MAX, 17)] {
            for mode in [None, Some(BlendMode::Unknown), Some(BlendMode::Standard)] {
                let blend = AlphaBlend {
                    enabled,
                    source,
                    destination,
                    mode,
                };
                assert_eq!(blend.requires_factor_semantics(), enabled != 0);
            }
        }
    }
}

#[test]
fn stage13_unknown_components_sum_without_collapsing_their_names() {
    let l = fo4_core::Limits::default();
    for (magic, version) in [(b"BGSM", 3), (b"BGSM", 22), (b"BGEM", 22)] {
        let w = fixture(magic, version, true, b"");
        let b = fo4_core::Budget::new(8 << 20);
        let mut material = Material::read(&w.v, "synthetic-components", &l, &b).unwrap();
        material.common.tile_flags |= 4;
        material.common.mask_writes = Some(128);
        material.common.alpha_blend.mode = None;
        let components = material
            .uninterpreted_components()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(components["tile_flags.extra_bits"], 1);
        assert_eq!(components["mask_writes.extra_bits"], 1);
        assert_eq!(components["alpha_blend.enabled_factors_unsourced"], 1);
        assert_eq!(
            material.uninterpreted_fields(),
            components.values().sum::<u64>()
        );
        if magic == b"BGSM" && version == 3 {
            assert_eq!(components["terrain.unknown_v3"], 1);
        }
        if magic == b"BGEM" {
            assert_eq!(components["bgem.glass_scalars"], 3);
        }
    }
}
