//! Objects of the `.hkx` animation and skeleton files: the top-level
//! container's named entries, the animation container's lists, the skeleton
//! (bone names, parents, reference pose), the binding (which bone each
//! transform track drives) and the stored header of the two compressed
//! animation classes, with the block layout of the spline-compressed one.
//!
//! Every rule enforced here held on every object of the winning `.hkx` files
//! of the measured installation; `spec/havok-animation.md` gives the counts.
//! Field names are this project's labels for stored values; class names are
//! the strings stored in `__classnames__`. That the values are bones, poses,
//! tracks and durations as the labels say is checked only as far as the spec
//! states (counts, references, unit quaternions, agreement with the skeleton
//! NIFs); the runtime use stays a hypothesis.
mod annotation;
mod spline;
pub use annotation::{annotation_tracks, Annotation, AnnotationTrack, ANNOTATION, MAX_ANNOTATIONS};
pub use spline::{
    de_boor, Component, Curve, DecodedSpline, Quantised, RotationTrack, Track, TrackPose,
    VectorTrack, ROTATION_40, ROTATION_48, VECTOR_16,
};

use crate::view::{norm, Array, View};
use crate::Container;
use fo4_core::{Budget, Result};
use serde::Serialize;

pub const ROOT_LEVEL: &str = "hkRootLevelContainer";
pub const ANIMATION_CONTAINER: &str = "hkaAnimationContainer";
pub const SKELETON: &str = "hkaSkeleton";
pub const BINDING: &str = "hkaAnimationBinding";
pub const SPLINE: &str = "hkaSplineCompressedAnimation";
pub const LOSSLESS: &str = "hkaLosslessCompressedAnimation";
/// Largest stored deviation of a reference-pose rotation from unit norm was
/// 2.9e-7 over all 11,896 bones.
pub const POSE_UNIT_TOLERANCE: f64 = 1e-5;
/// Stored kind word at +16 of each animation class.
pub const SPLINE_KIND: u32 = 3;
pub const LOSSLESS_KIND: u32 = 0;
/// Bytes of one element of the top-level container's array.
pub const ROOT_ENTRY: usize = 24;
/// Bytes of one skeleton bone and of one reference-pose entry.
pub const SKELETON_BONE: u32 = 16;
pub const SKELETON_POSE: u32 = 48;
/// Bytes of one annotation-track element of either animation class.
pub const ANNOTATION_TRACK: u32 = 24;

/// One element of the top-level object's array: a name, a class name and a
/// linked object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootEntry<'a> {
    pub name: &'a [u8],
    pub class_name: &'a [u8],
    /// Third-table index of the linked object.
    pub object: usize,
}

/// Reads the array at +0 of the `hkRootLevelContainer` object `index`.
pub fn root_entries<'a>(
    c: &Container<'a>,
    index: usize,
    file: &str,
    base: u64,
    b: &Budget,
) -> Result<Vec<RootEntry<'a>>> {
    let v = View::open(c, index, file, base, &[ROOT_LEVEL], "InvalidHavokRootLevel")?;
    let a = v.array(0, ROOT_ENTRY as u32)?;
    let mut out = Vec::new();
    b.reserve(&mut out, a.count as usize, file, v.abs(8))?;
    for i in 0..a.count as usize {
        let e = a.element_at(i).ok_or_else(|| v.err(8, "element", i))?;
        let entry = RootEntry {
            name: v.string(e, "entry name")?,
            class_name: v.string(e + 8, "entry class name")?,
            object: v.required_link(e + 16, "entry object")?,
        };
        let class = c.objects.get(entry.object).map(|o| o.class.as_bytes());
        if class != Some(entry.class_name) {
            return Err(v.err(
                e + 16,
                format!(
                    "an object of the named class {}",
                    String::from_utf8_lossy(entry.class_name)
                ),
                c.objects.get(entry.object).map_or("none", |o| o.class),
            ));
        }
        out.push(entry);
    }
    Ok(out)
}

/// The `hkaAnimationContainer` lists: arrays of links at +16, +32 and +48.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnimationContainer {
    pub object: usize,
    pub skeletons: Vec<usize>,
    pub animations: Vec<usize>,
    pub bindings: Vec<usize>,
    /// Element counts of the arrays at +64 and +80 (empty in every measured
    /// file; their element size is not established, so they are not read).
    pub unread_counts: [u32; 2],
}
impl AnimationContainer {
    pub fn read(
        c: &Container<'_>,
        index: usize,
        file: &str,
        base: u64,
        b: &Budget,
    ) -> Result<Self> {
        let v = View::open(
            c,
            index,
            file,
            base,
            &[ANIMATION_CONTAINER],
            "InvalidHavokAnimationContainer",
        )?;
        let mut lists: [Vec<usize>; 3] = Default::default();
        for (k, (field, classes)) in [
            (16usize, &[SKELETON][..]),
            (32, &[SPLINE, LOSSLESS][..]),
            (48, &[BINDING][..]),
        ]
        .into_iter()
        .enumerate()
        {
            let a = v.array(field, 8)?;
            b.reserve(&mut lists[k], a.count as usize, file, v.abs(field))?;
            for i in 0..a.count as usize {
                let e = a.element_at(i).ok_or_else(|| v.err(field, "element", i))?;
                let o = v.required_link(e, "list")?;
                let class = c.objects.get(o).map_or("none", |x| x.class);
                if !classes.contains(&class) {
                    return Err(v.err(e, classes.join(" or "), class));
                }
                lists[k].push(o);
            }
        }
        let [skeletons, animations, bindings] = lists;
        Ok(Self {
            object: index,
            skeletons,
            animations,
            bindings,
            unread_counts: [v.u32(72)?, v.u32(88)?],
        })
    }
}

/// One reference-pose entry, the 12 stored f32 as three groups of four.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BonePose {
    /// x, y, z; the fourth word varies and is not read.
    pub translation: [f32; 3],
    /// Four f32 of unit norm within [`POSE_UNIT_TOLERANCE`], in stored order
    /// (the vector part first, as measured against the skeleton NIFs).
    pub rotation: [f32; 4],
    /// Three f32, each within 2e-4 of 1 in the measured files; the fourth
    /// word is not read.
    pub scale: [f32; 3],
}

/// One `hkaSkeleton`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Skeleton<'a> {
    pub object: usize,
    /// Text at +16: in every measured skeleton, bone 0's name.
    pub name: &'a [u8],
    /// Parent of each bone: -1 for bone 0, below the bone's own index for
    /// the others (enforced).
    pub parents: Vec<i16>,
    pub bones: Vec<&'a [u8]>,
    /// Byte 8 of each 16-byte bone element (0 or 1 measured); bytes 9 to 15
    /// are zero in every measured bone and are not read.
    pub bone_flags: Vec<u8>,
    pub pose: Vec<BonePose>,
    /// Element counts of the arrays at +72 and +88 (one element in two
    /// skeletons, empty elsewhere; not read).
    pub unread_counts: [u32; 2],
}
impl<'a> Skeleton<'a> {
    pub fn read(
        c: &Container<'a>,
        index: usize,
        file: &str,
        base: u64,
        b: &Budget,
    ) -> Result<Self> {
        let v = View::open(c, index, file, base, &[SKELETON], "InvalidHavokSkeleton")?;
        let name = v.string(16, "skeleton name")?;
        let parents_a = v.array(24, 2)?;
        let bones_a = v.array(40, SKELETON_BONE)?;
        let pose_a = v.array(56, SKELETON_POSE)?;
        let n = bones_a.count as usize;
        for a in [&parents_a, &pose_a] {
            if a.count as usize != n {
                return Err(v.err(
                    a.field as usize + 8,
                    format!("{n} elements, as the bone array at +40"),
                    a.count,
                ));
            }
        }
        if n == 0 {
            return Err(v.err(48, "at least one bone", 0));
        }
        let mut s = Self {
            object: index,
            name,
            parents: Vec::new(),
            bones: Vec::new(),
            bone_flags: Vec::new(),
            pose: Vec::new(),
            unread_counts: [v.u32(80)?, v.u32(96)?],
        };
        b.reserve(&mut s.parents, n, file, v.abs(24))?;
        b.reserve(&mut s.bones, n, file, v.abs(40))?;
        b.reserve(&mut s.bone_flags, n, file, v.abs(40))?;
        b.reserve(&mut s.pose, n, file, v.abs(56))?;
        for i in 0..n {
            let at = parents_a
                .element_at(i)
                .ok_or_else(|| v.err(24, "element", i))?;
            let p = v.u16(at)? as i16;
            let ok = if i == 0 {
                p == -1
            } else {
                p >= 0 && (p as usize) < i
            };
            if !ok {
                let expected = if i == 0 {
                    "-1 for bone 0".to_string()
                } else {
                    format!("a parent in 0..{i}")
                };
                return Err(v.err(at, expected, p));
            }
            s.parents.push(p);
            let e = bones_a
                .element_at(i)
                .ok_or_else(|| v.err(40, "element", i))?;
            s.bones.push(v.string(e, "bone name")?);
            s.bone_flags.push(v.u8(e + 8)?);
            let e = pose_a
                .element_at(i)
                .ok_or_else(|| v.err(56, "element", i))?;
            let t = v.finite::<4>(e, "translation")?;
            let rotation = v.finite::<4>(e + 16, "rotation")?;
            let s3 = v.finite::<4>(e + 32, "scale")?;
            let r = norm(&rotation);
            if (r - 1.0).abs() > POSE_UNIT_TOLERANCE {
                return Err(v.err(e + 16, format!("unit norm within {POSE_UNIT_TOLERANCE}"), r));
            }
            s.pose.push(BonePose {
                translation: [t[0], t[1], t[2]],
                rotation,
                scale: [s3[0], s3[1], s3[2]],
            });
        }
        Ok(s)
    }
}

/// One `hkaAnimationBinding`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binding<'a> {
    pub object: usize,
    /// Text at +16: in every measured file, the name of a skeleton (its
    /// bone 0).
    pub skeleton_name: &'a [u8],
    /// The animation object linked from +24.
    pub animation: usize,
    /// Array at +32: for transform track i, the skeleton bone it drives.
    pub transform_tracks: Vec<i16>,
    /// Array at +48: for float track i, a slot (4 bindings have one).
    pub float_tracks: Vec<i16>,
    /// Element count of the array at +64 (empty in every measured file).
    pub unread_count: u32,
    /// Byte at +80 (0, 1 or 2 measured); bytes 81 to 95 are zero.
    pub byte80: u8,
}
impl<'a> Binding<'a> {
    pub fn read(
        c: &Container<'a>,
        index: usize,
        file: &str,
        base: u64,
        b: &Budget,
    ) -> Result<Self> {
        let v = View::open(c, index, file, base, &[BINDING], "InvalidHavokBinding")?;
        let skeleton_name = v.string(16, "skeleton name")?;
        let animation = v.required_link(24, "animation")?;
        let class = c.objects.get(animation).map_or("none", |x| x.class);
        if class != SPLINE && class != LOSSLESS {
            return Err(v.err(24, format!("{SPLINE} or {LOSSLESS}"), class));
        }
        let mut tracks: [Vec<i16>; 2] = Default::default();
        for (k, field) in [32usize, 48].into_iter().enumerate() {
            let a = v.array(field, 2)?;
            b.reserve(&mut tracks[k], a.count as usize, file, v.abs(field))?;
            for i in 0..a.count as usize {
                let e = a.element_at(i).ok_or_else(|| v.err(field, "element", i))?;
                tracks[k].push(v.u16(e)? as i16);
            }
        }
        let [transform_tracks, float_tracks] = tracks;
        Ok(Self {
            object: index,
            skeleton_name,
            animation,
            transform_tracks,
            float_tracks,
            unread_count: v.u32(72)?,
            byte80: v.u8(80)?,
        })
    }
    /// The binding fits `s` when its name is the skeleton's and every track
    /// names a bone of it, each bone at most once.
    pub fn fits(&self, s: &Skeleton<'_>) -> bool {
        let mut seen = vec![false; s.bones.len()];
        self.skeleton_name == s.name
            && self.transform_tracks.iter().all(|t| {
                usize::try_from(*t)
                    .ok()
                    .and_then(|i| seen.get_mut(i))
                    .is_some_and(|x| !std::mem::replace(x, true))
            })
    }
}

/// The words at +16 to +55 shared by both animation classes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AnimationHeader {
    pub object: usize,
    /// u32 at +16: [`SPLINE_KIND`] or [`LOSSLESS_KIND`], one per class.
    pub kind: u32,
    /// f32 at +20, finite and not negative.
    pub duration: f32,
    /// u32 at +24: equals the binding's transform-track count.
    pub transform_tracks: u32,
    /// u32 at +28: equals the binding's float-track count.
    pub float_tracks: u32,
    /// Object linked from +32, an `hkaDefaultAnimatedReferenceFrame`, if any.
    pub reference_frame: Option<usize>,
    /// Array at +40 of 24-byte elements, read by [`annotation_tracks`].
    pub annotation_tracks: Array,
}
impl AnimationHeader {
    pub fn read(c: &Container<'_>, index: usize, file: &str, base: u64) -> Result<Self> {
        let v = View::open(
            c,
            index,
            file,
            base,
            &[SPLINE, LOSSLESS],
            "InvalidHavokAnimation",
        )?;
        Self::from_view(&v, index)
    }
    fn from_view(v: &View<'_, '_>, index: usize) -> Result<Self> {
        let kind = v.u32(16)?;
        let expected = if v.span.class == SPLINE {
            SPLINE_KIND
        } else {
            LOSSLESS_KIND
        };
        if kind != expected {
            return Err(v.err(16, expected, kind));
        }
        let [duration] = v.finite::<1>(20, "duration")?;
        if duration < 0.0 {
            return Err(v.err(20, "duration >= 0", duration));
        }
        let reference_frame = v.link(32);
        if let Some(r) = reference_frame {
            let class = v.c.objects.get(r).map_or("none", |x| x.class);
            if class != "hkaDefaultAnimatedReferenceFrame" {
                return Err(v.err(32, "hkaDefaultAnimatedReferenceFrame", class));
            }
        }
        Ok(Self {
            object: index,
            kind,
            duration,
            transform_tracks: v.u32(24)?,
            float_tracks: v.u32(28)?,
            reference_frame,
            annotation_tracks: v.array(40, ANNOTATION_TRACK)?,
        })
    }
}

/// One `hkaSplineCompressedAnimation`: its header, block table and data
/// bytes. The bytes of each block are not decoded here.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SplineAnimation<'a> {
    pub header: AnimationHeader,
    /// u32 at +56: `duration / frame_duration` rounded, plus 1.
    pub frames: u32,
    /// u32 at +64 (256 in every measured file).
    pub frames_per_block: u32,
    /// u32 at +68: 4 bytes for each transform and float track.
    pub mask_bytes: u32,
    /// f32 at +72: `(frames_per_block - 1) * frame_duration`.
    pub block_duration: f32,
    /// f32 at +76: `1 / block_duration`.
    pub block_inverse_duration: f32,
    /// f32 at +80.
    pub frame_duration: f32,
    /// Array at +88: offset of each block in `data`, from 0, increasing,
    /// multiples of 16. The block count is the u32 at +60.
    pub block_offsets: Vec<u32>,
    /// Array at +104, one u32 per block; meaning not established.
    pub float_offsets: Vec<u32>,
    /// Storage of the byte array at +152.
    #[serde(skip)]
    pub data: &'a [u8],
    /// Absolute file offset of `data`'s first byte (`base` included).
    pub data_at: u64,
}
impl<'a> SplineAnimation<'a> {
    pub fn read(
        c: &Container<'a>,
        index: usize,
        file: &str,
        base: u64,
        b: &Budget,
    ) -> Result<Self> {
        let v = View::open(c, index, file, base, &[SPLINE], "InvalidHavokAnimation")?;
        let header = AnimationHeader::from_view(&v, index)?;
        let frames = v.u32(56)?;
        let blocks = v.u32(60)?;
        let frames_per_block = v.u32(64)?;
        let mask_bytes = v.u32(68)?;
        let [block_duration, block_inverse_duration, frame_duration] =
            v.finite::<3>(72, "block and frame durations")?;
        if frames == 0 || frames_per_block < 2 {
            return Err(v.err(
                56,
                "at least 1 frame and 2 frames per block",
                format!("{frames}, {frames_per_block}"),
            ));
        }
        let expected = (frames - 1).div_ceil(frames_per_block - 1).max(1);
        if blocks != expected {
            return Err(v.err(
                60,
                format!("{expected} blocks for {frames} frames of {frames_per_block}"),
                blocks,
            ));
        }
        let tracks = u64::from(header.transform_tracks) + u64::from(header.float_tracks);
        if u64::from(mask_bytes) != 4 * tracks {
            return Err(v.err(68, format!("4 x {tracks} tracks"), mask_bytes));
        }
        let mut lists: [Vec<u32>; 2] = Default::default();
        for (k, field) in [88usize, 104].into_iter().enumerate() {
            let a = v.array(field, 4)?;
            if a.count != blocks {
                return Err(v.err(field + 8, format!("{blocks} blocks"), a.count));
            }
            b.reserve(&mut lists[k], blocks as usize, file, v.abs(field))?;
            for i in 0..blocks as usize {
                let e = a.element_at(i).ok_or_else(|| v.err(field, "element", i))?;
                lists[k].push(v.u32(e)?);
            }
        }
        for field in [120usize, 136] {
            let n = v.u32(field + 8)?;
            if n != 0 {
                return Err(v.err(field + 8, "an empty array", n));
            }
        }
        let data_a = v.array(152, 1)?;
        let start = data_a.storage as usize;
        let data = v
            .bytes
            .get(start..start + data_a.count as usize)
            .unwrap_or(&[]);
        let [offsets, float_offsets] = lists;
        for (i, o) in offsets.iter().enumerate() {
            let ok = if i == 0 {
                *o == 0
            } else {
                *o > offsets[i - 1] && o.is_multiple_of(16)
            } && (*o as usize) < data.len();
            if !ok {
                return Err(v.err(
                    88,
                    format!(
                        "block {i} offset: 0 first, then increasing multiples of 16 below {}",
                        data.len()
                    ),
                    o,
                ));
            }
        }
        Ok(Self {
            header,
            frames,
            frames_per_block,
            mask_bytes,
            block_duration,
            block_inverse_duration,
            frame_duration,
            block_offsets: offsets,
            float_offsets,
            data,
            data_at: v.abs(start),
        })
    }
    /// Bytes of block `i`: from its offset to the next block's, or to the
    /// end of the data.
    pub fn block(&self, i: usize) -> Option<&'a [u8]> {
        let lo = *self.block_offsets.get(i)? as usize;
        let hi = self
            .block_offsets
            .get(i + 1)
            .map_or(self.data.len(), |x| *x as usize);
        self.data.get(lo..hi)
    }
}

#[cfg(test)]
mod tests;
