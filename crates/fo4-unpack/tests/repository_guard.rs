#![forbid(unsafe_code)]
#[path = "support/repository_guard.rs"]
mod guard;

use fo4_core::TestDir;
use guard::{require_clean_repository, scan_repository, Reason, Violation};
use std::{
    fs,
    path::{Path, PathBuf},
};

// Acceptance inputs from the documented data/provenance contract, independent of the scanner's rule tables.
const EXTENSIONS: &[&str] = &[
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
const SIGNATURE_CASES: &[(&[u8], &str)] = &[
    (b"BTDX", "BA2"),
    (b"TES4", "plugin"),
    (b"DDS ", "DDS"),
    (b"BSA\0", "BSA"),
    (b"Gamebryo File Format", "NIF"),
    (b"RIFF", "RIFF audio"),
];
const ROOT_SKIP_CASES: &[&str] = &[".git", ".progress-git", "target", ".cargo-home", "reports"];
#[test]
fn only_the_explicit_generated_build_root_is_exempt() {
    let root = TestDir::new("guard-bench");
    fs::create_dir_all(root.join("build/bench")).unwrap();
    fs::write(root.join("build/bench/Own.esp"), b"TES4").unwrap();
    fs::write(root.join("build/frame.png"), b"generated reference raster").unwrap();
    fs::write(root.join("Accidental.esp"), b"TES4").unwrap();
    let violations = scan_repository(&root).unwrap();
    assert_eq!(violations.len(), 2);
    assert!(violations
        .iter()
        .all(|v| v.path == Path::new("Accidental.esp")));
}

#[test]
fn workspace_contains_no_game_files() -> std::io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    require_clean_repository(&root)
}

#[test]
fn every_requested_extension_is_rejected_even_for_empty_files() {
    let root = TestDir::new("guard-extensions");
    for ext in EXTENSIONS {
        fs::write(root.join(format!("generated.{ext}")), []).unwrap();
    }
    let violations = scan_repository(&root).unwrap();
    assert_eq!(violations.len(), 28);
    for &ext in EXTENSIONS {
        assert!(violations.contains(&Violation {
            path: format!("generated.{ext}").into(),
            reason: Reason::Extension(ext)
        }));
    }
}

#[test]
fn every_signature_is_rejected_with_innocent_extension() {
    let root = TestDir::new("guard-signatures");
    for (i, &(signature, _)) in SIGNATURE_CASES.iter().enumerate() {
        let mut bytes = signature.to_vec();
        bytes.extend(b" generated synthetic bytes");
        fs::write(root.join(format!("notes-{i}.txt")), bytes).unwrap();
    }
    let violations = scan_repository(&root).unwrap();
    assert_eq!(violations.len(), 6);
    for (i, &(_, format)) in SIGNATURE_CASES.iter().enumerate() {
        assert!(violations.contains(&Violation {
            path: format!("notes-{i}.txt").into(),
            reason: Reason::Signature(format)
        }));
    }
}

#[test]
fn extension_case_is_ignored_and_both_rules_apply_to_one_file() {
    let root = TestDir::new("guard-case");
    fs::write(root.join("FOO.BA2"), b"BTDX").unwrap();
    fs::write(root.join("mixed.EsM"), b"empty synthetic envelope").unwrap();
    assert_eq!(
        scan_repository(&root).unwrap(),
        vec![
            Violation {
                path: "FOO.BA2".into(),
                reason: Reason::Extension("ba2")
            },
            Violation {
                path: "FOO.BA2".into(),
                reason: Reason::Signature("BA2")
            },
            Violation {
                path: "mixed.EsM".into(),
                reason: Reason::Extension("esm")
            },
        ]
    );
}

#[test]
fn deep_subdirectories_are_checked() {
    let root = TestDir::new("guard-deep");
    let mut relative = PathBuf::new();
    for _ in 0..32 {
        relative.push("sub");
    }
    fs::create_dir_all(root.join(&relative)).unwrap();
    relative.push("notes.txt");
    fs::write(root.join(&relative), b"TES4").unwrap();
    assert_eq!(
        scan_repository(&root).unwrap(),
        vec![Violation {
            path: relative,
            reason: Reason::Signature("plugin")
        }]
    );
}

#[test]
fn empty_files_partial_signatures_and_mentions_inside_text_are_allowed() {
    let root = TestDir::new("guard-text");
    fs::write(root.join("empty.txt"), []).unwrap();
    fs::write(
        root.join("spec.md"),
        b"Text mentions BTDX TES4 DDS BSA Gamebryo File Format RIFF in the middle.",
    )
    .unwrap();
    for (i, &(signature, _)) in SIGNATURE_CASES.iter().enumerate() {
        fs::write(
            root.join(format!("partial-{i}.txt")),
            &signature[..signature.len() - 1],
        )
        .unwrap();
        let mut offset_one = b"x".to_vec();
        offset_one.extend(signature);
        fs::write(root.join(format!("offset-one-{i}.txt")), offset_one).unwrap();
    }
    require_clean_repository(&root).unwrap();
}

#[test]
fn only_the_five_generated_root_directories_are_skipped() {
    let root = TestDir::new("guard-skip");
    for name in ROOT_SKIP_CASES {
        let directory = root.join(name).join("deep");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("generated.ba2"), b"BTDX").unwrap();
    }
    require_clean_repository(&root).unwrap();
    // A nested directory with an exempt name is not one of the ignored root paths.
    fs::create_dir_all(root.join("crates/target")).unwrap();
    fs::write(root.join("crates/target/generated.ba2"), []).unwrap();
    fs::create_dir(root.join(".hidden")).unwrap();
    fs::write(root.join(".hidden/.shadow-copy.esm"), []).unwrap();
    fs::write(root.join("data.local.toml"), b"BTDX").unwrap();
    assert_eq!(
        scan_repository(&root).unwrap(),
        vec![
            Violation {
                path: ".hidden/.shadow-copy.esm".into(),
                reason: Reason::Extension("esm")
            },
            Violation {
                path: "crates/target/generated.ba2".into(),
                reason: Reason::Extension("ba2")
            },
            Violation {
                path: "data.local.toml".into(),
                reason: Reason::Signature("BA2")
            },
        ]
    );
}

#[test]
fn exempt_names_do_not_hide_regular_files() {
    let root = TestDir::new("guard-skip-file");
    for name in ROOT_SKIP_CASES {
        fs::write(root.join(name), b"BTDX").unwrap();
    }
    let violations = scan_repository(&root).unwrap();
    assert_eq!(violations.len(), 5);
    assert!(violations
        .iter()
        .all(|v| v.reason == Reason::Signature("BA2")));
}

#[test]
fn fake_workspace_with_archive_fails_the_same_gate_as_the_repository() {
    let root = TestDir::new("guard-fake-workspace");
    fs::write(root.join("Cargo.toml"), b"[workspace]\nmembers = []\n").unwrap();
    require_clean_repository(&root).unwrap();
    fs::write(root.join("copied.ba2"), b"synthetic").unwrap();
    let error = require_clean_repository(&root).unwrap_err().to_string();
    assert!(error.contains("GameFilesInRepository"));
    assert!(error.contains("copied.ba2"));
    assert!(error.contains("extension .ba2"));
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_rejected_without_following_targets() {
    let root = TestDir::new("guard-links");
    std::os::unix::fs::symlink("missing", root.join("file-link")).unwrap();
    std::os::unix::fs::symlink(".", root.join("directory-link")).unwrap();
    assert_eq!(
        scan_repository(&root).unwrap(),
        vec![
            Violation {
                path: "directory-link".into(),
                reason: Reason::SymbolicLink
            },
            Violation {
                path: "file-link".into(),
                reason: Reason::SymbolicLink
            },
        ]
    );
}

#[test]
fn scan_errors_do_not_certify_an_unchecked_root() {
    let root = TestDir::new("guard-errors");
    assert!(require_clean_repository(&root.join("missing")).is_err());
    let file = root.join("file");
    fs::write(&file, []).unwrap();
    assert!(require_clean_repository(&file).is_err());
}

#[test]
fn versioned_machine_paths_are_rejected_across_read_boundaries() {
    let root = TestDir::new("guard-machine-path");
    let needle = concat!("/", "Users/");
    let mut bytes = vec![b'x'; 8190];
    bytes.extend_from_slice(needle.as_bytes());
    bytes.extend_from_slice(b"synthetic/project");
    fs::write(root.join("tracked.md"), bytes).unwrap();
    fs::write(root.join("untracked.md"), needle).unwrap();
    let violations = guard::scan_machine_paths(&root, &["tracked.md".into()]).unwrap();
    assert_eq!(
        violations,
        vec![Violation {
            path: "tracked.md".into(),
            reason: Reason::MachinePath
        }]
    );
    fs::write(root.join("tracked.md"), b"$MEASUREMENT_ROOT/docs").unwrap();
    assert!(guard::scan_machine_paths(&root, &["tracked.md".into()])
        .unwrap()
        .is_empty());
}
