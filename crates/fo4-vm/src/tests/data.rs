//! The data host and attaching scripts on need, over a synthetic plugin
//! (no game data): XLKR, the initially-disabled flag, NAME, FLTV, LNAM,
//! the load order, a quest's scripts and an alias's scripts.
use super::{assemble, base};
use crate::{bind, data, rules, Outcome, Val};
use fo4_core::{Budget, Limits, ReadAt, SliceSource};
use fo4_plugin::{typed::Store, FormKey};

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
fn group(kind: u32, label: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = b"GRUP".to_vec();
    out.extend(((24 + data.len()) as u32).to_le_bytes());
    out.extend(label);
    out.extend(kind.to_le_bytes());
    out.extend([0; 8]);
    out.extend(data);
    out
}
fn text(v: &mut Vec<u8>, t: &str) {
    v.extend((t.len() as u16).to_le_bytes());
    v.extend(t.as_bytes());
}
/// A VMAD object property.
fn object(v: &mut Vec<u8>, name: &str, alias: i16, id: u32) {
    text(v, name);
    v.extend([1, 1, 0, 0]);
    v.extend(alias.to_le_bytes());
    v.extend(id.to_le_bytes());
}
fn int(v: &mut Vec<u8>, name: &str, n: i32) {
    text(v, name);
    v.extend([3, 1]);
    v.extend(n.to_le_bytes());
}
fn xlkr(kw: u32, to: u32) -> Vec<u8> {
    let mut d = kw.to_le_bytes().to_vec();
    d.extend(to.to_le_bytes());
    sub(b"XLKR", &d)
}

struct Owned(Vec<u8>);
impl ReadAt for Owned {
    fn name(&self) -> &str {
        "test.esm"
    }
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_exact_at(&self, o: u64, out: &mut [u8]) -> fo4_core::Result<()> {
        SliceSource {
            name: self.name(),
            bytes: &self.0,
        }
        .read_exact_at(o, out)
    }
}

const KW: u32 = 0x800;
const GLOB: u32 = 0x801;
const FLST: u32 = 0x802;
const STAT: u32 = 0x803;
const ACTI: u32 = 0x804;
const CELL: u32 = 0x810;
const BUTTON: u32 = 0x811;
const BOX1: u32 = 0x812;
const BOX2: u32 = 0x813;
const BOX3: u32 = 0x814;
const QUEST: u32 = 0x820;
const CHILD1: u32 = 0x815;
const CHILD2: u32 = 0x816;

fn xesp(parent: u32, flags: u8) -> Vec<u8> {
    let mut d = parent.to_le_bytes().to_vec();
    d.extend([flags, 0, 0, 0]);
    sub(b"XESP", &d)
}

/// One cell: a button (ACTI with a script) linked to a box without a
/// keyword; three boxes chained by the keyword, the second initially
/// disabled; a quest with a script and an alias with a script.
fn store() -> Store {
    let mut vmad = vec![6, 0, 2, 0, 1, 0];
    text(&mut vmad, "Button");
    vmad.push(0);
    vmad.extend(5u16.to_le_bytes());
    object(&mut vmad, "Q", -1, QUEST);
    object(&mut vmad, "Alias", 2, QUEST);
    object(&mut vmad, "List", -1, FLST);
    object(&mut vmad, "G", -1, GLOB);
    object(&mut vmad, "K", -1, KW);
    let mut acti = sub(b"EDID", b"Button\0");
    acti.extend(sub(b"VMAD", &vmad));
    let mut qv = vec![6, 0, 2, 0, 1, 0];
    text(&mut qv, "QScript");
    qv.push(0);
    qv.extend(1u16.to_le_bytes());
    int(&mut qv, "Count", 7);
    qv.push(3);
    qv.extend(0u16.to_le_bytes());
    text(&mut qv, "");
    qv.extend(1u16.to_le_bytes());
    qv.extend([0, 0]);
    qv.extend(2i16.to_le_bytes());
    qv.extend(QUEST.to_le_bytes());
    qv.extend([6, 0, 2, 0, 1, 0]);
    text(&mut qv, "AliasScript");
    qv.push(0);
    qv.extend(1u16.to_le_bytes());
    int(&mut qv, "N", 3);
    let refs = [
        record(
            b"REFR",
            BUTTON,
            0,
            &[
                sub(b"NAME", &ACTI.to_le_bytes()),
                xlkr(0, BOX1),
                xlkr(KW, BOX3),
            ]
            .concat(),
        ),
        record(
            b"REFR",
            BOX1,
            0,
            &[sub(b"NAME", &STAT.to_le_bytes()), xlkr(KW, BOX2)].concat(),
        ),
        record(
            b"REFR",
            BOX2,
            fo4_plugin::DISABLED,
            &[sub(b"NAME", &STAT.to_le_bytes()), xlkr(KW, BOX3)].concat(),
        ),
        record(b"REFR", BOX3, 0, &sub(b"NAME", &STAT.to_le_bytes())),
        // Enable children: of the initially disabled box, and of the first
        // box with the opposite bit.
        record(
            b"REFR",
            CHILD1,
            0,
            &[sub(b"NAME", &STAT.to_le_bytes()), xesp(BOX2, 0)].concat(),
        ),
        record(
            b"REFR",
            CHILD2,
            0,
            &[sub(b"NAME", &STAT.to_le_bytes()), xesp(BOX1, 1)].concat(),
        ),
    ]
    .concat();
    let cell = record(b"CELL", CELL, 0, &sub(b"EDID", b"Room\0"));
    let children = group(6, CELL.to_le_bytes(), &group(9, CELL.to_le_bytes(), &refs));
    let mut body = group(2, [0; 4], &[cell, children].concat());
    body.extend(record(b"KYWD", KW, 0, &sub(b"EDID", b"Kw\0")));
    body.extend(record(
        b"GLOB",
        GLOB,
        0,
        &sub(b"FLTV", &2.5f32.to_le_bytes()),
    ));
    body.extend(record(b"FLST", FLST, 0, &sub(b"LNAM", &STAT.to_le_bytes())));
    body.extend(record(b"STAT", STAT, 0, &sub(b"EDID", b"Box\0")));
    body.extend(record(b"ACTI", ACTI, 0, &acti));
    body.extend(record(b"QUST", QUEST, 0, &sub(b"VMAD", &qv)));
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(12u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut plugin = record(b"TES4", 0, fo4_plugin::MASTER, &sub(b"HEDR", &hedr));
    plugin.extend(body);
    let sources: Vec<(String, Box<dyn ReadAt>)> =
        vec![("test.esm".into(), Box::new(Owned(plugin)))];
    Store::from_sources(
        fo4_core::Game::Fallout4,
        sources,
        Limits::default(),
        &Budget::new(8 << 20),
    )
    .unwrap()
}

fn key(id: u32) -> FormKey {
    FormKey {
        plugin: "test.esm".into(),
        object_id: id,
    }
}

fn library() -> crate::MapLibrary {
    let mut lib = base();
    lib.add(assemble(
        "script ObjectReference extends Form
         func OnActivate none ObjectReference akActionRef
         func Disable none native Bool abFade
         func Enable none native Bool abFade
         func IsDisabled Bool native
         func GetBaseObject Form native
         func GetLinkedRef ObjectReference native Keyword kw
         func CountLinkedRefChain Int native Keyword kw Int max
         func SetLinkedRef none native ObjectReference r Keyword kw",
    ));
    lib.add(assemble(
        "script Game
         func GetPlayer Actor native global
         func GetFormFromFile Form native global Int aiFormID String asFilename",
    ));
    lib.add(assemble(
        "script GlobalVariable extends Form
         func GetValue Float native",
    ));
    lib.add(assemble(
        "script FormList extends Form
         func HasForm Bool native Form akForm
         func GetSize Int native
         func GetAt Form native Int aiIndex",
    ));
    lib.add(assemble("script Quest extends Form"));
    lib.add(assemble("script Static extends Form"));
    lib.add(assemble("script Furniture extends Form"));
    lib.add(assemble("script Alias extends ScriptObject"));
    lib.add(assemble("script ReferenceAlias extends Alias"));
    lib.add(assemble(
        "script QScript extends Quest
         var Int ::Count_var
         prop Int Count ::Count_var",
    ));
    lib.add(assemble(
        "script AliasScript extends ReferenceAlias
         var Int ::N_var
         prop Int N ::N_var",
    ));
    lib.add(assemble(
        "script Button extends ObjectReference
         var QScript ::Q_var
         prop QScript Q ::Q_var
         var AliasScript ::Alias_var
         prop AliasScript Alias ::Alias_var
         var FormList ::List_var
         prop FormList List ::List_var
         var GlobalVariable ::G_var
         prop GlobalVariable G ::G_var
         var Keyword ::K_var
         prop Keyword K ::K_var
         var ObjectReference r1
         var Bool disabled1
         var Bool disabled2
         var Float value
         var Form base
         var Bool listed
         var Bool furniture
         var Int chain
         var Int count
         var Int n
         var Form fromfile
         var Form missing
         var Int size
         var Form first
         func OnActivate none ObjectReference akActionRef
         local none ::nonevar
         23 GetLinkedRef self r1 1 none
         23 IsDisabled r1 disabled1 0
         23 GetValue ::G_var value 0
         23 GetBaseObject r1 base 0
         23 HasForm ::List_var listed 1 base
         23 GetSize ::List_var size 0
         23 GetAt ::List_var first 1 0
         36 furniture base Furniture
         23 CountLinkedRefChain r1 chain 2 ::K_var 10
         28 Count ::Q_var count
         28 N ::Alias_var n
         25 Game GetFormFromFile fromfile 2 2049 \"TEST.ESM\"
         25 Game GetFormFromFile missing 2 2049 \"other.esm\"
         23 Disable r1 ::nonevar 1 false
         23 IsDisabled r1 disabled2 0",
    ));
    lib
}

fn var(r: &bind::Run, name: &str) -> Val {
    let i = r.bound.instances[0].1;
    r.world.instances[i]
        .vars
        .iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("{name}"))
        .value
        .clone()
}

#[test]
fn the_data_host_answers_from_the_records_and_quests_get_their_scripts() {
    let store = store();
    let mut lib = library();
    let b = Budget::new(1 << 24);
    let ev = [bind::Event::parse("activate").unwrap()];
    let r = bind::run(&mut lib, &store, Some(&key(BUTTON)), &key(ACTI), &ev, 0, &b).unwrap();
    assert_eq!(
        r.outcomes[0].2,
        Outcome::Completed(Val::None),
        "{}",
        bind::effect_log(&r)
    );
    let keyed = |v: Val| match v {
        Val::Object(o) => r.world.objects[o].key.clone().unwrap(),
        other => panic!("{other:?}"),
    };
    // XLKR without a keyword, the flag, FLTV, NAME, LNAM.
    assert_eq!(keyed(var(&r, "r1")), "test.esm:00000812");
    assert_eq!(var(&r, "disabled1"), Val::Bool(false));
    assert_eq!(var(&r, "value"), Val::Float(2.5));
    assert_eq!(keyed(var(&r, "base")), "test.esm:00000803");
    assert_eq!(var(&r, "listed"), Val::Bool(true));
    assert_eq!(var(&r, "size"), Val::Int(1));
    assert_eq!(keyed(var(&r, "first")), "test.esm:00000803");
    // A STAT handle is a Static and nothing else.
    assert_eq!(var(&r, "furniture"), Val::Bool(false));
    // 812 -> 813 -> 814 by the keyword.
    assert_eq!(var(&r, "chain"), Val::Int(2));
    // The quest's and the alias's scripts, attached when first needed.
    assert_eq!(var(&r, "count"), Val::Int(7));
    assert_eq!(var(&r, "n"), Val::Int(3));
    assert_eq!(r.attached.get("quest"), Some(&1));
    assert_eq!(r.attached.get("alias"), Some(&1));
    // The load order: a file of another case, and one not loaded.
    assert_eq!(keyed(var(&r, "fromfile")), "test.esm:00000801");
    assert_eq!(var(&r, "missing"), Val::None);
    // Disable in the same run answers IsDisabled.
    assert_eq!(var(&r, "disabled2"), Val::Bool(true));
    for rule in [
        rules::DATA_STATE,
        rules::LINKED_REF,
        rules::LINKED_REF_CHAIN,
        rules::IS_DISABLED,
        rules::BASE_OBJECT,
        rules::FORM_FROM_FILE,
        rules::GLOBAL_VALUE,
        rules::FORM_LIST,
        rules::SIGNATURE_TYPE,
        rules::ATTACH,
        rules::ALIAS_VALUE,
    ] {
        assert!(r.rules.contains(rule), "{rule}");
    }
    assert_eq!(
        bind::visibility_changes(&r),
        [("test.esm:00000812".to_string(), false)]
    );
}

#[test]
fn the_recording_host_leaves_the_same_natives_open() {
    let store = store();
    let mut lib = library();
    let b = Budget::new(1 << 24);
    let ev = [bind::Event::parse("activate").unwrap()];
    let r = bind::run_in(
        bind::Mode::Recording,
        &mut lib,
        &store,
        Some(&key(BUTTON)),
        &key(ACTI),
        &ev,
        0,
        &b,
    )
    .unwrap();
    // The first native result steers nothing, the property read blocks:
    // no instance of the quest's script without attaching.
    assert!(matches!(var(&r, "r1"), Val::Unknown(_)));
    assert!(
        matches!(&r.outcomes[0].2, Outcome::Blocked(bl) if matches!(bl.on, crate::Origin::NoInstance(..))),
        "{:?}",
        r.outcomes
    );
}

#[test]
fn linked_references_read_as_keyword_reference_pairs_and_keys_round_trip() {
    let store = store();
    let b = Budget::new(1 << 20);
    let (r, plugin) = store.raw_with_plugin(&key(BUTTON), &b).unwrap().unwrap();
    let links = data::linked_refs(&store, &r, plugin).unwrap();
    assert_eq!(
        links,
        [(None, Some(key(BOX1))), (Some(key(KW)), Some(key(BOX3)))]
    );
    assert_eq!(
        data::key_record(&bind::record_key(&key(BOX2))),
        Some(key(BOX2))
    );
    assert_eq!(data::key_record("test.esm:00000820#alias:2"), None);
    assert_eq!(data::signature_type(*b"QUST"), Some("quest"));
    assert_eq!(data::signature_type(*b"ZZZZ"), None);
}

#[test]
fn is_disabled_follows_the_enable_parent_chain() {
    use crate::host::{Host, NativeCall, Reply};
    let store = store();
    let b = Budget::new(1 << 24);
    let call = |name: &str, o: usize| NativeCall {
        script: "objectreference".into(),
        name: name.into(),
        receiver: Some(Val::Object(o)),
        instance: None,
        args: vec![Val::Bool(false)],
        param_types: vec!["bool".into()],
        return_type: if name == "isdisabled" { "bool" } else { "none" }.into(),
    };
    let mut host = crate::data::DataHost::new(&store, 1 << 24);
    let mut w = crate::World::default();
    let obj = |w: &mut crate::World, id: u32| {
        crate::data::record_object(w, &store, &key(id), "objectreference", &b).unwrap()
    };
    let (c1, c2, box1, box2) = (
        obj(&mut w, CHILD1),
        obj(&mut w, CHILD2),
        obj(&mut w, BOX1),
        obj(&mut w, BOX2),
    );
    let ask = |host: &mut crate::data::DataHost<'_>, w: &mut crate::World, o| match host
        .call(&call("isdisabled", o), w)
    {
        Reply::Value(Val::Bool(d)) => Some(d),
        _ => None,
    };
    // The parent is initially disabled: so is its child; the opposite bit
    // under an enabled parent: disabled.
    assert_eq!(ask(&mut host, &mut w, c1), Some(true));
    assert_eq!(ask(&mut host, &mut w, c2), Some(true));
    // The run enables the first parent and disables the first box.
    host.call(&call("enable", box2), &mut w);
    host.call(&call("disable", box1), &mut w);
    assert_eq!(ask(&mut host, &mut w, c1), Some(false));
    assert_eq!(ask(&mut host, &mut w, c2), Some(false));
    for r in [
        rules::IS_DISABLED_CHAIN,
        rules::XESP_FOLLOWS,
        rules::XESP_INITIAL,
        rules::XESP_OPPOSITE,
        rules::XESP_PROPAGATES,
    ] {
        assert!(host.rules().contains(&r), "{r}");
    }
    // The header flag alone, as before.
    let mut host = crate::data::DataHost::new(&store, 1 << 24);
    host.enable_chain = false;
    assert_eq!(ask(&mut host, &mut w, c1), Some(false));
}
