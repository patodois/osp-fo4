//! Keywords of references (`spec/vm.md`, Keywords): the keywords a placed
//! reference or the player carries in the run, read from the records and
//! changed by the scripts' `AddKeyword` and `RemoveKeyword`, and the
//! `HasKeyword` natives answered from them.
//!
//! Two readings, each a pending rule behind its own switch, both off by
//! default (the natives are then recorded as before):
//! `rules::KEYWORD_RECORDS` (a reference's keywords are its base record's
//! `KWDA`, an actor's with its race's) and `rules::KEYWORD_RUNTIME`
//! (`AddKeyword` and `RemoveKeyword` change the reference's set, the last
//! change of a keyword winning). Where a reading cannot settle the answer
//! the native stays undetermined, as before, and the reason is counted.
//! Nothing is measured in the running game.
use crate::{
    bind,
    data::key_record,
    host::{NativeCall, Reply},
    rules,
    world::{ObjId, Val, World},
};
use fo4_core::Budget;
use fo4_plugin::{typed::Store, FormKey, Record};
use std::collections::{BTreeMap, BTreeSet};

/// The run's keyword state (kept in [`World`]).
#[derive(Debug, Default)]
pub struct Keywords {
    /// A reference's keywords are read from the records
    /// (`rules::KEYWORD_RECORDS`, `quest_run --keywords-records`).
    pub records: bool,
    /// `AddKeyword` and `RemoveKeyword` change the reference's keywords
    /// (`rules::KEYWORD_RUNTIME`, `quest_run --keywords-runtime`).
    pub runtime: bool,
    /// The changes the run made, per object: keyword record key → present
    /// (true after `AddKeyword`, false after `RemoveKeyword`), the last
    /// change winning. Kept whenever either reading is on, so that the
    /// records reading alone knows where the run changed a keyword.
    pub changes: BTreeMap<ObjId, BTreeMap<String, bool>>,
    /// Objects the run changed with an undetermined keyword.
    pub open: BTreeSet<ObjId>,
    /// (real-time second, what happened), in order.
    pub log: Vec<(f64, String)>,
    pub counts: BTreeMap<String, u64>,
    /// Rules relied on inside a native call, taken by the host after it.
    pub used: Vec<&'static str>,
}
impl Keywords {
    pub fn count(&mut self, k: impl Into<String>) {
        *self.counts.entry(k.into()).or_default() += 1;
    }
    fn using(&mut self, r: &'static str) {
        if !self.used.contains(&r) {
            self.used.push(r);
        }
    }
    /// Whether either reading is on.
    pub fn on(&self) -> bool {
        self.records || self.runtime
    }
}

/// The natives the keyword model reads or answers.
pub const NATIVES: [&str; 4] = [
    "objectreference.haskeyword",
    "form.haskeyword",
    "objectreference.addkeyword",
    "objectreference.removekeyword",
];

/// The record types of placed records.
pub const PLACED: [&[u8; 4]; 10] = [
    b"REFR", b"ACHR", b"PGRE", b"PMIS", b"PHZD", b"PARW", b"PBAR", b"PBEA", b"PCON", b"PFLA",
];

/// What the records say of one object's keywords.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    /// The keywords, and whether a keyword absent from them is absent from
    /// the object (`false` where the reading does not cover every source:
    /// an actor base with a template, a weapon or armour reference whose
    /// object modifications are not read).
    Read(BTreeSet<FormKey>, bool),
    /// Nothing settles it (named).
    Open(&'static str),
}

/// The resolved FormIDs of a record's `KWDA`; `None` when the subrecord is
/// not whole words or a word names no record.
pub fn kwda(store: &Store, r: &Record, plugin: usize) -> Option<BTreeSet<FormKey>> {
    let mut out = BTreeSet::new();
    for s in r.subrecords.iter().filter(|s| s.signature == *b"KWDA") {
        let d = r.data(s);
        let (words, rest) = d.as_chunks::<4>();
        if !rest.is_empty() {
            return None;
        }
        for w in words {
            let id = u32::from_le_bytes(*w);
            let k = store
                .order()
                .remap(plugin, id, &r.source.file, s.offset)
                .ok()??;
            out.insert(k);
        }
    }
    Some(out)
}

/// The keywords the records give a base record (`rules::KEYWORD_RECORDS`):
/// its `KWDA`; for an actor base (NPC_) also its race's (`RNAM`), and an
/// actor base with a template (`TPLT`) leaves an absent keyword open.
pub fn of_base(store: &Store, base: &FormKey, b: &Budget) -> Base {
    let Ok(Some((r, plugin))) = store.raw_with_plugin(base, b) else {
        return Base::Open("the base names no winning record");
    };
    let Some(mut set) = kwda(store, &r, plugin) else {
        return Base::Open("the base's KWDA is not read");
    };
    match &r.signature {
        b"LVLN" => return Base::Open("a leveled actor base"),
        b"NPC_" => {
            let rnam = r.subrecords.iter().find(|s| s.signature == *b"RNAM");
            let race = rnam.and_then(|s| {
                let d = r.data(s);
                let w = <[u8; 4]>::try_from(d).ok()?;
                store
                    .order()
                    .remap(plugin, u32::from_le_bytes(w), &r.source.file, s.offset)
                    .ok()?
            });
            let Some(race) = race else {
                return Base::Open("the actor base names no race");
            };
            let Ok(Some((rr, rp))) = store.raw_with_plugin(&race, b) else {
                return Base::Open("the actor base's race names no winning record");
            };
            let Some(rk) = kwda(store, &rr, rp) else {
                return Base::Open("the race's KWDA is not read");
            };
            set.extend(rk);
            let templated = r.subrecords.iter().any(|s| s.signature == *b"TPLT");
            return Base::Read(set, !templated);
        }
        b"WEAP" | b"ARMO" => return Base::Read(set, false),
        _ => {}
    }
    Base::Read(set, true)
}

/// The keywords the records give an object of the run: a placed
/// reference's base's ([`of_base`]); the player's base where the caller
/// names it (`vm_player_base_object_v1`); else open.
pub fn of_object(store: &Store, max: u64, w: &World, o: ObjId) -> Base {
    let b = Budget::new(max);
    if Some(o) == w.player {
        return match &w.player_base_object {
            Some(base) => of_base(store, base, &b),
            None => Base::Open("the player's base is not named (--player-base-object)"),
        };
    }
    let Some(k) = w.objects[o].key.as_deref().and_then(key_record) else {
        return Base::Open("the receiver is not a record");
    };
    let Some(sig) = store.signature(&k) else {
        return Base::Open("the receiver names no winning record");
    };
    if !PLACED.contains(&&sig) {
        return Base::Open("the receiver is not a placed reference");
    }
    match bind::base_of(store, &k, &b) {
        Ok(Some(base)) => of_base(store, &base, &b),
        _ => Base::Open("the reference's base is not read"),
    }
}

/// The keyword record a call names.
fn keyword_arg(w: &World, v: Option<&Val>) -> Option<String> {
    match v {
        Some(Val::Object(o)) => {
            let k = w.objects[*o].key.as_deref()?;
            key_record(k).map(|f| bind::record_key(&f))
        }
        _ => None,
    }
}

/// The keyword natives. `AddKeyword` and `RemoveKeyword` are noted in
/// [`Keywords::changes`] (the call itself answers as before: `None` is
/// returned for it); `HasKeyword` on an object is answered where the
/// readings switched on settle it, else `None` (recorded as before).
pub fn native(store: &Store, max: u64, c: &NativeCall, w: &mut World) -> Option<Reply> {
    if !w.keywords.on() {
        return None;
    }
    let full = c.full_name();
    if !NATIVES.contains(&full.as_str()) {
        return None;
    }
    let Some(Val::Object(o)) = c.receiver.clone() else {
        return None;
    };
    let kw = keyword_arg(w, c.args.first());
    if full.ends_with(".addkeyword") || full.ends_with(".removekeyword") {
        let add = full.ends_with(".addkeyword");
        let what = if add { "AddKeyword" } else { "RemoveKeyword" };
        let t = w.now[crate::world::Clock::Real as usize];
        match kw {
            Some(k) => {
                w.keywords
                    .log
                    .push((t, format!("{what} on {}: {k}", w.objects[o].label)));
                w.keywords.changes.entry(o).or_default().insert(k, add);
                w.keywords.count(format!("changes kept\t{what}"));
            }
            None => {
                // Any keyword of the receiver may be the one changed: the
                // run's earlier changes on it no longer answer.
                w.keywords.changes.remove(&o);
                w.keywords.open.insert(o);
                w.keywords
                    .count(format!("changes kept\t{what}\tthe keyword is undetermined"));
            }
        }
        return None;
    }
    // HasKeyword.
    let Some(k) = kw else {
        w.keywords
            .count("HasKeyword not answered\tthe keyword is undetermined or none");
        return None;
    };
    if let Some(present) = w.keywords.changes.get(&o).and_then(|m| m.get(&k)).copied() {
        if !w.keywords.runtime {
            w.keywords.count(
                "HasKeyword not answered\tthe run changed that keyword on the receiver (--keywords-runtime off)",
            );
            return None;
        }
        w.keywords.using(rules::KEYWORD_RUNTIME);
        w.keywords.count(format!(
            "HasKeyword answered\tfrom the run's change\t{}",
            if present { "true" } else { "false" }
        ));
        return Some(Reply::Value(Val::Bool(present)));
    }
    if w.keywords.open.contains(&o) {
        w.keywords.count(
            "HasKeyword not answered\tthe run changed an undetermined keyword on the receiver",
        );
        return None;
    }
    if !w.keywords.records {
        w.keywords
            .count("HasKeyword not answered\tthe run did not change it (--keywords-records off)");
        return None;
    }
    let Some(key) = key_record(&k) else {
        w.keywords
            .count("HasKeyword not answered\tthe keyword is not a record");
        return None;
    };
    match of_object(store, max, w, o) {
        Base::Read(set, whole) => {
            let has = set.contains(&key);
            if !has && !whole {
                w.keywords.count("HasKeyword not answered\tabsent from the records read, which do not cover every source");
                return None;
            }
            w.keywords.using(rules::KEYWORD_RECORDS);
            w.keywords.count(format!(
                "HasKeyword answered\tfrom the records\t{}",
                if has { "true" } else { "false" }
            ));
            Some(Reply::Value(Val::Bool(has)))
        }
        Base::Open(why) => {
            w.keywords.count(format!("HasKeyword not answered\t{why}"));
            None
        }
    }
}
