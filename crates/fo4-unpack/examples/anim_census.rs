#![forbid(unsafe_code)]
//! Census of the winning `.hkx` files of the manifest's mounts with
//! `fo4_havok`: container outcome by header variant, the animation objects'
//! readers (`spec/havok-animation.md`), the relations between their stored
//! values, bindings against skeletons, and each `skeleton.hkx` against the
//! `skeleton.nif` beside it. Measurement support; writes plain text, counts
//! and small stored values only.
use fo4_core::{Budget, Manifest};
use fo4_havok as hk;
use fo4_nif::Nif;
use fo4_scene::actor::pose::{self, Affine};
use fo4_vfs::Vfs;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

fn bump<K: Ord>(m: &mut BTreeMap<K, u64>, k: K) {
    *m.entry(k).or_default() += 1;
}
fn check(m: &mut BTreeMap<String, [u64; 2]>, k: &str, ok: bool) {
    m.entry(k.to_string()).or_default()[usize::from(!ok)] += 1;
}

/// Rotation rows (acting on column vectors) of a quaternion stored as
/// (x, y, z, w), or their transpose.
pub fn quat_rows(q: [f32; 4], transpose: bool, w_first: bool) -> [[f64; 3]; 3] {
    let q = q.map(f64::from);
    let (x, y, z, w) = if w_first {
        (q[1], q[2], q[3], q[0])
    } else {
        (q[0], q[1], q[2], q[3])
    };
    let r = [
        [
            1. - 2. * (y * y + z * z),
            2. * (x * y - w * z),
            2. * (x * z + w * y),
        ],
        [
            2. * (x * y + w * z),
            1. - 2. * (x * x + z * z),
            2. * (y * z - w * x),
        ],
        [
            2. * (x * z - w * y),
            2. * (y * z + w * x),
            1. - 2. * (x * x + y * y),
        ],
    ];
    if transpose {
        std::array::from_fn(|i| std::array::from_fn(|j| r[j][i]))
    } else {
        r
    }
}

/// World transforms of a Havok skeleton's bones from its reference pose,
/// under one quaternion reading; the scale words are taken as uniform (their
/// first word), as the NIF nodes store one scale.
fn hk_worlds(s: &hk::Skeleton<'_>, transpose: bool, w_first: bool) -> Vec<Affine> {
    let mut out: Vec<Affine> = Vec::with_capacity(s.bones.len());
    for (i, p) in s.pose.iter().enumerate() {
        let local = Affine {
            rows: quat_rows(p.rotation, transpose, w_first),
            translation: p.translation.map(f64::from),
            scale: f64::from(p.scale[0]),
        };
        let w = match usize::try_from(s.parents[i]).ok().and_then(|k| out.get(k)) {
            Some(parent) => parent.compose(&local),
            None => local,
        };
        out.push(w);
    }
    out
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}
fn rot_err(a: &Affine, b: &Affine) -> f64 {
    let mut m: f64 = 0.;
    for i in 0..3 {
        for j in 0..3 {
            m = m.max((a.scale * a.rows[i][j] - b.scale * b.rows[i][j]).abs());
        }
    }
    m
}
fn bucket(v: f64) -> &'static str {
    if v < 1e-3 {
        "<0.001"
    } else if v < 1e-2 {
        "<0.01"
    } else if v < 0.1 {
        "<0.1"
    } else if v < 1.0 {
        "<1"
    } else {
        ">=1"
    }
}

struct SkelInfo {
    path: String,
    name: Vec<u8>,
    bones: usize,
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("anim_census MANIFEST OUT.txt".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut paths: Vec<Vec<u8>> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.clone())
        .filter(|k| k.ends_with(b".hkx"))
        .collect();
    paths.sort();
    paths.dedup();
    let mut c = BTreeMap::<String, u64>::new();
    let mut checks = BTreeMap::<String, [u64; 2]>::new();
    let mut values = BTreeMap::<String, u64>::new();
    let mut objects = BTreeMap::<String, u64>::new();
    let mut files = BTreeMap::<String, u64>::new();
    let mut bytes_total = 0u64;
    let mut skeletons: Vec<SkelInfo> = Vec::new();
    // Per binding: (file, skeleton name, largest track index + 1, tracks distinct).
    let mut bindings: Vec<(String, Vec<u8>, usize, bool)> = Vec::new();
    let mut nif_cmp = BTreeMap::<String, u64>::new();
    for p in &paths {
        let name = String::from_utf8_lossy(p).into_owned();
        let b = Budget::new(m.limits.max_memory);
        let bytes = match vfs.read_path_bytes(p, &b) {
            Ok(v) => v,
            Err(e) => {
                bump(&mut c, format!("file read error {}", e.code));
                continue;
            }
        };
        bytes_total += bytes.len() as u64;
        let extra = bytes.get(60..64).map_or(u32::MAX, |v| {
            u32::from_le_bytes([v[0], v[1], v[2], v[3]]) >> 16
        });
        let k = match hk::Container::read(&bytes, &name, 0, &b) {
            Ok(k) => k,
            Err(e) => {
                bump(
                    &mut c,
                    format!(
                        "container error, word 60 upper half {extra}: {} at {}: expected {}",
                        e.code, e.source.offset, e.expected
                    ),
                );
                continue;
            }
        };
        bump(
            &mut c,
            format!("container read, word 60 upper half {extra}"),
        );
        let mut seen = BTreeSet::new();
        for o in &k.objects {
            bump(&mut objects, o.class.to_string());
            seen.insert(o.class);
        }
        for s in &seen {
            bump(&mut files, s.to_string());
        }
        let mut outcome = |what: &str, r: Result<(), fo4_core::Error>| match r {
            Ok(()) => bump(&mut c, format!("{what}: read")),
            Err(e) => {
                bump(
                    &mut c,
                    format!("{what}: {} expected {}", e.code, e.expected),
                );
                if c.len() < 400 {
                    eprintln!("{name}: {what}: {e}");
                }
            }
        };
        let mut file_skeletons = Vec::new();
        for (i, o) in k.objects.iter().enumerate() {
            match o.class {
                hk::ROOT_LEVEL => outcome(
                    "root entries",
                    hk::root_entries(&k, i, &name, 0, &b).map(|e| {
                        for x in &e {
                            bump(
                                &mut values,
                                format!(
                                    "root entry name|class: {}|{}",
                                    String::from_utf8_lossy(x.name),
                                    String::from_utf8_lossy(x.class_name)
                                ),
                            );
                        }
                    }),
                ),
                hk::ANIMATION_CONTAINER => outcome(
                    "animation container",
                    hk::AnimationContainer::read(&k, i, &name, 0, &b).map(|a| {
                        bump(
                            &mut values,
                            format!(
                                "animation container lists skeletons|animations|bindings|unread: {}|{}|{}|{:?}",
                                a.skeletons.len(),
                                a.animations.len(),
                                a.bindings.len(),
                                a.unread_counts
                            ),
                        );
                    }),
                ),
                hk::SKELETON => outcome(
                    "skeleton",
                    hk::Skeleton::read(&k, i, &name, 0, &b).map(|s| {
                        check(&mut checks, "skeleton name is bone 0 name", s.bones.first() == Some(&s.name));
                        let flags: BTreeSet<u8> = s.bone_flags.iter().copied().collect();
                        bump(&mut values, format!("skeleton bone flags {flags:?}"));
                        bump(&mut values, format!("skeleton unread counts {:?}", s.unread_counts));
                        let dmax = s
                            .pose
                            .iter()
                            .flat_map(|p| p.scale)
                            .map(|v| (f64::from(v) - 1.).abs())
                            .fold(0., f64::max);
                        bump(&mut values, format!("skeleton largest |scale - 1| {}", bucket(dmax)));
                        let mut names = BTreeSet::new();
                        check(&mut checks, "skeleton bone names distinct", s.bones.iter().all(|n| names.insert(*n)));
                        file_skeletons.push(SkelInfo {
                            path: name.clone(),
                            name: s.name.to_vec(),
                            bones: s.bones.len(),
                        });
                    }),
                ),
                hk::BINDING => outcome(
                    "binding",
                    hk::Binding::read(&k, i, &name, 0, &b).map(|x| {
                        bump(&mut values, format!("binding byte 80 {}", x.byte80));
                        bump(&mut values, format!("binding unread count {}", x.unread_count));
                        bump(&mut values, format!("binding float tracks {}", x.float_tracks.len()));
                        let mut set = BTreeSet::new();
                        let distinct = x.transform_tracks.iter().all(|t| set.insert(*t));
                        let max = x.transform_tracks.iter().map(|t| i64::from(*t) + 1).max().unwrap_or(0);
                        check(&mut checks, "binding tracks not negative", x.transform_tracks.iter().all(|t| *t >= 0));
                        let header = hk::AnimationHeader::read(&k, x.animation, &name, 0);
                        check(
                            &mut checks,
                            "binding transform tracks == animation +24",
                            header.as_ref().is_ok_and(|h| h.transform_tracks as usize == x.transform_tracks.len()),
                        );
                        check(
                            &mut checks,
                            "binding float tracks == animation +28",
                            header.as_ref().is_ok_and(|h| h.float_tracks as usize == x.float_tracks.len()),
                        );
                        bindings.push((name.clone(), x.skeleton_name.to_vec(), max.max(0) as usize, distinct));
                    }),
                ),
                hk::SPLINE => outcome(
                    "spline animation",
                    hk::SplineAnimation::read(&k, i, &name, 0, &b).map(|s| {
                        let h = &s.header;
                        bump(&mut values, format!("spline frames per block {}", s.frames_per_block));
                        bump(&mut values, format!("spline frame duration {:.6}", s.frame_duration));
                        check(
                            &mut checks,
                            "spline frames == round(duration / frame duration) + 1",
                            s.frames == (h.duration / s.frame_duration).round() as u32 + 1,
                        );
                        let bd = (s.frames_per_block - 1) as f32 * s.frame_duration;
                        check(&mut checks, "spline block duration == (frames per block - 1) x frame duration, exact f32", s.block_duration == bd);
                        check(
                            &mut checks,
                            "spline block duration == (frames per block - 1) x frame duration, rel 1e-6",
                            (f64::from(s.block_duration) - f64::from(bd)).abs() <= 1e-6 * f64::from(bd),
                        );
                        check(
                            &mut checks,
                            "spline block inverse x block duration == 1 within 1e-6",
                            (f64::from(s.block_inverse_duration) * f64::from(s.block_duration) - 1.).abs() < 1e-6,
                        );
                        check(&mut checks, "spline annotation tracks == transform tracks", h.annotation_tracks.count == h.transform_tracks);
                        check(&mut checks, "spline has a reference frame", h.reference_frame.is_some());
                        check(&mut checks, "spline data length multiple of 16", s.data.len().is_multiple_of(16));
                        bump(&mut values, format!("spline blocks {}", s.block_offsets.len().min(9)));
                    }),
                ),
                hk::LOSSLESS => outcome(
                    "lossless animation header",
                    hk::AnimationHeader::read(&k, i, &name, 0).map(|h| {
                        check(&mut checks, "lossless annotation tracks == transform tracks", h.annotation_tracks.count == h.transform_tracks);
                    }),
                ),
                _ => {}
            }
        }
        // A skeleton.hkx against the skeleton.nif beside it.
        if name.ends_with("/skeleton.hkx") {
            let nif = format!("{}.nif", name.trim_end_matches(".hkx"));
            compare_nif(&vfs, &m, &k, &name, &nif, &mut nif_cmp, &b);
        }
        skeletons.extend(file_skeletons);
    }
    // Bindings against skeletons of the same name.
    for (file, sname, max, distinct) in &bindings {
        let same: Vec<_> = skeletons.iter().filter(|s| &s.name == sname).collect();
        let fit = same.iter().filter(|s| *max <= s.bones && *distinct).count();
        // The skeleton.hkx of the clip's actor folder, if any: the path up to
        // the folder after meshes/actors/, then /characterassets/skeleton.hkx.
        let parts: Vec<&str> = file.split('/').collect();
        let folder_skel = (parts.len() > 3 && parts[0] == "meshes" && parts[1] == "actors")
            .then(|| format!("meshes/actors/{}/characterassets/skeleton.hkx", parts[2]));
        let own = folder_skel
            .as_ref()
            .and_then(|f| skeletons.iter().find(|s| &s.path == f && &s.name == sname));
        bump(
            &mut values,
            format!(
                "binding: skeletons of its name {}, fitting {}, actor folder skeleton.hkx {}",
                same.len().min(3),
                fit.min(3),
                match own {
                    None => "absent or other name",
                    Some(s) if *max <= s.bones && *distinct => "fits",
                    Some(_) => "does not fit",
                }
            ),
        );
    }
    let mut out = String::new();
    writeln!(
        out,
        "winning .hkx paths\t{}\nbytes\t{bytes_total}",
        paths.len()
    )?;
    writeln!(out, "\n# outcomes")?;
    for (k, n) in &c {
        writeln!(out, "{n}\t{k}")?;
    }
    writeln!(out, "\n# checks: holds, fails")?;
    for (k, [h, f]) in &checks {
        writeln!(out, "{h}\t{f}\t{k}")?;
    }
    writeln!(out, "\n# values")?;
    for (k, n) in &values {
        writeln!(out, "{n}\t{k}")?;
    }
    writeln!(out, "\n# skeleton.hkx against skeleton.nif")?;
    for (k, n) in &nif_cmp {
        writeln!(out, "{n}\t{k}")?;
    }
    writeln!(out, "\n# class\tfiles\tobjects")?;
    for (k, n) in &files {
        writeln!(out, "{k}\t{n}\t{}", objects[k])?;
    }
    std::fs::write(&args[1], out)?;
    Ok(())
}

/// Bone names, parents and reference-pose world transforms of the first
/// skeleton of a `skeleton.hkx` against the nodes of the `skeleton.nif`.
fn compare_nif(
    vfs: &Vfs,
    m: &Manifest,
    k: &hk::Container<'_>,
    name: &str,
    nif: &str,
    out: &mut BTreeMap<String, u64>,
    b: &Budget,
) {
    let Some(i) = k.objects.iter().position(|o| o.class == hk::SKELETON) else {
        bump(out, "files: no skeleton object".into());
        return;
    };
    let Ok(s) = hk::Skeleton::read(k, i, name, 0, b) else {
        bump(out, "files: skeleton not read".into());
        return;
    };
    let Ok(bytes) = vfs.read_path_bytes(nif.as_bytes(), b) else {
        bump(out, "files: no skeleton.nif beside it".into());
        return;
    };
    let Ok(n) = Nif::read(&bytes, nif, fo4_core::Game::Fallout4, &m.limits, b) else {
        bump(out, "files: skeleton.nif not read".into());
        return;
    };
    let Ok(rest) = pose::Skeleton::read(&n, nif, &m.limits, b) else {
        bump(out, "files: skeleton.nif rest pose not read".into());
        return;
    };
    bump(out, "files: compared".into());
    // Parent names in the NIF: the nearest named node above each node.
    let mut nif_parent: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for (bi, blk) in n.blocks.iter().enumerate() {
        if let Some(fo4_nif::Data::Node { children, .. }) = &blk.decoded {
            let Some(pn) = pose::name(&n, bi) else {
                continue;
            };
            for ch in children.iter().filter(|c| **c >= 0) {
                if let Some(cn) = pose::name(&n, *ch as usize) {
                    nif_parent.insert(cn.to_vec(), pn.to_vec());
                }
            }
        }
    }
    let found = s
        .bones
        .iter()
        .filter(|b| rest.bones.contains_key(**b))
        .count();
    *out.entry(format!(
        "files: bones found in the NIF {}",
        if found == s.bones.len() {
            "all"
        } else {
            "not all"
        }
    ))
    .or_default() += 1;
    *out.entry("bones".into()).or_default() += s.bones.len() as u64;
    *out.entry("bones found in the NIF".into()).or_default() += found as u64;
    for (j, bn) in s.bones.iter().enumerate() {
        let hp = usize::try_from(s.parents[j]).ok().map(|p| s.bones[p]);
        let np = nif_parent.get(*bn).map(Vec::as_slice);
        let key = match (hp, np) {
            (Some(a), Some(b)) if a == b => "bone parent: same name in the NIF",
            (Some(_), Some(_)) => "bone parent: other name in the NIF",
            (Some(_), None) => "bone parent: none named in the NIF",
            (None, _) => "bone parent: root bone",
        };
        *out.entry(key.into()).or_default() += 1;
    }
    // Bones whose every ancestor has the same parent name in both files.
    let mut chain = vec![false; s.bones.len()];
    for j in 0..s.bones.len() {
        let own = rest.bones.contains_key(s.bones[j]);
        chain[j] = own
            && match usize::try_from(s.parents[j]) {
                Err(_) => true,
                Ok(p) => {
                    chain[p] && nif_parent.get(s.bones[j]).map(Vec::as_slice) == Some(s.bones[p])
                }
            };
    }
    let worlds = hk_worlds(&s, false, false);
    let (mut dt, mut dr, mut nc) = (0f64, 0f64, 0u64);
    for (j, bn) in s.bones.iter().enumerate() {
        if !chain[j] {
            continue;
        }
        let nw = &rest.bones[*bn];
        dt = dt.max(dist(worlds[j].translation, nw.translation));
        dr = dr.max(rot_err(&worlds[j], nw));
        nc += 1;
    }
    *out.entry("bones with a matching parent chain".into())
        .or_default() += nc;
    *out.entry(format!(
        "files: matching chain, largest translation distance {}",
        bucket(dt)
    ))
    .or_default() += 1;
    *out.entry(format!(
        "files: matching chain, largest rotation difference {}",
        bucket(dr)
    ))
    .or_default() += 1;
    eprintln!(
        "NIFCMP\t{name}\tbones {}\tfound {found}\tchain {nc}\tmax translation {dt:.5}\tmax rotation {dr:.6}",
        s.bones.len()
    );
    for (transpose, w_first, label) in [
        (false, false, "q(x,y,z,w) rows"),
        (true, false, "q(x,y,z,w) transposed"),
        (false, true, "q(w,x,y,z) rows"),
        (true, true, "q(w,x,y,z) transposed"),
    ] {
        let worlds = hk_worlds(&s, transpose, w_first);
        for (j, bn) in s.bones.iter().enumerate() {
            let Some(nw) = rest.bones.get(*bn) else {
                continue;
            };
            let hw = &worlds[j];
            *out.entry(format!(
                "{label}: world translation distance {}",
                bucket(dist(hw.translation, nw.translation))
            ))
            .or_default() += 1;
            *out.entry(format!(
                "{label}: world rotation largest element difference {}",
                bucket(rot_err(hw, nw))
            ))
            .or_default() += 1;
        }
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
