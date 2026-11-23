//! Own, versioned draw-list interchange. It contains no inferred game fields.
use crate::{Alpha, Draw, DrawList, Mesh, Texture, Vertex};
use fo4_core::{Budget, Cursor, Error, Limits, Result, SliceSource};
use fo4_scene::transform::Transform;
use std::io::Write;
/// Lossless transport for large private snapshots; the inner format is F4RL0003.
pub fn write_compact(
    list: &DrawList,
    writer: &mut impl Write,
    l: &Limits,
    b: &Budget,
) -> Result<u64> {
    let declared = write(list, &mut std::io::sink(), l)?;
    let scratch = b.child();
    scratch.charge(512 << 10, "snapshot zlib workspace", 0)?;
    writer
        .write_all(b"F4RZ0001")
        .and_then(|_| writer.write_all(&declared.to_le_bytes()))
        .map_err(|e| fo4_core::io_error("compact snapshot", 0, e))?;
    let mut z = flate2::write::ZlibEncoder::new(writer, flate2::Compression::new(6));
    let actual = write(list, &mut z, l)?;
    if actual != declared {
        return Err(Error::new(
            "SnapshotSizeChanged",
            "snapshot",
            0,
            declared,
            actual,
        ));
    }
    z.finish()
        .map_err(|e| fo4_core::io_error("compact snapshot", 16, e))?;
    Ok(declared)
}
struct Output<'a, W> {
    writer: &'a mut W,
    bytes: u64,
    limits: &'a Limits,
}
impl<W: Write> Output<'_, W> {
    fn bytes(&mut self, v: &[u8]) -> Result<()> {
        self.bytes = fo4_core::add(self.bytes, v.len() as u64, "draw snapshot", self.bytes)?;
        fo4_core::limit(
            self.bytes,
            self.limits.max_payload,
            "draw snapshot",
            self.bytes,
        )?;
        self.writer
            .write_all(v)
            .map_err(|e| fo4_core::io_error("draw snapshot", self.bytes, e))
    }
    fn count(&mut self, n: usize) -> Result<()> {
        fo4_core::limit(
            n as u64,
            self.limits.max_entries,
            "draw snapshot",
            self.bytes,
        )?;
        self.bytes(
            &u32::try_from(n)
                .map_err(|e| Error::new("SnapshotCount", "draw snapshot", self.bytes, "u32", e))?
                .to_le_bytes(),
        )
    }
    fn u32(&mut self, n: u32) -> Result<()> {
        self.bytes(&n.to_le_bytes())
    }
    fn f64(&mut self, n: f64) -> Result<()> {
        self.bytes(&n.to_le_bytes())
    }
}
pub fn write(list: &DrawList, writer: &mut impl Write, l: &Limits) -> Result<u64> {
    let mut w = Output {
        writer,
        bytes: 0,
        limits: l,
    };
    for d in &list.draws {
        if d.sidedness.material_two_sided.is_some_and(|v| v > 1) {
            return Err(Error::new(
                "SnapshotSidedness",
                "snapshot",
                0,
                "material two-sided byte 0 or 1",
                format!("{:?}", d.sidedness.material_two_sided),
            ));
        }
        if d.uv_offset
            .iter()
            .chain(&d.uv_scale)
            .any(|v| !v.is_finite())
        {
            return Err(Error::new(
                "SnapshotNonFinite",
                "snapshot",
                0,
                "finite UV offset and scale",
                format!("{:?}/{:?}", d.uv_offset, d.uv_scale),
            ));
        }
    }
    w.bytes(b"F4RL0003")?;
    w.count(list.meshes.len())?;
    w.count(list.textures.len())?;
    w.count(list.draws.len())?;
    for m in &list.meshes {
        w.count(m.vertices.len())?;
        w.count(m.triangles.len())?;
        for v in &m.vertices {
            for n in v.position.into_iter().chain(v.uv) {
                w.f64(n)?;
            }
        }
        for t in &m.triangles {
            for n in t {
                w.bytes(&n.to_le_bytes())?;
            }
        }
    }
    if !list.blocks.is_empty() {
        // The interchange carries decoded texels only.
        return Err(Error::new(
            "SnapshotBlockTexture",
            "snapshot",
            w.bytes,
            "decoded texels",
            "stored blocks",
        ));
    }
    for t in &list.textures {
        t.validate()?;
        w.u32(t.width)?;
        w.u32(t.height)?;
        w.u32(t.mip)?;
        w.bytes(&[t.srgb as u8])?;
        let n = u32::try_from(t.rgba.len())
            .map_err(|e| Error::new("SnapshotTextureSize", "snapshot", w.bytes, "u32", e))?;
        w.u32(n)?;
        for v in &t.rgba {
            w.bytes(&v.to_le_bytes())?;
        }
    }
    if !list.overlays.is_empty() {
        // The interchange has no field for them; refuse rather than drop them.
        return Err(Error::new(
            "SnapshotOverlayDraws",
            "snapshot",
            w.bytes,
            "no overlay draws",
            list.overlays.len(),
        ));
    }
    if let Some(d) = list.draws.iter().position(|d| d.falloff.is_some()) {
        // Nor for an effect surface's falloff and its mesh's normals.
        return Err(Error::new(
            "SnapshotFalloffDraws",
            "snapshot",
            w.bytes,
            "no falloff draws",
            d,
        ));
    }
    if let Some(d) = list.draws.iter().position(|d| d.colour.is_some()) {
        // Nor for a colour the clock drives.
        return Err(Error::new(
            "SnapshotColourDraws",
            "snapshot",
            w.bytes,
            "no draws with a clock colour",
            d,
        ));
    }
    if let Some(d) = list.draws.iter().position(|d| d.palette.is_some()) {
        // Nor for an effect surface's palette lookup.
        return Err(Error::new(
            "SnapshotPaletteDraws",
            "snapshot",
            w.bytes,
            "no palette draws",
            d,
        ));
    }
    for d in &list.draws {
        d.alpha.validate()?;
        w.u32(d.instance)?;
        w.u32(
            u32::try_from(d.mesh)
                .map_err(|e| Error::new("SnapshotMeshIndex", "snapshot", w.bytes, "u32", e))?,
        )?;
        w.u32(
            u32::try_from(d.texture)
                .map_err(|e| Error::new("SnapshotTextureIndex", "snapshot", w.bytes, "u32", e))?,
        )?;
        for x in d
            .world
            .rotation
            .into_iter()
            .flatten()
            .chain(d.world.translation)
            .chain([d.world.scale])
            .chain(d.uv_offset)
            .chain(d.uv_scale)
        {
            w.f64(x)?;
        }
        let a = d.alpha;
        w.bytes(&[a.enabled])?;
        w.u32(a.source)?;
        w.u32(a.destination)?;
        w.bytes(&[
            a.test_enabled,
            a.test_ref,
            a.nif_test_function.unwrap_or(255),
        ])?;
        w.bytes(&a.opacity.to_le_bytes())?;
        w.bytes(&[a.nif_flags.is_some() as u8])?;
        if let Some(f) = a.nif_flags {
            w.bytes(&f.to_le_bytes())?;
        }
        w.bytes(&[d.raw_tile_flags.is_some() as u8])?;
        if let Some(t) = d.raw_tile_flags {
            w.u32(t)?;
        }
    }
    for t in &list.textures {
        w.count(t.mips.len())?;
        for m in &t.mips {
            w.u32(m.width)?;
            w.u32(m.height)?;
            w.u32(m.mip)?;
            w.count(m.rgba.len())?;
            for v in &m.rgba {
                w.bytes(&v.to_le_bytes())?;
            }
        }
    }
    for d in &list.draws {
        w.bytes(&[d.sidedness.material_two_sided.unwrap_or(255)])?;
        w.bytes(&[d.sidedness.nif_shader_flags_2.is_some() as u8])?;
        if let Some(f) = d.sidedness.nif_shader_flags_2 {
            w.u32(f)?;
        }
    }
    for m in &list.meshes {
        w.bytes(&[m.vertex_colors.is_some() as u8])?;
        if let Some(colors) = &m.vertex_colors {
            if colors.len() != m.vertices.len() || colors.iter().flatten().any(|v| !v.is_finite()) {
                return Err(Error::new(
                    "SnapshotVertexColors",
                    "snapshot",
                    w.bytes,
                    "one finite RGBA per vertex",
                    colors.len(),
                ));
            }
            for c in colors {
                for v in c {
                    w.bytes(&v.to_le_bytes())?;
                }
            }
        }
    }
    Ok(w.bytes)
}
fn f64(c: &mut Cursor<'_>) -> Result<f64> {
    let at = c.pos;
    let v = f64::from_le_bytes(c.take::<8>()?);
    if !v.is_finite() {
        return Err(Error::new(
            "SnapshotNonFinite",
            "snapshot",
            at,
            "finite float64",
            v,
        ));
    }
    Ok(v)
}
fn count(c: &mut Cursor<'_>, l: &Limits) -> Result<usize> {
    let at = c.pos;
    let n = c.u32()?;
    fo4_core::limit(n as u64, l.max_entries, "snapshot", at)?;
    Ok(n as usize)
}
fn tag(c: &mut Cursor<'_>) -> Result<bool> {
    let at = c.pos;
    match c.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        n => Err(Error::new("SnapshotBoolean", "snapshot", at, "0 or 1", n)),
    }
}
pub fn read(bytes: &[u8], l: &Limits, b: &Budget) -> Result<DrawList> {
    if bytes.starts_with(b"F4RZ") {
        if bytes.get(..8) != Some(b"F4RZ0001".as_slice()) {
            return Err(Error::new(
                "UnsupportedSnapshotEncoding",
                "snapshot",
                0,
                "F4RZ0001",
                fo4_core::HexBytes(bytes.get(..8).unwrap_or(bytes)),
            ));
        }
        let header = bytes.get(8..16).ok_or_else(|| {
            Error::new(
                "TruncatedCompactSnapshot",
                "snapshot",
                8,
                8,
                bytes.len().saturating_sub(8),
            )
        })?;
        let declared = u64::from_le_bytes(
            header
                .try_into()
                .map_err(|e| Error::new("CompactSnapshotSize", "snapshot", 8, "u64", e))?,
        );
        let work = b.child();
        let raw = fo4_core::zlib(&bytes[16..], declared, l, &work, "compact snapshot", 16)?;
        if !raw.starts_with(b"F4RL") {
            return Err(Error::new(
                "UnsupportedSnapshotInnerEncoding",
                "snapshot",
                16,
                "uncompressed F4RL",
                fo4_core::HexBytes(raw.get(..8).unwrap_or(&raw)),
            ));
        }
        return read(&raw, l, b);
    }
    l.validate("draw snapshot")?;
    fo4_core::limit(bytes.len() as u64, l.max_payload, "snapshot", 0)?;
    let source = SliceSource {
        name: "draw snapshot",
        bytes,
    };
    let mut c = Cursor::new(&source);
    let magic = c.take::<8>()?;
    let version3 = magic == *b"F4RL0003";
    let version2 = version3 || magic == *b"F4RL0002";
    if !version2 && magic != *b"F4RL0001" {
        return Err(Error::new(
            "UnsupportedRenderSnapshot",
            "snapshot",
            0,
            "F4RL0001, F4RL0002 or F4RL0003",
            fo4_core::HexBytes(&magic),
        ));
    }
    let nm = count(&mut c, l)?;
    let nt = count(&mut c, l)?;
    let nd = count(&mut c, l)?;
    let mut list = DrawList::default();
    // Minimum packet bytes are checked before allocation as well as count ceilings.
    fo4_core::range(
        c.pos,
        fo4_core::add(
            fo4_core::mul(nm as u64, 8, "snapshot", c.pos)?,
            fo4_core::add(
                fo4_core::mul(nt as u64, 17, "snapshot", c.pos)?,
                fo4_core::mul(nd as u64, 166, "snapshot", c.pos)?,
                "snapshot",
                c.pos,
            )?,
            "snapshot",
            c.pos,
        )?,
        bytes.len() as u64,
        "snapshot",
    )?;
    b.reserve(&mut list.meshes, nm, "snapshot meshes", c.pos)?;
    b.reserve(&mut list.textures, nt, "snapshot textures", c.pos)?;
    b.reserve(&mut list.draws, nd, "snapshot draws", c.pos)?;
    for _ in 0..nm {
        let nv = count(&mut c, l)?;
        let ni = count(&mut c, l)?;
        fo4_core::range(
            c.pos,
            fo4_core::add(
                fo4_core::mul(nv as u64, 40, "snapshot", c.pos)?,
                fo4_core::mul(ni as u64, 6, "snapshot", c.pos)?,
                "snapshot",
                c.pos,
            )?,
            bytes.len() as u64,
            "snapshot",
        )?;
        let mut m = Mesh {
            normals: None,
            vertex_colors: None,
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        b.reserve(&mut m.vertices, nv, "snapshot vertices", c.pos)?;
        b.reserve(&mut m.triangles, ni, "snapshot indices", c.pos)?;
        for _ in 0..nv {
            m.vertices.push(Vertex {
                position: [f64(&mut c)?, f64(&mut c)?, f64(&mut c)?],
                uv: [f64(&mut c)?, f64(&mut c)?],
            });
        }
        for _ in 0..ni {
            let t = [c.u16()?, c.u16()?, c.u16()?];
            if t.iter().any(|i| *i as usize >= nv) {
                return Err(Error::new(
                    "SnapshotVertexIndex",
                    "snapshot",
                    c.pos - 6,
                    nv,
                    format!("{t:?}"),
                ));
            }
            m.triangles.push(t);
        }
        list.meshes.push(m);
    }
    for _ in 0..nt {
        let width = c.u32()?;
        let height = c.u32()?;
        let mip = c.u32()?;
        let srgb = tag(&mut c)?;
        let n = c.u32()? as u64;
        if n != fo4_core::mul(
            fo4_core::mul(width as u64, height as u64, "snapshot texture", c.pos)?,
            4,
            "snapshot texture",
            c.pos,
        )? {
            return Err(Error::new(
                "SnapshotTextureElements",
                "snapshot",
                c.pos - 4,
                "width*height*4",
                n,
            ));
        }
        fo4_core::range(
            c.pos,
            fo4_core::mul(n, 4, "snapshot", c.pos)?,
            bytes.len() as u64,
            "snapshot",
        )?;
        let mut rgba = Vec::new();
        b.reserve(
            &mut rgba,
            usize::try_from(n)
                .map_err(|e| Error::new("SnapshotSize", "snapshot", c.pos, "usize", e))?,
            "snapshot rgba",
            c.pos,
        )?;
        for _ in 0..n {
            rgba.push(c.f32()?);
        }
        let t = Texture {
            mips: Vec::new(),
            width,
            height,
            mip,
            srgb,
            rgba,
        };
        t.validate()?;
        list.textures.push(t);
    }
    for _ in 0..nd {
        let instance = c.u32()?;
        let mesh = c.u32()? as usize;
        let texture = c.u32()? as usize;
        if mesh >= nm || texture >= nt {
            return Err(Error::new(
                "SnapshotDrawIndex",
                "snapshot",
                c.pos - 8,
                "existing mesh and texture",
                format!("{mesh},{texture}"),
            ));
        }
        let rotation = [
            [f64(&mut c)?, f64(&mut c)?, f64(&mut c)?],
            [f64(&mut c)?, f64(&mut c)?, f64(&mut c)?],
            [f64(&mut c)?, f64(&mut c)?, f64(&mut c)?],
        ];
        let translation = [f64(&mut c)?, f64(&mut c)?, f64(&mut c)?];
        let scale = f64(&mut c)?;
        let uv_offset = [f64(&mut c)?, f64(&mut c)?];
        let uv_scale = [f64(&mut c)?, f64(&mut c)?];
        let enabled = c.u8()?;
        let source = c.u32()?;
        let destination = c.u32()?;
        let test_enabled = c.u8()?;
        let test_ref = c.u8()?;
        let raw_function = c.u8()?;
        let nif_test_function = if raw_function == 255 {
            None
        } else if raw_function <= 7 {
            Some(raw_function)
        } else {
            return Err(Error::new(
                "SnapshotAlphaFunction",
                "snapshot",
                c.pos - 1,
                "0..7 or 255",
                raw_function,
            ));
        };
        let opacity = c.f32()?;
        let nif_flags = if tag(&mut c)? { Some(c.u16()?) } else { None };
        let raw_tile_flags = if tag(&mut c)? { Some(c.u32()?) } else { None };
        let alpha = Alpha {
            enabled,
            source,
            destination,
            test_enabled,
            test_ref,
            nif_test_function,
            opacity,
            nif_flags,
        };
        alpha.validate()?;
        list.draws.push(Draw {
            falloff: None,
            palette: None,
            colour: None,
            sidedness: crate::Sidedness::default(),
            instance,
            mesh,
            texture,
            world: Transform {
                rotation,
                translation,
                scale,
            },
            alpha,
            uv_offset,
            uv_scale,
            raw_tile_flags,
        });
    }
    if version2 {
        for t in &mut list.textures {
            let n = count(&mut c, l)?;
            fo4_core::limit(n as u64, 32, "mip levels", c.pos)?;
            fo4_core::range(
                c.pos,
                fo4_core::mul(n as u64, 16, "mip headers", c.pos)?,
                bytes.len() as u64,
                "snapshot",
            )?;
            b.reserve(&mut t.mips, n, "snapshot mips", c.pos)?;
            for _ in 0..n {
                let width = c.u32()?;
                let height = c.u32()?;
                let mip = c.u32()?;
                let count = count(&mut c, l)?;
                let expected = fo4_core::mul(
                    fo4_core::mul(width as u64, height as u64, "mip", c.pos)?,
                    4,
                    "mip",
                    c.pos,
                )?;
                if count as u64 != expected {
                    return Err(Error::new(
                        "SnapshotMipSize",
                        "snapshot",
                        c.pos,
                        expected,
                        count,
                    ));
                }
                fo4_core::range(
                    c.pos,
                    fo4_core::mul(count as u64, 4, "mip bytes", c.pos)?,
                    bytes.len() as u64,
                    "snapshot",
                )?;
                let mut rgba = Vec::new();
                b.reserve(&mut rgba, count, "snapshot mip", c.pos)?;
                for _ in 0..count {
                    rgba.push(c.f32()?);
                }
                t.mips.push(crate::Mip {
                    width,
                    height,
                    mip,
                    rgba,
                });
            }
            t.validate()?;
        }
        for d in &mut list.draws {
            let value = c.u8()?;
            d.sidedness.material_two_sided = match value {
                255 => None,
                0 | 1 => Some(value),
                _ => {
                    return Err(Error::new(
                        "SnapshotSidedness",
                        "snapshot",
                        c.pos - 1,
                        "0/1/255",
                        value,
                    ))
                }
            };
            d.sidedness.nif_shader_flags_2 = if tag(&mut c)? { Some(c.u32()?) } else { None };
        }
    }
    if version3 {
        for m in &mut list.meshes {
            if tag(&mut c)? {
                let mut colors = Vec::new();
                b.reserve(
                    &mut colors,
                    m.vertices.len(),
                    "snapshot vertex colors",
                    c.pos,
                )?;
                for _ in &m.vertices {
                    let color = [c.f32()?, c.f32()?, c.f32()?, c.f32()?];
                    if color.iter().any(|v| !v.is_finite()) {
                        return Err(Error::new(
                            "SnapshotVertexColors",
                            "snapshot",
                            c.pos - 16,
                            "finite RGBA",
                            "nonfinite",
                        ));
                    }
                    colors.push(color);
                }
                m.vertex_colors = Some(colors);
            }
        }
    }
    if c.pos != bytes.len() as u64 {
        return Err(Error::new(
            "UnexpectedRenderSnapshotTail",
            "snapshot",
            c.pos,
            "end of input",
            bytes.len() as u64 - c.pos,
        ));
    }
    Ok(list)
}
