//! Where a build's memory goes (measurement only, no effect on any draw):
//! the bytes of its textures (stored blocks or decoded texels) and meshes,
//! by who draws them (placed references, actors, both, nobody), how many
//! actors share each, and which ones repeat the same content under another
//! entry. `report` is written by the viewer's `--memory-report FILE`.
use crate::draw::Build;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// FNV-1a over bytes.
fn hash(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// The bytes texture `i` keeps: its stored blocks, else its decoded
/// texels and coarser levels (`f32` each), and a hash of that content.
fn texture_bytes(build: &Build, i: usize) -> (u64, u64) {
    let mut h = 0xcbf2_9ce4_8422_2325;
    if let Some(b) = build.list.blocks.get(&i) {
        hash(&mut h, &b.bytes);
        hash(&mut h, &[b.format as u8, u8::from(b.opaque)]);
        hash(&mut h, &b.levels.to_le_bytes());
        return (b.bytes.len() as u64, h);
    }
    let t = &build.list.textures[i];
    let mut n = t.rgba.len() as u64 * 4;
    for x in &t.rgba {
        hash(&mut h, &x.to_bits().to_le_bytes());
    }
    for m in &t.mips {
        n += m.rgba.len() as u64 * 4;
        for x in &m.rgba {
            hash(&mut h, &x.to_bits().to_le_bytes());
        }
    }
    hash(&mut h, &[u8::from(t.srgb)]);
    hash(&mut h, &t.width.to_le_bytes());
    hash(&mut h, &t.height.to_le_bytes());
    (n, h)
}

/// The bytes mesh `i` keeps (vertices, triangles, colours, normals) and a
/// hash of that content.
fn mesh_bytes(build: &Build, i: usize) -> (u64, u64) {
    let m = &build.list.meshes[i];
    let mut h = 0xcbf2_9ce4_8422_2325;
    let mut n = 0u64;
    for v in &m.vertices {
        for x in v.position.iter().chain(v.uv.iter()) {
            hash(&mut h, &x.to_bits().to_le_bytes());
        }
        n += 40;
    }
    for t in &m.triangles {
        for x in t {
            hash(&mut h, &x.to_le_bytes());
        }
        n += 6;
    }
    if let Some(c) = &m.vertex_colors {
        for v in c {
            for x in v {
                hash(&mut h, &x.to_bits().to_le_bytes());
            }
        }
        n += c.len() as u64 * 16;
    }
    if let Some(c) = &m.normals {
        for v in c {
            for x in v {
                hash(&mut h, &x.to_bits().to_le_bytes());
            }
        }
        n += c.len() as u64 * 24;
    }
    (n, h)
}

fn mib(n: u64) -> String {
    format!("{:.1} MiB", n as f64 / f64::from(1u32 << 20))
}

/// The report (see the module): one line per class, numbers only.
pub fn report(build: &Build) -> String {
    let actors: BTreeSet<u32> = build.actors.iter().copied().collect();
    let omitted: BTreeSet<u32> = build.omitted.iter().map(|o| o.instance).collect();
    let mut tex_users: BTreeMap<usize, BTreeSet<u32>> = BTreeMap::new();
    let mut mesh_users: BTreeMap<usize, BTreeSet<u32>> = BTreeMap::new();
    for d in &build.list.draws {
        tex_users.entry(d.texture).or_default().insert(d.instance);
        if let Some(p) = &d.palette {
            tex_users.entry(p.texture).or_default().insert(d.instance);
        }
        mesh_users.entry(d.mesh).or_default().insert(d.instance);
    }
    let mut s = String::new();
    let kinds = |users: Option<&BTreeSet<u32>>| -> &'static str {
        match users {
            None => "drawn by nothing (left by an instance the build omitted)",
            Some(u) => {
                let a = u.iter().filter(|x| actors.contains(x)).count();
                match (a, u.len() - a) {
                    (0, _) => "drawn by placed references only",
                    (_, 0) if a == 1 => "drawn by one actor only",
                    (_, 0) => "drawn by several actors only",
                    _ => "drawn by placed references and actors",
                }
            }
        }
    };
    for (what, count, bytes_of, users) in [
        (
            "textures",
            build.list.textures.len(),
            texture_bytes as fn(&Build, usize) -> (u64, u64),
            &tex_users,
        ),
        ("meshes", build.list.meshes.len(), mesh_bytes, &mesh_users),
    ] {
        let mut by: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
        let mut content: BTreeMap<u64, Vec<(usize, u64)>> = BTreeMap::new();
        let mut blocks = 0u64;
        for i in 0..count {
            let (n, h) = bytes_of(build, i);
            if what == "textures" && build.list.blocks.contains_key(&i) {
                blocks += n;
            }
            let e = by.entry(kinds(users.get(&i))).or_default();
            e.0 += 1;
            e.1 += n;
            content.entry(h).or_default().push((i, n));
        }
        let total: u64 = by.values().map(|x| x.1).sum();
        let _ = writeln!(s, "{what}\t{count}\t{}", mib(total));
        if what == "textures" {
            let _ = writeln!(s, "  as stored blocks\t{}", mib(blocks));
        }
        for (k, (n, b)) in &by {
            let _ = writeln!(s, "  {k}\t{n}\t{}", mib(*b));
        }
        let (mut copies, mut wasted, mut actor_copies, mut actor_wasted) = (0u64, 0u64, 0u64, 0u64);
        for v in content.values().filter(|v| v.len() > 1) {
            copies += v.len() as u64 - 1;
            wasted += v.iter().skip(1).map(|x| x.1).sum::<u64>();
            let by_actor = v
                .iter()
                .filter(|(i, _)| {
                    users
                        .get(i)
                        .is_some_and(|u| u.iter().any(|x| actors.contains(x)))
                })
                .count() as u64;
            if by_actor > 1 {
                actor_copies += by_actor - 1;
                actor_wasted += v
                    .iter()
                    .filter(|(i, _)| {
                        users
                            .get(i)
                            .is_some_and(|u| u.iter().any(|x| actors.contains(x)))
                    })
                    .skip(1)
                    .map(|x| x.1)
                    .sum::<u64>();
            }
        }
        let _ = writeln!(
            s,
            "  entries repeating another entry's content\t{copies}\t{}",
            mib(wasted)
        );
        let _ = writeln!(
            s,
            "  of them repeats among entries actors draw\t{actor_copies}\t{}",
            mib(actor_wasted)
        );
        if what == "textures" {
            // The same path kept twice (block caps of plain and alpha-tested use).
            let mut paths: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            for (i, c) in build.texture_choices.iter().enumerate() {
                paths.entry(c.path.as_str()).or_default().push(i);
            }
            let (mut twice, mut same, mut same_bytes) = (0u64, 0u64, 0u64);
            let (mut tail, mut tail_bytes) = (0u64, 0u64);
            for v in paths.values().filter(|v| v.len() > 1) {
                twice += 1;
                let first = texture_bytes(build, v[0]).1;
                for i in &v[1..] {
                    let (n, h) = texture_bytes(build, *i);
                    if h == first {
                        same += 1;
                        same_bytes += n;
                        continue;
                    }
                    // The smaller entry's blocks are the larger's coarser levels.
                    if let (Some(a), Some(b)) =
                        (build.list.blocks.get(&v[0]), build.list.blocks.get(i))
                    {
                        let (big, small) = if a.bytes.len() >= b.bytes.len() {
                            (a, b)
                        } else {
                            (b, a)
                        };
                        if big.bytes.ends_with(&small.bytes) && big.format == small.format {
                            tail += 1;
                            tail_bytes += small.bytes.len() as u64;
                        }
                    }
                }
            }
            let _ = writeln!(
                s,
                "  paths kept more than once\t{twice}; later entries with the first's content\t{same}\t{}; entries whose blocks are the coarser levels of the other's\t{tail}\t{}",
                mib(same_bytes),
                mib(tail_bytes)
            );
        }
    }
    // Per actor: bytes of what only it draws, and what it shares.
    let mut per: Vec<(u32, u64, u64)> = Vec::new();
    for a in &actors {
        if omitted.contains(a) {
            continue;
        }
        let (mut own, mut shared) = (0u64, 0u64);
        for (users, size) in [
            (&tex_users, texture_bytes as fn(&Build, usize) -> (u64, u64)),
            (&mesh_users, mesh_bytes),
        ] {
            for (i, u) in users {
                if !u.contains(a) {
                    continue;
                }
                let n = size(build, *i).0;
                if u.len() == 1 {
                    own += n;
                } else {
                    shared += n / u.len() as u64;
                }
            }
        }
        per.push((*a, own, shared));
    }
    let drawn = per.len() as u64;
    let own: u64 = per.iter().map(|x| x.1).sum();
    let shared: u64 = per.iter().map(|x| x.2).sum();
    let _ = writeln!(
        s,
        "actors drawn\t{drawn}; textures and meshes only one actor draws\t{}; its share of those several draw\t{}",
        mib(own),
        mib(shared)
    );
    let _ = writeln!(
        s,
        "actors omitted\t{}",
        actors.iter().filter(|a| omitted.contains(a)).count()
    );
    s
}
