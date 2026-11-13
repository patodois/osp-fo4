//! Repository-only guard, written from the rules in the documented data/provenance contract.
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

const FORBIDDEN_EXTENSIONS: &[&str] = &[
    "esm",
    "esp",
    "esl",
    "ba2",
    "bsa",
    "nif",
    "kf",
    "hkx",
    "dds",
    "bgsm",
    "bgem",
    "strings",
    "dlstrings",
    "ilstrings",
    "pex",
    "psc",
    "swf",
    "wav",
    "xwm",
    "fuz",
    "lip",
    "bk2",
    "btr",
    "bto",
    "cdx",
    "exe",
    "dll",
    "fos",
];
const SIGNATURES: &[(&[u8], &str)] = &[
    (b"BTDX", "BA2"),
    (b"TES4", "plugin"),
    (b"DDS ", "DDS"),
    (b"BSA\0", "BSA"),
    (b"Gamebryo File Format", "NIF"),
    (b"RIFF", "RIFF audio"),
];
const SKIPPED_ROOT_DIRECTORIES: &[&str] =
    &[".git", ".progress-git", "target", ".cargo-home", "reports"];

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    Extension(&'static str),
    MachinePath,
    Signature(&'static str),
    SymbolicLink,
    UnsupportedFileType,
}
impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MachinePath => {
                f.write_str("machine-specific absolute home path in versioned file")
            }
            Self::Extension(ext) => write!(f, "forbidden game-file extension .{ext}"),
            Self::Signature(format) => write!(f, "game-file signature at offset 0: {format}"),
            Self::SymbolicLink => f.write_str("symbolic link: target not followed or verified"),
            Self::UnsupportedFileType => f.write_str("unsupported filesystem entry: not verified"),
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    /// Relative to the supplied root, never the contents of a link target.
    pub path: PathBuf,
    pub reason: Reason,
}

fn at(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}
fn push<T>(items: &mut Vec<T>, item: T, path: &Path) -> io::Result<()> {
    items
        .try_reserve(1)
        .map_err(|e| at(path, io::Error::other(e)))?;
    items.push(item);
    Ok(())
}

/// Read only enough bytes to distinguish the longest requested signature.
fn prefix(path: &Path) -> io::Result<([u8; 20], usize)> {
    let mut file = File::open(path).map_err(|e| at(path, e))?;
    let mut bytes = [0; 20];
    let mut size = 0;
    while size < bytes.len() {
        match file.read(&mut bytes[size..]) {
            Ok(0) => break,
            Ok(n) => size += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(at(path, e)),
        }
    }
    Ok((bytes, size))
}

/// Check all files, including hidden and otherwise gitignored files. Only the
/// five generated directories directly under root are pruned. I/O errors fail
/// the scan instead of turning an unreadable subtree into a clean result.
pub fn scan_repository(root: &Path) -> io::Result<Vec<Violation>> {
    if !fs::symlink_metadata(root)
        .map_err(|e| at(root, e))?
        .file_type()
        .is_dir()
    {
        return Err(at(
            root,
            io::Error::other("scan root must be a directory, not a link"),
        ));
    }
    let mut violations = Vec::new();
    let mut pending = Vec::new();
    push(&mut pending, PathBuf::new(), root)?;
    while let Some(relative_directory) = pending.pop() {
        let directory = root.join(&relative_directory);
        for entry in fs::read_dir(&directory).map_err(|e| at(&directory, e))? {
            let entry = entry.map_err(|e| at(&directory, e))?;
            let name = entry.file_name();
            let relative = relative_directory.join(&name);
            let path = root.join(&relative);
            let kind = entry.file_type().map_err(|e| at(&path, e))?;
            if kind.is_dir() {
                // Generated benchmark and raster outputs are confined to ignored build/.
                if relative == Path::new("build") {
                    continue;
                }
                if relative_directory.as_os_str().is_empty()
                    && SKIPPED_ROOT_DIRECTORIES
                        .iter()
                        .any(|skip| name == OsStr::new(skip))
                {
                    continue;
                }
                push(&mut pending, relative, &path)?;
                continue;
            }
            if let Some(ext) = path.extension().and_then(OsStr::to_str) {
                if let Some(&ext) = FORBIDDEN_EXTENSIONS
                    .iter()
                    .find(|known| ext.eq_ignore_ascii_case(known))
                {
                    push(
                        &mut violations,
                        Violation {
                            path: relative.clone(),
                            reason: Reason::Extension(ext),
                        },
                        &path,
                    )?;
                }
            }
            if kind.is_symlink() || !kind.is_file() {
                push(
                    &mut violations,
                    Violation {
                        path: relative,
                        reason: if kind.is_symlink() {
                            Reason::SymbolicLink
                        } else {
                            Reason::UnsupportedFileType
                        },
                    },
                    &path,
                )?;
                continue;
            }
            // Extension and content are independent: inspect even forbidden extensions.
            let (bytes, size) = prefix(&path)?;
            for &(signature, format) in SIGNATURES {
                if bytes[..size].starts_with(signature) {
                    push(
                        &mut violations,
                        Violation {
                            path: relative.clone(),
                            reason: Reason::Signature(format),
                        },
                        &path,
                    )?;
                }
            }
        }
    }
    violations.sort_unstable_by(|a, b| a.path.cmp(&b.path).then_with(|| a.reason.cmp(&b.reason)));
    Ok(violations)
}

/// The real workspace test and its fake-workspace regression use the same gate.
pub fn require_clean_repository(root: &Path) -> io::Result<()> {
    let mut violations = scan_repository(root)?;
    if violations.is_empty() {
        violations.extend(scan_machine_paths(root, &versioned_paths(root)?)?);
    }
    if violations.is_empty() {
        return Ok(());
    }
    let mut message = String::from("GameFilesInRepository:");
    for violation in violations {
        use std::fmt::Write;
        writeln!(
            &mut message,
            "\n{}: {}",
            violation.path.display(),
            violation.reason
        )
        .map_err(io::Error::other)?;
    }
    Err(io::Error::other(message))
}

/// Content check for the explicit versioned set; untracked user instructions are not published files.
pub fn scan_machine_paths(root: &Path, paths: &[PathBuf]) -> io::Result<Vec<Violation>> {
    let needle = concat!("/", "Users/").as_bytes();
    let mut violations = Vec::new();
    for relative in paths {
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(io::Error::other("InvalidVersionedPath"));
        }
        let path = root.join(relative);
        let meta = fs::symlink_metadata(&path)?;
        if !meta.is_file() {
            return Err(at(
                &path,
                io::Error::other("versioned entry is not a regular file"),
            ));
        }
        let mut f = File::open(&path)?;
        let mut bytes = [0u8; 8192];
        let mut carry = Vec::new();
        loop {
            let n = f.read(&mut bytes)?;
            if n == 0 {
                break;
            }
            carry.try_reserve(n).map_err(io::Error::other)?;
            carry.extend_from_slice(&bytes[..n]);
            if carry.windows(needle.len()).any(|w| w == needle) {
                push(
                    &mut violations,
                    Violation {
                        path: relative.clone(),
                        reason: Reason::MachinePath,
                    },
                    &path,
                )?;
                break;
            }
            let keep = carry.len().saturating_sub(needle.len() - 1);
            carry.drain(..keep);
        }
    }
    Ok(violations)
}
fn versioned_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let dir = if root.join(".progress-git/HEAD").is_file() {
        root.join(".progress-git")
    } else if root.join(".git/HEAD").is_file() {
        root.join(".git")
    } else {
        return Ok(Vec::new());
    };
    let out = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(dir)
        .arg("--work-tree")
        .arg(root)
        .args(["ls-files", "--full-name", "-z"])
        .output()?;
    if !out.status.success() || out.stdout.len() > 8 << 20 {
        return Err(io::Error::other("VersionedPathInventoryUnavailable"));
    }
    let mut paths = Vec::new();
    for raw in out.stdout.split(|c| *c == 0).filter(|r| !r.is_empty()) {
        let text = std::str::from_utf8(raw).map_err(io::Error::other)?;
        push(&mut paths, PathBuf::from(text), root)?;
    }
    Ok(paths)
}
