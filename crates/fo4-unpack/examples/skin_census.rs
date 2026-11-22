#![forbid(unsafe_code)]
//! Census of skinned shapes over every winning NIF of the manifest's mounts:
//! the blocks a shape's Skin link reaches, their sizes against candidate
//! layouts, the twelve per-vertex skin bytes against candidate meanings,
//! and which space each bone's sphere is in. Measurement support for
//! `spec/nif.md`; reads one file at a time and writes a plain-text report.
use fo4_core::{half_to_f32, Budget, Game, Manifest};
use fo4_nif::{Data, Nif, Value};
use fo4_vfs::Vfs;
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
}
fn f32_at(d: &[u8], o: usize) -> Option<f32> {
    u32_at(d, o).map(f32::from_bits)
}
fn bump<K: Ord>(m: &mut BTreeMap<K, u64>, k: K) {
    *m.entry(k).or_default() += 1;
}
fn shape_ref(fields: &[fo4_nif::Field], name: &str) -> Option<i32> {
    fields.iter().find_map(|f| match f.value {
        Value::Ref(r) if f.name == name => Some(r),
        _ => None,
    })
}

#[derive(Default)]
struct Census {
    files: u64,
    failed: u64,
    files_with_skin: u64,
    skin_types: BTreeMap<String, u64>,
    shape_skin_targets: BTreeMap<String, u64>,
    shapes_skinned: u64,
    shapes_skinned_without_attr: u64,
    shapes_attr_without_skin: u64,
    instance_size_ok: u64,
    instance_size_bad: u64,
    instance_tail_counts: BTreeMap<u32, u64>,
    instance_first: BTreeMap<String, u64>,
    instance_second: BTreeMap<String, u64>,
    instance_bones: BTreeMap<String, u64>,
    bonedata_size_ok: u64,
    bonedata_size_bad: u64,
    bonedata_count_match: u64,
    bonedata_count_mismatch: u64,
    vertices: u64,
    weight_sum: BTreeMap<String, u64>,
    index_in_range: u64,
    index_out_of_range: u64,
    zero_weight_index_nonzero: u64,
    weights_descending: u64,
    weights_not_descending: u64,
    scale_values: BTreeMap<String, u64>,
    rotation_orthonormal: u64,
    rotation_not_orthonormal: u64,
    inside: [u64; 3],
    pairs: u64,
    vertex_desc_attrs: BTreeMap<String, u64>,
    shape_types: BTreeMap<String, u64>,
    shape_identity: [u64; 2],
}

fn rot(d: &[u8], o: usize) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| f32_at(d, o + 4 * (i * 3 + j)).unwrap() as f64))
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("skin_census MANIFEST OUT.txt [PATH_PREFIX]".into());
    }
    let prefix = args.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
    let m = Manifest::load(Path::new(&args[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut paths: Vec<Vec<u8>> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.clone())
        .filter(|k| k.ends_with(b".nif") && k.starts_with(prefix.as_bytes()))
        .collect();
    paths.sort();
    paths.dedup();
    let mut c = Census::default();
    for path in &paths {
        let b = Budget::new(m.limits.max_memory);
        let Ok(bytes) = vfs.read_path_bytes(path, &b) else {
            c.failed += 1;
            continue;
        };
        let name = String::from_utf8_lossy(path).into_owned();
        let Ok(n) = Nif::read(&bytes, &name, Game::Fallout4, &m.limits, &b) else {
            c.failed += 1;
            continue;
        };
        c.files += 1;
        let ty = |i: usize| -> String {
            n.blocks
                .get(i)
                .map(|b| n.block_types[b.type_index as usize].clone())
                .unwrap_or_else(|| "<none>".into())
        };
        let mut any = false;
        for blk in &n.blocks {
            let t = &n.block_types[blk.type_index as usize];
            if t.contains("Skin") {
                bump(&mut c.skin_types, t.clone());
                any = true;
            }
        }
        if any {
            c.files_with_skin += 1;
        }
        for blk in &n.blocks {
            let Some(Data::Mesh(mesh)) = &blk.decoded else {
                continue;
            };
            let st = n.block_types[blk.type_index as usize].clone();
            let skin = shape_ref(&mesh.fields, "Skin").unwrap_or(-1);
            let attrs = mesh.vertex_desc.map(|d| (d >> 44) & 0xfff).unwrap_or(0);
            if skin < 0 {
                if attrs & 64 != 0 {
                    c.shapes_attr_without_skin += 1;
                }
                continue;
            }
            bump(&mut c.shape_types, st);
            // The shape's own transform words (translation, rows, scale).
            let words: Vec<f32> = mesh
                .fields
                .iter()
                .filter(|f| matches!(f.name, "Translation" | "Rotation" | "Scale"))
                .filter_map(|f| match f.value {
                    Value::FloatBits(b) => Some(f32::from_bits(b)),
                    _ => None,
                })
                .collect();
            let identity = [0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.];
            c.shape_identity[(words.len() == 13 && words.iter().zip(identity).all(|(a, b)| *a == b))
                as usize] += 1;
            bump(&mut c.vertex_desc_attrs, format!("{attrs:#x}"));
            c.shapes_skinned += 1;
            if attrs & 64 == 0 {
                c.shapes_skinned_without_attr += 1;
            }
            let target = ty(skin as usize);
            bump(&mut c.shape_skin_targets, target.clone());
            if target != "BSSkin::Instance" {
                continue;
            }
            let inst = &bytes[n.blocks[skin as usize].data.clone()];
            let nb = u32_at(inst, 8).unwrap_or(u32::MAX) as usize;
            let tail_at = 12 + 4 * nb;
            let tail = u32_at(inst, tail_at);
            if let Some(t) = tail {
                bump(&mut c.instance_tail_counts, t);
            }
            let expected = tail.map(|t| tail_at + 4 + 12 * t as usize);
            if expected == Some(inst.len()) {
                c.instance_size_ok += 1;
            } else {
                c.instance_size_bad += 1;
                continue;
            }
            let r0 = u32_at(inst, 0).unwrap() as i32;
            let r1 = u32_at(inst, 4).unwrap() as i32;
            bump(
                &mut c.instance_first,
                if r0 < 0 {
                    "null".into()
                } else {
                    format!(
                        "{}{}",
                        ty(r0 as usize),
                        if r0 == 0 { " (block 0)" } else { "" }
                    )
                },
            );
            bump(
                &mut c.instance_second,
                if r1 < 0 {
                    "null".into()
                } else {
                    ty(r1 as usize)
                },
            );
            for k in 0..nb {
                let r = u32_at(inst, 12 + 4 * k).unwrap() as i32;
                bump(
                    &mut c.instance_bones,
                    if r < 0 { "null".into() } else { ty(r as usize) },
                );
            }
            if r1 < 0 || ty(r1 as usize) != "BSSkin::BoneData" {
                continue;
            }
            let bd = &bytes[n.blocks[r1 as usize].data.clone()];
            let bn = u32_at(bd, 0).unwrap_or(0) as usize;
            if bd.len() == 4 + 68 * bn {
                c.bonedata_size_ok += 1;
            } else {
                c.bonedata_size_bad += 1;
                continue;
            }
            if bn == nb {
                c.bonedata_count_match += 1;
            } else {
                c.bonedata_count_mismatch += 1;
                continue;
            }
            let mut bones = Vec::new();
            for k in 0..bn {
                let o = 4 + 68 * k;
                let sphere = [0, 1, 2, 3].map(|i| f32_at(bd, o + 4 * i).unwrap() as f64);
                let r = rot(bd, o + 16);
                let t = [0, 1, 2].map(|i| f32_at(bd, o + 52 + 4 * i).unwrap() as f64);
                let s = f32_at(bd, o + 64).unwrap() as f64;
                bump(&mut c.scale_values, format!("{s:.4}"));
                let mut err: f64 = 0.;
                for i in 0..3 {
                    for j in 0..3 {
                        let d: f64 = (0..3).map(|q| r[i][q] * r[j][q]).sum();
                        err = err.max((d - if i == j { 1. } else { 0. }).abs());
                    }
                }
                if err < 1e-3 {
                    c.rotation_orthonormal += 1;
                } else {
                    c.rotation_not_orthonormal += 1;
                }
                bones.push((sphere, r, t, s));
            }
            let Some(verts) = &mesh.vertices else {
                continue;
            };
            for v in verts {
                let Some(range) = &v.raw_skin else { continue };
                let raw = &bytes[range.clone()];
                c.vertices += 1;
                let w: [f32; 4] = std::array::from_fn(|i| {
                    half_to_f32(u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]))
                });
                let idx: [u8; 4] = [raw[8], raw[9], raw[10], raw[11]];
                let sum: f32 = w.iter().sum();
                let d = (sum - 1.).abs();
                bump(
                    &mut c.weight_sum,
                    if d < 1e-3 {
                        "|sum-1|<1e-3".into()
                    } else if d < 1e-2 {
                        "<1e-2".into()
                    } else {
                        format!("other {:.2}", sum)
                    },
                );
                if w.windows(2).all(|p| p[0] >= p[1]) {
                    c.weights_descending += 1;
                } else {
                    c.weights_not_descending += 1;
                }
                for i in 0..4 {
                    if w[i] == 0. {
                        if idx[i] != 0 {
                            c.zero_weight_index_nonzero += 1;
                        }
                        continue;
                    }
                    if (idx[i] as usize) < bn {
                        c.index_in_range += 1;
                    } else {
                        c.index_out_of_range += 1;
                        continue;
                    }
                    let Some(p) = v.position else { continue };
                    let p = p.map(f64::from);
                    let (sphere, r, t, s) = bones[idx[i] as usize];
                    c.pairs += 1;
                    let inside = |q: [f64; 3]| {
                        let d = ((q[0] - sphere[0]).powi(2)
                            + (q[1] - sphere[1]).powi(2)
                            + (q[2] - sphere[2]).powi(2))
                        .sqrt();
                        d <= sphere[3] * 1.001 + 1e-3
                    };
                    // Skin space as stored; bone space with the rotation applied as
                    // rows times the vector, and as the vector times the rows.
                    let rows: [f64; 3] = std::array::from_fn(|a| {
                        t[a] + s * (0..3).map(|b2| r[a][b2] * p[b2]).sum::<f64>()
                    });
                    let cols: [f64; 3] = std::array::from_fn(|a| {
                        t[a] + s * (0..3).map(|b2| r[b2][a] * p[b2]).sum::<f64>()
                    });
                    c.inside[0] += inside(p) as u64;
                    c.inside[1] += inside(rows) as u64;
                    c.inside[2] += inside(cols) as u64;
                }
            }
        }
    }
    let mut o = String::new();
    writeln!(
        o,
        "files read {} failed {} with any *Skin* block {}",
        c.files, c.failed, c.files_with_skin
    )?;
    writeln!(o, "block types containing Skin: {:?}", c.skin_types)?;
    writeln!(
        o,
        "shapes with a Skin link: {} by type {:?}",
        c.shapes_skinned, c.shape_types
    )?;
    writeln!(o, "  Skin link targets: {:?}", c.shape_skin_targets)?;
    writeln!(
        o,
        "  shape transform exactly identity: {} not: {}",
        c.shape_identity[1], c.shape_identity[0]
    )?;
    writeln!(o, "  vertex attribute words: {:?}", c.vertex_desc_attrs)?;
    writeln!(
        o,
        "  without vertex attribute 64: {}; attribute 64 without Skin link: {}",
        c.shapes_skinned_without_attr, c.shapes_attr_without_skin
    )?;
    writeln!(
        o,
        "Instance size = 16 + 4n + 12m: ok {} bad {}; m values {:?}",
        c.instance_size_ok, c.instance_size_bad, c.instance_tail_counts
    )?;
    writeln!(o, "  first word target {:?}", c.instance_first)?;
    writeln!(o, "  second word target {:?}", c.instance_second)?;
    writeln!(o, "  bone link targets {:?}", c.instance_bones)?;
    writeln!(
        o,
        "BoneData size = 4 + 68n: ok {} bad {}; n equal to instance n: {} differ {}",
        c.bonedata_size_ok, c.bonedata_size_bad, c.bonedata_count_match, c.bonedata_count_mismatch
    )?;
    writeln!(
        o,
        "  rotation orthonormal (1e-3) {} not {}",
        c.rotation_orthonormal, c.rotation_not_orthonormal
    )?;
    let mut sv: Vec<_> = c.scale_values.iter().collect();
    sv.sort_by_key(|a| std::cmp::Reverse(*a.1));
    writeln!(o, "  scale word values (top) {:?}", &sv[..sv.len().min(10)])?;
    writeln!(
        o,
        "skin vertices {}: four halfs summed {:?}",
        c.vertices, c.weight_sum
    )?;
    writeln!(
        o,
        "  non-zero weights with byte index < n {} >= n {}; zero weights with non-zero index {}",
        c.index_in_range, c.index_out_of_range, c.zero_weight_index_nonzero
    )?;
    writeln!(
        o,
        "  weights non-increasing {} not {}",
        c.weights_descending, c.weights_not_descending
    )?;
    writeln!(
        o,
        "vertex-bone pairs {}: inside the bone sphere as stored {}, rows*v+t {}, v*rows+t {}",
        c.pairs, c.inside[0], c.inside[1], c.inside[2]
    )?;
    std::fs::write(&args[1], o)?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
