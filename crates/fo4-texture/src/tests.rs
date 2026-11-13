use super::*;
fn dds(dx: Option<u32>, width: u32, height: u32, mips: u32, faces: u32) -> Vec<u8> {
    let mut h = [0u32; 31];
    h[0] = 124;
    h[1] = 0x21007;
    h[2] = height;
    h[3] = width;
    h[6] = mips;
    h[18] = 32;
    h[19] = 4;
    h[20] = u32::from_le_bytes(if dx.is_some() { *b"DX10" } else { *b"DXT1" });
    h[26] = 0x401008;
    if faces > 1 {
        h[27] = 0xfe00;
    }
    let mut out = b"DDS ".to_vec();
    for x in h {
        out.extend(x.to_le_bytes());
    }
    if let Some(format) = dx {
        for x in [format, 3, if faces == 6 { 4 } else { 0 }, 1, 0] {
            out.extend(x.to_le_bytes());
        }
    }
    for _ in 0..faces {
        for m in 0..mips {
            let w = (width >> m).max(1);
            let h = (height >> m).max(1);
            let size = w.div_ceil(4)
                * h.div_ceil(4)
                * if matches!(dx, None | Some(71 | 72 | 80 | 81)) {
                    8
                } else {
                    16
                };
            out.resize(out.len() + size as usize, 0);
        }
    }
    out
}
#[test]
fn all_bcn_formats_mips_cubes_and_hdr() {
    for f in [71, 72, 74, 75, 77, 78, 80, 81, 83, 84, 95, 96, 98, 99] {
        let bytes = dds(Some(f), 4, 4, 3, 6);
        let b = Budget::new(1 << 20);
        let t = Texture::read(&bytes, "synthetic.dds", &Limits::default(), &b).unwrap();
        assert_eq!(t.surfaces.len(), 18);
        assert!(t.cubemap);
        assert_eq!(t.srgb, matches!(f, 72 | 75 | 78 | 99));
        for i in 0..18 {
            assert_eq!(
                t.decode(i, &Limits::default(), &b.child()).unwrap().len(),
                t.surfaces[i].width as usize * t.surfaces[i].height as usize * 4
            );
        }
        assert!(t.verify_decode(&Limits::default(), &b).unwrap() > 0);
    }
    let mut bytes = dds(None, 4, 4, 1, 1);
    bytes[128..136].copy_from_slice(&[0, 0xf8, 0x1f, 0, 0, 0, 0, 0]);
    let t = Texture::read(&bytes, "red", &Limits::default(), &Budget::new(4096)).unwrap();
    assert_eq!(
        &t.decode(0, &Limits::default(), &Budget::new(4096)).unwrap()[..4],
        &[1., 0., 0., 1.]
    );
}
#[test]
fn every_truncation_header_field_mutation_and_budget() {
    let bytes = dds(Some(98), 4, 4, 3, 6);
    for end in 0..bytes.len() {
        assert!(Texture::read(
            &bytes[..end],
            "truncated",
            &Limits::default(),
            &Budget::new(8192)
        )
        .is_err());
    }
    for offset in [0, 4, 12, 16, 28, 76, 128, 132, 136, 140, 144] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(
            Texture::read(&bad, "mutated", &Limits::default(), &Budget::new(8192)).is_err(),
            "{offset}"
        );
    }
    for offset in 0..148 {
        let mut bad = bytes.clone();
        bad[offset] ^= 0x80;
        let budget = Budget::new(8192);
        if let Ok(t) = Texture::read(&bad, "bit flip", &Limits::default(), &budget) {
            let _ = t.verify_decode(&Limits::default(), &budget);
        }
        assert!(budget.used() <= 8192);
    }
    let mut tail = bytes.clone();
    tail.push(0);
    assert_eq!(
        Texture::read(&tail, "tail", &Limits::default(), &Budget::new(8192))
            .unwrap_err()
            .code,
        "TrailingDdsData"
    );
    for n in [0, 16, 128, 256] {
        let b = Budget::new(n);
        let _ = Texture::read(&bytes, "budget", &Limits::default(), &b);
        assert!(b.used() <= n);
    }
}
#[test]
fn half_values_are_ieee_binary16() {
    for (bits, value) in [
        (0, 0.),
        (0x8000, -0.),
        (1, 2f32.powi(-24)),
        (0x3c00, 1.),
        (0xbc00, -1.),
        (0x7bff, 65504.),
        (0x0400, 2f32.powi(-14)),
    ] {
        assert_eq!(half_to_f32(bits).to_bits(), value.to_bits());
    }
    assert!(half_to_f32(0x7c00).is_infinite());
    assert!(half_to_f32(0x7e00).is_nan());
}
#[test]
fn strip_decoder_has_small_memory_for_large_surfaces() {
    let bytes = dds(Some(98), 2048, 2048, 1, 1);
    let t = Texture::read(&bytes, "large", &Limits::default(), &Budget::new(8192)).unwrap();
    let b = Budget::new(1 << 18);
    assert_eq!(
        t.verify_decode(&Limits::default(), &b).unwrap(),
        2048 * 2048 * 16
    );
    assert!(b.used() <= 1 << 18);
}

#[test]
fn dx10_arrays_volumes_and_partial_legacy_cube_order() {
    let mut bytes = dds(Some(77), 4, 4, 2, 6);
    let image = bytes[148..].to_vec();
    bytes.extend(&image);
    bytes[140..144].copy_from_slice(&2u32.to_le_bytes());
    let t = Texture::read(&bytes, "array", &Limits::default(), &Budget::new(8192)).unwrap();
    assert_eq!(t.surfaces.len(), 24);
    assert_eq!(t.surfaces[12].array, 1);
    assert_eq!(t.surfaces[12].face, 0);
    assert_eq!(t.surfaces[12].mip, 0);
    let mut cube = dds(None, 4, 4, 1, 2);
    cube[112..116].copy_from_slice(&(0x200u32 | 0x400 | 0x4000).to_le_bytes());
    let t = Texture::read(
        &cube,
        "partial cube",
        &Limits::default(),
        &Budget::new(8192),
    )
    .unwrap();
    assert_eq!(
        t.surfaces.iter().map(|s| s.face).collect::<Vec<_>>(),
        [0, 4]
    );
    let mut volume = dds(Some(77), 4, 4, 3, 1);
    volume[24..28].copy_from_slice(&4u32.to_le_bytes());
    volume[132..136].copy_from_slice(&4u32.to_le_bytes());
    volume.truncate(148);
    volume.resize(148 + 16 * (4 + 2 + 1), 0);
    let t = Texture::read(&volume, "volume", &Limits::default(), &Budget::new(8192)).unwrap();
    assert_eq!(
        t.surfaces.iter().map(|s| s.depth).collect::<Vec<_>>(),
        [4, 2, 1]
    );
    assert!(t
        .verify_decode(&Limits::default(), &Budget::new(8192))
        .is_ok());
}
#[test]
fn uncompressed_byte_and_float_surfaces_preserve_values() {
    for (format, pixel, expected) in [
        (28, vec![255u8, 0, 128, 255], [1., 0., 128. / 255., 1.]),
        (87, vec![128, 0, 255, 255], [1., 0., 128. / 255., 1.]),
        (61, vec![128], [128. / 255., 0., 0., 1.]),
        (10, vec![0, 0x3c, 0, 0x40, 0, 0, 0, 0x3c], [1., 2., 0., 1.]),
    ] {
        let mut bytes = dds(Some(98), 1, 1, 1, 1);
        bytes[128..132].copy_from_slice(&(format as u32).to_le_bytes());
        bytes.truncate(148);
        bytes.extend(pixel);
        let t = Texture::read(&bytes, "plain", &Limits::default(), &Budget::new(8192)).unwrap();
        assert_eq!(
            t.decode(0, &Limits::default(), &Budget::new(8192)).unwrap(),
            expected
        );
        assert_eq!(
            t.verify_decode(&Limits::default(), &Budget::new(8192))
                .unwrap(),
            16
        );
    }
}

#[test]
fn documented_24_bit_bgr_masks_decode_in_byte_order() {
    let mut bytes = dds(None, 1, 1, 1, 1);
    bytes[80..84].copy_from_slice(&0x40u32.to_le_bytes());
    bytes[88..92].copy_from_slice(&24u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&0x00ff0000u32.to_le_bytes());
    bytes[96..100].copy_from_slice(&0x0000ff00u32.to_le_bytes());
    bytes[100..104].copy_from_slice(&0x000000ffu32.to_le_bytes());
    bytes.truncate(128);
    bytes.extend([0, 128, 255]);
    let t = Texture::read(
        &bytes,
        "synthetic-bgr.dds",
        &Limits::default(),
        &Budget::new(8192),
    )
    .unwrap();
    assert_eq!(
        t.decode(0, &Limits::default(), &Budget::new(8192)).unwrap(),
        [1., 128. / 255., 0., 1.]
    );
}
