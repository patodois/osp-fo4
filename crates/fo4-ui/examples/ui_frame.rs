#![forbid(unsafe_code)]
//! A headless picture of one movie's first frame (`fo4_ui::raster`):
//! presentation only, written outside the repository, never versioned.
//!
//! `ui_frame MANIFEST PATH OUT.png [SCALE]` writes the picture and prints
//! what it holds and what it leaves out.
use fo4_core::{Budget, Manifest};
use fo4_ui::{raster, Limits, Movie};
use fo4_vfs::Vfs;
use std::{io::Write as _, path::Path};

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// An 8-bit RGBA PNG (the PNG specification's chunk layout).
fn png(img: &raster::Image) -> std::io::Result<Vec<u8>> {
    let mut raw = Vec::with_capacity((img.width as usize * 4 + 1) * img.height as usize);
    for row in img.pixels.chunks(img.width as usize) {
        raw.push(0);
        for p in row {
            for v in p {
                raw.push((v.clamp(0., 1.) * 255. + 0.5) as u8);
            }
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw)?;
    let idat = z.finish()?;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        return Err("usage: ui_frame MANIFEST PATH OUT.png [SCALE]".into());
    }
    let scale: f64 = a.get(3).map(|s| s.parse()).transpose()?.unwrap_or(0.5);
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let b = Budget::new(1 << 30);
    let l = Limits::default();
    let mv = Movie::open(&vfs, &a[1], &l, &b)?;
    let (img, counts) = raster::first_frame(&mv, scale, 16 << 20, &l, &b)?;
    std::fs::write(&a[2], png(&img)?)?;
    println!("{} x {} px", img.width, img.height);
    println!("{counts:#?}");
    Ok(())
}
