use super::*;
use crate::{Header, LoadOrder, Subrecord};
use fo4_core::{Limits, SliceSource, SourceSpan};

fn header(flags: u32) -> Header {
    let mut hedr = 0.95f32.to_le_bytes().to_vec();
    hedr.extend(0u32.to_le_bytes());
    hedr.extend(0x900u32.to_le_bytes());
    let mut data = b"HEDR".to_vec();
    data.extend((hedr.len() as u16).to_le_bytes());
    data.extend(hedr);
    let mut bytes = b"TES4".to_vec();
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(flags.to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend([0; 4]);
    bytes.extend(131u16.to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    bytes.extend(data);
    Header::read(
        "test.esm",
        &SliceSource {
            name: "test.esm",
            bytes: &bytes,
        },
        &Limits::default(),
        &Budget::new(1 << 20),
    )
    .unwrap()
}
fn order(flags: u32) -> LoadOrder {
    LoadOrder::new(vec![header(flags)], &Budget::new(1 << 20)).unwrap()
}
fn rec(fields: &[(&[u8; 4], Vec<u8>)]) -> Record {
    let mut payload = Vec::new();
    let mut subrecords = Vec::new();
    for (tag, data) in fields {
        let offset = 24 + payload.len() as u64;
        payload.extend(*tag);
        payload.extend((data.len() as u16).to_le_bytes());
        let start = payload.len();
        payload.extend(data);
        subrecords.push(Subrecord {
            signature: **tag,
            data: start..payload.len(),
            offset,
            extended: false,
        });
    }
    Record {
        signature: *b"QUST",
        flags: 0,
        form_id: 0x801,
        revision: 0,
        version: 131,
        unknown: 0,
        source: SourceSpan::new("synthetic", 0, 24 + payload.len() as u64),
        payload,
        subrecords,
        payload_is_decompressed: false,
    }
}
fn w(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn z(t: &str) -> Vec<u8> {
    let mut v = t.as_bytes().to_vec();
    v.push(0);
    v
}

/// A quest with every part: header, conditions on both sides of NEXT,
/// two stages (one with two log entries), an objective with a target,
/// and a reference, a collection and a location alias.
fn fixture(localized: bool) -> Vec<(&'static [u8; 4], Vec<u8>)> {
    let text = |t: &str, id: u32| if localized { w(id) } else { z(t) };
    let mut dnam = vec![0x11, 0x01, 30, 0];
    dnam.extend(w(0));
    dnam.extend(w(6));
    let mut qsta = w(0);
    qsta.extend(w(0));
    qsta.extend(w(0x850));
    let mut alla = w(0x850);
    alla.extend(w(2));
    let mut cnto = w(0x860);
    cnto.extend(w(3));
    vec![
        (b"EDID", z("Q")),
        (b"VMAD", vec![6, 0, 2, 0, 0, 0]),
        (b"FULL", text("n", 7)),
        (b"DNAM", dnam),
        (b"ENAM", b"SCPT".to_vec()),
        (b"XNAM", w(0x840)),
        (b"QTGL", w(0x841)),
        (b"FLTR", z("f\\")),
        (b"CTDA", vec![1; 32]),
        (b"CIS2", z("x")),
        (b"NEXT", vec![]),
        (b"CTDA", vec![2; 32]),
        (b"INDX", vec![10, 0, 2, 0x74]),
        (b"QSDT", vec![0]),
        (b"CTDA", vec![3; 32]),
        (b"NAM2", z("note")),
        (b"QSDT", vec![1]),
        (b"CNAM", text("log", 9)),
        (b"INDX", vec![0xe8, 3, 4, 0]),
        (b"QOBJ", vec![10, 0]),
        (b"FNAM", w(0)),
        (b"NNAM", text("obj", 8)),
        (b"QSTA", qsta),
        (b"CTDA", vec![4; 32]),
        (b"ANAM", w(3)),
        (b"ALST", w(0)),
        (b"ALID", z("A")),
        (b"FNAM", w(0)),
        (b"ALFR", w(0x870)),
        (b"VTCK", w(0)),
        (b"ALED", vec![]),
        (b"ALCS", w(1)),
        (b"ALMI", vec![5]),
        (b"ALST", w(1)),
        (b"ALID", z("C")),
        (b"FNAM", w(0x80)),
        (b"ALCO", w(0x880)),
        (b"ALCA", w(0x8000_0000)),
        (b"ALCL", w(1)),
        (b"KSIZ", w(1)),
        (b"KWDA", w(0x850)),
        (b"COCT", w(1)),
        (b"CNTO", cnto),
        (b"ALLA", alla),
        (b"CTDA", vec![5; 32]),
        (b"CIS1", z("y")),
        (b"ALED", vec![]),
        (b"ALLS", w(2)),
        (b"ALID", z("L")),
        (b"FNAM", w(0)),
        (b"ALFA", w(0)),
        (b"ALRT", w(0x890)),
        (b"ALED", vec![]),
        (b"GNAM", w(0x851)),
    ]
}

fn ctx(o: &LoadOrder) -> Context<'_> {
    Context {
        order: o,
        plugin: 0,
    }
}

#[test]
fn reads_every_part_in_order_to_the_last_byte() {
    let o = order(1);
    let b = Budget::new(1 << 20);
    let fields = fixture(false);
    let q = Quest::read(&rec(&fields), ctx(&o), &b).unwrap();
    assert_eq!(q.editor_id.as_deref(), Some(&b"Q"[..]));
    assert_eq!(q.vmad, Some(1));
    assert_eq!(q.name, Some(Text::Bytes(b"n".to_vec())));
    let g = q.general.unwrap();
    assert_eq!((g.flags, g.priority, g.kind), (0x111, 30, 6));
    assert_eq!(q.event, Some(*b"SCPT"));
    assert_eq!(q.text_globals.len(), 1);
    assert_eq!(q.conditions.len(), 1);
    assert_eq!(q.conditions[0].cis2.as_deref(), Some(&b"x"[..]));
    assert_eq!(q.event_conditions.len(), 1);
    assert_eq!(q.stages.len(), 2);
    let s = q.stage(10).unwrap();
    assert_eq!((s.flags, s.byte3, s.entries.len()), (2, 0x74, 2));
    assert_eq!(s.entries[0].conditions.len(), 1);
    assert_eq!(s.entries[0].note.as_deref(), Some(&b"note"[..]));
    assert_eq!(s.entries[1].flags, 1);
    assert_eq!(s.entries[1].text, Some(Text::Bytes(b"log".to_vec())));
    assert!(q.stage(1000).unwrap().entries.is_empty());
    let ob = q.objective(10).unwrap();
    assert_eq!(ob.targets.len(), 1);
    assert_eq!(ob.targets[0].keyword.key.as_ref().unwrap().object_id, 0x850);
    assert_eq!(ob.targets[0].conditions.len(), 1);
    assert_eq!(q.next_alias, Some(3));
    assert_eq!(q.aliases.len(), 3);
    let a = q.alias(0).unwrap();
    assert_eq!(a.kind, AliasKind::Reference);
    assert_eq!(
        a.forced_reference()
            .unwrap()
            .key
            .as_ref()
            .unwrap()
            .object_id,
        0x870
    );
    let c = q.alias(1).unwrap();
    assert_eq!(
        c.kind,
        AliasKind::Collection {
            collection: 1,
            max_initial: 5
        }
    );
    assert!(matches!(
        c.values[0],
        AliasValue::CreateObject {
            alias: 0,
            word: 0x8000,
            level: 1,
            ..
        }
    ));
    let tags: Vec<[u8; 4]> = c.values.iter().map(AliasValue::tag).collect();
    assert_eq!(tags, [*b"ALCO", *b"KSIZ", *b"COCT", *b"ALLA", *b"CTDA"]);
    let l = q.alias(2).unwrap();
    assert_eq!(l.kind, AliasKind::Location);
    assert!(matches!(
        &l.values[0],
        AliasValue::LocationAliasReference {
            alias: 0,
            ref_type: Some(_)
        }
    ));
    assert!(q.gnam.is_some() && q.snam.is_none());
    // Every subrecord is placed once, in order; CTDA and VMAD are raw.
    assert_eq!(q.fields.len(), fields.len());
    assert!(q.fields.iter().enumerate().all(|(i, f)| f.index == i));
    let raw: Vec<&[u8; 4]> = q
        .fields
        .iter()
        .filter(|f| !f.typed)
        .map(|f| &f.tag)
        .collect();
    assert_eq!(raw.len(), 6);
    assert!(raw.iter().all(|t| *t == b"CTDA" || *t == b"VMAD"));
}

#[test]
fn texts_are_string_ids_in_a_localized_plugin() {
    let o = order(0x81);
    let b = Budget::new(1 << 20);
    let q = Quest::read(&rec(&fixture(true)), ctx(&o), &b).unwrap();
    assert_eq!(q.name, Some(Text::Id(7)));
    assert_eq!(q.objectives[0].text, Some(Text::Id(8)));
    assert_eq!(q.stages[0].entries[1].text, Some(Text::Id(9)));
}

#[test]
fn what_does_not_fit_is_a_named_error() {
    let o = order(1);
    let b = Budget::new(1 << 20);
    let code = |f: &[(&[u8; 4], Vec<u8>)]| Quest::read(&rec(f), ctx(&o), &b).unwrap_err().code;
    let base = fixture(false);
    // Out of order: a stage after an objective.
    let mut f = base.clone();
    let at = f.iter().position(|x| x.0 == b"ANAM").unwrap();
    f.insert(at, (b"INDX", vec![0; 4]));
    assert_eq!(code(&f), "QuestSubrecordOrder");
    // A wrong size.
    let mut f = base.clone();
    f[3].1.push(0);
    assert_eq!(code(&f), "QuestFieldSize");
    // A text without its NUL.
    let mut f = base.clone();
    f[0].1.pop();
    assert_eq!(code(&f), "QuestFieldText");
    // A count that does not match.
    let mut f = base.clone();
    let k = f.iter().position(|x| x.0 == b"KSIZ").unwrap();
    f[k].1 = w(2);
    assert_eq!(code(&f), "QuestCount");
    let mut f = base.clone();
    let k = f.iter().position(|x| x.0 == b"CNTO").unwrap();
    let extra = f[k].clone();
    f.insert(k, extra);
    assert_eq!(code(&f), "QuestCount");
    // An alias without its end, or an unknown tag inside it.
    let mut f = base.clone();
    f.pop();
    f.pop();
    assert_eq!(code(&f), "QuestSubrecordOrder");
    let mut f = base.clone();
    let k = f.iter().position(|x| x.0 == b"VTCK").unwrap();
    f[k].0 = b"ZZZZ";
    assert_eq!(code(&f), "QuestSubrecordOrder");
    // A pair cut short.
    let mut f = base.clone();
    let k = f.iter().position(|x| x.0 == b"ALCA").unwrap();
    f.remove(k);
    assert_eq!(code(&f), "QuestSubrecordOrder");
    // Another record type.
    let mut r = rec(&base);
    r.signature = *b"ACTI";
    assert_eq!(
        Quest::read(&r, ctx(&o), &b).unwrap_err().code,
        "RecordTypeMismatch"
    );
    // Something after the closing subrecords.
    let mut f = base;
    f.push((b"EDID", z("late")));
    assert_eq!(code(&f), "QuestSubrecordOrder");
}
