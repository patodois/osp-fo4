use crate::blocks::{Field, Reader, Value};
use fo4_core::{add, half_to_f32, limit, mul, range, Budget, Cursor, Error, Result};
use serde::Serialize;
use std::ops::Range;
#[derive(Debug, Serialize, Default)]
pub struct Vertex {
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub position: Option<[f32; 3]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub uv: Option<[f32; 2]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub normal: Option<[f32; 3]>,
    pub normal_bytes: Option<[u8; 4]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub tangent: Option<[f32; 3]>,
    pub tangent_bytes: Option<[u8; 4]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub bitangent: Option<[f32; 3]>,
    pub color: Option<[u8; 4]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub color_float: Option<[f32; 4]>,
    pub raw_skin: Option<Range<usize>>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub eye: Option<f32>,
}
#[derive(Debug, Serialize)]
pub struct Mesh {
    pub non_finite: crate::NonFiniteCounts,
    pub fields: Vec<Field>,
    pub vertex_desc: Option<u64>,
    pub declared_vertices: u32,
    pub declared_triangles: u32,
    pub vertices: Option<Vec<Vertex>>,
    pub triangles: Option<Vec<[u16; 3]>>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub dynamic_vertices: Option<Vec<[f32; 4]>>,
    pub raw_vertex_aux: Vec<Range<usize>>,
}
fn vec3(c: &mut Cursor<'_>) -> Result<[f32; 3]> {
    Ok([c.f32()?, c.f32()?, c.f32()?])
}
fn half(c: &mut Cursor<'_>) -> Result<f32> {
    Ok(half_to_f32(c.u16()?))
}
fn norm(v: [u8; 4]) -> [f32; 3] {
    [
        v[0] as f32 * 2. / 255. - 1.,
        v[1] as f32 * 2. / 255. - 1.,
        v[2] as f32 * 2. / 255. - 1.,
    ]
}
fn triangles(c: &mut Cursor<'_>, n: u32, nv: u32, b: &Budget) -> Result<Vec<[u16; 3]>> {
    range(
        c.pos,
        mul(n as u64, 6, c.source.name(), c.pos)?,
        c.end,
        c.source.name(),
    )?;
    let mut out = Vec::new();
    b.reserve(&mut out, n as usize, c.source.name(), c.pos)?;
    for _ in 0..n {
        let at = c.pos;
        let t = [c.u16()?, c.u16()?, c.u16()?];
        if t.iter().any(|v| *v as u32 >= nv) {
            return Err(Error::new(
                "InvalidNifTriangleIndex",
                c.source.name(),
                at,
                format!("< {nv}"),
                format!("{t:?}"),
            ));
        }
        out.push(t);
    }
    Ok(out)
}
pub(crate) fn compact(name: &str, mut r: Reader<'_, '_>) -> Result<Mesh> {
    if r.bs < 100 {
        return Err(Error::new(
            "UnsupportedNifGeometryVersion",
            r.c.source.name(),
            r.c.pos,
            "BS 100 or 130 for BSTriShape",
            r.bs,
        ));
    }
    r.av()?;
    r.floats("Bounding Sphere", 4)?;
    r.link("Skin")?;
    r.link("Shader Property")?;
    r.link("Alpha Property")?;
    let o = r.c.pos;
    let desc = r.c.u64()?;
    r.put("Vertex Desc", o, Value::U64(desc))?;
    if desc >> 56 != 0 {
        return Err(Error::new(
            "UnsupportedNifVertexDesc",
            r.c.source.name(),
            o,
            "documented lower 56 bits",
            desc,
        ));
    }
    let attrs = (desc >> 44) & 0xfff;
    if attrs & !0x57b != 0 || attrs & 16 != 0 && attrs & 8 == 0 {
        return Err(Error::new(
            "UnsupportedNifVertexAttributes",
            r.c.source.name(),
            o,
            "position/UV/normal/tangent/color/skin/eye/full precision",
            format!("{attrs:#x}"),
        ));
    }
    let nt = if r.bs == 100 {
        r.u16("Num Triangles")? as u32
    } else {
        r.u32("Num Triangles")?
    };
    let nv = r.u16("Num Vertices")? as u32;
    let size = r.u32("Data Size")? as u64;
    limit(nt as u64, r.l.max_entries, r.c.source.name(), r.c.pos)?;
    let stride = (desc & 15) * 4;
    let mut vertices = None;
    let mut tris = None;
    let mut auxiliary = Vec::new();
    if size > 0 {
        let expected = add(
            mul(nv as u64, stride, r.c.source.name(), o)?,
            mul(nt as u64, 6, r.c.source.name(), o)?,
            r.c.source.name(),
            o,
        )?;
        if size != expected {
            return Err(Error::new(
                "NifGeometrySizeMismatch",
                r.c.source.name(),
                r.c.pos - 4,
                expected,
                size,
            ));
        }
        range(r.c.pos, size, r.c.end, r.c.source.name())?;
        let position_size = if r.bs == 100 || attrs & 0x400 != 0 {
            16
        } else {
            8
        };
        let mut sections = Vec::new();
        r.b.reserve(&mut sections, 8, r.c.source.name(), o)?;
        for (flag, shift, size) in [
            (1, 0, position_size),
            (2, 8, 4),
            (8, 16, 4),
            (16, 20, 4),
            (32, 24, 4),
            (64, 28, 12),
            (256, 36, 4),
        ] {
            if attrs & flag != 0 {
                let at = if flag == 1 {
                    0
                } else {
                    ((desc >> shift) & 15) * 4
                };
                range(at, size, stride, r.c.source.name())?;
                sections.push((at, size, flag));
            }
        }
        sections.sort_unstable();
        let mut end = 0;
        for (at, n, _) in &sections {
            if *at < end {
                return Err(Error::new(
                    "OverlappingNifVertexAttributes",
                    r.c.source.name(),
                    o,
                    "nonoverlapping descriptor offsets",
                    at,
                ));
            }
            end = at + n;
        }
        let mut verts = Vec::new();
        r.b.reserve(&mut verts, nv as usize, r.c.source.name(), r.c.pos)?;
        for _ in 0..nv {
            let start = r.c.pos;
            let mut v = Vertex::default();
            let mut bx = None;
            for (offset, length, flag) in &sections {
                let at = add(start, *offset, r.c.source.name(), start)?;
                let mut c = Cursor::at(r.c.source, at, add(at, *length, r.c.source.name(), at)?)?;
                match *flag {
                    1 => {
                        v.position = Some(if position_size == 16 {
                            vec3(&mut c)?
                        } else {
                            [half(&mut c)?, half(&mut c)?, half(&mut c)?]
                        });
                        if attrs & 16 != 0 {
                            bx = Some(if position_size == 16 {
                                c.f32()?
                            } else {
                                half(&mut c)?
                            });
                        }
                    }
                    2 => v.uv = Some([half(&mut c)?, half(&mut c)?]),
                    8 => {
                        let raw = c.take::<4>()?;
                        v.normal = Some(norm(raw));
                        v.normal_bytes = Some(raw);
                    }
                    16 => {
                        let raw = c.take::<4>()?;
                        v.tangent = Some(norm(raw));
                        v.tangent_bytes = Some(raw);
                    }
                    32 => v.color = Some(c.take()?),
                    64 => v.raw_skin = Some(at as usize..(at + length) as usize),
                    256 => v.eye = Some(c.f32()?),
                    _ => {}
                }
            }
            if let (Some(x), Some(n), Some(t)) = (bx, v.normal_bytes, v.tangent_bytes) {
                v.bitangent = Some([
                    x,
                    n[3] as f32 * 2. / 255. - 1.,
                    t[3] as f32 * 2. / 255. - 1.,
                ]);
            }
            // Gaps/unused position W remain available, without assigning a value.
            let mut end = 0;
            for (at, n, flag) in &sections {
                if *at > end {
                    r.b.reserve(&mut auxiliary, 1, r.c.source.name(), start)?;
                    auxiliary.push((start + end) as usize..(start + at) as usize);
                }
                if *flag == 1 && attrs & 16 == 0 {
                    r.b.reserve(&mut auxiliary, 1, r.c.source.name(), start)?;
                    auxiliary.push(
                        (start + at + n - position_size / 4) as usize..(start + at + n) as usize,
                    );
                }
                end = at + n;
            }
            if end < stride {
                r.b.reserve(&mut auxiliary, 1, r.c.source.name(), start)?;
                auxiliary.push((start + end) as usize..(start + stride) as usize);
            }
            r.c.pos = add(start, stride, r.c.source.name(), start)?;
            verts.push(v);
        }
        vertices = Some(verts);
        tris = Some(triangles(r.c, nt, nv, r.b)?);
    }
    if r.bs == 100 {
        let ps = r.u32("Particle Data Size")?;
        if ps > 0 {
            let n = add(
                mul(nv as u64, 12, r.c.source.name(), r.c.pos)?,
                mul(nt as u64, 6, r.c.source.name(), r.c.pos)?,
                r.c.source.name(),
                r.c.pos,
            )?;
            r.raw("Particle Vertices Normals Triangles", n)?;
        }
    }
    match name {
        "BSSubIndexTriShape" => {
            if r.bs == 100 {
                let n = r.u32("Num Segments")?;
                r.check_array(n as u64, 9)?;
                for _ in 0..n {
                    r.u8("Segment Flags")?;
                    r.u32("Segment Start")?;
                    r.u32("Segment Primitives")?;
                }
            } else if size > 0 {
                r.u32("Num Primitives")?;
                let n = r.u32("Num Segments")?;
                let total = r.u32("Total Segments")?;
                r.check_array(n as u64, 16)?;
                for _ in 0..n {
                    r.u32("Segment Start")?;
                    r.u32("Segment Primitives")?;
                    r.u32("Segment Parent")?;
                    let sub = r.u32("Num Sub Segments")?;
                    r.check_array(sub as u64, 16)?;
                    for _ in 0..sub {
                        r.u32("Sub Segment Start")?;
                        r.u32("Sub Segment Primitives")?;
                        r.u32("Sub Segment Parent")?;
                        r.u32("Sub Segment Unused")?;
                    }
                }
                if n < total {
                    let shared = r.u32("Shared Num Segments")?;
                    let total = r.u32("Shared Total Segments")?;
                    r.check_array(shared as u64, 4)?;
                    for _ in 0..shared {
                        r.u32("Segment Start")?;
                    }
                    r.check_array(total as u64, 12)?;
                    for _ in 0..total {
                        r.u32("User Index")?;
                        r.u32("Bone ID")?;
                        let cuts = r.u32("Num Cut Offsets")?;
                        limit(cuts as u64, 8, r.c.source.name(), r.c.pos - 4)?;
                        r.floats("Cut Offset", cuts as u64)?;
                    }
                    let o = r.c.pos;
                    let n = r.c.u16()? as u64;
                    limit(n, r.l.max_string, r.c.source.name(), o)?;
                    let text = r.c.bytes(n, r.l, r.b)?;
                    r.put("SSF File", o, Value::Bytes(text))?;
                }
            }
        }
        "BSMeshLODTriShape" => {
            r.u32("LOD0 Size")?;
            r.u32("LOD1 Size")?;
            r.u32("LOD2 Size")?;
        }
        "BSDynamicTriShape" => {}
        _ => {}
    }
    let dynamic_vertices = if name == "BSDynamicTriShape" {
        let ds = r.u32("Dynamic Data Size")?;
        if ds as u64 != mul(nv as u64, 16, r.c.source.name(), r.c.pos - 4)? {
            return Err(Error::new(
                "NifDynamicSizeMismatch",
                r.c.source.name(),
                r.c.pos - 4,
                nv as u64 * 16,
                ds,
            ));
        }
        r.check_array(nv as u64, 16)?;
        let mut out = Vec::new();
        r.b.reserve(&mut out, nv as usize, r.c.source.name(), r.c.pos)?;
        for _ in 0..nv {
            out.push([r.c.f32()?, r.c.f32()?, r.c.f32()?, r.c.f32()?]);
        }
        Some(out)
    } else {
        None
    };
    let mut mesh = Mesh {
        non_finite: crate::NonFiniteCounts::default(),
        fields: r.fields,
        vertex_desc: Some(desc),
        declared_vertices: nv,
        declared_triangles: nt,
        vertices,
        triangles: tris,
        dynamic_vertices,
        raw_vertex_aux: auxiliary,
    };
    mesh.non_finite = crate::NonFiniteCounts::from_mesh(&mesh);
    Ok(mesh)
}
pub(crate) fn legacy(mut r: Reader<'_, '_>) -> Result<Mesh> {
    r.u32("Group ID")?;
    let nv = r.u16("Num Vertices")? as u32;
    r.u8("Keep Flags")?;
    r.u8("Compress Flags")?;
    let mut vertices = Vec::new();
    r.b.reserve(&mut vertices, nv as usize, r.c.source.name(), r.c.pos)?;
    for _ in 0..nv {
        vertices.push(Vertex::default());
    }
    if r.bool("Has Vertices")? {
        r.check_array(nv as u64, 12)?;
        for v in &mut vertices {
            v.position = Some(vec3(r.c)?);
        }
    }
    let flags = r.u16("BS Data Flags")?;
    r.u32("Material CRC")?;
    if r.bool("Has Normals")? {
        r.check_array(nv as u64, 12)?;
        for v in &mut vertices {
            v.normal = Some(vec3(r.c)?);
        }
        if flags & 4096 != 0 {
            r.check_array(nv as u64, 24)?;
            for v in &mut vertices {
                v.tangent = Some(vec3(r.c)?);
            }
            for v in &mut vertices {
                v.bitangent = Some(vec3(r.c)?);
            }
        }
    }
    r.floats("Bounding Sphere", 4)?;
    if r.bool("Has Colors")? {
        r.check_array(nv as u64, 16)?;
        for v in &mut vertices {
            v.color_float = Some([r.c.f32()?, r.c.f32()?, r.c.f32()?, r.c.f32()?]);
        }
    }
    if flags & 1 != 0 {
        r.check_array(nv as u64, 8)?;
        for v in &mut vertices {
            v.uv = Some([r.c.f32()?, r.c.f32()?]);
        }
    }
    r.u16("Consistency Flags")?;
    r.link("Additional Data")?;
    let nt = r.u16("Num Triangles")? as u32;
    r.u32("Num Triangle Points")?;
    let tris = if r.bool("Has Triangles")? {
        Some(triangles(r.c, nt, nv, r.b)?)
    } else {
        None
    };
    let matches = r.u16("Num Match Groups")?;
    r.check_array(matches as u64, 2)?;
    for _ in 0..matches {
        let n = r.u16("Match Group Vertices")?;
        r.check_array(n as u64, 2)?;
        for _ in 0..n {
            r.u16("Match Vertex")?;
        }
    }
    let mut mesh = Mesh {
        non_finite: crate::NonFiniteCounts::default(),
        fields: r.fields,
        vertex_desc: None,
        declared_vertices: nv,
        declared_triangles: nt,
        vertices: Some(vertices),
        triangles: tris,
        dynamic_vertices: None,
        raw_vertex_aux: Vec::new(),
    };
    mesh.non_finite = crate::NonFiniteCounts::from_mesh(&mesh);
    Ok(mesh)
}
