//! The annotation tracks of the two animation classes (`spec/havok-animation.md`,
//! Annotation tracks): each 24-byte element of the header's array at +40 is
//! a track name (text pointer at +0) and an array at +8 of 16-byte
//! annotations, each an f32 time at +0, a zero word at +4 and a text pointer
//! at +8. Measured on every clip of one installation (`anim_event_census`);
//! what the game does with an annotation is not read here.
use super::{AnimationHeader, LOSSLESS, SPLINE};
use crate::view::View;
use crate::Container;
use fo4_core::{Budget, Result};
use serde::Serialize;

/// Bytes of one annotation.
pub const ANNOTATION: u32 = 16;
/// The most annotations one track is read with.
pub const MAX_ANNOTATIONS: u32 = 65_536;

/// One annotation: a time in clip seconds and a text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Annotation<'a> {
    pub time: f32,
    pub text: &'a [u8],
}

/// One annotation track: its name and its annotations.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnnotationTrack<'a> {
    pub name: &'a [u8],
    pub annotations: Vec<Annotation<'a>>,
}

/// Reads every annotation track of the animation object `index`. A time
/// that is not finite or lies outside [0, duration], or a non-zero word at
/// +4, is a named error.
pub fn annotation_tracks<'a>(
    c: &Container<'a>,
    index: usize,
    file: &str,
    base: u64,
    b: &Budget,
) -> Result<Vec<AnnotationTrack<'a>>> {
    let h = AnimationHeader::read(c, index, file, base)?;
    let v = View::open(
        c,
        index,
        file,
        base,
        &[SPLINE, LOSSLESS],
        "InvalidHavokAnnotation",
    )?;
    let tracks = h.annotation_tracks;
    let mut out = Vec::new();
    b.reserve(&mut out, tracks.count as usize, file, v.abs(40))?;
    for i in 0..tracks.count as usize {
        let e = tracks
            .element_at(i)
            .ok_or_else(|| v.err(40, "element", i))?;
        let name = v.string(e, "annotation track name")?;
        let a = v.array(e + 8, ANNOTATION)?;
        if a.count > MAX_ANNOTATIONS {
            return Err(v.err(
                e + 16,
                format!("at most {MAX_ANNOTATIONS} annotations"),
                a.count,
            ));
        }
        let mut annotations = Vec::new();
        b.reserve(&mut annotations, a.count as usize, file, v.abs(e + 8))?;
        for j in 0..a.count as usize {
            let x = a.element_at(j).ok_or_else(|| v.err(e + 8, "element", j))?;
            let [time] = v.finite::<1>(x, "annotation time")?;
            if !(0.0..=h.duration).contains(&time) {
                return Err(v.err(x, format!("a time in [0, {}]", h.duration), time));
            }
            let w = v.u32(x + 4)?;
            if w != 0 {
                return Err(v.err(x + 4, 0, w));
            }
            annotations.push(Annotation {
                time,
                text: v.string(x + 8, "annotation text")?,
            });
        }
        out.push(AnnotationTrack { name, annotations });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::tests::{lay, AT, NAMES};
    use crate::tests::{build, name_offset, read, Spec};

    /// The synthetic clip with two annotation tracks: the first holds one
    /// annotation, the second none. Texts lie after the clip's data.
    fn file(time: f32, word: u32) -> Vec<u8> {
        let mut l = lay();
        let end = 976;
        l.bytes.resize(end + 128, 0);
        let a = AT[4];
        l.text(a + 176, end, "T0");
        l.array(a + 176 + 8, 1, end + 16);
        l.put(end + 16, time.to_bits());
        l.put(end + 20, word);
        l.text(end + 24, end + 32, "Hit");
        l.text(a + 200, end + 48, "T1");
        l.array(a + 200 + 8, 0, 0);
        let names: Vec<(&'static str, u32)> = NAMES.iter().map(|n| (*n, 7)).collect();
        let root = name_offset(&names, 1);
        build(&Spec {
            names,
            object_bytes: end + 128,
            content: vec![(0, l.bytes.clone())],
            pointers: l.pointers.clone(),
            links: l.links.clone(),
            objects: l.objects.clone(),
            root: [2, 0, 0, root],
        })
    }

    #[test]
    fn reads_tracks_and_annotations() {
        let bytes = file(0.5, 0);
        let c = read(&bytes).unwrap();
        let t = annotation_tracks(&c, 4, "synthetic.hkx", 1000, &Budget::new(1 << 20)).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].name, b"T0");
        assert_eq!(
            t[0].annotations,
            [Annotation {
                time: 0.5,
                text: b"Hit"
            }]
        );
        assert!(t[1].annotations.is_empty());
    }

    #[test]
    fn refuses_a_time_past_the_duration_and_a_stored_word() {
        let b = Budget::new(1 << 20);
        for (time, word) in [(2.0, 0), (0.5, 7)] {
            let bytes = file(time, word);
            let c = read(&bytes).unwrap();
            let e = annotation_tracks(&c, 4, "synthetic.hkx", 1000, &b).unwrap_err();
            assert_eq!(e.code, "InvalidHavokAnnotation");
        }
    }
}
