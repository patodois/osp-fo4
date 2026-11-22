#![forbid(unsafe_code)]
//! Prints the container of one sound file of the manifest's mounts and,
//! when its codec is decoded by `fo4_audio`, writes the decoded samples as
//! a plain wave file (`spec/audio.md`). Measurement support.
//!
//! `sound_dump MANIFEST PATH [OUT.wav]`. PATH is the mounted path (any case,
//! either separator; a leading `data` folder is removed), or `first:EXT`
//! for the first mounted path, in key order, with that extension. The
//! output is
//! refused inside a Git working tree, so a dump never lands in the
//! repository. Nothing is played.
use fo4_audio::{packets, pcm, Sound};
use fo4_core::{Budget, Manifest};
use fo4_vfs::Vfs;
use std::path::Path;

fn inside_git(path: &Path) -> bool {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    abs.ancestors().skip(1).any(|d| d.join(".git").exists())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err("sound_dump MANIFEST PATH [OUT.wav]".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut path = args[1].replace('\\', "/");
    if path.to_ascii_lowercase().starts_with("data/") {
        path = path[5..].to_string();
    }
    if let Some(ext) = args[1].strip_prefix("first:") {
        let suffix = format!(".{}", ext.to_ascii_lowercase());
        path = vfs
            .candidates
            .iter()
            .filter_map(|c| c.key.as_ref())
            .find(|k| k.ends_with(suffix.as_bytes()))
            .map(|k| String::from_utf8_lossy(k).into_owned())
            .ok_or("no mounted path with that extension")?;
        println!("path	{path}");
    }
    let b = Budget::new(m.limits.max_payload);
    let bytes = vfs.read(&path, &b)?;
    let s = Sound::read(&bytes, &path, &b)?;
    let f = &s.wave.format;
    println!("container\t{}", s.container.label());
    println!("bytes\t{}", bytes.len());
    println!(
        "fmt\ttag {:#06x} channels {} rate {} average bytes/s {} block align {} bits {} extension {}",
        f.tag,
        f.channels,
        f.sample_rate,
        f.avg_bytes_per_sec,
        f.block_align,
        f.bits,
        f.extension.as_ref().map_or("none".into(), |e| format!("{} bytes", e.len()))
    );
    if let Some(z) = &s.fuz {
        println!(
            "voice container\tversion {} lip half {} bytes at {} audio half at {}",
            z.version, z.lip.length, z.lip.offset, z.audio.offset
        );
    }
    for c in &s.wave.riff.chunks {
        println!(
            "chunk\t{}\tat {}\t{} bytes{}",
            String::from_utf8_lossy(&c.id),
            s.audio_offset + c.header,
            c.data.length,
            if c.pad.is_some() { " + pad" } else { "" }
        );
    }
    println!("parts\t{:?} (total {})", s.parts, s.parts.total());
    if !s.wave.dpds.is_empty() {
        println!(
            "WMA packets\t{:?}\tdpds entries {}\tfirst {:?}\tlast {:?}",
            packets(&s.wave),
            s.wave.dpds.len(),
            s.wave.dpds.first(),
            s.wave.dpds.last()
        );
    }
    match s.status() {
        pcm::Status::Pcm => {
            let p = s.decode(&bytes, &path, &b)?;
            println!(
                "decoded\t{} frames, {:.3} s",
                p.frames(),
                p.frames() as f64 / f64::from(p.sample_rate.max(1))
            );
            if let Some(out) = args.get(2) {
                let out = Path::new(out);
                if inside_git(out) {
                    return Err("refusing to write inside a Git working tree".into());
                }
                let wav = pcm::encode_wav(&p, &b)?;
                let back = Sound::read(&wav, "dump", &b)?;
                println!(
                    "written data chunk identical to the source data chunk\t{}",
                    back.data(&wav) == s.data(&bytes)
                );
                std::fs::write(out, wav)?;
                println!("written\t{}", out.display());
            }
        }
        pcm::Status::WmaOpen => println!("decoded\tno: WMA family, payload kept raw"),
        pcm::Status::Unsupported => println!("decoded\tno: format not supported"),
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
