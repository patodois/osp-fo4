//! Placed actors of a scene: from each ACHR child of its cells to the
//! skeleton and the armour addon models to draw (`spec/actor.md`).
//!
//! Every layout comes from `fo4_plugin::actor`. Which records the game uses
//! for an actor's look is behaviour, and each step that relies on one is
//! covered by a pending rule of `spec/hypotheses.tsv`; an actor whose look
//! the data does not settle is left out with its reasons. Nothing here
//! evaluates levels, chances or conditions.
pub mod clip;
pub mod pose;
#[cfg(test)]
mod tests;

use crate::{hypotheses, paths::Citation, AssetKind, Loader, Scene, State};
use fo4_core::{Budget, Error, Result, SourceSpan};
use fo4_plugin::{
    actor::{Achr, Arma, Armo, Context, Hdpt, Leveled, Npc, Otft, Race},
    FormKey, Record,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Bound on the templates and leveled-list entries followed for one actor.
pub const MAX_CANDIDATES: u64 = 256;
/// Bound on the depth of template and leveled-list chains.
pub const MAX_CHAIN: u32 = 8;
/// The bits of NPC_ ACBS u16 at 14 that any winning record sets (bits 6,
/// 13, 14 and 15 never are; `spec/actor.md`).
pub const TEMPLATE_BITS: u16 = 0b1_1111_1011_1111;

/// One armour addon model drawn for an actor.
#[derive(Debug, Serialize)]
pub struct Part {
    pub armor: FormKey,
    pub addon: FormKey,
    /// From the skin armour (NPC_ or RACE WNAM), not the outfit.
    pub skin: bool,
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub path: Vec<u8>,
    pub asset: usize,
}
/// One head part model drawn for an actor (`npc_head_parts_unmorphed_v1`).
#[derive(Debug, Serialize)]
pub struct HeadPart {
    pub part: FormKey,
    /// HDPT PNAM.
    pub kind: Option<u32>,
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub path: Vec<u8>,
    pub asset: usize,
}
/// An actor's head: its parts, the blockers of their models and the rule
/// they rest on, and the state those give. A head that is not ready at a
/// level leaves the head out, not the actor.
#[derive(Debug, Serialize)]
pub struct Head {
    pub parts: Vec<HeadPart>,
    /// The parts are the race's HEAD of the section (the NPC_ lists none).
    pub from_race: bool,
    pub blockers: Vec<Error>,
    pub state: State,
}
/// What an actor is drawn with.
#[derive(Debug, Serialize)]
pub struct Plan {
    pub race: FormKey,
    /// 0 for the MNAM section of the race and MOD2, 1 for FNAM and MOD3.
    pub section: usize,
    #[serde(serialize_with = "fo4_core::serialize_hex")]
    pub skeleton: Vec<u8>,
    pub skeleton_asset: usize,
    pub parts: Vec<Part>,
    pub head: Option<Head>,
}
#[derive(Debug, Serialize)]
pub struct Actor {
    pub reference: FormKey,
    pub source: SourceSpan,
    pub base: Option<FormKey>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub transform: Option<[f32; 6]>,
    #[serde(serialize_with = "fo4_core::ieee::serialize")]
    pub scale: Option<f32>,
    pub plan: Option<Plan>,
    /// Why the actor or a part of it is not drawn, one line each
    /// (for example `head: not read`).
    pub notes: Vec<String>,
    pub blockers: Vec<Error>,
    pub state: State,
    /// For an actor with an XESP: its chain and whether the rules say it
    /// starts enabled (`crate::enable`).
    pub enable: Option<crate::enable::Presence>,
    /// Skin a bone the skeleton lacks at its bind transform
    /// (`actor_missing_bone_bind_pose_v1`; set by a caller's switch, as
    /// `crate::timeline::cast::Options::missing_bones_at_bind`). Off: such
    /// a bone fails the shape (`ActorSkinBoneMissing`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub missing_bones_at_bind: bool,
    /// The key of the pose a posed build draws this actor in, among the
    /// poses given to `fo4_render::draw::build_with_posed_actors`, instead
    /// of its skeleton asset (`crate::timeline::cast::start_poses`); `None`:
    /// the skeleton asset is the key, as before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pose: Option<usize>,
}
impl Actor {
    /// Whether the enable parent rules say this actor starts disabled.
    pub fn disabled_by_enable_parent(&self) -> bool {
        self.enable
            .as_ref()
            .is_some_and(|p| p.enabled == Some(false))
    }
}
#[derive(Debug, Serialize, Default)]
pub struct Actors {
    pub actors: Vec<Actor>,
    /// ACHR children that are initially disabled or in a distant group.
    pub disabled: u64,
    pub distant: u64,
}
impl Actors {
    /// Actors with a plan whose state is drawable at `level`.
    pub fn drawable(&self, level: crate::RuleLevel) -> impl Iterator<Item = &Actor> {
        self.actors.iter().filter(move |a| {
            a.plan.is_some() && a.state.ready_at(level) && !a.disabled_by_enable_parent()
        })
    }
}

fn note(notes: &mut Vec<String>, line: String, b: &Budget) -> Result<()> {
    b.charge(line.len() as u64 + 32, "actor note", 0)?;
    b.reserve(notes, 1, "actor note", 0)?;
    notes.push(line);
    Ok(())
}
fn rule(code: &str, src: &SourceSpan, found: impl std::fmt::Display) -> Error {
    Error::new(code, &src.file, src.offset, "registered actor rule", found)
}
fn read(loader: &Loader<'_>, key: &FormKey, b: &Budget) -> Result<Option<(Record, usize)>> {
    loader.plugins.raw_with_plugin(key, b)
}
fn ctx<'a>(loader: &'a Loader<'_>, plugin: usize) -> Context<'a> {
    Context {
        order: loader.plugins.order(),
        plugin,
    }
}
fn short(key: &FormKey) -> String {
    format!("{}:{:06x}", key.plugin, key.object_id)
}

/// The values a templated or leveled actor base may take, one set per
/// property: a property is settled when its set holds one value.
#[derive(Default)]
struct Look {
    race: BTreeSet<FormKey>,
    section: BTreeSet<usize>,
    skin: BTreeSet<Option<FormKey>>,
    outfit: BTreeSet<Option<FormKey>>,
    templated: bool,
    leveled: bool,
    /// A leveled list followed has a non-zero LVLF (`npc_leveled_list_flags_v1`).
    leveled_flags: bool,
    /// An NPC_ with every template bit set gave no own values
    /// (`npc_template_all_bits_v1`).
    all_bits: bool,
    /// Per NPC_ visited: its PNAM head parts, sorted, or its race's HEAD of
    /// its section when it lists none (marked true).
    heads: BTreeSet<(Vec<FormKey>, bool)>,
    visits: u64,
}
fn gather(
    loader: &Loader<'_>,
    key: &FormKey,
    depth: u32,
    look: &mut Look,
    b: &Budget,
) -> std::result::Result<(), String> {
    look.visits += 1;
    if look.visits > MAX_CANDIDATES || depth > MAX_CHAIN {
        return Err(format!(
            "template: more than {MAX_CANDIDATES} candidates or {MAX_CHAIN} levels"
        ));
    }
    let work = b.child();
    let Some((r, plugin)) =
        read(loader, key, &work).map_err(|e| format!("base {}: {}", short(key), e.code))?
    else {
        return Err(format!("base {}: no winning record", short(key)));
    };
    if r.deleted() {
        return Err(format!("base {}: deleted", short(key)));
    }
    match &r.signature {
        b"LVLN" => {
            let l = Leveled::decode(&r, ctx(loader, plugin), &work)
                .map_err(|e| format!("leveled list {}: {}", short(key), e.code))?;
            // LVLD and LVLM not zero: refused. LVLF not zero: followed, every
            // entry a candidate, under its own rule.
            if l.bytes[..2].iter().any(|v| v.is_some_and(|v| v != 0)) {
                return Err(format!("template: leveled list {} with LVLD or LVLM not zero (meaning not established)", short(key)));
            }
            if l.bytes[2].is_some_and(|v| v != 0) {
                look.leveled_flags = true;
            }
            if l.entries.is_empty() {
                return Err(format!("template: leveled list {} is empty", short(key)));
            }
            look.leveled = true;
            for e in &l.entries {
                let k = e
                    .reference
                    .key
                    .clone()
                    .ok_or_else(|| format!("leveled list {}: null entry", short(key)))?;
                gather(loader, &k, depth + 1, look, b)?;
            }
            Ok(())
        }
        b"NPC_" => {
            let n = Npc::decode(&r, ctx(loader, plugin), &work)
                .map_err(|e| format!("base {}: {}", short(key), e.code))?;
            let race = n
                .race
                .as_ref()
                .and_then(|f| f.key.clone())
                .ok_or_else(|| format!("base {}: no race", short(key)))?;
            let flags = n
                .flags
                .ok_or_else(|| format!("base {}: no ACBS", short(key)))?;
            let Some((rr, rp)) = read(loader, &race, &work)
                .map_err(|e| format!("race {}: {}", short(&race), e.code))?
            else {
                return Err(format!("race {}: no winning record", short(&race)));
            };
            let race_record = Race::decode(&rr, ctx(loader, rp), &work)
                .map_err(|e| format!("race {}: {}", short(&race), e.code))?;
            let mut heads: Vec<FormKey> =
                n.head_parts.iter().filter_map(|f| f.key.clone()).collect();
            let from_race = heads.is_empty();
            if from_race {
                heads.extend(
                    race_record.section_head_parts[(flags & 1) as usize]
                        .iter()
                        .filter_map(|f| f.key.clone()),
                );
            }
            heads.sort();
            heads.dedup();
            let skin = match n.skin.as_ref().and_then(|f| f.key.clone()) {
                Some(k) => Some(k),
                None => race_record.skin.and_then(|f| f.key),
            };
            let tf = n.template_flags.unwrap_or(0);
            // With every template bit set, every property comes from the
            // templates: the own values are no candidates.
            if tf & TEMPLATE_BITS == TEMPLATE_BITS {
                look.all_bits = true;
            } else {
                look.heads.insert((heads, from_race));
                look.race.insert(race);
                look.section.insert((flags & 1) as usize);
                look.skin.insert(skin);
                look.outfit.insert(n.outfit.and_then(|f| f.key));
            }
            if tf != 0 {
                look.templated = true;
                let mut targets: Vec<FormKey> = Vec::new();
                match &n.template_slots {
                    Some(slots) => {
                        for (k, s) in slots.iter().enumerate() {
                            if tf & (1 << k) != 0 {
                                if let Some(t) = s.as_ref().and_then(|f| f.key.clone()) {
                                    if !targets.contains(&t) {
                                        targets.push(t);
                                    }
                                }
                            }
                        }
                    }
                    None => targets.extend(n.template.and_then(|f| f.key)),
                }
                if targets.is_empty() {
                    return Err(format!(
                        "template: base {} has template flags but no template",
                        short(key)
                    ));
                }
                for t in &targets {
                    gather(loader, t, depth + 1, look, b)?;
                }
            }
            Ok(())
        }
        other => Err(format!(
            "base {}: record type {}",
            short(key),
            String::from_utf8_lossy(other)
        )),
    }
}

/// The armours an outfit puts on: its ARMO entries, and the one ARMO every
/// entry of a leveled item list gives.
fn outfit_armors(
    loader: &Loader<'_>,
    key: &FormKey,
    notes: &mut Vec<String>,
    b: &Budget,
) -> Result<std::result::Result<Vec<FormKey>, String>> {
    let Some((r, plugin)) = read(loader, key, b)? else {
        return Ok(Err(format!("outfit {}: no winning record", short(key))));
    };
    let o = Otft::decode(&r, ctx(loader, plugin), b)?;
    let mut out = Vec::new();
    for item in &o.items {
        let Some(k) = item.key.clone() else {
            return Ok(Err("outfit: null entry".into()));
        };
        match loader.plugins.signature(&k) {
            Some(s) if s == *b"ARMO" => out.push(k),
            Some(s) if s == *b"LVLI" => {
                let mut found = BTreeSet::new();
                if let Err(reason) = leveled_items(loader, &k, 0, &mut found, &mut 0, b)? {
                    return Ok(Err(reason));
                }
                if found.len() != 1 {
                    return Ok(Err(format!(
                        "outfit: leveled item list {} gives {} different armours",
                        short(&k),
                        found.len()
                    )));
                }
                note(
                    notes,
                    format!("outfit: leveled item list {} gives one armour", short(&k)),
                    b,
                )?;
                out.extend(found);
            }
            s => {
                return Ok(Err(format!(
                    "outfit: entry of type {:?}",
                    s.map(|s| String::from_utf8_lossy(&s).into_owned())
                )));
            }
        }
    }
    Ok(Ok(out))
}
fn leveled_items(
    loader: &Loader<'_>,
    key: &FormKey,
    depth: u32,
    found: &mut BTreeSet<FormKey>,
    visits: &mut u64,
    b: &Budget,
) -> Result<std::result::Result<(), String>> {
    *visits += 1;
    if *visits > MAX_CANDIDATES || depth > MAX_CHAIN {
        return Ok(Err("outfit: leveled item lists too large or deep".into()));
    }
    let work = b.child();
    let Some((r, plugin)) = read(loader, key, &work)? else {
        return Ok(Err(format!(
            "outfit: list {} has no winning record",
            short(key)
        )));
    };
    let l = Leveled::decode(&r, ctx(loader, plugin), &work)?;
    if l.bytes.iter().any(|v| v.is_some_and(|v| v != 0)) {
        return Ok(Err(format!(
            "outfit: leveled item list {} with LVLD/LVLM/LVLF not zero (meaning not established)",
            short(key)
        )));
    }
    if l.entries.is_empty() {
        return Ok(Err(format!(
            "outfit: leveled item list {} is empty",
            short(key)
        )));
    }
    for e in &l.entries {
        let Some(k) = e.reference.key.clone() else {
            return Ok(Err("outfit: null leveled entry".into()));
        };
        match loader.plugins.signature(&k) {
            Some(s) if s == *b"ARMO" => {
                found.insert(k);
            }
            Some(s) if s == *b"LVLI" => {
                if let Err(reason) = leveled_items(loader, &k, depth + 1, found, visits, b)? {
                    return Ok(Err(reason));
                }
            }
            s => {
                return Ok(Err(format!(
                    "outfit: leveled entry of type {:?}",
                    s.map(|s| String::from_utf8_lossy(&s).into_owned())
                )))
            }
        }
    }
    Ok(Ok(()))
}

/// Codes of the asset graph that actor skinning reads instead of the
/// static frame: they are set aside for actor models only.
fn handled_by_skinning(e: &Error) -> bool {
    e.code == "UnsupportedSkinning" || e.found.starts_with("BSSkin::")
}

/// Resolves every placed actor of `scene` and loads its models into the
/// loader's asset graph.
pub fn resolve(scene: &Scene, loader: &mut Loader<'_>, b: &Budget) -> Result<Actors> {
    let mut out = Actors::default();
    let mut children = Vec::new();
    for e in &scene.exclusions {
        if e.child.signature != *b"ACHR" {
            continue;
        }
        if e.child.group_kind == 10 {
            out.distant += 1;
            continue;
        }
        b.reserve(&mut children, 1, "actors", 0)?;
        children.push(&e.child);
    }
    out.disabled = scene
        .disabled
        .iter()
        .filter(|c| c.signature == *b"ACHR")
        .count() as u64;
    // Same order as the draw list's placed references: plugin, object ID.
    children
        .sort_by(|a, b| (&a.key.plugin, a.key.object_id).cmp(&(&b.key.plugin, b.key.object_id)));
    for child in children {
        let actor = resolve_reference(loader, &child.key, &child.source, b)?;
        b.reserve(&mut out.actors, 1, "actors", 0)?;
        out.actors.push(actor);
    }
    Ok(out)
}

/// Resolves one placed actor (ACHR) `key` read at `source`, as [`resolve`]
/// does for each actor of a scene.
pub fn resolve_reference(
    loader: &mut Loader<'_>,
    key: &FormKey,
    source: &SourceSpan,
    b: &Budget,
) -> Result<Actor> {
    let mut actor = Actor {
        reference: key.clone(),
        source: source.clone(),
        base: None,
        transform: None,
        scale: None,
        plan: None,
        notes: Vec::new(),
        blockers: Vec::new(),
        state: State::Blocked,
        enable: None,
        missing_bones_at_bind: false,
        pose: None,
    };
    let work = b.child();
    let Some((r, plugin)) = read(loader, key, &work)? else {
        note(&mut actor.notes, "reference: no winning record".into(), b)?;
        return Ok(actor);
    };
    let achr = Achr::decode(&r, ctx(loader, plugin), &work)?;
    actor.transform = achr.transform;
    actor.scale = achr.scale;
    actor.base = achr.base.as_ref().and_then(|f| f.key.clone());
    let Some(base) = actor.base.clone() else {
        note(&mut actor.notes, "base: NAME absent or null".into(), b)?;
        return Ok(actor);
    };
    if actor.transform.is_none() {
        note(&mut actor.notes, "placement: DATA absent".into(), b)?;
        return Ok(actor);
    }
    let mut look = Look::default();
    if let Err(reason) = gather(loader, &base, 0, &mut look, &work) {
        note(&mut actor.notes, reason, b)?;
        return Ok(actor);
    }
    let mut unsettled = Vec::new();
    for (name, n) in [
        ("race", look.race.len()),
        ("section (ACBS bit 0)", look.section.len()),
        ("skin", look.skin.len()),
        ("outfit", look.outfit.len()),
    ] {
        if n != 1 {
            unsettled.push(format!("{name}: {n} candidates"));
        }
    }
    if !unsettled.is_empty() {
        let source = if look.leveled {
            "leveled list"
        } else {
            "template"
        };
        note(
            &mut actor.notes,
            format!("{source}: {}", unsettled.join(", ")),
            b,
        )?;
        return Ok(actor);
    }
    let race = look.race.pop_first().expect("one race");
    let section = look.section.pop_first().expect("one section");
    let skin = look.skin.pop_first().expect("one skin");
    let outfit = look.outfit.pop_first().expect("one outfit");
    let src = &actor.source;
    let mut blockers = Vec::new();
    let rules = |blockers: &mut Vec<Error>, code: &str, found: &str| -> Result<()> {
        b.reserve(blockers, 1, &src.file, src.offset)?;
        blockers.push(rule(code, src, found));
        Ok(())
    };
    rules(
        &mut blockers,
        "UnverifiedActorPlacement",
        "ACHR DATA/XSCL placed as REFR",
    )?;
    if r.subrecords
        .iter()
        .any(|s| s.signature == fo4_plugin::enable::XESP)
    {
        if let Some(chain) = fo4_plugin::enable::chain(loader.plugins, key, &work)? {
            crate::enable::blockers(&chain, src, &mut blockers, b)?;
            actor.enable = Some(crate::enable::Presence::of(chain));
        }
    }
    rules(
        &mut blockers,
        "UnverifiedActorSection",
        if section == 0 {
            "ACBS bit 0 clear: MNAM skeleton, MOD2"
        } else {
            "ACBS bit 0 set: FNAM skeleton, MOD3"
        },
    )?;
    rules(
        &mut blockers,
        "UnverifiedActorAddons",
        "every addon of a worn ARMO serving the race",
    )?;
    rules(
        &mut blockers,
        "UnverifiedActorSkinning",
        "linear blend of the skeleton's rest pose",
    )?;
    if look.templated || look.leveled {
        rules(
            &mut blockers,
            "UnverifiedActorCandidates",
            "every template or leveled candidate agrees",
        )?;
    }
    if look.leveled_flags {
        rules(
            &mut blockers,
            "UnverifiedActorLeveledFlags",
            "leveled list with LVLF not zero: every entry a candidate",
        )?;
    }
    if look.all_bits {
        rules(
            &mut blockers,
            "UnverifiedActorTemplateAllBits",
            "every template bit set: no own values",
        )?;
    }
    // Race: skeleton of the section.
    let Some((rr, rp)) = read(loader, &race, &work)? else {
        note(
            &mut actor.notes,
            format!("race {}: no winning record", short(&race)),
            b,
        )?;
        return Ok(actor);
    };
    let race_record = Race::decode(&rr, ctx(loader, rp), &work)?;
    let Some(skeleton) = race_record.skeletons[section].clone() else {
        note(
            &mut actor.notes,
            format!(
                "race {}: no skeleton (ANAM) in section {}",
                short(&race),
                section + 1
            ),
            b,
        )?;
        return Ok(actor);
    };
    // Worn armours: the skin, then the outfit's.
    let mut worn: Vec<(FormKey, bool)> = Vec::new();
    match &skin {
        Some(k) => worn.push((k.clone(), true)),
        None => note(
            &mut actor.notes,
            "skin: none (NPC_ and RACE WNAM absent)".into(),
            b,
        )?,
    }
    match &outfit {
        Some(o) => {
            rules(&mut blockers, "UnverifiedActorOutfit", "DOFT outfit worn")?;
            match outfit_armors(loader, o, &mut actor.notes, &work)? {
                Ok(list) => {
                    for k in list {
                        worn.push((k, false));
                    }
                }
                Err(reason) => {
                    note(&mut actor.notes, reason, b)?;
                    note(
                        &mut actor.notes,
                        "outfit: unresolved; actor not drawn".into(),
                        b,
                    )?;
                    return Ok(actor);
                }
            }
        }
        None => note(&mut actor.notes, "outfit: none declared".into(), b)?,
    }
    let mut armors = Vec::new();
    for (k, is_skin) in &worn {
        let Some((ar, ap)) = read(loader, k, &work)? else {
            note(
                &mut actor.notes,
                format!("armour {}: no winning record", short(k)),
                b,
            )?;
            return Ok(actor);
        };
        armors.push((
            k.clone(),
            *is_skin,
            Armo::decode(&ar, ctx(loader, ap), &work)?,
        ));
    }
    let outfit_slots = armors
        .iter()
        .filter(|a| !a.1)
        .fold(0u32, |s, a| s | a.2.slots.unwrap_or(0));
    if armors.iter().any(|a| !a.1) {
        rules(
            &mut blockers,
            "UnverifiedActorSkinSlots",
            "skin addons sharing a BOD2 bit with a worn armour are hidden",
        )?;
    }
    let mut chosen: Vec<(FormKey, FormKey, bool, Vec<u8>)> = Vec::new();
    for (armor, is_skin, a) in &armors {
        let mut served = 0;
        for addon in &a.addons {
            let Some(ak) = addon.addon.key.clone() else {
                continue;
            };
            let Some((xr, xp)) = read(loader, &ak, &work)? else {
                note(
                    &mut actor.notes,
                    format!("addon {}: no winning record", short(&ak)),
                    b,
                )?;
                return Ok(actor);
            };
            let arma = Arma::decode(&xr, ctx(loader, xp), &work)?;
            if !arma.serves(&race) {
                continue;
            }
            served += 1;
            if *is_skin && arma.slots.unwrap_or(0) & outfit_slots != 0 {
                note(
                    &mut actor.notes,
                    format!("skin addon {}: hidden by worn armour slots", short(&ak)),
                    b,
                )?;
                continue;
            }
            match arma.models[section].clone() {
                Some(p) => chosen.push((armor.clone(), ak, *is_skin, p)),
                None => {
                    note(
                        &mut actor.notes,
                        format!(
                            "addon {}: no model (MOD{}) for section {}; actor not drawn",
                            short(&ak),
                            section + 2,
                            section + 1
                        ),
                        b,
                    )?;
                    return Ok(actor);
                }
            }
        }
        if served == 0 {
            note(
                &mut actor.notes,
                format!(
                    "armour {}: no addon for race {}",
                    short(armor),
                    short(&race)
                ),
                b,
            )?;
        }
    }
    if chosen.is_empty() {
        note(&mut actor.notes, "no model to draw".into(), b)?;
        return Ok(actor);
    }
    // Paths and assets.
    let mut path_rules: Vec<&'static str> = Vec::new();
    let resolve_model = |loader: &mut Loader<'_>,
                         raw: &[u8],
                         path_rules: &mut Vec<&'static str>|
     -> Result<(Vec<u8>, usize)> {
        let resolved = loader.assets.resolve_path(Citation::RecordModel, raw, b)?;
        for h in &resolved.hypotheses {
            if !path_rules.contains(h) {
                path_rules.push(h);
            }
        }
        let id = loader.assets.load(AssetKind::Model, &resolved.key, b)?;
        Ok((resolved.key, id))
    };
    let (skeleton_key, skeleton_asset) = match resolve_model(loader, &skeleton, &mut path_rules) {
        Ok(v) => v,
        Err(e) => {
            note(&mut actor.notes, format!("skeleton: {}", e.code), b)?;
            return Ok(actor);
        }
    };
    let mut parts = Vec::new();
    for (armor, addon, is_skin, raw) in chosen {
        match resolve_model(loader, &raw, &mut path_rules) {
            Ok((path, asset)) => {
                b.reserve(&mut parts, 1, "actor parts", 0)?;
                parts.push(Part {
                    armor,
                    addon,
                    skin: is_skin,
                    path,
                    asset,
                });
            }
            Err(e) => {
                note(
                    &mut actor.notes,
                    format!("addon {} model: {}", short(&addon), e.code),
                    b,
                )?;
                return Ok(actor);
            }
        }
    }
    // Blockers of the models' closures, apart from what skinning reads.
    let roots: Vec<usize> = parts.iter().map(|p| p.asset).collect();
    for i in loader.assets.closure(&roots, &work)? {
        for e in &loader.assets.nodes[i].blockers {
            if !handled_by_skinning(e) {
                b.reserve(&mut blockers, 1, &src.file, src.offset)?;
                blockers.push(e.clone());
            }
        }
    }
    for h in path_rules {
        let code = match h {
            hypotheses::PATH_STRIP => "HypothesisAssetDataPrefixStrip",
            hypotheses::PATH_ROOT => "HypothesisAssetMissingRoot",
            _ => "HypothesisAssetDataRelativeStrip",
        };
        rules(&mut blockers, code, "actor model path")?;
    }
    actor.state = hypotheses::classify(&blockers, false, b)?;
    actor.blockers = blockers;
    let head = match look.heads.len() {
        0 => None,
        1 => {
            let (list, from_race) = look.heads.pop_first().expect("one head list");
            head(loader, &list, from_race, &parts, src, &mut actor.notes, b)?
        }
        n => {
            note(
                &mut actor.notes,
                format!("head: {n} candidate part lists; not drawn"),
                b,
            )?;
            None
        }
    };
    actor.plan = Some(Plan {
        race,
        section,
        skeleton: skeleton_key,
        skeleton_asset,
        parts,
        head,
    });
    Ok(actor)
}

/// Per bone name of the skinned shapes of model `asset`: the sum of its
/// vertex weights and the number of vertices whose largest weight it has;
/// and the number of vertices.
type Profile = (BTreeMap<Vec<u8>, (f64, u64)>, u64);
fn skin_profile(loader: &Loader<'_>, asset: usize, b: &Budget) -> Result<Profile> {
    let scratch = b.child();
    let out = loader.assets.with_decoded(asset, &scratch, |d| {
        let crate::DecodedAsset::Model(n) = d else {
            return Ok((BTreeMap::new(), 0));
        };
        let file = String::from_utf8_lossy(&loader.assets.nodes[asset].path).into_owned();
        let mut bones: BTreeMap<Vec<u8>, (f64, u64)> = BTreeMap::new();
        let mut vertices = 0u64;
        for i in 0..n.blocks.len() {
            let Some(fo4_nif::Data::Mesh(m)) = &n.blocks[i].decoded else {
                continue;
            };
            let Some(skin) =
                fo4_nif::skin::shape_skin(&n, i, &file, loader.vfs.limits(), &scratch)?
            else {
                continue;
            };
            let names: Vec<Option<&[u8]>> = skin
                .bones
                .iter()
                .map(|bone| pose::name(&n, bone.node))
                .collect();
            for v in m.vertices.iter().flatten() {
                let Some(w) = fo4_nif::skin::vertex_weights(n.bytes, v) else {
                    continue;
                };
                vertices += 1;
                let mut best: Option<(f32, usize)> = None;
                for (weight, bone) in w.weights.iter().zip(w.bones) {
                    let Some(Some(name)) = names.get(bone as usize) else {
                        continue;
                    };
                    if *weight > 0. {
                        bones.entry(name.to_vec()).or_default().0 += f64::from(*weight);
                        if best.is_none_or(|(x, _)| *weight > x) {
                            best = Some((*weight, bone as usize));
                        }
                    }
                }
                if let Some((_, k)) = best {
                    if let Some(Some(name)) = names.get(k) {
                        bones.entry(name.to_vec()).or_default().1 += 1;
                    }
                }
            }
        }
        Ok((bones, vertices))
    })?;
    b.charge(
        out.0.keys().map(|n| n.len() as u64 + 48).sum(),
        "actor bone profile",
        0,
    )?;
    Ok(out)
}

/// The head of an actor whose head part list is settled
/// (`npc_head_parts_unmorphed_v1`): every listed part's model, unless a
/// part has conditions or a worn outfit addon covers the head (has vertices
/// whose largest weight is on the bone carrying most of the head parts'
/// weight).
fn head(
    loader: &mut Loader<'_>,
    list: &[FormKey],
    from_race: bool,
    parts: &[Part],
    src: &SourceSpan,
    notes: &mut Vec<String>,
    b: &Budget,
) -> Result<Option<Head>> {
    if list.is_empty() {
        return Ok(None);
    }
    let work = b.child();
    let mut head = Head {
        parts: Vec::new(),
        from_race,
        blockers: Vec::new(),
        state: State::Blocked,
    };
    let mut path_rules: Vec<&'static str> = Vec::new();
    for k in list {
        let Some((r, plugin)) = read(loader, k, &work)? else {
            note(
                notes,
                format!("head: part {} has no winning record; not drawn", short(k)),
                b,
            )?;
            return Ok(None);
        };
        if r.signature != *b"HDPT" {
            note(
                notes,
                format!("head: part {} is not an HDPT; not drawn", short(k)),
                b,
            )?;
            return Ok(None);
        }
        let h = Hdpt::decode(&r, ctx(loader, plugin), &work)?;
        if h.conditions > 0 {
            note(
                notes,
                format!("head: part {} has conditions; not drawn", short(k)),
                b,
            )?;
            return Ok(None);
        }
        let Some(raw) = h.model else {
            note(
                notes,
                format!("head: part {} has no model; not drawn", short(k)),
                b,
            )?;
            return Ok(None);
        };
        let resolved = match loader.assets.resolve_path(Citation::RecordModel, &raw, b) {
            Ok(r) => r,
            Err(e) => {
                note(
                    notes,
                    format!("head: part {} model: {}; not drawn", short(k), e.code),
                    b,
                )?;
                return Ok(None);
            }
        };
        for p in &resolved.hypotheses {
            if !path_rules.contains(p) {
                path_rules.push(p);
            }
        }
        let asset = loader.assets.load(AssetKind::Model, &resolved.key, b)?;
        b.reserve(&mut head.parts, 1, "actor head", 0)?;
        head.parts.push(HeadPart {
            part: k.clone(),
            kind: h.kind,
            path: resolved.key,
            asset,
        });
    }
    // The head's main bone: the one carrying the largest share of the head
    // parts' vertex weight. A worn outfit addon with vertices whose largest
    // weight is on that bone covers the head.
    let mut head_bones: BTreeMap<Vec<u8>, (f64, u64)> = BTreeMap::new();
    for p in &head.parts {
        for (k, v) in skin_profile(loader, p.asset, &work)?.0 {
            let e = head_bones.entry(k).or_default();
            e.0 += v.0;
            e.1 += v.1;
        }
    }
    let main = head_bones
        .iter()
        .max_by(|a, b| a.1 .0.total_cmp(&b.1 .0))
        .map(|(k, _)| k.clone());
    if let Some(main) = main {
        for p in parts.iter().filter(|p| !p.skin) {
            let (bones, total) = skin_profile(loader, p.asset, &work)?;
            let on_main = bones.get(&main).map_or(0, |v| v.1);
            if on_main > 0 {
                note(
                    notes,
                    format!(
                        "head: covered by worn armour {} ({on_main} of {total} vertices mostly on the head's main bone); not drawn",
                        short(&p.armor)
                    ),
                    b,
                )?;
                return Ok(None);
            }
        }
    }
    let rule = |code: &str, found: &str| rule(code, src, found);
    head.blockers.push(rule(
        "UnverifiedActorHeadParts",
        "listed head parts drawn unmorphed",
    ));
    for h in path_rules {
        let code = match h {
            hypotheses::PATH_STRIP => "HypothesisAssetDataPrefixStrip",
            hypotheses::PATH_ROOT => "HypothesisAssetMissingRoot",
            _ => "HypothesisAssetDataRelativeStrip",
        };
        head.blockers.push(rule(code, "actor head model path"));
    }
    let roots: Vec<usize> = head.parts.iter().map(|p| p.asset).collect();
    for i in loader.assets.closure(&roots, &work)? {
        for e in &loader.assets.nodes[i].blockers {
            if !handled_by_skinning(e) {
                b.reserve(&mut head.blockers, 1, &src.file, src.offset)?;
                head.blockers.push(e.clone());
            }
        }
    }
    head.state = hypotheses::classify(&head.blockers, false, b)?;
    note(
        notes,
        format!(
            "head: {} parts{}",
            head.parts.len(),
            if from_race { " (race default)" } else { "" }
        ),
        b,
    )?;
    Ok(Some(head))
}
