//! A furniture's markers, where an actor sits on it, read from its base
//! record (`spec/timeline.md`, Deaths and furniture; census in
//! `spec/seen-census.txt`).
//!
//! Under `furniture_marker_snam_v1` a FURN record's SNAM holds one entry
//! per marker, 24 bytes each when its length is a multiple of 24, else 20
//! (measured over every FURN of the load order: 20, 24, 40, 48, 72, 96 and
//! 168 bytes), each three f32 (an offset in the furniture's own axes), one
//! f32 (a heading in radians), a FormID (a keyword or zero) and, in
//! 24-byte entries, a word kept as stored. The count of entries does not
//! always equal the count of MNAM bits (`spec/seen-census.txt`); MNAM is
//! not read. What the numbers mean (an offset and a heading) is a reading.
//! The models' own furniture marker blocks are counted by the census, not
//! read here.
use crate::hypotheses;
use crate::transform::Transform;
use fo4_core::{Budget, Result};
use fo4_plugin::{
    typed::{Refr, Store},
    FormKey, Record,
};

/// One marker of a furniture base, as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub offset: [f32; 3],
    pub heading: f32,
    pub keyword: Option<FormKey>,
    /// The fourth word of a 24-byte entry.
    pub word: Option<u32>,
}

/// The markers of a FURN record (`furniture_marker_snam_v1`), or why
/// they are not read.
pub fn markers(
    store: &Store,
    r: &Record,
    plugin: usize,
) -> Result<std::result::Result<Vec<Marker>, String>> {
    let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"SNAM") else {
        return Ok(Err("no SNAM".into()));
    };
    let d = r.data(s);
    let size = match d.len() {
        0 => return Ok(Err("an empty SNAM".into())),
        n if n % 24 == 0 => 24,
        n if n % 20 == 0 => 20,
        n => {
            return Ok(Err(format!(
                "SNAM of {n} bytes, a multiple of neither 24 nor 20"
            )))
        }
    };
    let word = |e: &[u8], at: usize| u32::from_le_bytes([e[at], e[at + 1], e[at + 2], e[at + 3]]);
    let mut out = Vec::new();
    for (k, e) in d.chunks_exact(size).enumerate() {
        let f = |at: usize| f32::from_bits(word(e, at));
        let offset = [f(0), f(4), f(8)];
        let heading = f(12);
        if offset.iter().chain([&heading]).any(|v| !v.is_finite()) {
            return Ok(Err(format!("marker {k}: a value that is not finite")));
        }
        let keyword = store.order().remap(
            plugin,
            word(e, 16),
            &r.source.file,
            s.offset + (k * size + 16) as u64,
        )?;
        out.push(Marker {
            offset,
            heading,
            keyword,
            word: (size == 24).then(|| word(e, 20)),
        });
    }
    Ok(Ok(out))
}

/// Where a sitter is placed: the marker's place in the world, its heading
/// (radians about the vertical axis) and which marker.
#[derive(Debug, Clone, PartialEq)]
pub struct Seat {
    pub pos: [f64; 3],
    pub heading: f32,
    /// The marker's index among the base's markers, and their count.
    pub index: usize,
    pub of: usize,
    /// The furniture's base record.
    pub base: FormKey,
}

/// The marker of placed furniture `furniture` nearest `near`
/// (`timeline_sit_marker_nearest_v1`, the markers read under
/// `furniture_marker_snam_v1`): its offset through the placement's
/// transform (rotation and scale as stored), its heading added to the
/// placement's rotation about the vertical axis; or why none.
pub fn seat_near(
    store: &Store,
    furniture: &FormKey,
    near: [f64; 3],
    b: &Budget,
) -> Result<std::result::Result<Seat, String>> {
    let _ = hypotheses::FURNITURE_MARKER_SNAM;
    let Some(refr) = store.get::<Refr>(furniture, b)? else {
        return Ok(Err("the furniture is no placed reference".into()));
    };
    let Some(t) = refr.data.transform else {
        return Ok(Err("the furniture has no stored placement".into()));
    };
    let Some(base) = refr.data.base.as_ref().and_then(|f| f.key.clone()) else {
        return Ok(Err("the furniture's base is not resolved".into()));
    };
    let Some((r, plugin)) = store.raw_with_plugin(&base, b)? else {
        return Ok(Err("the furniture's base has no winning record".into()));
    };
    if r.signature != *b"FURN" {
        return Ok(Err(format!(
            "the base is a {}, not a FURN",
            String::from_utf8_lossy(&r.signature)
        )));
    }
    let all = match markers(store, &r, plugin)? {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => return Ok(Err("the base stores no marker".into())),
        Err(why) => return Ok(Err(why)),
    };
    let world = match Transform::euler(t, refr.data.scale.unwrap_or(1.)) {
        Ok(w) => w,
        Err(e) => return Ok(Err(e.code.to_string())),
    };
    let d2 = |p: [f64; 3]| (0..3).map(|i| (p[i] - near[i]).powi(2)).sum::<f64>();
    let mut best: Option<(usize, [f64; 3])> = None;
    for (k, m) in all.iter().enumerate() {
        let p = world.apply(m.offset.map(f64::from));
        if best.is_none_or(|(_, q)| d2(p) < d2(q)) {
            best = Some((k, p));
        }
    }
    let Some((index, pos)) = best else {
        return Ok(Err("no marker".into()));
    };
    Ok(Ok(Seat {
        pos,
        heading: t[5] + all[index].heading,
        index,
        of: all.len(),
        base,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{fixture_with, group, record, sub};

    /// The fixture's reference placing a furniture (0x880) whose SNAM holds
    /// `entries` (offset, heading), 24 bytes each.
    fn seat_of(entries: &[([f32; 3], f32)]) -> std::result::Result<Seat, String> {
        let f = fixture_with(false, &[], |rf| {
            let mut rf = rf.to_vec();
            rf[6..10].copy_from_slice(&0x880u32.to_le_bytes());
            group(9, 0x800, &record(b"REFR", 0x802, 0, &rf))
        });
        let path = f.manifest.root.join("base.esm");
        let mut bytes = std::fs::read(&path).unwrap();
        let mut snam = Vec::new();
        for (o, h) in entries {
            for v in o.iter().chain([h]) {
                snam.extend(v.to_le_bytes());
            }
            snam.extend(0u32.to_le_bytes());
            snam.extend(u32::MAX.to_le_bytes());
        }
        let mut furn = sub(b"MODL", b"a.nif\0");
        if !entries.is_empty() {
            furn.extend(sub(b"SNAM", &snam));
        }
        bytes.extend(group(
            0,
            u32::from_le_bytes(*b"FURN"),
            &record(b"FURN", 0x880, 0, &furn),
        ));
        std::fs::write(&path, bytes).unwrap();
        let b = Budget::new(8 << 20);
        let store = Store::open(&f.manifest, &b).unwrap();
        let k = FormKey {
            plugin: "base.esm".into(),
            object_id: 0x802,
        };
        seat_near(&store, &k, [12.5, -2.0, 3.0], &b).unwrap()
    }

    #[test]
    fn a_seat_is_the_nearest_marker_through_the_placements_transform() {
        // The fixture's placement: (12.5, -2, 3), rotated (0.1, 0.2, 0.3),
        // scale 0.7.
        let world = Transform::euler([12.5, -2.0, 3.0, 0.1, 0.2, 0.3], 0.7).unwrap();
        let one = seat_of(&[([0.0, -63.0, 0.0], -std::f32::consts::PI)]).unwrap();
        let want = world.apply([0.0, -63.0, 0.0]);
        assert!(
            (0..3).all(|i| (one.pos[i] - want[i]).abs() < 1e-4),
            "{one:?}"
        );
        assert_eq!((one.index, one.of), (0, 1));
        assert!((one.heading - (0.3 - std::f32::consts::PI)).abs() < 1e-6);
        // Of two markers the one nearer the point asked about.
        let two = seat_of(&[([0.0, -200.0, 0.0], 0.0), ([5.0, 0.0, 0.0], 0.0)]).unwrap();
        assert_eq!((two.index, two.of), (1, 2));
        // No SNAM: no seat, and why.
        assert_eq!(seat_of(&[]).unwrap_err(), "no SNAM");
    }
}
