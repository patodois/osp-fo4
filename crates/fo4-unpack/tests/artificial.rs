use fo4_core::{digest_bytes, Limits, Manifest, Mount, PluginSpec};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};
struct Install {
    base: fo4_core::TestDir,
    root: PathBuf,
    manifest: PathBuf,
}
fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = sig.to_vec();
    v.extend((data.len() as u16).to_le_bytes());
    v.extend(data);
    v
}
fn record(sig: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    let mut out = sig.to_vec();
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(id.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(131u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(data);
    out
}
fn plugin(master: Option<&str>, flags: u32, count: u32, body: &[u8]) -> Vec<u8> {
    let mut h = 0.95f32.to_le_bytes().to_vec();
    h.extend(count.to_le_bytes());
    h.extend(0x900u32.to_le_bytes());
    let mut payload = sub(b"HEDR", &h);
    if let Some(master) = master {
        let mut m = master.as_bytes().to_vec();
        m.push(0);
        payload.extend(sub(b"MAST", &m));
        payload.extend(sub(b"DATA", &[0; 8]));
    }
    let mut out = record(b"TES4", 0, flags, &payload);
    out.extend(body);
    out
}
fn archive(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut table = Vec::new();
    let mut payload = Vec::new();
    for (_, data, compressed) in files {
        table.extend([0; 12]);
        table.extend(0u32.to_le_bytes());
        table.extend(((24 + 36 * files.len() + payload.len()) as u64).to_le_bytes());
        let stored = if *compressed {
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(data).unwrap();
            z.finish().unwrap()
        } else {
            data.to_vec()
        };
        table.extend((if *compressed { stored.len() as u32 } else { 0 }).to_le_bytes());
        table.extend((data.len() as u32).to_le_bytes());
        table.extend(0xBAADF00Du32.to_le_bytes());
        payload.extend(stored);
    }
    let mut out = b"BTDX".to_vec();
    out.extend(8u32.to_le_bytes());
    out.extend(b"GNRL");
    out.extend((files.len() as u32).to_le_bytes());
    out.extend(((24 + table.len() + payload.len()) as u64).to_le_bytes());
    out.extend(table);
    out.extend(payload);
    for (name, _, _) in files {
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(name.as_bytes());
    }
    out
}
fn strings() -> Vec<u8> {
    let mut v = 1u32.to_le_bytes().to_vec();
    v.extend(13u32.to_le_bytes());
    v.extend(71u32.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v.extend(b"Display Name\0");
    v
}
fn setup() -> Install {
    let base = fo4_core::TestDir::new("cli");
    let root = base.join("Data");
    fs::create_dir_all(root.join("loose/meshes")).unwrap();
    let names = [
        "Fallout4.esm",
        "DLCRobot.esm",
        "DLCworkshop01.esm",
        "DLCCoast.esm",
        "DLCworkshop02.esm",
        "DLCworkshop03.esm",
        "DLCNukaWorld.esm",
    ];
    let mut fields = sub(b"EDID", b"SyntheticObject\0");
    fields.extend(sub(b"MODL", b"a.nif\0"));
    fields.extend(sub(b"FULL", &71u32.to_le_bytes()));
    let body = record(b"STAT", 0x800, 0, &fields);
    fs::write(root.join(names[0]), plugin(None, 0x81, 1, &body)).unwrap();
    for name in &names[1..] {
        fs::write(root.join(name), plugin(Some(names[0]), 1, 0, &[])).unwrap();
    }
    fs::write(
        root.join("Override.esp"),
        plugin(Some(names[0]), 0, 1, &record(b"STAT", 0x800, 0x20, &[])),
    )
    .unwrap();
    fs::write(
        root.join("a.ba2"),
        archive(&[
            ("meshes/a.nif", b"synthetic archive asset", true),
            ("Strings/Fallout4_english.STRINGS", &strings(), false),
        ]),
    )
    .unwrap();
    fs::write(
        root.join("b.ba2"),
        archive(&[("Meshes\\A.NIF", b"synthetic second archive", false)]),
    )
    .unwrap();
    fs::write(root.join("loose/meshes/a.nif"), b"synthetic loose winner").unwrap();
    let m = Manifest {
        game: fo4_core::Game::Fallout4,
        archive_invalidation: Some(fo4_core::ArchiveInvalidation {
            enabled: true,
            origin: fo4_core::InvalidationOrigin::ExplicitProfile,
        }),
        root: "Data".into(),
        language: "english".into(),
        plugins: names
            .iter()
            .copied()
            .chain(["Override.esp"])
            .map(|n| PluginSpec {
                name: n.into(),
                path: n.into(),
            })
            .collect(),
        mounts: vec![
            Mount::Archive {
                path: "a.ba2".into(),
                plugin: None,
            },
            Mount::Archive {
                path: "b.ba2".into(),
                plugin: Some("Fallout4.esm".into()),
            },
            Mount::Loose {
                base: Some("loose".into()),
                files: vec!["meshes/a.nif".into()],
                plugin: None,
            },
        ],
        required_masters: names.iter().map(|s| s.to_string()).collect(),
        excluded: vec!["HighResolutionTexturePack01.ba2".into()],
        limits: Limits::default(),
        build: Some("synthetic-only-1".into()),
    };
    let manifest = base.join("profile.json");
    fs::write(&manifest, serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    Install {
        base,
        root,
        manifest,
    }
}
fn run(i: &Install, command: &str, scope: Option<&str>, name: &str) -> (Output, Value, Vec<Value>) {
    let out = i.base.join(name);
    let mut cmd = Command::new(
        std::env::var_os("FO4_STAGE10_TEST_CLI")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_fo4-unpack").into()),
    );
    cmd.arg(command)
        .arg("--manifest")
        .arg(&i.manifest)
        .arg("--out")
        .arg(&out)
        .arg("--all")
        .arg("--items");
    if let Some(scope) = scope {
        cmd.arg("--scope").arg(scope);
    }
    let output = cmd.output().unwrap();
    assert!(out.join("summary.json").exists(), "{:?}", output);
    let summary = serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
    let items = fs::read_to_string(out.join("items.jsonl"))
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    (output, summary, items)
}
fn snapshot(root: &Path) -> Vec<(PathBuf, String)> {
    fn visit(p: &Path, out: &mut Vec<(PathBuf, String)>) {
        for entry in fs::read_dir(p).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, out);
            } else {
                out.push((path.clone(), digest_bytes(&fs::read(&path).unwrap())));
            }
        }
    }
    let mut out = Vec::new();
    visit(root, &mut out);
    out.sort();
    out
}
#[test]
fn all_four_commands_seven_masters_and_no_data_writes() {
    let i = setup();
    let before = snapshot(&i.root);
    let (o, s, items) = run(&i, "inventory", None, "inventory");
    assert!(o.status.success(), "{s}");
    assert_eq!(s["files_enumerated"], 11);
    assert!(items.iter().any(|x| x["name"] == "DLCNukaWorld.esm"));
    let (o, s, _) = run(&i, "sweep", Some("archives"), "archives");
    assert!(o.status.success(), "{s}");
    assert_eq!(s["entries_enumerated"], 3);
    assert_eq!(s["chunks_attempted"], 3);
    assert_eq!(s["chunks_valid"], 3);
    let (o, s, items) = run(&i, "resolve", None, "resolve");
    assert!(o.status.success(), "{s}");
    let assets: Vec<_> = items
        .iter()
        .filter(|x| x["candidate"]["key_hex"] == "6d65736865732f612e6e6966")
        .collect();
    assert_eq!(assets.len(), 3);
    assert_eq!(assets[2]["winner"], true);
    assert_eq!(assets[2]["sha256"], digest_bytes(b"synthetic loose winner"));
    let (o, s, items) = run(&i, "sweep", Some("records"), "records");
    assert!(o.status.success(), "{s}");
    assert_eq!(s["records"], 2);
    assert_eq!(s["strings_resolved"], 1);
    assert_eq!(s["deleted"], 1);
    assert!(items
        .iter()
        .any(|x| x["kind"] == "localized" && x["text"] == "Display Name"));
    assert!(items
        .iter()
        .any(|x| x["kind"] == "override" && x["winner"] == true && x["record"]["deleted"] == true));
    assert_eq!(snapshot(&i.root), before);
}
#[test]
fn corrupted_archive_continues_and_writes_failure_reports() {
    let i = setup();
    let mut bytes = fs::read(i.root.join("a.ba2")).unwrap();
    bytes[96] ^= 0x55;
    fs::write(i.root.join("a.ba2"), bytes).unwrap();
    let (o, s, _) = run(&i, "sweep", Some("archives"), "bad");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["chunks_attempted"], 3);
    assert_eq!(s["chunks_failed"], 1);
    assert_eq!(s["chunks_valid"], 2);
    let diagnostics = fs::read_to_string(i.base.join("bad/diagnostics.jsonl")).unwrap();
    let d: Value = serde_json::from_str(diagnostics.lines().next().unwrap()).unwrap();
    assert!(d["source"]["offset"].as_u64().unwrap() >= 96);
    assert!(!d["expected"].as_str().unwrap().is_empty());
}
#[test]
fn malformed_table_still_attempts_next_archive() {
    let i = setup();
    fs::write(i.root.join("a.ba2"), b"BTDX").unwrap();
    let (o, s, _) = run(&i, "sweep", Some("archives"), "truncated");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["files_attempted"], 2);
    assert_eq!(s["files_failed"], 1);
    assert_eq!(s["non_enumerable_regions"], 1);
    assert_eq!(s["entries_valid"], 1);
}
#[test]
fn tardy_master_and_report_inside_data_are_errors() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.plugins.swap(0, 7);
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let (o, s, _) = run(&i, "sweep", Some("records"), "order");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["failures"], 1);
    assert!(fs::read_to_string(i.base.join("order/diagnostics.jsonl"))
        .unwrap()
        .contains("MasterAfterDependent"));
    let output = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["inventory", "--manifest"])
        .arg(&i.manifest)
        .arg("--out")
        .arg(i.root.join("reports"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!i.root.join("reports").exists());
}
#[test]
fn bad_args_and_missing_order_return_usage_status() {
    let output = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["sweep", "--scope", "nonsense"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.plugins.clear();
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["inventory", "--manifest"])
        .arg(&i.manifest)
        .arg("--out")
        .arg(i.base.join("bad-order"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("LoadOrderRequired"));
}
#[test]
fn repeated_reports_are_deterministic_and_order_changes_fingerprint() {
    let i = setup();
    let (_, a, _) = run(&i, "resolve", None, "first");
    let (_, b, _) = run(&i, "resolve", None, "second");
    assert_eq!(a, b);
    assert_eq!(
        fs::read(i.base.join("first/items.jsonl")).unwrap(),
        fs::read(i.base.join("second/items.jsonl")).unwrap()
    );
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.mounts.reverse();
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let (_, c, _) = run(&i, "resolve", None, "reverse");
    assert_ne!(a["fingerprint"], c["fingerprint"]);
}

#[test]
fn record_caps_are_per_plugin() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.limits.max_records = 1;
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let (o, s, _) = run(&i, "sweep", Some("records"), "per-plugin");
    assert!(o.status.success(), "{s}");
    assert_eq!(s["records"], 2);
}
#[test]
fn unavailable_counts_missing_headers() {
    let i = setup();
    fs::write(i.root.join("DLCRobot.esm"), b"bad").unwrap();
    let (_, s, _) = run(&i, "sweep", Some("records"), "missing");
    assert_eq!(s["files_unavailable"], 1);
    assert_eq!(s["files_blocked"], 7);
}
#[cfg(target_os = "macos")]
#[test]
fn firmlink_reports_never_write_inside_data() {
    let i = setup();
    let alias = Path::new("/System/Volumes/Data")
        .join(i.root.strip_prefix("/").unwrap())
        .join("reports");
    let result = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["inventory", "--manifest"])
        .arg(&i.manifest)
        .arg("--out")
        .arg(alias)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(!i.root.join("reports").exists());
}
#[test]
fn hardlinked_report_is_never_truncated() {
    let i = setup();
    let out = i.base.join("linked-report");
    fs::create_dir(&out).unwrap();
    let original = i.root.join("Fallout4.esm");
    let before = fs::read(&original).unwrap();
    fs::hard_link(&original, out.join("diagnostics.jsonl")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["inventory", "--manifest"])
        .arg(&i.manifest)
        .arg("--out")
        .arg(out)
        .output()
        .unwrap();
    assert_eq!(fs::read(original).unwrap(), before);
    assert_eq!(result.status.code(), Some(2));
}

#[test]
fn bad_archive_entry_keeps_localized_strings_available() {
    let i = setup();
    let mut blocked = archive(&[
        ("meshes/a.nif", b"shared", false),
        ("other.nif", b"shared", false),
    ]);
    let offset = blocked[40..48].to_vec();
    blocked[76..84].copy_from_slice(&offset);
    fs::write(i.root.join("b.ba2"), blocked).unwrap();
    let (o, s, items) = run(&i, "sweep", Some("records"), "entry-local");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["strings_resolved"], 1);
    assert!(items
        .iter()
        .any(|v| v["kind"] == "localized" && v["text"] == "Display Name"));
    let (o, s, _) = run(&i, "sweep", Some("archives"), "all-chunks");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["chunks_attempted"], 4);
    assert_eq!(s["non_enumerable_regions"], 0);
}
#[test]
fn count_divergence_is_reported_without_unreadable_region() {
    let i = setup();
    let path = i.root.join("Fallout4.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes[34..38].copy_from_slice(&999u32.to_le_bytes());
    fs::write(path, bytes).unwrap();
    let (o, s, _) = run(&i, "sweep", Some("records"), "hedr-mismatch");
    assert!(o.status.success(), "{s}");
    assert_eq!(s["divergences"], 1);
    assert_eq!(s["non_enumerable_regions"], 0);
    assert_eq!(s["records"], 2);
}
#[test]
fn failed_localized_lookup_is_not_counted_as_interpreted() {
    let i = setup();
    let mut table = strings();
    table[18] = 0x81;
    fs::write(
        i.root.join("a.ba2"),
        archive(&[("Strings/Fallout4_english.STRINGS", &table, false)]),
    )
    .unwrap();
    let (o, s, items) = run(&i, "sweep", Some("records"), "raw-text");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["strings_resolved"], 0);
    assert_eq!(s["interpreted_fields"], 2);
    assert_eq!(s["uninterpreted_fields"], 1);
    assert!(items
        .iter()
        .any(|v| v["kind"] == "unresolved_string" && v["id"] == 71));
    let diagnostics = diagnostics(&i, "raw-text");
    let raw: Vec<_> = diagnostics
        .iter()
        .filter(|v| v["code"] == "InvalidStringEncoding")
        .collect();
    assert_eq!(raw.len(), 1);
    assert!(raw[0]["sample"]["raw"]
        .as_array()
        .unwrap()
        .contains(&Value::from(129)));
    assert_eq!(s["strings_by_language"]["english"]["undecoded"], 1);
}

#[test]
fn configured_target_helper() {
    let directory = fo4_core::TestDir::new("target-rule");
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    assert!(directory.starts_with(target.canonicalize().unwrap()));
}
#[test]
fn custom_target_is_honored_in_child() {
    let directory = fo4_core::TestDir::new("redirected-target");
    let target = directory.join("custom");
    fs::create_dir(&target).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .env("CARGO_TARGET_DIR", &target)
        .args(["--exact", "configured_target_helper"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(fs::read_dir(target.join("synthetic")).unwrap().count(), 0);
}
#[test]
fn memory_accounts_are_independent() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.limits.max_override_memory = 4096;
    m.limits.max_vfs_memory = 1 << 20;
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let (o, s, _) = run(&i, "sweep", Some("records"), "accounts");
    assert!(o.status.success(), "{s}");
    assert!(s["override_memory"].as_u64().unwrap() <= 4096);
    assert!(s["vfs_memory"].as_u64().unwrap() > s["override_memory"].as_u64().unwrap());
    assert_eq!(s["strings_resolved"], 1);
}

#[test]
fn structural_failure_prevents_vfs_lookup() {
    let i = setup();
    fs::write(i.root.join("b.ba2"), b"BTDX").unwrap();
    let (o, s, _) = run(&i, "sweep", Some("records"), "broken-archive");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["strings_resolved"], 0);
    assert_eq!(s["records"], 2);
    assert!(diagnostics(&i, "broken-archive")
        .iter()
        .any(|v| v["code"] == "VfsIncomplete"));
}

#[test]
fn subrecord_caps_are_per_plugin() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.limits.max_subrecords = 3;
    fs::write(
        i.root.join("Override.esp"),
        plugin(
            Some("Fallout4.esm"),
            0,
            1,
            &record(b"STAT", 0x800, 0x20, &sub(b"EDID", b"deleted metadata\0")),
        ),
    )
    .unwrap();
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let (o, s, _) = run(&i, "sweep", Some("records"), "per-plugin-subrecords");
    assert!(o.status.success(), "{s}");
    assert_eq!(s["subrecords"], 4);
}

#[test]
fn refused_creation_rolls_back_earlier_reports() {
    let i = setup();
    let out = i.base.join("atomic");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("items.jsonl"), b"occupied").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["inventory", "--manifest"])
        .arg(&i.manifest)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(!out.join("summary.json").exists());
    assert!(!out.join("diagnostics.jsonl").exists());
    assert_eq!(fs::read(out.join("items.jsonl")).unwrap(), b"occupied");
}

fn diagnostics(i: &Install, name: &str) -> Vec<Value> {
    fs::read_to_string(i.base.join(name).join("diagnostics.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn reports_are_bounded_and_string_diagnostics_are_not_duplicated() {
    let i = setup();
    let mut table = 200u32.to_le_bytes().to_vec();
    table.extend(100_000u32.to_le_bytes());
    for id in 0..200u32 {
        table.extend(id.to_le_bytes());
        table.extend(0u32.to_le_bytes());
    }
    table.extend(vec![b'x'; 100_000]);
    fs::write(
        i.root.join("a.ba2"),
        archive(&[("Strings/Fallout4_english.STRINGS", &table, false)]),
    )
    .unwrap();
    let (o, s, items) = run(&i, "sweep", Some("records"), "bounded-raw");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["string_entries_undecoded"], 200);
    let ds = diagnostics(&i, "bounded-raw");
    let strings: Vec<_> = ds
        .iter()
        .filter(|v| v["code"] == "UnterminatedString")
        .collect();
    assert_eq!(strings.len(), 1);
    assert_eq!(strings[0]["count"], 200); // One group, each bad entry counted once.
    for d in strings {
        assert_eq!(d["sample"]["raw"].as_array().unwrap().len(), 64);
        assert_eq!(d["sample"]["length"], 100_000);
        assert_eq!(d["sample"]["offset"], 1608);
    }
    assert!(items.iter().all(|v| v.get("raw").is_none()));
    assert!(s["report_bytes"].as_u64().unwrap() < 200_000);
}

#[test]
fn injection_requires_completed_base_and_is_counted() {
    for truncated in [false, true] {
        let i = setup();
        let mut base = plugin(None, 1, 0, &[]);
        if truncated {
            base.extend(b"STAT");
        }
        fs::write(i.root.join("Fallout4.esm"), base).unwrap();
        fs::write(
            i.root.join("Override.esp"),
            plugin(Some("Fallout4.esm"), 0, 1, &record(b"STAT", 0x900, 0, &[])),
        )
        .unwrap();
        let (o, s, items) = run(&i, "sweep", Some("records"), "injected");
        assert_eq!(s["injected_records"], if truncated { 0 } else { 1 });
        if truncated {
            assert_eq!(o.status.code(), Some(1));
            assert!(diagnostics(&i, "injected")
                .iter()
                .any(|d| d["code"] == "BaseUnavailable"));
        } else {
            assert!(o.status.success(), "{s}");
            assert!(items
                .iter()
                .any(|v| v["kind"] == "override" && v["record"]["injected"] == true));
        }
    }
}

#[test]
fn fingerprint_scopes_and_missing_plugin_resolve() {
    let i = setup();
    let (_, assets, _) = run(&i, "resolve", None, "assets-before");
    let (_, records, _) = run(&i, "sweep", Some("records"), "records-before");
    let path = i.root.join("Fallout4.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes[16] = 1; // Record revision, same structure and semantic values.
    fs::write(&path, bytes).unwrap();
    let (_, assets_after, _) = run(&i, "resolve", None, "assets-after");
    let (_, records_after, _) = run(&i, "sweep", Some("records"), "records-after");
    assert_eq!(assets["fingerprint_scope"], "assets");
    assert_eq!(records["fingerprint_scope"], "assets+plugins");
    assert_eq!(assets["fingerprint"], assets_after["fingerprint"]);
    assert_ne!(records["fingerprint"], records_after["fingerprint"]);
    fs::remove_file(path).unwrap();
    let (o, s, items) = run(&i, "resolve", None, "missing-plugin");
    assert!(o.status.success(), "{s}");
    assert!(items.iter().any(|v| v["winner"] == true));
}

#[test]
fn manifest_condition_and_origin_reach_report() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    for enabled in [false, true] {
        m.archive_invalidation = Some(fo4_core::ArchiveInvalidation {
            enabled,
            origin: fo4_core::InvalidationOrigin::Ini {
                file: "Fallout4Custom.ini".into(),
                line: 7,
            },
        });
        fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
        let name = if enabled {
            "invalidation-on"
        } else {
            "invalidation-off"
        };
        let (o, s, items) = run(&i, "resolve", None, name);
        assert_eq!(s["archive_invalidation"]["origin"]["line"], 7);
        assert_eq!(s["archive_invalidation"]["enabled"], enabled);
        if enabled {
            assert!(o.status.success());
            assert!(items
                .iter()
                .any(|v| v["winner"] == true && v["candidate"]["location"]["kind"] == "loose"));
        } else {
            assert_eq!(o.status.code(), Some(1));
            assert!(diagnostics(&i, name)
                .iter()
                .any(|v| v["code"] == "UnsupportedMountRule"));
            assert!(!items
                .iter()
                .any(|v| v["winner"] == true
                    && v["candidate"]["key_hex"] == "6d65736865732f612e6e6966"));
        }
    }
}

#[test]
fn resolve_never_certifies_a_shadowed_loser() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.mounts.truncate(2);
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let mut high = archive(&[
        ("meshes/a.nif", b"MODDED!", false),
        ("other.nif", b"MODDED!", false),
    ]);
    let offset = high[40..48].to_vec();
    high[76..84].copy_from_slice(&offset); // Deliberately shared payload.
    fs::write(i.root.join("b.ba2"), high).unwrap();
    let (o, _, items) = run(&i, "resolve", None, "blocked");
    assert_eq!(o.status.code(), Some(1));
    let chain: Vec<_> = items
        .iter()
        .filter(|v| v["candidate"]["key_hex"] == "6d65736865732f612e6e6966")
        .collect();
    assert_eq!(chain.len(), 2);
    assert!(chain.iter().all(|v| v["winner"] == false));
    let ds = diagnostics(&i, "blocked");
    let error = ds
        .iter()
        .find(|v| v["code"] == "WinnerUnsupported")
        .unwrap();
    assert_eq!(error["source"]["archive_entry"], "meshes/a.nif");
    assert!(error["source"]["file"].as_str().unwrap().ends_with("b.ba2"));
    fs::write(i.root.join("b.ba2"), b"BTDX").unwrap();
    let (o, _, items) = run(&i, "resolve", None, "incomplete");
    assert_eq!(o.status.code(), Some(1));
    assert!(items.iter().all(|v| v["winner"] == false));
    assert!(diagnostics(&i, "incomplete")
        .iter()
        .any(|v| v["code"] == "VfsIncomplete"));
}

#[test]
fn scan_counts_decoding_by_language() {
    for (language, text, expected) in [
        ("en", b"Caf\xe9".as_slice(), "Café"),
        ("fr", "été 日本語".as_bytes(), "été 日本語"),
    ] {
        let i = setup();
        let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
        m.language = language.into();
        fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
        let mut table = 1u32.to_le_bytes().to_vec();
        table.extend(((text.len() + 1) as u32).to_le_bytes());
        table.extend(71u32.to_le_bytes());
        table.extend(0u32.to_le_bytes());
        table.extend(text);
        table.push(0);
        fs::write(
            i.root.join("a.ba2"),
            archive(&[(
                &format!("Strings/Fallout4_{language}.STRINGS"),
                &table,
                false,
            )]),
        )
        .unwrap();
        let (o, s, items) = run(&i, "sweep", Some("records"), "language");
        assert!(o.status.success(), "{s}");
        assert_eq!(s["strings_by_language"][language]["decoded"], 1);
        assert_eq!(s["strings_by_language"][language]["undecoded"], 0);
        assert!(items
            .iter()
            .any(|v| v["kind"] == "localized" && v["text"] == expected));
    }
}

#[test]
fn too_small_budget_is_refused_before_output() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.limits.max_report_bytes = 2048;
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let out = i.base.join("too-small");
    let o = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["inventory", "--manifest"])
        .arg(&i.manifest)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8(o.stderr)
        .unwrap()
        .contains("ReportBudgetTooSmall"));
    assert!(!out.exists());
}

#[cfg(unix)]
#[test]
fn symlinks_to_data_subdirectories_never_create_reports() {
    for intermediate in [false, true] {
        let i = setup();
        let sub = i.root.join("sub");
        fs::create_dir(&sub).unwrap();
        let link = i.base.join("sub-link");
        std::os::unix::fs::symlink(&sub, &link).unwrap();
        let before = snapshot(&i.root);
        let out = link.join(if intermediate {
            "newdir/reports"
        } else {
            "reports"
        });
        let result = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
            .args(["inventory", "--manifest"])
            .arg(&i.manifest)
            .arg("--out")
            .arg(out)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert_eq!(
            fs::read_dir(&sub).unwrap().count(),
            0,
            "no directory or file may be created in synthetic Data"
        );
        assert_eq!(snapshot(&i.root), before);
    }
}

fn string_entries(entries: &[(u32, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut directory = Vec::new();
    for (id, text) in entries {
        directory.extend(id.to_le_bytes());
        directory.extend((data.len() as u32).to_le_bytes());
        data.extend(*text);
        data.push(0);
    }
    let mut bytes = (entries.len() as u32).to_le_bytes().to_vec();
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(directory);
    bytes.extend(data);
    bytes
}

#[test]
fn bom_counts_per_language_preserve_decoded_prefix() {
    for (language, expected) in [("en", "ï»¿BOM"), ("fr", "\u{feff}BOM")] {
        let i = setup();
        let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
        m.language = language.into();
        fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
        let table = string_entries(&[(71, b"\xef\xbb\xbfBOM"), (72, b"plain")]);
        fs::write(
            i.root.join("a.ba2"),
            archive(&[(
                &format!("Strings/Fallout4_{language}.STRINGS"),
                &table,
                false,
            )]),
        )
        .unwrap();
        let (o, s, items) = run(&i, "sweep", Some("records"), "bom");
        assert!(o.status.success(), "{s}");
        assert_eq!(s["strings_by_language"][language]["bom_prefixed"], 1);
        assert_eq!(s["strings_by_language"][language]["decoded"], 2);
        assert!(items
            .iter()
            .any(|v| v["kind"] == "localized" && v["text"] == expected));
    }
}

#[test]
fn review_observation_english_utf8_candidates_are_counted_without_codec_change() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.language = "en".into();
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let table = string_entries(&[
        (71, b"Caf\xc3\xa9"),
        (72, b"plain"),
        (73, b"Caf\xe9"),
        (74, b"\xc0\xaf"),
        (75, b"\xd4\x81"),
    ]);
    fs::write(
        i.root.join("a.ba2"),
        archive(&[("Strings/Fallout4_en.STRINGS", &table, false)]),
    )
    .unwrap();
    let (o, s, items) = run(&i, "sweep", Some("records"), "utf8-candidates");
    assert_eq!(o.status.code(), Some(1)); // CP1252 byte 0x81 remains invalid.
    assert_eq!(s["strings_by_language"]["en"]["english_utf8_multibyte"], 2);
    assert_eq!(s["strings_by_language"]["en"]["decoded"], 4);
    assert_eq!(s["strings_by_language"]["en"]["undecoded"], 1);
    assert!(items
        .iter()
        .any(|v| v["kind"] == "localized" && v["text"] == "CafÃ©"));
}

#[test]
fn resolve_marks_all_known_candidates_incomplete_for_unusable_name() {
    let i = setup();
    fs::write(
        i.root.join("b.ba2"),
        archive(&[("meshes/./a.nif", b"raw", false)]),
    )
    .unwrap();
    let (o, _, items) = run(&i, "resolve", None, "unusable-name");
    assert_eq!(o.status.code(), Some(1));
    assert!(items
        .iter()
        .any(|v| v["candidate"]["blocked"] == true && v["candidate"]["key_hex"].is_null()));
    assert!(items
        .iter()
        .all(|v| v["winner"] == false && v["resolution"] == "incomplete"));
    assert!(diagnostics(&i, "unusable-name")
        .iter()
        .any(|v| v["code"] == "VfsIncomplete"));
}

#[cfg(unix)]
#[test]
fn existing_private_empty_output_is_refused_unchanged() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let i = setup();
    let out = i.base.join("private-report");
    fs::create_dir(&out).unwrap();
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).unwrap();
    let identity = |m: fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.mode(),
            m.uid(),
            m.gid(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )
    };
    let before = identity(fs::metadata(&out).unwrap());
    let data_before = snapshot(&i.root);
    let result = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["inventory", "--manifest"])
        .arg(&i.manifest)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("ReportDirectoryExists"));
    assert_eq!(identity(fs::metadata(&out).unwrap()), before);
    assert_eq!(fs::metadata(&out).unwrap().mode() & 0o7777, 0o700);
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    assert!(fs::read_dir(i.base.to_path_buf())
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".fo4-report-stage-")));
    assert_eq!(snapshot(&i.root), data_before);
}

#[test]
fn default_is_aggregated_and_items_are_explicit() {
    let i = setup();
    for (command, scope) in [
        ("inventory", None),
        ("resolve", None),
        ("sweep", Some("archives")),
        ("sweep", Some("records")),
    ] {
        let out = i.base.join(format!("aggregate-{command}-{scope:?}"));
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"));
        cmd.arg(command)
            .arg("--manifest")
            .arg(&i.manifest)
            .arg("--out")
            .arg(&out)
            .arg("--all");
        if let Some(scope) = scope {
            cmd.args(["--scope", scope]);
        }
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "{:?}", o);
        assert_eq!(fs::metadata(out.join("items.jsonl")).unwrap().len(), 0);
        let s: Value =
            serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
        if scope == Some("records") {
            assert_eq!(s["records"], 2);
            assert_eq!(s["records_by_plugin"]["Fallout4.esm"]["records"], 1);
            assert_eq!(s["histograms"]["subrecord.FULL"], 1);
            assert_eq!(s["override_records"], 1);
        }
    }
}

#[test]
fn report_limit_does_not_stop_enumeration_or_drop_counts() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.limits.max_report_bytes = 8 << 20;
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let mut body = Vec::new();
    for id in 0..12_000 {
        body.extend(record(
            b"STAT",
            0x800 + id,
            0,
            &sub(b"EDID", b"synthetic\0"),
        ));
    }
    fs::write(i.root.join("Fallout4.esm"), plugin(None, 1, 12_000, &body)).unwrap();
    let (o, s, _) = run(&i, "sweep", Some("records"), "complete-census");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["report_limit_exceeded"], true);
    assert_eq!(s["records"], 12_001);
    assert_eq!(s["records_by_plugin"]["Fallout4.esm"]["records"], 12_000);
    assert_eq!(s["records_by_plugin"]["Fallout4.esm"]["complete"], true);
    assert_eq!(s["histograms"]["record.STAT"], 12_001);
    assert_eq!(s["subrecords"], 12_000);
    let bytes: u64 = ["summary.json", "diagnostics.jsonl", "items.jsonl"]
        .iter()
        .map(|p| {
            fs::metadata(i.base.join("complete-census").join(p))
                .unwrap()
                .len()
        })
        .sum();
    assert!(bytes <= 8 << 20);
    assert_eq!(s["report_bytes"], bytes);
    assert_eq!(
        diagnostics(&i, "complete-census")
            .iter()
            .filter(|v| v["code"] == "ReportLimitExceeded")
            .count(),
        1
    );
}

#[test]
fn english_alias_and_all_mounted_localization_formats_are_counted() {
    let i = setup();
    let simple = string_entries(&[(71, b"Name")]);
    let mut length_prefixed = 1u32.to_le_bytes().to_vec();
    length_prefixed.extend(9u32.to_le_bytes());
    length_prefixed.extend(71u32.to_le_bytes());
    length_prefixed.extend(0u32.to_le_bytes());
    length_prefixed.extend(5u32.to_le_bytes());
    length_prefixed.extend(b"Name\0");
    fs::write(
        i.root.join("a.ba2"),
        archive(&[
            ("Strings/Fallout4_en.STRINGS", &simple, false),
            ("Strings/Fallout4_en.DLSTRINGS", &length_prefixed, false),
            ("Strings/Fallout4_en.ILSTRINGS", &length_prefixed, false),
            ("Strings/Fallout4_fr.STRINGS", &simple, false),
        ]),
    )
    .unwrap();
    let (o, s, items) = run(&i, "sweep", Some("records"), "all-locales");
    assert!(o.status.success(), "{s}");
    assert_eq!(s["string_tables_loaded"], 4);
    assert_eq!(s["string_entries_decoded"], 4);
    assert_eq!(s["strings_by_language"]["en"]["tables"], 3);
    assert_eq!(s["strings_by_language"]["fr"]["tables"], 1);
    assert!(items
        .iter()
        .any(|v| v["kind"] == "localized" && v["text"] == "Name"));
}

fn sse_bsa(files: &[(&[u8], &[u8])]) -> Vec<u8> {
    let folder = b"strings\0";
    let names: Vec<u8> = files
        .iter()
        .flat_map(|(n, _)| n.iter().copied().chain([0]))
        .collect();
    let data_start = 36 + 24 + 1 + folder.len() + 16 * files.len() + names.len();
    let mut out = b"BSA\0".to_vec();
    for n in [
        105,
        36,
        3,
        1,
        files.len() as u32,
        folder.len() as u32,
        names.len() as u32,
        0,
    ] {
        out.extend(n.to_le_bytes());
    }
    out.extend(0u64.to_le_bytes());
    out.extend((files.len() as u32).to_le_bytes());
    out.extend([0; 12]);
    out.push(folder.len() as u8);
    out.extend(folder);
    let mut offset = data_start;
    for (_, data) in files {
        out.extend(0u64.to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += data.len();
    }
    out.extend(names);
    for (_, data) in files {
        out.extend(*data);
    }
    out
}
#[test]
fn sse_four_commands_use_explicit_profile_and_bsa() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.game = fo4_core::Game::SkyrimSe;
    m.plugins.clear();
    m.required_masters.clear();
    m.mounts.clear();
    let mut fields = sub(b"FULL", &71u32.to_le_bytes());
    fields.extend(sub(b"DATA", &[1, 2, 3]));
    for name in m.game.fixed_masters() {
        let mut body = if *name == "Skyrim.esm" {
            record(b"STAT", 0x800, 0, &fields)
        } else {
            vec![]
        };
        if !body.is_empty() {
            body[20..22].copy_from_slice(&44u16.to_le_bytes());
        }
        let mut p = plugin(
            None,
            fo4_plugin::MASTER | fo4_plugin::LOCALIZED,
            if body.is_empty() { 0 } else { 1 },
            &body,
        );
        p[20..22].copy_from_slice(&44u16.to_le_bytes());
        p[30..34].copy_from_slice(&1.7f32.to_le_bytes());
        fs::write(i.root.join(name), p).unwrap();
        m.plugins.push(PluginSpec {
            name: (*name).into(),
            path: (*name).into(),
        });
        m.required_masters.push((*name).into());
    }
    let mut extended = sub(b"XXXX", &4u32.to_le_bytes());
    extended.extend(b"ZZZZ");
    extended.extend(0u16.to_le_bytes());
    extended.extend([1, 2, 3, 4]);
    let mut body = record(b"STAT", 0x800, fo4_plugin::DELETED, &extended);
    body[20..22].copy_from_slice(&44u16.to_le_bytes());
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&body[24..]).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut packed = (body.len() as u32 - 24).to_le_bytes().to_vec();
    packed.extend(compressed);
    body = record(
        b"STAT",
        0x800,
        fo4_plugin::DELETED | fo4_plugin::COMPRESSED,
        &packed,
    );
    body[20..22].copy_from_slice(&44u16.to_le_bytes());
    let mut group = b"GRUP".to_vec();
    group.extend((body.len() as u32 + 24).to_le_bytes());
    group.extend(b"STAT");
    group.extend(0u32.to_le_bytes());
    group.extend([0; 8]);
    group.extend(body);
    let mut override_plugin = plugin(Some("Skyrim.esm"), 0, 2, &group);
    override_plugin[20..22].copy_from_slice(&44u16.to_le_bytes());
    override_plugin[30..34].copy_from_slice(&1.71f32.to_le_bytes());
    fs::write(i.root.join("SseOverride.esp"), override_plugin).unwrap();
    m.plugins.push(PluginSpec {
        name: "SseOverride.esp".into(),
        path: "SseOverride.esp".into(),
    });
    let table = string_entries(&[(71, b"Caf\xe9")]);
    let utf8 = string_entries(&[(71, "Café".as_bytes())]);
    let mut lp = 1u32.to_le_bytes().to_vec();
    lp.extend(9u32.to_le_bytes());
    lp.extend(71u32.to_le_bytes());
    lp.extend(0u32.to_le_bytes());
    lp.extend(5u32.to_le_bytes());
    lp.extend(b"Name\0");
    fs::write(
        i.root.join("test.bsa"),
        sse_bsa(&[
            (b"Skyrim_english.strings", &table),
            (b"Skyrim_french.strings", &utf8),
            (b"Skyrim_english.dlstrings", &lp),
            (b"Skyrim_english.ilstrings", &lp),
        ]),
    )
    .unwrap();
    m.mounts.push(Mount::Archive {
        path: "test.bsa".into(),
        plugin: None,
    });
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    for (command, scope, name) in [
        ("inventory", None, "sse-inventory"),
        ("sweep", Some("archives"), "sse-archives"),
        ("resolve", None, "sse-resolve"),
        ("sweep", Some("records"), "sse-records"),
    ] {
        let (output, summary, _) = run(&i, command, scope, name);
        assert!(
            output.status.success(),
            "{summary}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(summary["game"], "skyrim-se");
        assert_eq!(summary["failures"], 0);
        if scope == Some("archives") {
            assert_eq!(summary["entries_valid"], 4);
        }
        if scope == Some("records") {
            assert_eq!(summary["records"], 2);
            assert_eq!(summary["deleted"], 1);
            assert_eq!(summary["groups"], 1);
            assert_eq!(summary["strings_resolved"], 1);
            assert_eq!(summary["strings_by_language"]["english"]["decoded"], 3);
            assert_eq!(summary["strings_by_language"]["french"]["decoded"], 1);
        }
        let destination = i.base.join(format!("{name}-aggregate"));
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"));
        cmd.arg(command)
            .arg("--manifest")
            .arg(&i.manifest)
            .arg("--out")
            .arg(&destination)
            .arg("--all");
        if let Some(scope) = scope {
            cmd.arg("--scope").arg(scope);
        }
        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::metadata(destination.join("items.jsonl")).unwrap().len(),
            0
        );
        let aggregate: Value =
            serde_json::from_slice(&fs::read(destination.join("summary.json")).unwrap()).unwrap();
        assert_eq!(aggregate["game"], "skyrim-se");
        assert_eq!(aggregate["max_report_bytes"], 64 << 20);
        assert_eq!(aggregate["failures"], 0);
    }
    // Structurally unknown later archives cannot promote a lower-priority asset.
    let mut broken = fs::read(i.root.join("test.bsa")).unwrap();
    broken[4..8].copy_from_slice(&104u32.to_le_bytes());
    fs::write(i.root.join("broken.bsa"), broken).unwrap();
    m.mounts.push(Mount::Archive {
        path: "broken.bsa".into(),
        plugin: None,
    });
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let (output, summary, _) = run(&i, "resolve", None, "sse-incomplete");
    assert!(!output.status.success());
    assert!(summary["failures"].as_u64().unwrap() > 0);
    let d = diagnostics(&i, "sse-incomplete");
    assert!(d.iter().any(|d| d["code"] == "UnsupportedBsaVersion"));
}

#[test]
fn cli_accepts_asset_sweeps() {
    for scope in ["nif", "materials", "textures"] {
        for args in [
            vec![
                "sweep",
                "--scope",
                scope,
                "--manifest",
                "profile.json",
                "--out",
                "reports/new",
            ],
            vec![
                "sweep",
                scope,
                "--manifest",
                "profile.json",
                "--out",
                "reports/new",
            ],
        ] {
            let options = fo4_unpack::Options::parse(args.into_iter().map(String::from)).unwrap();
            assert_eq!(options.scope.as_deref(), Some(scope));
        }
    }
}

fn asset_node_nif(bs: u32) -> Vec<u8> {
    let mut node = Vec::new();
    for n in [u32::MAX, 0, u32::MAX, 0] {
        node.extend(n.to_le_bytes());
    }
    for n in [0f32, 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.] {
        node.extend(n.to_le_bytes());
    }
    node.extend(u32::MAX.to_le_bytes());
    node.extend(0u32.to_le_bytes());
    if bs < 130 {
        node.extend(0u32.to_le_bytes());
    }
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    out.extend(0x14020007u32.to_le_bytes());
    out.push(1);
    for n in [12, 1, bs] {
        out.extend(n.to_le_bytes());
    }
    out.extend(vec![0; if bs == 130 { 4 } else { 3 }]);
    out.extend(1u16.to_le_bytes());
    out.extend(6u32.to_le_bytes());
    out.extend(b"NiNode");
    out.extend(0u16.to_le_bytes());
    out.extend((node.len() as u32).to_le_bytes());
    for _ in 0..3 {
        out.extend(0u32.to_le_bytes());
    }
    out.extend(node);
    out.extend(1u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out
}
fn asset_dds() -> Vec<u8> {
    let mut h = [0u32; 31];
    h[0] = 124;
    h[1] = 0x1007;
    h[2] = 1;
    h[3] = 1;
    h[18] = 32;
    h[19] = 4;
    h[20] = u32::from_le_bytes(*b"DXT1");
    h[26] = 0x1000;
    let mut out = b"DDS ".to_vec();
    for v in h {
        out.extend(v.to_le_bytes());
    }
    out.extend([0; 8]);
    out
}
#[test]
fn asset_commands_both_games_and_truncated_material_is_explicit() {
    for (game, bs) in [
        (fo4_core::Game::Fallout4, 130),
        (fo4_core::Game::SkyrimSe, 100),
    ] {
        let i = setup();
        let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
        m.game = game;
        let nif = asset_node_nif(bs);
        let texture = asset_dds();
        let material = b"BGSM\x02\0\0\0payload-without-claimed-layout";
        // This test uses BA2 for FO4; SSE's own BSA route is exercised by case8 and the reader tests.
        if game == fo4_core::Game::SkyrimSe {
            fs::create_dir_all(i.root.join("assets/models")).unwrap();
            fs::create_dir_all(i.root.join("assets/textures")).unwrap();
            fs::create_dir_all(i.root.join("assets/materials")).unwrap();
            fs::write(i.root.join("assets/models/a.nif"), &nif).unwrap();
            fs::write(i.root.join("assets/textures/a.dds"), &texture).unwrap();
            fs::write(i.root.join("assets/materials/a.bgsm"), material).unwrap();
            m.mounts = vec![Mount::Loose {
                base: Some("assets".into()),
                files: vec![
                    "models/a.nif".into(),
                    "textures/a.dds".into(),
                    "materials/a.bgsm".into(),
                ],
                plugin: None,
            }];
        } else {
            fs::write(
                i.root.join("assets.ba2"),
                archive(&[
                    ("models/a.nif", &nif, true),
                    ("textures/a.dds", &texture, true),
                    ("materials/a.bgsm", material, true),
                ]),
            )
            .unwrap();
            m.mounts = vec![Mount::Archive {
                path: "assets.ba2".into(),
                plugin: None,
            }];
        }
        fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
        let before = snapshot(&i.root);
        for scope in ["nif", "textures", "materials"] {
            let (output, s, _) = run(&i, "sweep", Some(scope), scope);
            assert_eq!(s["files_enumerated"], 1);
            if scope == "materials" {
                assert_eq!(output.status.code(), Some(1));
                assert!(diagnostics(&i, scope)
                    .iter()
                    .any(|d| d["code"] == "Truncated"));
            } else {
                assert!(output.status.success(), "{s}");
                assert_eq!(s["files_valid"], 1);
                if scope == "nif" {
                    assert_eq!(s["blocks"], 1);
                } else {
                    assert_eq!(s["texture_surfaces"], 1);
                    assert_eq!(s["decoded_texture_bytes"], 16);
                }
            }
        }
        assert_eq!(snapshot(&i.root), before);
    }
}

#[test]
fn failure_code_counts_survive_diagnostic_group_cap() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    let names: Vec<_> = (0..4200).map(|i| format!("materials/m{i}.bgsm")).collect();
    let payload = b"BGSM\x02\0\0\0raw";
    let files: Vec<_> = names
        .iter()
        .map(|n| (n.as_str(), payload.as_slice(), false))
        .collect();
    fs::write(i.root.join("material-census.ba2"), archive(&files)).unwrap();
    m.mounts = vec![Mount::Archive {
        path: "material-census.ba2".into(),
        plugin: None,
    }];
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let (output, s, _) = run(&i, "sweep", Some("materials"), "code-counts");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(s["files_failed"], 4200);
    assert!(s["diagnostic_groups_omitted"].as_u64().unwrap() > 0);
    assert_eq!(s["failure_codes"]["Truncated"], 4200);
}

fn material_v2(path: &[u8], nonfinite: bool) -> Vec<u8> {
    let mut v = b"BGEM".to_vec();
    v.extend(2u32.to_le_bytes());
    v.extend(3u32.to_le_bytes());
    for f in [0f32, 0., 1., 1., 1.] {
        v.extend(f.to_le_bytes());
    }
    if nonfinite {
        v[28..32].copy_from_slice(&0x7fc12345u32.to_le_bytes());
    }
    v.push(0);
    v.extend([0; 8]);
    v.extend([128, 0]);
    v.extend([1, 1, 0, 0, 0, 0, 0, 0]);
    v.extend([0; 2]);
    v.extend(0f32.to_le_bytes());
    v.push(0);
    v.extend(1f32.to_le_bytes());
    v.push(0);
    for _ in 0..5 {
        v.extend(((path.len() + 1) as u32).to_le_bytes());
        v.extend(path);
        v.push(0);
    }
    v.extend([0; 6]);
    for _ in 0..9 {
        v.extend(1f32.to_le_bytes());
    }
    v.push(0);
    v.extend(1f32.to_le_bytes());
    v
}
fn shader_nif(path: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    for n in [0u32, 0, u32::MAX, 0, 0] {
        data.extend(n.to_le_bytes());
    }
    for f in [0f32, 0., 1., 1.] {
        data.extend(f.to_le_bytes());
    }
    data.extend(0u32.to_le_bytes());
    data.extend([3, 255, 0, 0]);
    for bits in [0x7fc12345u32, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        data.extend(bits.to_le_bytes());
    }
    for _ in 0..4 {
        data.extend(0u32.to_le_bytes());
    }
    data.extend(1f32.to_le_bytes());
    let mut v = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    v.extend(0x14020007u32.to_le_bytes());
    v.push(1);
    for n in [12u32, 1, 130] {
        v.extend(n.to_le_bytes());
    }
    v.extend([0; 4]);
    v.extend(1u16.to_le_bytes());
    let name = b"BSEffectShaderProperty";
    v.extend((name.len() as u32).to_le_bytes());
    v.extend(name);
    v.extend(0u16.to_le_bytes());
    v.extend((data.len() as u32).to_le_bytes());
    v.extend(1u32.to_le_bytes());
    v.extend((path.len() as u32).to_le_bytes());
    v.extend((path.len() as u32).to_le_bytes());
    v.extend(path);
    v.extend(0u32.to_le_bytes());
    v.extend(data);
    v.extend(1u32.to_le_bytes());
    v.extend(0u32.to_le_bytes());
    v
}
#[test]
fn asset_notices_slots_and_vfs_cross_references() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    let mat = material_v2(b"a.dds", true);
    let nif = shader_nif(b"Materials/Test.BGEM");
    let dds = asset_dds();
    fs::write(
        i.root.join("case10.ba2"),
        archive(&[
            ("materials/test.bgem", &mat, true),
            ("meshes/test.nif", &nif, true),
            ("textures/a.dds", &dds, false),
        ]),
    )
    .unwrap();
    m.mounts = vec![Mount::Archive {
        path: "case10.ba2".into(),
        plugin: None,
    }];
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let before = snapshot(&i.root);
    for scope in ["nif", "materials"] {
        let (o, s, _) = run(&i, "sweep", Some(scope), scope);
        assert!(o.status.success(), "{s}");
        assert_eq!(s["files_valid"], 1);
        assert_eq!(s["files_failed"], 0);
        assert_eq!(s["non_finite_values"], 1);
        assert_eq!(s["files_with_non_finite"], 1);
        assert!(diagnostics(&i, scope)
            .iter()
            .any(|e| e["notice"] == true && e["code"].as_str().unwrap().starts_with("NonFinite")));
        if scope == "nif" {
            assert_eq!(s["material_references"]["present"], 1);
        } else {
            assert_eq!(s["histograms"]["material.version.2"], 1);
            assert_eq!(
                s["histograms"]["material.texture_slot.BGEM.base.nonempty"],
                1
            );
            assert_eq!(s["texture_references"]["present"], 5);
            assert_eq!(s["texture_references"]["present_with_class_prefix"], 5);
        }
    }
    assert_eq!(snapshot(&i.root), before);
}
#[test]
fn unresolved_references_are_complete_deduplicated_notices() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    let mat = material_v2(b"missing.dds", false);
    let nif = shader_nif(b"missing.bgem");
    fs::write(
        i.root.join("missing.ba2"),
        archive(&[
            ("materials/test.bgem", &mat, false),
            ("meshes/test.nif", &nif, false),
        ]),
    )
    .unwrap();
    m.mounts = vec![Mount::Archive {
        path: "missing.ba2".into(),
        plugin: None,
    }];
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    for scope in ["materials", "nif"] {
        let (o, s, _) = run(&i, "sweep", Some(scope), scope);
        assert!(o.status.success(), "{s}");
        let kind = if scope == "nif" {
            "material_references"
        } else {
            "texture_references"
        };
        let count = if scope == "nif" { 1 } else { 5 };
        assert_eq!(s[kind]["missing"], count);
        assert_eq!(s[kind]["unique_unresolved"], 1);
        assert_eq!(
            s["diagnostics"],
            if scope == "nif" { count + 1 } else { count }
        );
        let d = diagnostics(&i, scope);
        let rows: Vec<_> = d
            .iter()
            .filter(|d| d["code"] == "MissingReferencedAsset")
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["count"], count);
        assert_eq!(rows[0]["notice"], true);
    }
}
#[test]
fn record_ieee_notices_preserve_values_and_plugin_counts() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    m.plugins.retain(|p| p.name != "Override.esp");
    let mut transform = Vec::new();
    for bits in [0x7fc12345u32, 0x7f800000, 0xff800000, 0, 0, 0] {
        transform.extend(bits.to_le_bytes());
    }
    let mut payload = sub(b"DATA", &transform);
    payload.extend(sub(b"XSCL", &0xffc54321u32.to_le_bytes()));
    fs::write(
        i.root.join("Fallout4.esm"),
        plugin(None, 1, 1, &record(b"REFR", 0x801, 0, &payload)),
    )
    .unwrap();
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let (o, s, items) = run(&i, "sweep", Some("records"), "ieee-records");
    assert!(o.status.success(), "{s}");
    assert_eq!(s["non_finite_values"], 4);
    assert_eq!(s["records_with_non_finite"], 1);
    assert_eq!(s["files_with_non_finite"], 1);
    assert_eq!(
        s["records_by_plugin"]["Fallout4.esm"]["non_finite_values"],
        4
    );
    let r = items.iter().find(|v| v["kind"] == "record").unwrap();
    assert_eq!(r["fields"][0]["non_finite_values"], 3);
    assert_eq!(
        r["fields"][0]["value"]["value"][0]["ieee754_bits"],
        0x7fc12345u32
    );
    assert!(diagnostics(&i, "ieee-records")
        .iter()
        .any(|e| e["code"] == "NonFiniteRecordValues" && e["notice"] == true));
}

#[test]
fn reference_work_limit_keeps_the_unchecked_denominator() {
    let i = setup();
    let mut m: Manifest = serde_json::from_slice(&fs::read(&i.manifest).unwrap()).unwrap();
    fs::create_dir_all(i.root.join("materials")).unwrap();
    fs::write(
        i.root.join("materials/limit.bgem"),
        material_v2(b"missing.dds", false),
    )
    .unwrap();
    m.mounts = vec![Mount::Loose {
        base: None,
        files: vec!["materials/limit.bgem".into()],
        plugin: None,
    }];
    m.limits.max_memory = 2048;
    fs::write(&i.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let (o, s, _) = run(&i, "sweep", Some("materials"), "reference-limit");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(s["histograms"]["material.version.2"], 1);
    assert_eq!(s["failure_codes"]["LimitExceeded"], 1);
    assert_eq!(s["texture_references"]["unchecked"], 1);
    assert_eq!(s["texture_references"]["files_not_fully_checked"], 1);
    let c = &s["texture_references"];
    assert_eq!(
        c["cited"].as_u64().unwrap(),
        ["present", "missing", "invalid", "blocked", "unchecked"]
            .iter()
            .map(|k| c[*k].as_u64().unwrap())
            .sum::<u64>()
    );
}

fn shape_nif() -> Vec<u8> {
    let mut v = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    v.extend(0x14020007u32.to_le_bytes());
    v.push(1);
    for n in [12u32, 0, 130] {
        v.extend(n.to_le_bytes());
    }
    v.extend([0; 4]);
    v.extend(0u16.to_le_bytes());
    for _ in 0..4 {
        v.extend(0u32.to_le_bytes());
    }
    v
}
fn cell_group(kind: u32, id: u32, data: &[u8]) -> Vec<u8> {
    let mut v = b"GRUP".to_vec();
    v.extend((data.len() as u32 + 24).to_le_bytes());
    v.extend(id.to_le_bytes());
    v.extend(kind.to_le_bytes());
    v.extend([0; 8]);
    v.extend(data);
    v
}
#[test]
fn cells_unread_and_paths_close_synthetic_denominators() {
    let install = setup();
    let mut m = Manifest::load(&install.manifest).unwrap();
    m.plugins.truncate(7);
    let mut cell = sub(b"EDID", b"Room\0");
    cell.extend(sub(b"FULL", b"Synthetic room\0"));
    let mut refs = Vec::new();
    for (id, base, flags) in [
        (0x802u32, 0x801u32, 0),
        (0x804, 0x803, 0),
        (0x805, 0x801, 0x20),
        (0x806, 0x801, 0x800),
    ] {
        let mut data = sub(b"NAME", &base.to_le_bytes());
        data.extend(sub(b"DATA", &[0; 24]));
        data.extend(sub(b"XSCL", &1f32.to_le_bytes()));
        refs.extend(record(b"REFR", id, flags, &data));
    }
    refs.extend(record(b"ACHR", 0x807, 0, &[]));
    let mut body = cell_group(
        2,
        0,
        &[
            record(b"CELL", 0x800, 0, &cell),
            cell_group(6, 0x800, &cell_group(9, 0x800, &refs)),
        ]
        .concat(),
    );
    body.extend(record(b"STAT", 0x801, 0, &sub(b"MODL", b"test.nif\0")));
    body.extend(record(b"LIGH", 0x803, 0, &sub(b"DATA", &[0; 64])));
    body.extend(record(b"IDLE", 0x808, 0, &sub(b"ZZZZ", b"envelope")));
    fs::write(install.root.join("Fallout4.esm"), plugin(None, 1, 0, &body)).unwrap();
    let hkx = b"\x57\xe0\xe0\x57\x10\xc0\xc0\x10hk_2014.1.0-r1\0";
    let nif = shape_nif();
    fs::write(
        install.root.join("case11.ba2"),
        archive(&[
            ("meshes/test.nif", &nif, true),
            ("meshes/actors/test.hkx", hkx, true),
            ("meshes/animtextdata/test.txt", b"synthetic text\n", false),
        ]),
    )
    .unwrap();
    m.mounts = vec![Mount::Archive {
        plugin: None,
        path: "case11.ba2".into(),
    }];
    fs::write(&install.manifest, serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    for scope in ["cells", "unread", "paths"] {
        let out = install.base.join(format!("case11-{scope}"));
        let output = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
            .args(["sweep", "--manifest"])
            .arg(&install.manifest)
            .args(["--scope", scope, "--out"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{scope}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let summary: Value =
            serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
        if scope == "cells" {
            let s = &summary["cell_census"];
            assert_eq!(s["attempted"], 1);
            assert_eq!(s["read"], 1);
            assert_eq!(s["failed"], 0);
            assert_eq!(s["ready"], 2);
            assert_eq!(s["deleted"], 1);
            assert_eq!(s["disabled"], 1);
            assert_eq!(s["excluded"], 1);
            assert!(s["candidates"].as_array().unwrap().is_empty());
        }
        if scope == "unread" {
            let u = &summary["unread"];
            assert_eq!(u["winners"], 3);
            assert_eq!(u["sampled_hkx"], 1);
            assert_eq!(u["extensions"][".hkx"]["bytes"], hkx.len() as u64);
            assert_eq!(u["hkx_version_text"]["hk_2014.1.0-r1"], 1);
            assert_eq!(u["animtext_content"]["utf8"], 1);
            assert_eq!(u["plugin_record_envelopes"]["IDLE"], 1);
        }
        assert!(fs::metadata(out.join("items.jsonl")).unwrap().len() > 0 || scope == "paths");
    }
}

#[test]
fn candidates_are_smallest_twenty_and_never_use_precombine() {
    let install = setup();
    let mut m = Manifest::load(&install.manifest).unwrap();
    m.plugins.truncate(7);
    let mut body = Vec::new();
    for i in 0..25u32 {
        let cellid = 0x1000 + i;
        let mut cell = sub(b"EDID", format!("Room{i}\0").as_bytes());
        if i == 24 {
            cell.extend(sub(b"PCMB", &[]));
        }
        let mut refs = Vec::new();
        for j in 0..(26 - i) {
            let mut data = sub(b"NAME", &0x801u32.to_le_bytes());
            data.extend(sub(b"DATA", &[0; 24]));
            data.extend(sub(b"XSCL", &1f32.to_le_bytes()));
            refs.extend(record(b"REFR", 0x2000 + i * 32 + j, 0, &data));
        }
        body.extend(cell_group(
            2,
            0,
            &[
                record(b"CELL", cellid, 0, &cell),
                cell_group(6, cellid, &cell_group(9, cellid, &refs)),
            ]
            .concat(),
        ));
    }
    body.extend(record(b"LIGH", 0x801, 0, &sub(b"DATA", &[0; 64])));
    fs::write(install.root.join("Fallout4.esm"), plugin(None, 1, 0, &body)).unwrap();
    m.mounts.clear();
    fs::write(&install.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    let out = install.base.join("case11-candidates");
    let output = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["sweep", "--scope", "cells", "--manifest"])
        .arg(&install.manifest)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let s: Value = serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
    let c = &s["cell_census"];
    assert_eq!(c["attempted"], 25);
    assert_eq!(c["read"], 25);
    assert_eq!(c["failed"], 0);
    let list = c["candidates"].as_array().unwrap();
    assert_eq!(list.len(), 20);
    assert_eq!(list[0]["key"]["object_id"], 0x1017);
    assert_eq!(list[0]["references"], 3);
    assert_eq!(list[19]["references"], 22);
    assert!(list.iter().all(|r| r["precombine"] == false));
}

fn census_install(oversized: bool) -> Install {
    let install = setup();
    let mut m = Manifest::load(&install.manifest).unwrap();
    m.plugins.truncate(7);
    let mut body = Vec::new();
    for (id, refs) in [
        (0x1000u32, vec![(0x2000u32, 0x800u32), (0x2001, 0x801)]),
        (0x1001, vec![(0x2002, 0x800)]),
        (0x1002, vec![(0x2003, 0x801)]),
    ] {
        let mut cell = sub(b"EDID", format!("Cell{id}\0").as_bytes());
        if id == 0x1000 {
            cell.extend(sub(b"CCCC", &[1, 2, 3, 4]));
        }
        let mut children = Vec::new();
        for (refid, base) in refs {
            let mut data = sub(b"NAME", &base.to_le_bytes());
            data.extend(sub(b"DATA", &[0; 24]));
            data.extend(sub(b"XSCL", &1f32.to_le_bytes()));
            if id == 0x1001 {
                data.extend(sub(b"XLKR", &[0; 8]));
            }
            children.extend(record(b"REFR", refid, 0, &data));
        }
        body.extend(cell_group(
            2,
            0,
            &[
                record(b"CELL", id, 0, &cell),
                cell_group(6, id, &cell_group(9, id, &children)),
            ]
            .concat(),
        ));
    }
    let mut stat = sub(b"MODL", b"test.nif\0");
    stat.extend(sub(b"DNAM", &[0; 12])); // case14: deliberately unverified size for sole-cause tests
    body.extend(record(b"STAT", 0x800, 0, &stat));
    body.extend(record(b"LIGH", 0x801, 0, &sub(b"DATA", &[0; 64])));
    fs::write(install.root.join("Fallout4.esm"), plugin(None, 1, 0, &body)).unwrap();
    let mut nif = shape_nif();
    if oversized {
        nif.resize(2 << 20, 0);
        m.limits.max_memory = 1 << 20;
    }
    fs::write(
        install.root.join("case12.ba2"),
        archive(&[("meshes/test.nif", &nif, true)]),
    )
    .unwrap();
    m.mounts = vec![Mount::Archive {
        plugin: None,
        path: "case12.ba2".into(),
    }];
    fs::write(&install.manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    install
}
#[test]
fn detail_sole_causes_closest_and_cell_cache_are_explicit() {
    let install = census_install(false);
    let out = install.base.join("case12-details");
    let output = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["sweep", "--scope", "cells", "--manifest"])
        .arg(&install.manifest)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success() || output.status.code() == Some(1));
    let s: Value = serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
    let c = &s["cell_census"];
    assert_eq!(c["attempted"], 3);
    assert_eq!(c["read"], 3);
    assert_eq!(c["failed"], 0);
    assert_eq!(c["ready"], 2);
    assert_eq!(c["blocked"], 2);
    assert_eq!(c["cache_instances"], 1);
    assert_eq!(c["distinct_models"], 1);
    assert!(c["cache_peak_bytes"].as_u64().unwrap() > 0);
    assert_eq!(
        c["cell_causes"]["UninterpretedVisualSubrecord:CELL.CCCC"]["instances"],
        2
    );
    assert_eq!(
        c["cell_causes"]["UninterpretedVisualSubrecord:CELL.CCCC"]["sole_cause_instances"],
        1
    );
    assert_eq!(
        c["detailed"]["UninterpretedVisualSubrecord:STAT.DNAM.unverified_size"]["instances"],
        2
    );
    assert_eq!(
        c["detailed"]["UninterpretedVisualSubrecord:STAT.DNAM.unverified_size"]
            ["sole_cause_instances"],
        2
    );
    assert!(c["detailed"]
        .get("UninterpretedVisualSubrecord:REFR.XLKR")
        .is_none());
    assert!(c["closest"].as_array().unwrap().is_empty());
    assert_eq!(c["outside_fields"]["REFR.XLKR"], 1);
    assert_eq!(c["candidates"].as_array().unwrap().len(), 1);
}
#[test]
fn resource_failure_is_infrastructure_and_next_cell_still_reads() {
    let install = census_install(true);
    let out = install.base.join("case12-cache-resource");
    let output = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["sweep", "--scope", "cells", "--manifest"])
        .arg(&install.manifest)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let s: Value = serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
    let c = &s["cell_census"];
    assert_eq!(c["attempted"], 3);
    assert_eq!(c["read"], 1);
    assert_eq!(c["failed"], 2);
    assert_eq!(c["infrastructure_failures"], 2);
    assert_eq!(c["data_failures"], 0);
    assert_eq!(c["cache_instances"], 3);
    assert_eq!(c["ready"], 1);
    assert!(c["detailed"].as_object().unwrap().is_empty());
}
#[test]
fn check_cell_is_strict_and_reports_each_blocked_instance() {
    let install = census_install(false);
    for (editor_id, success, count) in [("Cell4098", true, 1), ("Cell4096", false, 2)] {
        let out = install.base.join(format!("check-{editor_id}"));
        let output = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
            .arg("check-cell")
            .arg(&install.manifest)
            .arg(editor_id)
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let items = fs::read_to_string(out.join("items.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        let instances = items
            .iter()
            .filter(|row| row["kind"] == "checked_instance")
            .collect::<Vec<_>>();
        assert_eq!(instances.len(), count);
        assert!(instances.iter().any(|row| row["state"] == "ready"));
        if !success {
            assert!(instances.iter().any(|row| row["causes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|cause| cause["found"] == "STAT.DNAM.unverified_size")));
        }
    }
}

#[test]
fn root_survey_keeps_raw_and_class_prefixed_results_separate() {
    let install = census_install(false);
    let out = install.base.join("case12-roots");
    let output = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["sweep", "--scope", "roots", "--manifest"])
        .arg(&install.manifest)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let s: Value = serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
    let c = &s["root_census"]["classes"]["record_modl"];
    assert_eq!(c["distinct"], 1);
    assert_eq!(c["citations"], 1);
    assert_eq!(c["as_is_only"], 0);
    assert_eq!(c["prefixed_only"], 1);
    assert_eq!(c["both"], 0);
    assert_eq!(c["starts_with_root"], 0);
}

#[test]
fn cache_batch_boundary_preserves_global_distinct_counts() {
    let install = census_install(false);
    let path = install.root.join("Fallout4.esm");
    let mut bytes = fs::read(&path).unwrap();
    for i in 0..33u32 {
        let cellid = 0x5000 + i;
        let mut data = sub(b"NAME", &0x800u32.to_le_bytes());
        data.extend(sub(b"DATA", &[0; 24]));
        data.extend(sub(b"XSCL", &1f32.to_le_bytes()));
        bytes.extend(cell_group(
            2,
            0,
            &[
                record(b"CELL", cellid, 0, &[]),
                cell_group(
                    6,
                    cellid,
                    &cell_group(9, cellid, &record(b"REFR", 0x6000 + i, 0, &data)),
                ),
            ]
            .concat(),
        ));
    }
    fs::write(path, bytes).unwrap();
    let (_, s, _) = run(&install, "sweep", Some("cells"), "case12-cache-batches");
    let c = &s["cell_census"];
    assert_eq!(c["attempted"], 36);
    assert_eq!(c["read"], 36);
    assert_eq!(c["failed"], 0);
    assert_eq!(c["cache_batch_cells"], 32);
    assert_eq!(c["cache_instances"], 2);
    assert_eq!(c["distinct_models"], 1);
    assert!(c["cache_peak_bytes"].as_u64().unwrap() <= c["cache_memory_limit"].as_u64().unwrap());
}

#[test]
fn bases_ignore_cell_causes_and_publish_reference_counts() {
    let install = census_install(false);
    let out = install.base.join("case13-bases");
    let status = Command::new(env!("CARGO_BIN_EXE_fo4-unpack"))
        .args(["sweep", "--scope", "bases", "--manifest"])
        .arg(&install.manifest)
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success() || status.code() == Some(1));
    let s: Value = serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
    let c = &s["base_census"];
    assert_eq!(c["attempted"], 2);
    assert_eq!(c["ready"], 1);
    assert_eq!(c["blocked"], 1);
    assert_eq!(c["failed"], 0);
    assert_eq!(c["reference_winners"], 4);
    assert_eq!(c["references_read"], 4);
    assert_eq!(c["ready_references"], 2);
    assert_eq!(c["ready_interior_references"], 2);
    assert_eq!(
        c["causes"]["UninterpretedVisualSubrecord:STAT.DNAM.unverified_size"]["sole_bases"],
        1
    );
    assert!(c["causes"]
        .get("UninterpretedVisualSubrecord:CELL.CCCC")
        .is_none());
    let items = fs::read_to_string(out.join("items.jsonl")).unwrap();
    let ready = items
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|r| r["kind"] == "base_census" && r["state"] == "ready")
        .unwrap();
    assert_eq!(ready["record_type"], "LIGH");
    assert_eq!(ready["references"], 2);
    assert_eq!(ready["interior_references"], 2);
}

#[test]
fn base_subset_preserves_reference_counts_and_reports_explicit_scope() {
    let install = census_install(false);
    let out = install.base.join("base-subset");
    let options = fo4_unpack::Options::parse([
        "sweep".to_string(),
        "--scope".into(),
        "bases".into(),
        "--manifest".into(),
        install.manifest.to_string_lossy().into_owned(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
    ])
    .unwrap();
    let key = fo4_plugin::FormKey {
        plugin: "fallout4.esm".into(),
        object_id: 0x801,
    };
    assert_eq!(
        fo4_unpack::run_base_subset(&options, &[key.clone(), key]).unwrap(),
        0
    );
    let summary: Value =
        serde_json::from_slice(&fs::read(out.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["base_census"]["selected_bases"], 1);
    assert_eq!(summary["base_census"]["attempted"], 1);
    assert_eq!(summary["base_census"]["ready_references"], 2);
    let items = fs::read_to_string(out.join("items.jsonl")).unwrap();
    assert_eq!(
        items
            .lines()
            .filter(|line| line.contains("base_census"))
            .count(),
        1
    );
    assert!(fo4_unpack::run_base_subset(&options, &[]).is_err());
}

#[test]
fn teleport_door_prefix_is_partial_and_only_for_observed_size() {
    let install = setup();
    let mut m = Manifest::load(&install.manifest).unwrap();
    m.plugins.truncate(7);
    let mut body = Vec::new();
    for (i, size) in [35usize, 36, 37].into_iter().enumerate() {
        let mut value = vec![0xa5; size];
        value[..4].copy_from_slice(&0x900u32.to_le_bytes());
        body.extend(record(b"REFR", 0x800 + i as u32, 0, &sub(b"XTEL", &value)));
    }
    fs::write(install.root.join("Fallout4.esm"), plugin(None, 1, 0, &body)).unwrap();
    let b = fo4_core::Budget::new(16 << 20);
    let store = fo4_plugin::typed::Store::open(&m, &b).unwrap();
    for (i, size) in [35usize, 36, 37].into_iter().enumerate() {
        let k = fo4_plugin::FormKey {
            plugin: "fallout4.esm".into(),
            object_id: 0x800 + i as u32,
        };
        let r = store
            .get::<fo4_plugin::typed::Refr>(&k, &b)
            .unwrap()
            .unwrap();
        let fields: Vec<_> = r
            .data
            .common
            .ficha_fields
            .iter()
            .filter(|f| f.label == "\"Door\"")
            .collect();
        assert_eq!(fields.len(), usize::from(size == 36));
        if size == 36 {
            assert!(matches!(&fields[0].value,fo4_plugin::ficha::Value::Form(v) if v.raw==0x900));
        }
        assert!(r
            .data
            .common
            .uninterpreted
            .iter()
            .any(|s| s.signature == *b"XTEL"));
        assert!(r.record.data(&r.record.subrecords[0])[4..]
            .iter()
            .all(|v| *v == 0xa5));
    }
}
