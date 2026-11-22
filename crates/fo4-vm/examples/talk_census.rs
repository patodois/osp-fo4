#![forbid(unsafe_code)]
//! Dialogue outside scenes and the fills of reference aliases, the data
//! the talk step reads (`spec/vm.md`, Dialogue outside scenes, and Unique
//! actor aliases): `talk_census MANIFEST OUT.txt NAMES.txt [--player-base
//! EDITOR_ID]`.
//!
//! OUT.txt holds counts and kinds only (`spec/talk-census.txt`); NAMES.txt
//! names records and is never versioned. Nothing runs:
//!
//! 1. every winning topic (DIAL) read by `fo4_plugin::dial`: its
//!    subrecords and sizes, its subtype code (SNAM) with the category byte
//!    of DATA, the record type QNAM names;
//! 2. topics and their lines by subtype and by whether a scene action
//!    names the topic (any field of an action resolving to it) or the
//!    topic belongs to a dialogue branch (BNAM);
//! 3. the lines carrying a stage field (TIQS with a non-negative half),
//!    by the same split, and whether their owning quest holds the stage;
//! 4. who a line's conditions name as speaker and listener: function 72
//!    on run-on 0 and 1, naming the player's base (when the caller names
//!    it), another actor base or nothing, by subtype;
//! 5. lines with a script block (VMAD), whose fragments no line outside a
//!    scene runs;
//! 6. game settings whose editor ID names greetings or hellos (counted;
//!    the names file lists them with their values);
//! 7. every reference alias of every winning quest by how it is filled,
//!    and the unique actor aliases by the number of placed actors of their
//!    base in the loaded plugins.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    dial::Topic,
    info::Info,
    quest::{AliasKind, AliasValue, Quest},
    scen::Scene,
    typed::Store,
    FormKey,
};
use fo4_vm::bind::{self, record_key};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

/// What the census keeps of one topic.
struct TopicRow {
    subtype: String,
    named: bool,
    branch: bool,
    quest: Option<FormKey>,
}

fn split(t: &TopicRow) -> &'static str {
    match (t.named, t.branch) {
        (true, _) => "named by a scene action",
        (false, true) => "in a dialogue branch, named by no scene action",
        (false, false) => "named by no scene action, in no branch",
    }
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut player_name = None;
    if let Some(i) = a.iter().position(|x| x == "--player-base") {
        player_name = Some(a.get(i + 1).cloned().ok_or("--player-base EDITOR_ID")?);
        a.drain(i..i + 2);
    }
    if a.len() != 3 {
        return Err("talk_census MANIFEST OUT.txt NAMES.txt [--player-base EDITOR_ID]".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let b = Budget::new(m.limits.max_memory);
    let mut c = Counts::new();
    let mut names = String::new();
    let winners = |sig: &[u8; 4]| -> Vec<FormKey> {
        store
            .winners()
            .filter(|w| !w.deleted && w.signature == *sig)
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect()
    };
    // The player's base, as the caller names it.
    let player = match &player_name {
        Some(n) => winners(b"NPC_").into_iter().find(|k| {
            store
                .raw(k, &b.child())
                .ok()
                .flatten()
                .and_then(|r| bind::editor_id(&r))
                .is_some_and(|e| e.eq_ignore_ascii_case(n))
        }),
        None => None,
    };
    // Topics named by a scene action: any action field resolving to a DIAL.
    let mut named: BTreeSet<String> = BTreeSet::new();
    for k in winners(b"SCEN") {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let Ok(s) = Scene::read(&r, ctx, &bb) else {
            bump(&mut c, "1 topics\tscenes that do not read");
            continue;
        };
        for act in &s.actions {
            for f in &act.fields {
                if let Some(t) = f.form.as_ref().and_then(|f| f.key.clone()) {
                    if store.signature(&t) == Some(*b"DIAL") {
                        named.insert(record_key(&t));
                    }
                }
            }
        }
    }
    // 1. Topics.
    let mut topics: BTreeMap<String, TopicRow> = BTreeMap::new();
    for k in winners(b"DIAL") {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        bump(&mut c, "1 topics\twinning topics");
        for s in &r.subrecords {
            bump(
                &mut c,
                format!(
                    "1 topics\tsubrecords\t{}\t{}",
                    String::from_utf8_lossy(&s.signature),
                    if s.signature == *b"EDID" {
                        "a text".to_string()
                    } else {
                        format!("{} bytes", r.data(s).len())
                    }
                ),
            );
        }
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let t = match Topic::read(&r, ctx, &bb) {
            Ok(t) => t,
            Err(e) => {
                bump(&mut c, format!("1 topics\tdo not read\t{}", e.code));
                continue;
            }
        };
        let subtype = t.subtype_text().unwrap_or_else(|| "none".into());
        bump(
            &mut c,
            format!(
                "1 topics\tby subtype and category byte\t{subtype}\t{}",
                t.category().map_or("none".into(), |x| x.to_string())
            ),
        );
        let q = t.quest.as_ref().and_then(|q| q.key.clone());
        bump(
            &mut c,
            format!(
                "1 topics\tQNAM names\t{}",
                match &q {
                    Some(k) => store.signature(k).map_or("no winning record".into(), |s| {
                        String::from_utf8_lossy(&s).into_owned()
                    }),
                    None => "nothing".into(),
                }
            ),
        );
        topics.insert(
            record_key(&k),
            TopicRow {
                subtype,
                named: named.contains(&record_key(&k)),
                branch: t.branch.is_some(),
                quest: q,
            },
        );
    }
    // The subtypes whose distinct category bytes are more than one.
    let mut cats: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for k in c.keys() {
        if let Some(rest) = k.strip_prefix("1 topics\tby subtype and category byte\t") {
            if let Some((s, cat)) = rest.split_once('\t') {
                cats.entry(s.to_string())
                    .or_default()
                    .insert(cat.to_string());
            }
        }
    }
    *c.entry("1 topics\tsubtype codes".into()).or_default() += cats.len() as u64;
    *c.entry("1 topics\tsubtype codes stored with more than one category byte".into())
        .or_default() += cats.values().filter(|v| v.len() > 1).count() as u64;
    for row in topics.values() {
        bump(
            &mut c,
            format!("2 topics by subtype\t{}\t{}", row.subtype, split(row)),
        );
    }
    // 2-5. Lines.
    let mut quests: BTreeMap<String, Option<BTreeSet<u16>>> = BTreeMap::new();
    for (o, topic) in store.topic_children() {
        if o.signature != *b"INFO" || o.deleted {
            continue;
        }
        let Some(row) = topics.get(&record_key(&topic)) else {
            bump(&mut c, "2 lines by subtype\tunder no winning topic");
            continue;
        };
        let k = store.overrides().form_key(o.key, store.order());
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        let sp = split(row);
        let sub = &row.subtype;
        bump(&mut c, format!("2 lines by subtype\t{sub}\t{sp}"));
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let Ok(info) = Info::read(&r, ctx, &bb) else {
            bump(&mut c, "2 lines by subtype\tlines that do not read");
            continue;
        };
        if r.subrecords.iter().any(|s| s.signature == *b"VMAD") {
            bump(&mut c, format!("5 lines with a script block\t{sub}\t{sp}"));
        }
        let tiqs = r
            .subrecords
            .iter()
            .find(|s| s.signature == *b"TIQS")
            .and_then(|s| <[u8; 4]>::try_from(r.data(s)).ok())
            .map(|d| {
                [
                    i16::from_le_bytes([d[0], d[1]]),
                    i16::from_le_bytes([d[2], d[3]]),
                ]
            });
        if let Some([b0, b1]) = tiqs.filter(|[x, y]| *x >= 0 || *y >= 0) {
            let halves = match (b0 >= 0, b1 >= 0) {
                (true, true) => "both halves",
                (true, false) => "begin half",
                _ => "end half",
            };
            bump(
                &mut c,
                format!("3 lines with a stage field\t{sub}\t{sp}\t{halves}"),
            );
            // Whether the topic's quest holds the stages named.
            let held = match &row.quest {
                Some(q) => {
                    let qk = record_key(q);
                    if !quests.contains_key(&qk) {
                        let st = store
                            .raw_with_plugin(q, &b.child())
                            .ok()
                            .flatten()
                            .and_then(|(qr, p)| {
                                let ctx = Context {
                                    order: store.order(),
                                    plugin: p,
                                };
                                Quest::read(&qr, ctx, &b.child()).ok()
                            })
                            .map(|qq| qq.stages.iter().map(|s| s.stage).collect());
                        quests.insert(qk.clone(), st);
                    }
                    match &quests[&qk] {
                        Some(st) => [b0, b1]
                            .iter()
                            .filter(|x| **x >= 0)
                            .all(|x| st.contains(&(*x as u16))),
                        None => false,
                    }
                }
                None => false,
            };
            bump(
                &mut c,
                format!(
                    "3 lines with a stage field: the topic's quest holds every stage named\t{}\t{sp}",
                    if held { "yes" } else { "no" }
                ),
            );
            if !row.named {
                let _ = writeln!(
                    names,
                    "## TIQS {b0} {b1} on {} of topic {} ({sub}, {sp}), topic quest {}",
                    record_key(&k),
                    record_key(&topic),
                    row.quest.as_ref().map(record_key).unwrap_or_default()
                );
            }
        }
        // 4. Who the conditions name.
        let mut roles: BTreeSet<String> = BTreeSet::new();
        for cond in &info.conditions {
            let x = &cond.ctda;
            if x.function != 72 || x.run_on > 1 {
                continue;
            }
            let who = match fo4_plugin::condition::form(ctx, x.param1, &r.source.file, 0) {
                Ok(Some(n)) if Some(&n) == player.as_ref() => "the player's base",
                Ok(Some(n)) if store.signature(&n) == Some(*b"NPC_") => "another actor base",
                _ => "another record",
            };
            roles.insert(format!(
                "{} {who}",
                if x.run_on == 0 { "speaker" } else { "listener" }
            ));
        }
        let who = if roles.is_empty() {
            "no function 72 on speaker or listener".to_string()
        } else {
            roles.into_iter().collect::<Vec<_>>().join(", ")
        };
        bump(&mut c, format!("4 lines by who they name\t{sub}\t{who}"));
    }
    // 6. Game settings that name greetings or hellos.
    for k in winners(b"GMST") {
        let Some(r) = store.raw(&k, &b.child())? else {
            continue;
        };
        let Some(e) = bind::editor_id(&r) else {
            continue;
        };
        let l = e.to_ascii_lowercase();
        if !(l.contains("greeting") || l.contains("hello")) {
            continue;
        }
        let d = r
            .subrecords
            .iter()
            .find(|s| s.signature == *b"DATA")
            .map(|s| r.data(s).to_vec())
            .unwrap_or_default();
        bump(
            &mut c,
            format!(
                "6 game settings whose editor ID names greetings or hellos\tfirst letter {}",
                e.chars().next().unwrap_or(' ')
            ),
        );
        let v = <[u8; 4]>::try_from(d.as_slice())
            .map(|w| format!("{} as f32", f32::from_le_bytes(w)))
            .unwrap_or_else(|_| format!("{} bytes", d.len()));
        let _ = writeln!(names, "## GMST {e}: {v}");
    }
    // 7. Reference aliases by fill.
    let mut placed: BTreeMap<String, usize> = BTreeMap::new();
    for (o, _) in store.placement_cells() {
        if o.signature != *b"ACHR" || o.deleted {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        if let Ok(Some(base)) = bind::base_of(&store, &k, &b.child()) {
            *placed.entry(record_key(&base)).or_default() += 1;
        }
    }
    for k in winners(b"QUST") {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let Ok(q) = Quest::read(&r, ctx, &bb) else {
            bump(&mut c, "7 quests that do not read");
            continue;
        };
        for al in &q.aliases {
            let kind = match al.kind {
                AliasKind::Location => "location alias",
                AliasKind::Collection { .. } => "collection alias",
                AliasKind::Reference => "reference alias",
            };
            let tags: BTreeSet<[u8; 4]> = al.values.iter().map(|v| v.tag()).collect();
            let has = |t: &[u8; 4]| tags.contains(t);
            let fill = if has(b"ALFR") {
                "forced reference (ALFR)"
            } else if has(b"ALUA") {
                "unique actor (ALUA)"
            } else if has(b"ALFA") {
                "from a location alias (ALFA)"
            } else if has(b"ALFE") {
                "from the story event (ALFE)"
            } else if has(b"ALCO") {
                "created at run time (ALCO)"
            } else if has(b"ALEQ") {
                "an alias of another quest (ALEQ)"
            } else if has(b"ALNA") {
                "linked to another alias (ALNA)"
            } else if has(b"ALFL") {
                "forced location (ALFL)"
            } else if has(b"ALDN") {
                "a display name only (ALDN)"
            } else if has(b"CTDA") {
                "by conditions only (CTDA)"
            } else {
                "no fill value"
            };
            bump(&mut c, format!("7 aliases by fill\t{kind}\t{fill}"));
            if al.kind == AliasKind::Location {
                continue;
            }
            let unique = al.values.iter().find_map(|v| match v {
                AliasValue::UniqueActor(f) => Some(f.key.clone()),
                _ => None,
            });
            let Some(u) = unique else { continue };
            let what = match &u {
                None => "the unique actor names no record".to_string(),
                Some(n) if Some(n) == player.as_ref() => "the player's base".to_string(),
                Some(n) => match placed.get(&record_key(n)).copied().unwrap_or(0) {
                    0 => "no placed actor of the base".into(),
                    1 => "one placed actor of the base (resolved)".into(),
                    _ => "several placed actors of the base".into(),
                },
            };
            bump(
                &mut c,
                format!(
                    "7 unique actor aliases by placed actors of the base\t{kind}\t{what}{}",
                    if has(b"ALFR") {
                        " (a forced reference too)"
                    } else {
                        ""
                    }
                ),
            );
        }
    }
    let mut out = String::from(
        "# talk_census: kinds and counts only (spec/vm.md, Dialogue outside scenes, and Unique actor aliases). Sections: 1 topics (DIAL); 2 topics and lines by subtype code and whether a scene action names the topic;\n# 3 lines with a stage field (TIQS); 4 lines by whom function 72 names as speaker (run-on 0) and listener (run-on 1); 5 lines with a script block; 6 game settings naming greetings or hellos; 7 reference aliases by fill and unique actor aliases by placed actors.\n",
    );
    if player.is_none() {
        out.push_str(
            "# The player's base was not named: section 4 counts it as another actor base.\n",
        );
    }
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    std::fs::write(&a[1], out)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
