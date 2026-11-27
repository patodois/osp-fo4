//! The sweeps of `quest_run` over a synthetic manifest whose scripts are
//! written as `.pex` files ([`super::assemble_bytes`]); no game data.
use super::assemble_bytes;
use super::quest::{group, object, record, sub, text};
use fo4_core::{Budget, TestDir};

const QUEST: u32 = 0x820;
const PACKAGE: u32 = 0x850;
/// Stages of the quest, each fragment setting the next: more jobs than
/// `crate::run::MAX_JOBS` for one run.
const STAGES: u16 = 300;

fn quest_vmad(frags: u16) -> Vec<u8> {
    let mut v = vec![6, 0, 2, 0, 0, 0, 3];
    v.extend(frags.to_le_bytes());
    text(&mut v, "QF");
    v.push(0);
    v.extend(0u16.to_le_bytes());
    for n in 0..frags {
        v.extend(n.to_le_bytes());
        v.extend([0, 0]);
        v.extend(0i32.to_le_bytes());
        v.push(1);
        text(&mut v, "QF");
        text(&mut v, &format!("Fragment_Stage_{n}"));
    }
    v.extend(0u16.to_le_bytes());
    v
}

/// A package whose begin fragment sets the quest's first stage, and a
/// quest whose stage fragments each set the next stage.
fn plugin() -> Vec<u8> {
    let mut q = [sub(b"EDID", b"QC\0"), sub(b"VMAD", &quest_vmad(STAGES))].concat();
    let mut d = 0u16.to_le_bytes().to_vec();
    d.extend([30, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    q.extend(sub(b"DNAM", &d));
    for n in 0..STAGES {
        let mut i = n.to_le_bytes().to_vec();
        i.extend([0, 0]);
        q.extend(sub(b"INDX", &i));
        q.extend(sub(b"QSDT", &[0]));
    }
    let mut pv = vec![6, 0, 2, 0, 0, 0, 3, 1];
    text(&mut pv, "PF");
    pv.push(0);
    pv.extend(1u16.to_le_bytes());
    object(&mut pv, "Q", -1, QUEST);
    pv.push(1);
    text(&mut pv, "PF");
    text(&mut pv, "Fragment_Begin");
    let pack = [sub(b"EDID", b"PK\0"), sub(b"VMAD", &pv)].concat();
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(2u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = {
        let mut r = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        r[8..12].copy_from_slice(&fo4_plugin::MASTER.to_le_bytes());
        r
    };
    plugin.extend(group(0, *b"QUST", &record(b"QUST", QUEST, &q)));
    plugin.extend(group(0, *b"PACK", &record(b"PACK", PACKAGE, &pack)));
    plugin
}

fn scripts() -> Vec<(&'static str, String)> {
    let mut qf = String::from("script QF extends Quest\n");
    for n in 0..STAGES {
        qf.push_str(&format!(
            "func Fragment_Stage_{n} none\nlocal Bool ::b\n23 SetCurrentStageID self ::b 1 {}\n",
            n + 1
        ));
    }
    vec![
        (
            "ScriptObject",
            "script ScriptObject
             func OnInit none"
                .into(),
        ),
        ("Form", "script Form extends ScriptObject".into()),
        (
            "ObjectReference",
            "script ObjectReference extends Form".into(),
        ),
        ("Actor", "script Actor extends ObjectReference".into()),
        ("Package", "script Package extends Form".into()),
        (
            "Quest",
            "script Quest extends Form
             func SetCurrentStageID Bool native Int aiStageID
             func GetCurrentStageID Int native
             func IsRunning Bool native
             func Start Bool native"
                .into(),
        ),
        ("QF", qf),
        (
            "PF",
            "script PF extends Package
             var Quest ::Q_var
             prop Quest Q ::Q_var
             func Fragment_Begin none Actor akActor
             local Bool ::b
             23 SetCurrentStageID ::Q_var ::b 1 0"
                .into(),
        ),
    ]
}

/// code-2c F04: a fragment whose jobs pass the job limit is counted as cut
/// (as the stage sweep counts its runs), so the census does not read as a
/// complete run.
#[test]
fn the_fragment_sweep_counts_runs_cut_at_the_job_limit() {
    let dir = TestDir::new("vm-sweep-cut");
    std::fs::write(dir.join("test.esm"), plugin()).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let mut files = Vec::new();
    for (name, src) in scripts() {
        let path = format!("scripts/{name}.pex");
        std::fs::write(dir.join(&path), assemble_bytes(&src)).unwrap();
        files.push(format!("{path:?}"));
    }
    let manifest = format!(
        "{{\"game\":\"fallout4\",\"root\":{:?},\"language\":\"english\",\"build\":\"synthetic\",\"plugins\":[{{\"name\":\"test.esm\",\"path\":\"test.esm\"}}],\"required_masters\":[],\"mounts\":[{{\"kind\":\"loose\",\"base\":null,\"plugin\":null,\"files\":[{}]}}],\"excluded\":[]}}",
        dir.to_str().unwrap(),
        files.join(",")
    );
    std::fs::write(dir.join("m.json"), manifest).unwrap();
    let arg = |p: &str| dir.join(p).to_str().unwrap().to_string();
    let none = |_: &[u8], _: &str, _: &Budget| None;
    let args = vec![
        arg("m.json"),
        "fragments".into(),
        arg("out"),
        "--queued".into(),
    ];
    crate::run::main_with(args, &none, None).unwrap();
    let read = |f: &str| std::fs::read_to_string(dir.join("out").join(f)).unwrap();
    let census = read("quest-run-census.txt");
    assert!(
        census.contains("1\tPACK begin fragment runs that reached the job limit (cascade cut)"),
        "{census}"
    );
    let names = read("quest-run-names.txt");
    assert!(names.contains("job limit reached: "), "{names}");
}
