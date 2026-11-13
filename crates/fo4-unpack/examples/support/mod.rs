#![forbid(unsafe_code)]
//! Explicit scene collection and isolated per-camera CPU runs. No windows or GPU.
use std::{
    io::Write,
    path::{Component, Path, PathBuf},
};
pub fn memory() -> Result<(), String> {
    let Some(value) = std::env::var_os("FO4_MIN_FREE_PERCENT") else {
        return Ok(());
    };
    let floor = value
        .to_str()
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|n| (1..=100).contains(n))
        .ok_or("InvalidMemoryFloor")?;
    let out = std::process::Command::new("/usr/bin/memory_pressure")
        .arg("-Q")
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8(out.stdout).map_err(|e| e.to_string())?;
    let free = text
        .lines()
        .find_map(|s| s.strip_prefix("System-wide memory free percentage: "))
        .and_then(|s| s.trim_end_matches('%').parse::<u8>().ok())
        .ok_or("MemoryMonitorUnavailable")?;
    if free < floor {
        return Err(format!("MemoryFloorReached: {free}% < {floor}%"));
    }
    Ok(())
}

pub fn output_dir(arg: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    output_dir_away_from(arg, None)
}
pub fn output_dir_away_from(
    arg: &str,
    forbidden: Option<&Path>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    let path = Path::new(arg);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("RenderOutputParentTraversal".into());
    }
    let out = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    if !out.starts_with(cwd.join("build")) {
        return Err("RenderOutputOutsideBuild".into());
    }
    for ancestor in out.ancestors() {
        if let Ok(m) = std::fs::symlink_metadata(ancestor) {
            if m.file_type().is_symlink() {
                return Err("RenderOutputSymlink".into());
            }
        }
    }
    if forbidden.is_some_and(|root| out.starts_with(root)) {
        return Err("RenderOutputInsideInputRoot".into());
    }
    std::fs::create_dir_all(&out)?;
    Ok(out)
}
pub fn new_file(path: &Path) -> Result<std::fs::File, std::io::Error> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}
pub fn json(path: &Path, value: &impl serde::Serialize) -> Result<(), Box<dyn std::error::Error>> {
    struct Limited {
        file: std::io::BufWriter<std::fs::File>,
        bytes: u64,
    }
    impl Write for Limited {
        fn write(&mut self, v: &[u8]) -> std::io::Result<usize> {
            let next = self
                .bytes
                .checked_add(v.len() as u64)
                .ok_or_else(|| std::io::Error::other("RenderReportOverflow"))?;
            if next > 16 << 20 {
                return Err(std::io::Error::other("RenderReportLimit"));
            }
            self.file.write_all(v)?;
            self.bytes = next;
            Ok(v.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.file.flush()
        }
    }
    let mut w = Limited {
        file: std::io::BufWriter::new(new_file(path)?),
        bytes: 0,
    };
    serde_json::to_writer_pretty(&mut w, value)?;
    w.flush()?;
    Ok(())
}
pub fn output_bytes(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let mut f = new_file(path)?;
    f.write_all(bytes)
}
