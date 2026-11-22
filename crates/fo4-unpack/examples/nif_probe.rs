#![forbid(unsafe_code)]
//! Prints the blocks of NIF files read through the manifest's archives:
//! type, size, decoded fields and a hex prefix of undecoded bytes.
//! Measurement support only; output goes to standard output.
use fo4_core::{Budget, Game, Manifest};
use fo4_vfs::Vfs;
use std::path::Path;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let m = Manifest::load(Path::new(&args[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let hex_max: usize = args[1].parse()?;
    for path in &args[2..] {
        let b = Budget::new(m.limits.max_memory);
        let bytes = vfs.read(path, &b)?;
        let n = fo4_nif::Nif::read(&bytes, path, Game::Fallout4, &m.limits, &b)?;
        println!("== {path} blocks {} roots {:?}", n.blocks.len(), n.roots);
        for (i, block) in n.blocks.iter().enumerate() {
            let t = &n.block_types[block.type_index as usize];
            let mut line = format!("[{i}] {t} size {}", block.data.len());
            match &block.decoded {
                Some(fo4_nif::Data::Node { fields, children }) => {
                    for f in fields {
                        if let fo4_nif::Value::StringIndex(s) = f.value {
                            if f.name == "Name" {
                                line.push_str(&format!(
                                    " name {:?}",
                                    n.strings
                                        .get(s as usize)
                                        .map(|v| String::from_utf8_lossy(v).into_owned())
                                ));
                            }
                        }
                    }
                    line.push_str(&format!(" children {children:?}"));
                }
                Some(fo4_nif::Data::Mesh(mesh)) => {
                    line.push_str(&format!(
                        " mesh desc {:x?} verts {} tris {}",
                        mesh.vertex_desc, mesh.declared_vertices, mesh.declared_triangles
                    ));
                    for f in &mesh.fields {
                        match f.value {
                            fo4_nif::Value::Ref(r) => line.push_str(&format!(" {}={r}", f.name)),
                            fo4_nif::Value::StringIndex(s) if f.name == "Name" => {
                                line.push_str(&format!(
                                    " name {:?}",
                                    n.strings
                                        .get(s as usize)
                                        .map(|v| String::from_utf8_lossy(v).into_owned())
                                ))
                            }
                            _ => {}
                        }
                    }
                }
                Some(fo4_nif::Data::Fields(_)) => line.push_str(" fields"),
                None => {
                    let raw = &bytes[block.data.clone()];
                    let mut h = String::new();
                    for (k, c) in raw.iter().take(hex_max).enumerate() {
                        if k % 4 == 0 {
                            h.push(' ');
                        }
                        h.push_str(&format!("{c:02x}"));
                    }
                    line.push_str(&format!(" raw{h}"));
                }
            }
            if !block.raw_tail.is_empty() && block.decoded.is_some() {
                line.push_str(&format!(" tail {}", block.raw_tail.len()));
            }
            println!("{line}");
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
