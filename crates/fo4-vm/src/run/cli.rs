//! The command line of `quest_run`: [`parse`] reads it into a [`Command`],
//! [`Command::run`] opens the data and runs the mode.
use super::*;
use std::path::PathBuf;

/// What the command line sets besides [`Options`] and [`Then`]: the
/// condition evaluator's switches and the caller's assumptions, the scene
/// clock, and the timeline's file. Each run reads them from here (the
/// example once kept them per thread).
#[derive(Clone)]
pub struct Settings {
    /// The condition evaluator switches (`--no-cond-fn-N`,
    /// `--no-cond-run-on`) and the caller's assumptions (`--assume-cond
    /// N=VALUE`).
    pub cond: (crate::cond::Switches, BTreeMap<u16, f32>),
    /// The caller's assumptions scoped to one quest's records
    /// (`--assume-cond-for QUEST N=VALUE`).
    pub cond_for: BTreeMap<String, BTreeMap<u16, f32>>,
    /// The scene clock and the caller's assumptions as the command line
    /// gives them; [`Command::run`] turns it on (with the voice files'
    /// seconds unless `--no-line-audio`) when `--scene-clock N` is given,
    /// and leaves the default clock otherwise.
    pub clock: scene::Clock,
    /// Lines timed by their audio (not `--no-line-audio`).
    pub line_audio: bool,
    /// `--timeline FILE`: the new-game run writes its timeline there.
    pub timeline: Option<PathBuf>,
    /// The groups of kind 10, read by [`Command::run`] when scenes are on.
    pub groups: Option<Rc<scene::Groups>>,
}

/// A parsed command line: `MANIFEST MODE ARG` and the switches.
pub struct Command {
    pub manifest: PathBuf,
    /// `stages`, `new-game`, `fragments`, `scenes`, or a quest's editor ID.
    pub mode: String,
    /// OUT_DIR, or the stage of `QUEST STAGE`.
    pub arg: String,
    pub options: Options,
    pub then: Then,
    pub settings: Settings,
}

/// Parses and runs a `quest_run` command line (without the program's
/// name); a new-game run with the scene clock on is stepped by `stepper`
/// when one is given ([`Stepper`]).
pub fn main_with(
    args: Vec<String>,
    seconds: HeaderSeconds<'_>,
    stepper: Option<&mut dyn Stepper>,
) -> Result<(), E> {
    parse(args)?.run(seconds, stepper)
}

/// Reads a `quest_run` command line (without the program's name). Every
/// usage error is named here; the data is not opened.
pub fn parse(mut a: Vec<String>) -> Result<Command, E> {
    let flag = |a: &mut Vec<String>, f: &str| {
        let on = a.iter().any(|x| x == f);
        a.retain(|x| x != f);
        on
    };
    let mut timeline = None;
    if let Some(i) = a.iter().position(|x| x == "--timeline") {
        let f = a.get(i + 1).cloned().ok_or("--timeline FILE")?;
        timeline = Some(PathBuf::from(f));
        a.drain(i..i + 2);
    }
    let no_custom = flag(&mut a, "--no-custom");
    let custom_nested = flag(&mut a, "--custom-nested");
    let opts = Options {
        nested: !flag(&mut a, "--queued"),
        conditions: !flag(&mut a, "--no-conditions"),
        stage_event: !flag(&mut a, "--no-stage-event"),
        remote: !flag(&mut a, "--no-remote"),
        custom: match (no_custom, custom_nested) {
            (true, _) => None,
            (false, true) => Some(CustomOrder::Nested),
            (false, false) => Some(CustomOrder::Queued),
        },
        story: !flag(&mut a, "--no-story"),
        enable_chain: !flag(&mut a, "--no-enable-chain"),
        scenes: !flag(&mut a, "--no-scenes"),
        scenes_with_quest: !flag(&mut a, "--no-scenes-with-quest"),
        stage_fields: flag(&mut a, "--stage-fields"),
        action_conditions: flag(&mut a, "--cond-550-clock"),
        location_fills: flag(&mut a, "--alias-location-refs"),
        line_fields: flag(&mut a, "--line-stage-fields"),
        lines: crate::line::Switches {
            fragments: flag(&mut a, "--line-fragments"),
            start_scene: flag(&mut a, "--line-start-scene"),
        },
        none_arrays: crate::NoneArrays {
            length: flag(&mut a, "--none-array-length"),
            read: flag(&mut a, "--none-array-read"),
            find: flag(&mut a, "--none-array-find"),
            change: flag(&mut a, "--none-array-change"),
        },
        child_flag_ignored: flag(&mut a, "--enable-child-flag-ignored"),
        action_percent: flag(&mut a, "--cond-131-clock"),
        conditions_end: flag(&mut a, "--phase-conditions-end"),
        texts_ignore_case: flag(&mut a, "--text-equal-ignore-case"),
        auto_state_inherited: flag(&mut a, "--auto-state-inherited"),
        property_implicit_instance: flag(&mut a, "--property-implicit-instance"),
    };
    let mut switches = crate::cond::Switches::all();
    let mut assume: BTreeMap<u16, f32> = BTreeMap::new();
    a.retain(|x| {
        if let Some(n) = x
            .strip_prefix("--no-cond-fn-")
            .and_then(|n| n.parse::<u16>().ok())
        {
            switches.off(n);
            false
        } else if x == "--no-cond-run-on" {
            switches.run_on_off = true;
            false
        } else {
            true
        }
    });
    while let Some(i) = a.iter().position(|x| x == "--assume-cond") {
        let v = a.get(i + 1).cloned().ok_or("--assume-cond N=VALUE")?;
        let (f, x) = v.split_once('=').ok_or("--assume-cond N=VALUE")?;
        assume.insert(f.parse()?, x.parse()?);
        a.drain(i..i + 2);
    }
    let mut assume_for: BTreeMap<String, BTreeMap<u16, f32>> = BTreeMap::new();
    while let Some(i) = a.iter().position(|x| x == "--assume-cond-for") {
        let (Some(q), Some(v)) = (a.get(i + 1).cloned(), a.get(i + 2).cloned()) else {
            return Err("--assume-cond-for PLUGIN:OBJECTID N=VALUE".into());
        };
        let q = crate::player::parse_key(&q).ok_or("--assume-cond-for PLUGIN:OBJECTID N=VALUE")?;
        let (f, x) = v
            .split_once('=')
            .ok_or("--assume-cond-for PLUGIN:OBJECTID N=VALUE")?;
        assume_for
            .entry(crate::bind::record_key(&q))
            .or_default()
            .insert(f.parse()?, x.parse()?);
        a.drain(i..i + 3);
    }
    let mut then = Then {
        next_blockers: flag(&mut a, "--next-blockers"),
        recheck: flag(&mut a, "--scene-recheck").then(|| std::cell::Cell::new(0)),
        recheck_variables: flag(&mut a, "--scene-recheck-variables"),
        outer_calls: flag(&mut a, "--setters-outer-calls"),
        unique_fills: flag(&mut a, "--alias-unique-actors"),
        player_no_record: flag(&mut a, "--player-no-record"),
        location_distinct: flag(&mut a, "--location-refs-distinct"),
        location_placed: flag(&mut a, "--alias-location-placed-refs"),
        location_shared: flag(&mut a, "--location-refs-shared"),
        hit_events: flag(&mut a, "--hit-events"),
        hit_init: flag(&mut a, "--hit-init-registrations"),
        combat_events: flag(&mut a, "--combat-state-events"),
        death_events: flag(&mut a, "--death-events"),
        dead_state: flag(&mut a, "--dead-state"),
        combat_natives: flag(&mut a, "--combat-state-natives"),
        global_values: flag(&mut a, "--global-values"),
        global_value_undetermined: flag(&mut a, "--global-value-undetermined"),
        census_run_state: flag(&mut a, "--census-run-state"),
        lock_state: flag(&mut a, "--lock-state"),
        lock_events: flag(&mut a, "--lock-events"),
        lock_events_script: flag(&mut a, "--lock-events-script"),
        terminal_fragments: flag(&mut a, "--terminal-fragments"),
        item_added_events: flag(&mut a, "--item-added-events"),
        inventory_filter_init: flag(&mut a, "--inventory-filter-init"),
        item_added_counts: flag(&mut a, "--item-added-counts"),
        perks_base: flag(&mut a, "--perks-base"),
        factions_base: flag(&mut a, "--factions-base"),
        alias_created: flag(&mut a, "--alias-created-refs"),
        alias_external: flag(&mut a, "--alias-external"),
        line_script_events: flag(&mut a, "--line-script-events"),
        quest_partial_previous: flag(&mut a, "--quest-partial-previous"),
        container_events: flag(&mut a, "--container-change-events"),
        location_events: flag(&mut a, "--location-change-events"),
        location_parents: flag(&mut a, "--location-parents"),
        timers_per_timer: flag(&mut a, "--timers-per-timer"),
        timeline_deaths: flag(&mut a, "--timeline-deaths"),
        timeline_sits: flag(&mut a, "--timeline-sits"),
        timeline_sequences: flag(&mut a, "--timeline-sequences"),
        cond_distance: flag(&mut a, "--cond-1-distance"),
        player_word: flag(&mut a, "--cond-player-word"),
        cond_player_sex: flag(&mut a, "--cond-70-player-sex"),
        activate_actors: flag(&mut a, "--activate-actors"),
        sit_events: flag(&mut a, "--sit-events"),
        sit_activate: flag(&mut a, "--activate-furniture-sits"),
        sit_in_use: flag(&mut a, "--furniture-in-use"),
        sit_activate_events: flag(&mut a, "--activate-furniture-events"),
        sit_activate_get_up: flag(&mut a, "--activate-furniture-get-up"),
        sit_state: flag(&mut a, "--sit-state"),
        anim_start_settled: flag(&mut a, "--anim-graph-start-settled"),
        anim_sequence_case: flag(&mut a, "--anim-sequence-case"),
        player_base_object: flag(&mut a, "--player-base-object"),
        menu_events: flag(&mut a, "--menu-events"),
        keywords_records: flag(&mut a, "--keywords-records"),
        keywords_runtime: flag(&mut a, "--keywords-runtime"),
        collection_fills: flag(&mut a, "--collection-location-refs"),
        anim: [
            flag(&mut a, "--anim-events"),
            flag(&mut a, "--anim-event-case"),
            flag(&mut a, "--anim-graphs"),
            flag(&mut a, "--anim-graph-keys"),
        ],
        talk: crate::talk::Switches {
            greeting: flag(&mut a, "--greeting-on-talk"),
            hello: flag(&mut a, "--hello-lines"),
            priority: flag(&mut a, "--dialogue-priority"),
            stage_fields: false,
            info_groups: flag(&mut a, "--info-groups"),
        },
        ..Then::default()
    };
    while let Some(i) = a.iter().position(|x| x == "--cond-70-player-sex-for") {
        let q = a
            .get(i + 1)
            .and_then(|q| crate::player::parse_key(q))
            .ok_or("--cond-70-player-sex-for takes a quest as PLUGIN:OBJECTID")?;
        then.cond_player_sex_for.push(crate::bind::record_key(&q));
        a.drain(i..i + 2);
    }
    // The caller's button in message boxes.
    if let Some(i) = a.iter().position(|x| x == "--assume-message-button") {
        let v = a.get(i + 1).ok_or("--assume-message-button N")?;
        then.message_button = Some(v.parse()?);
        a.drain(i..i + 2);
    }
    // The caller's answer of whether the player is in combat.
    if let Some(i) = a.iter().position(|x| x == "--assume-player-in-combat") {
        let v = match a.get(i + 1).map(String::as_str) {
            Some("true") => true,
            Some("false") => false,
            _ => return Err("--assume-player-in-combat true|false".into()),
        };
        then.player_combat = Some(v);
        a.drain(i..i + 2);
    }
    // The caller's answer of whether a menu is open.
    if let Some(i) = a.iter().position(|x| x == "--assume-menu-mode") {
        let v = match a.get(i + 1).map(String::as_str) {
            Some("true") => true,
            Some("false") => false,
            _ => return Err("--assume-menu-mode true|false".into()),
        };
        then.menu_mode = Some(v);
        a.drain(i..i + 2);
    }
    // The caller's answer of animation event registrations.
    if let Some(i) = a.iter().position(|x| x == "--assume-anim-registration") {
        let v = match a.get(i + 1).map(String::as_str) {
            Some("true") => true,
            Some("false") => false,
            _ => return Err("--assume-anim-registration true|false".into()),
        };
        then.anim_registration = Some(v);
        a.drain(i..i + 2);
    }
    // The caller's assumed animation events: REF EVENT[@SECONDS].
    while let Some(i) = a.iter().position(|x| x == "--assume-anim-event") {
        let usage = "--assume-anim-event PLUGIN:OBJECTID|player EVENT[@SECONDS]";
        let key = a.get(i + 1).cloned().ok_or(usage)?;
        let v = a.get(i + 2).cloned().ok_or(usage)?;
        let (event, at) = match v.rsplit_once('@') {
            Some((e, t)) => (e.to_string(), Some(t.parse::<f64>()?)),
            None => (v, None),
        };
        then.actions.push(Caller {
            what: What::Anim(key, event),
            at,
            after: None,
        });
        a.drain(i..i + 3);
    }
    // The caller's assumed actor moves: REF SPACE X Y Z@SECONDS.
    while let Some(i) = a.iter().position(|x| x == "--assume-actor-at") {
        let usage = "--assume-actor-at REF SPACE X Y Z@SECONDS";
        let v: Vec<String> = a.get(i + 1..i + 6).ok_or(usage)?.to_vec();
        let (z, t) = v[4].rsplit_once('@').ok_or(usage)?;
        let num = |s: &str| -> Result<f64, E> {
            let x: f64 = s.parse()?;
            if x.is_finite() {
                Ok(x)
            } else {
                Err("a number that is not finite".into())
            }
        };
        then.actor_moves.push(crate::track::ActorMove {
            t: num(t)?,
            actor: crate::player::parse_key(&v[0]).ok_or(usage)?,
            space: crate::player::parse_key(&v[1]).ok_or(usage)?,
            pos: [num(&v[2])?, num(&v[3])?, num(z)?],
        });
        a.drain(i..i + 6);
    }
    // The caller's fight events: REF[@SECONDS], REF TARGET STATE[@SECONDS],
    // REF KILLER[@SECONDS].
    while let Some(i) = a
        .iter()
        .position(|x| x == "--assume-hit" || x == "--assume-combat-state" || x == "--assume-death")
    {
        let usage = "--assume-hit REF[@SECONDS], --assume-combat-state REF TARGET|player STATE[@SECONDS], --assume-death REF KILLER|player[@SECONDS]";
        let n = match a[i].as_str() {
            "--assume-hit" => 1,
            "--assume-death" => 2,
            _ => 3,
        };
        let mut v: Vec<String> = a.get(i + 1..i + 1 + n).ok_or(usage)?.to_vec();
        let last = v.pop().ok_or(usage)?;
        let (body, at) = match last.rsplit_once('@') {
            Some((x, t)) => (x.to_string(), Some(t.parse::<f64>()?)),
            None => (last, None),
        };
        v.push(body);
        let what = match a[i].as_str() {
            "--assume-hit" => What::Hit(v[0].clone()),
            "--assume-death" => What::Death(v[0].clone(), v[1].clone()),
            _ => What::Combat(v[0].clone(), v[1].clone(), v[2].parse()?),
        };
        then.actions.push(Caller {
            what,
            at,
            after: None,
        });
        a.drain(i..i + 1 + n);
    }
    // The distance past which the track ends the player's sit.
    if let Some(i) = a.iter().position(|x| x == "--sit-end-walk") {
        let v: f64 = a.get(i + 1).ok_or("--sit-end-walk UNITS")?.parse()?;
        if !(v.is_finite() && v > 0.0) {
            return Err("--sit-end-walk takes a positive distance".into());
        }
        then.sit_end_walk = Some(v);
        a.drain(i..i + 2);
    }
    // The caller's unlocks: REF[@SECONDS].
    while let Some(i) = a.iter().position(|x| x == "--assume-unlock") {
        let v = a
            .get(i + 1)
            .cloned()
            .ok_or("--assume-unlock PLUGIN:OBJECTID[@SECONDS]")?;
        let (body, at) = match v.rsplit_once('@') {
            Some((x, t)) => (x.to_string(), Some(t.parse::<f64>()?)),
            None => (v.clone(), None),
        };
        then.actions.push(Caller {
            what: What::Unlock(body),
            at,
            after: None,
        });
        a.drain(i..i + 2);
    }
    // The caller's item counts: HOLDER|player ITEM N.
    while let Some(i) = a.iter().position(|x| x == "--assume-item-count") {
        let usage = "--assume-item-count PLUGIN:OBJECTID|player PLUGIN:OBJECTID N";
        let v: Vec<String> = a.get(i + 1..i + 4).ok_or(usage)?.to_vec();
        let key = |s: &str| -> Result<String, E> {
            if s == "player" {
                return Ok(s.to_string());
            }
            let k = crate::player::parse_key(s).ok_or(usage)?;
            Ok(crate::bind::record_key(&k))
        };
        then.item_counts
            .push(((key(&v[0])?, key(&v[1])?), v[2].parse()?));
        a.drain(i..i + 4);
    }
    // The caller's terminal picks: REF ID[@SECONDS].
    while let Some(i) = a.iter().position(|x| x == "--assume-terminal-item") {
        let usage = "--assume-terminal-item PLUGIN:OBJECTID ID[@SECONDS]";
        let key = a.get(i + 1).cloned().ok_or(usage)?;
        let v = a.get(i + 2).cloned().ok_or(usage)?;
        let (id, at) = match v.rsplit_once('@') {
            Some((x, t)) => (x.to_string(), Some(t.parse::<f64>()?)),
            None => (v, None),
        };
        crate::player::parse_key(&key).ok_or(usage)?;
        then.actions.push(Caller {
            what: What::Terminal(key, id.parse()?),
            at,
            after: None,
        });
        a.drain(i..i + 3);
    }
    // The caller's items added: HOLDER|player ITEM N SOURCE[@SECONDS].
    while let Some(i) = a.iter().position(|x| x == "--assume-item-added") {
        let usage =
            "--assume-item-added PLUGIN:OBJECTID|player PLUGIN:OBJECTID N player|none[@SECONDS]";
        let v: Vec<String> = a.get(i + 1..i + 5).ok_or(usage)?.to_vec();
        let (source, at) = match v[3].rsplit_once('@') {
            Some((x, t)) => (x.to_string(), Some(t.parse::<f64>()?)),
            None => (v[3].clone(), None),
        };
        if source != "player" && source != "none" {
            return Err(usage.into());
        }
        if v[0] != "player" {
            crate::player::parse_key(&v[0]).ok_or(usage)?;
        }
        crate::player::parse_key(&v[1]).ok_or(usage)?;
        then.actions.push(Caller {
            what: What::ItemAdded(v[0].clone(), v[1].clone(), v[2].parse()?, source),
            at,
            after: None,
        });
        a.drain(i..i + 5);
    }
    // The caller's container changes: REF NEW OLD[@SECONDS], NEW and OLD a
    // holder, player or none.
    while let Some(i) = a.iter().position(|x| x == "--assume-container-change") {
        let usage = "--assume-container-change PLUGIN:OBJECTID PLUGIN:OBJECTID|player|none PLUGIN:OBJECTID|player|none[@SECONDS]";
        let v: Vec<String> = a.get(i + 1..i + 4).ok_or(usage)?.to_vec();
        let (old, at) = match v[2].rsplit_once('@') {
            Some((x, t)) => (x.to_string(), Some(t.parse::<f64>()?)),
            None => (v[2].clone(), None),
        };
        crate::player::parse_key(&v[0]).ok_or(usage)?;
        for h in [&v[1], &old] {
            if h != "player" && h != "none" {
                crate::player::parse_key(h).ok_or(usage)?;
            }
        }
        then.actions.push(Caller {
            what: What::ContainerChange(v[0].clone(), v[1].clone(), old),
            at,
            after: None,
        });
        a.drain(i..i + 4);
    }
    // The caller's location changes: ACTOR OLD NEW[@SECONDS], ACTOR player,
    // a placed actor or EDITOR_ID#ALIAS, OLD and NEW a location or none.
    while let Some(i) = a.iter().position(|x| x == "--assume-location-change") {
        let usage = "--assume-location-change PLUGIN:OBJECTID|EDITOR_ID#ALIAS|player PLUGIN:OBJECTID|none PLUGIN:OBJECTID|none[@SECONDS]";
        let v: Vec<String> = a.get(i + 1..i + 4).ok_or(usage)?.to_vec();
        let (new, at) = match v[2].rsplit_once('@') {
            Some((x, t)) => (x.to_string(), Some(t.parse::<f64>()?)),
            None => (v[2].clone(), None),
        };
        if v[0] != "player" && !v[0].contains('#') {
            crate::player::parse_key(&v[0]).ok_or(usage)?;
        }
        for l in [&v[1], &new] {
            if l != "none" {
                crate::player::parse_key(l).ok_or(usage)?;
            }
        }
        then.actions.push(Caller {
            what: What::LocationChange(v[0].clone(), v[1].clone(), new),
            at,
            after: None,
        });
        a.drain(i..i + 4);
    }
    // The caller's actor values: HOLDER|player AVIF N.
    while let Some(i) = a.iter().position(|x| x == "--assume-actor-value") {
        let usage = "--assume-actor-value PLUGIN:OBJECTID|player PLUGIN:OBJECTID N";
        let v: Vec<String> = a.get(i + 1..i + 4).ok_or(usage)?.to_vec();
        let key = |s: &str| -> Result<String, E> {
            if s == "player" {
                return Ok(s.to_string());
            }
            let k = crate::player::parse_key(s).ok_or(usage)?;
            Ok(crate::bind::record_key(&k))
        };
        let x: f32 = v[2].parse()?;
        if !x.is_finite() {
            return Err(usage.into());
        }
        then.actor_values.push(((key(&v[0])?, key(&v[1])?), x));
        a.drain(i..i + 4);
    }
    // The caller's furniture use and menu events.
    while let Some(i) = a.iter().position(|x| {
        x == "--assume-sit" || x == "--assume-get-up" || x == "--assume-looks-menu-event"
    }) {
        let v = a.get(i + 1).cloned().ok_or(
            "--assume-sit and --assume-get-up take PLUGIN:OBJECTID[@SECONDS], --assume-looks-menu-event N[@SECONDS]",
        )?;
        let (body, at) = match v.rsplit_once('@') {
            Some((x, t)) => (x.to_string(), Some(t.parse::<f64>()?)),
            None => (v.clone(), None),
        };
        let what = match a[i].as_str() {
            "--assume-sit" => What::Sit(body, true),
            "--assume-get-up" => What::Sit(body, false),
            _ => What::Looks(body.parse()?),
        };
        then.actions.push(Caller {
            what,
            at,
            after: None,
        });
        a.drain(i..i + 2);
    }
    if let Some(i) = a.iter().position(|x| x == "--furniture-graph-events") {
        let usage = "--furniture-graph-events SIT_EVENT GET_UP_EVENT";
        let sit = a.get(i + 1).cloned().ok_or(usage)?;
        let up = a.get(i + 2).cloned().ok_or(usage)?;
        then.furniture_graph_events = Some((sit, up));
        a.drain(i..i + 3);
    }
    if let Some(i) = a.iter().position(|x| x == "--assume-player-sex") {
        let v = a.get(i + 1).ok_or("--assume-player-sex N")?;
        then.player_sex = Some(v.parse()?);
        a.drain(i..i + 2);
    }
    while let Some(i) = a.iter().position(|x| x == "--assume-menu") {
        let usage = "--assume-menu NAME open|close[@SECONDS]";
        let name = a.get(i + 1).cloned().ok_or(usage)?;
        let v = a.get(i + 2).cloned().ok_or(usage)?;
        let (how, at) = match v.rsplit_once('@') {
            Some((x, t)) => (x.to_string(), Some(t.parse::<f64>()?)),
            None => (v.clone(), None),
        };
        let opening = match how.as_str() {
            "open" => true,
            "close" => false,
            _ => return Err(usage.into()),
        };
        then.actions.push(Caller {
            what: What::Menu(name, opening),
            at,
            after: None,
        });
        a.drain(i..i + 3);
    }
    while let Some(i) = a.iter().position(|x| {
        x == "--player-base" || x == "--hello-distance-setting" || x == "--assume-line"
    }) {
        let v = a.get(i + 1).cloned().ok_or(
            "--player-base and --hello-distance-setting take an editor ID, --assume-line a line",
        )?;
        match a[i].as_str() {
            "--player-base" => then.player_base = Some(v),
            "--assume-line" => then.assume_lines.push(v),
            _ => then.hello_setting = Some(v),
        }
        a.drain(i..i + 2);
    }
    while let Some(i) = a.iter().position(|x| {
        x == "--then"
            || x == "--timers"
            || x == "--time-scale-global"
            || x == "--assume-talk"
            || crate::player::Action::from_switch(x).is_some()
    }) {
        let v = a.get(i + 1).cloned().ok_or(
            "--then, --timers, --time-scale-global and the --assume-<action> switches take a value",
        )?;
        // An optional `@SECONDS`: when the caller's action happens.
        let (body, at) = match v.rsplit_once('@') {
            Some((x, t)) if a[i] != "--timers" && a[i] != "--time-scale-global" => {
                (x.to_string(), Some(t.parse::<f64>()?))
            }
            _ => (v.clone(), None),
        };
        if let Some(action) = crate::player::Action::from_switch(&a[i]) {
            then.actions.push(Caller {
                what: What::Player(action, body),
                at,
                after: None,
            });
        } else if a[i] == "--assume-talk" {
            then.actions.push(Caller {
                what: What::Talk(body),
                at,
                after: None,
            });
        } else if a[i] == "--then" {
            let (q, n) = body
                .split_once(':')
                .ok_or("--then EDITOR_ID:STAGE[@SECONDS]")?;
            then.actions.push(Caller {
                what: What::Stage(q.to_string(), n.parse()?),
                at,
                after: None,
            });
        } else if a[i] == "--timers" {
            then.timers = v.parse()?;
        } else {
            then.time_scale = Some(v);
        }
        a.drain(i..i + 2);
    }
    // The player's track and the world model's readings (`crate::track`).
    let world_sw = crate::track::Switches {
        boxes: flag(&mut a, "--trigger-box"),
        spheres: flag(&mut a, "--trigger-sphere"),
        point: flag(&mut a, "--trigger-point"),
        instant: flag(&mut a, "--trigger-instant"),
        enabled: flag(&mut a, "--trigger-enabled"),
        order: flag(&mut a, "--trigger-order"),
        distance: flag(&mut a, "--distance-events"),
        distance_once: flag(&mut a, "--distance-once"),
        placed: flag(&mut a, "--placed-positions"),
        position_natives: flag(&mut a, "--position-natives"),
        cell_natives: flag(&mut a, "--cell-natives"),
        location_native: flag(&mut a, "--location-native"),
        assume_los: None,
        body: None,
        actors: flag(&mut a, "--actor-positions"),
        actor_triggers: flag(&mut a, "--actor-triggers"),
        moveto_target: flag(&mut a, "--moveto-target"),
        load_doors: flag(&mut a, "--load-doors"),
    };
    then.loads.enters = flag(&mut a, "--load-in-cell");
    then.loads.arrival = flag(&mut a, "--load-on-arrival");
    let mut world_sw = world_sw;
    let mut track_file = None;
    while let Some(i) = a.iter().position(|x| {
        x == "--track" || x == "--assume-los" || x == "--trigger-body" || x == "--load-grid"
    }) {
        let v = a.get(i + 1).cloned().ok_or(
            "--track FILE, --assume-los true|false, --trigger-body HEIGHT and --load-grid N take a value",
        )?;
        match a[i].as_str() {
            "--track" => track_file = Some(v),
            "--trigger-body" => world_sw.body = Some(v.parse()?),
            "--load-grid" => then.loads.grid = Some(v.parse()?),
            _ => world_sw.assume_los = Some(v.parse()?),
        }
        a.drain(i..i + 2);
    }
    match track_file {
        Some(f) => {
            let text = std::fs::read_to_string(&f)?;
            then.track = Some((crate::track::Track::parse(&text)?, world_sw));
        }
        None if world_sw != crate::track::Switches::default() || then.loads.any() => {
            return Err("the world model's switches need --track FILE".into());
        }
        None => {}
    }
    if world_sw.point && world_sw.body.is_some() {
        return Err(
            "--trigger-point and --trigger-body are two readings of one thing: give one".into(),
        );
    }
    // The scene clock and the caller's assumptions about scene actions.
    let mut clock = scene::Clock {
        timers: !flag(&mut a, "--no-timer-actions"),
        assume_packages: flag(&mut a, "--assume-packages-complete"),
        assume_start_scenes: flag(&mut a, "--assume-start-scene-actions"),
        assume_first_line: flag(&mut a, "--assume-first-line"),
        start_skipped: flag(&mut a, "--scene-actions-skipped-start"),
        npc_responses: flag(&mut a, "--npc-response-lines"),
        ..scene::Clock::default()
    };
    let line_audio = !flag(&mut a, "--no-line-audio");
    // The caller's assumptions scoped to one quest's scenes.
    while let Some(i) = a
        .iter()
        .position(|x| x == "--assume-player-choice-for" || x == "--assume-first-line-for")
    {
        let q = a
            .get(i + 1)
            .and_then(|q| crate::player::parse_key(q))
            .ok_or("--assume-player-choice-for and --assume-first-line-for take a scene or a quest as PLUGIN:OBJECTID")?;
        let q = crate::bind::record_key(&q);
        if a[i] == "--assume-first-line-for" {
            clock.first_line_for.insert(q);
            a.drain(i..i + 2);
        } else {
            let n: usize = a
                .get(i + 2)
                .ok_or("--assume-player-choice-for SCENE|QUEST N")?
                .parse()?;
            if n >= scene::PLAYER_CHOICES.len() {
                return Err("--assume-player-choice-for takes 0 to 3".into());
            }
            clock.choice_for.insert(q, n);
            a.drain(i..i + 3);
        }
    }
    while let Some(i) = a.iter().position(|x| {
        x == "--scene-clock"
            || x == "--assume-line-seconds"
            || x == "--assume-player-choice"
            || x == "--package-destinations"
    }) {
        let v = a.get(i + 1).cloned().ok_or(
            "--scene-clock, --assume-line-seconds, --assume-player-choice and --package-destinations take a value",
        )?;
        match a[i].as_str() {
            "--scene-clock" => then.scene_clock = v.parse()?,
            "--assume-line-seconds" => clock.assume_line_seconds = Some(v.parse()?),
            "--package-destinations" => {
                let n: f64 = v.parse()?;
                if !n.is_finite() || n < 0.0 {
                    return Err("--package-destinations takes seconds, not negative".into());
                }
                clock.package_arrival = Some(n);
            }
            _ => clock.assume_player_choice = Some(v.parse()?),
        }
        a.drain(i..i + 2);
    }
    let assumed = clock.assume_packages
        || clock.assume_start_scenes
        || clock.assume_line_seconds.is_some()
        || clock.assume_player_choice.is_some()
        || clock.assume_first_line
        || !clock.choice_for.is_empty()
        || !clock.first_line_for.is_empty()
        || clock.start_skipped
        || clock.npc_responses
        || opts.line_fields
        || opts.lines.fragments
        || opts.lines.start_scene;
    if then.scene_clock == 0 && (assumed || !clock.timers || !line_audio) {
        return Err("the scene action switches and assumptions need --scene-clock N".into());
    }
    if then.scene_clock == 0 && then.track.is_some() {
        return Err("--track needs --scene-clock N".into());
    }
    let talk_on = then.talk != crate::talk::Switches::default()
        || then.actions.iter().any(|x| matches!(x.what, What::Talk(_)));
    if talk_on && then.scene_clock == 0 {
        return Err("--greeting-on-talk, --hello-lines, --dialogue-priority and --assume-talk need --scene-clock N".into());
    }
    if clock.package_arrival.is_some() && !world_sw.actors {
        return Err("--package-destinations needs --track FILE and --actor-positions".into());
    }
    if !then.actor_moves.is_empty() && !world_sw.actors {
        return Err("--assume-actor-at needs --track FILE and --actor-positions".into());
    }
    if then.talk.hello && (!world_sw.actors || then.hello_setting.is_none()) {
        return Err(
            "--hello-lines needs --track FILE, --actor-positions and --hello-distance-setting EDITOR_ID"
                .into(),
        );
    }
    let assumed_anim = then
        .actions
        .iter()
        .any(|x| matches!(x.what, What::Anim(..)));
    let assumed_menu = then
        .actions
        .iter()
        .any(|x| matches!(x.what, What::Menu(..) | What::Looks(_)));
    if (then.player_base_object || then.player_sex.is_some()) && then.player_base.is_none() {
        return Err(
            "--player-base-object and --assume-player-sex need --player-base EDITOR_ID".into(),
        );
    }
    if then.player_sex.is_some() && !then.player_base_object {
        return Err("--assume-player-sex needs --player-base-object".into());
    }
    if (then.location_distinct || then.location_placed) && !opts.location_fills {
        return Err(
            "--location-refs-distinct and --alias-location-placed-refs need --alias-location-refs"
                .into(),
        );
    }
    if then.cond_distance && then.track.is_none() {
        return Err("--cond-1-distance needs --track FILE".into());
    }
    // The caller's gates: --action-after TARGET@SECONDS EDITOR_ID:STAGE.
    while let Some(i) = a.iter().position(|x| x == "--action-after") {
        let usage = "--action-after TARGET@SECONDS EDITOR_ID:STAGE (TARGET: the action's reference key, menu name, looks event number or EDITOR_ID:STAGE)";
        let target = a.get(i + 1).cloned().ok_or(usage)?;
        let gate = a.get(i + 2).cloned().ok_or(usage)?;
        let (target, at) = target.rsplit_once('@').ok_or(usage)?;
        let at: f64 = at.parse()?;
        let (q, n) = gate.rsplit_once(':').ok_or(usage)?;
        let n: i32 = n.parse()?;
        // Every timed caller action on that target at that second.
        let mut found = false;
        for x in then
            .actions
            .iter_mut()
            .filter(|x| x.at == Some(at) && x.target().eq_ignore_ascii_case(target))
        {
            x.after = Some((q.to_string(), n));
            found = true;
        }
        if !found {
            then.unmatched_gates += 1;
        }
        a.drain(i..i + 3);
    }
    if then.actions.iter().any(|x| x.after.is_some()) && then.scene_clock == 0 {
        return Err("--action-after needs --scene-clock N".into());
    }
    if then.anim_registration.is_some() && !then.anim[0] {
        return Err("--assume-anim-registration needs --anim-events".into());
    }
    let assumed_sit = then.actions.iter().any(|x| matches!(x.what, What::Sit(..)));
    if (assumed_sit
        || then.sit_activate
        || then.sit_in_use
        || then.sit_activate_events
        || then.sit_activate_get_up
        || then.sit_state)
        && !then.sit_events
    {
        return Err("--assume-sit, --assume-get-up, --activate-furniture-sits, --activate-furniture-events, --activate-furniture-get-up, --furniture-in-use and --sit-state need --sit-events".into());
    }
    if then.furniture_graph_events.is_some()
        && (!then.anim[2]
            || !(then.sit_activate || then.sit_activate_events || then.sit_activate_get_up))
    {
        return Err("--furniture-graph-events needs --anim-graphs and a scripted activation reading (--activate-furniture-sits, --activate-furniture-events or --activate-furniture-get-up)".into());
    }
    let assumed = |f: fn(&What) -> bool| then.actions.iter().any(|x| f(&x.what));
    if (assumed(|w| matches!(w, What::Hit(_))) || then.hit_init) && !then.hit_events {
        return Err("--assume-hit and --hit-init-registrations need --hit-events".into());
    }
    if assumed(|w| matches!(w, What::Combat(..))) && !then.combat_events {
        return Err("--assume-combat-state needs --combat-state-events".into());
    }
    if assumed(|w| matches!(w, What::Death(..))) && !then.death_events {
        return Err("--assume-death needs --death-events".into());
    }
    if then.combat_natives && then.player_combat.is_some() {
        return Err(
            "--combat-state-natives and --assume-player-in-combat answer the same native: give one"
                .into(),
        );
    }
    if then.timers_per_timer && then.timers == 0 {
        return Err("--timers-per-timer needs --timers N".into());
    }
    if then.sit_end_walk.is_some() && !then.sit_events {
        return Err("--sit-end-walk needs --sit-events".into());
    }
    if then.lock_events_script && !then.lock_state {
        return Err("--lock-events-script needs --lock-state".into());
    }
    if then.inventory_filter_init && !then.item_added_events {
        return Err("--inventory-filter-init needs --item-added-events".into());
    }
    if then.perks_base && !then.player_base_object {
        return Err("--perks-base needs --player-base-object".into());
    }
    if then.global_value_undetermined && !then.global_values {
        return Err("--global-value-undetermined needs --global-values".into());
    }
    if then.location_shared && !opts.location_fills {
        return Err("--location-refs-shared needs --alias-location-refs".into());
    }
    if assumed_menu && !then.menu_events {
        return Err("--assume-menu and --assume-looks-menu-event need --menu-events".into());
    }
    if (assumed_anim || then.anim[1]) && !then.anim[0] {
        return Err("--assume-anim-event and --anim-event-case need --anim-events".into());
    }
    if (opts.action_percent || opts.conditions_end) && then.scene_clock == 0 {
        return Err("--cond-131-clock and --phase-conditions-end need --scene-clock N".into());
    }
    if (then.anim_start_settled || then.anim_sequence_case) && !then.anim[2] {
        return Err(
            "--anim-graph-start-settled and --anim-sequence-case need --anim-graphs".into(),
        );
    }
    if then.anim[3] && !then.anim[2] {
        return Err("--anim-graph-keys needs --anim-graphs".into());
    }
    if then.anim[2] && then.scene_clock == 0 {
        return Err("--anim-graphs needs --scene-clock N".into());
    }
    if then.scene_clock == 0 && then.actions.iter().any(|x| x.at.is_some()) {
        return Err("an action at a second (@SECONDS) needs --scene-clock N".into());
    }
    if clock
        .assume_player_choice
        .is_some_and(|n| n >= scene::PLAYER_CHOICES.len())
    {
        return Err("--assume-player-choice takes 0 to 3".into());
    }
    let usage = "quest_run MANIFEST (stages OUT_DIR | new-game OUT_DIR | fragments OUT_DIR | scenes OUT_DIR | QUEST STAGE) [--queued] [--no-conditions] [--no-stage-event] [--no-remote] [--no-custom | --custom-nested] [--no-story] [--no-enable-chain] [--no-scenes] [--no-scenes-with-quest] [--no-cond-fn-N]... [--no-cond-run-on] [--assume-cond N=VALUE]... [--assume-cond-for QUEST N=VALUE]...; fragments OUT_DIR and scenes OUT_DIR run every INFO, PACK and PERK fragment and start every scene in a fresh interpreter; new-game also takes --then EDITOR_ID:STAGE[@SECONDS] (repeated), --assume-activate, --assume-trigger-enter, --assume-trigger-leave and --assume-open PLUGIN:OBJECTID[@SECONDS] (repeated), --timers N and --time-scale-global EDITOR_ID, assumptions of the caller, --scene-recheck, --stage-fields, --cond-550-clock, --alias-location-refs and --next-blockers; new-game and scenes take --scene-clock N (advance the scene clock up to N times) with --no-timer-actions, --no-line-audio, --assume-line-seconds X, --assume-player-choice N (0 to 3), --assume-player-choice-for SCENE|QUEST N and --assume-first-line-for SCENE|QUEST (as PLUGIN:OBJECTID, repeated; a scene's over its quest's), --assume-packages-complete, --assume-start-scene-actions, --line-stage-fields, --line-fragments, --line-start-scene and --assume-first-line; every mode takes --none-array-length, --none-array-read, --none-array-find, --none-array-change, --enable-child-flag-ignored, --text-equal-ignore-case and --auto-state-inherited; new-game with --scene-clock takes --track FILE (the player's track, lines SECONDS SPACE X Y Z [FACING]) with --trigger-box, --trigger-sphere, --trigger-point or --trigger-body HEIGHT, --trigger-instant, --trigger-enabled, --trigger-order, --distance-events, --distance-once, --placed-positions, --position-natives, --cell-natives, --location-native and --assume-los true|false, --actor-positions, --actor-triggers and --assume-actor-at REF SPACE X Y Z@SECONDS (repeated); new-game takes --alias-unique-actors, --player-base EDITOR_ID and --player-no-record, and with --scene-clock --assume-talk PLUGIN:OBJECTID[@SECONDS] (repeated), --greeting-on-talk, --hello-lines with --hello-distance-setting EDITOR_ID, --dialogue-priority, --info-groups and --assume-line PLUGIN:OBJECTID (repeated); new-game with --track and --actor-positions takes --package-destinations SECONDS; new-game takes --anim-events with --anim-event-case and --assume-anim-event PLUGIN:OBJECTID|player EVENT[@SECONDS] (repeated), and with --scene-clock --anim-graphs with --anim-graph-keys, --anim-graph-start-settled and --anim-sequence-case; new-game with --track takes --moveto-target, --load-doors, --load-in-cell, --load-on-arrival and --load-grid N, and --assume-load PLUGIN:OBJECTID[@SECONDS] (repeated); new-game and scenes with --scene-clock take --cond-131-clock and --phase-conditions-end; new-game takes --location-refs-distinct and --alias-location-placed-refs and --location-refs-shared (with --alias-location-refs), --cond-1-distance (with --track), --cond-player-word and --cond-70-player-sex or --cond-70-player-sex-for QUEST (repeated; with --player-base-object and --assume-player-sex), --activate-actors, --sit-events with --assume-sit and --assume-get-up PLUGIN:OBJECTID[@SECONDS] (repeated), --activate-furniture-sits, --activate-furniture-events, --activate-furniture-get-up, --furniture-in-use and --sit-state, with --anim-graphs --furniture-graph-events SIT_EVENT GET_UP_EVENT, --menu-events with --assume-menu NAME open|close[@SECONDS] and --assume-looks-menu-event N[@SECONDS] (repeated), --assume-menu-mode true|false, --assume-message-button N, --assume-player-in-combat true|false, and --player-base-object (with --player-base) with --assume-player-sex N, --keywords-records and --keywords-runtime, and --collection-location-refs, and with --anim-events --assume-anim-registration true|false; new-game and scenes with --scene-clock take --scene-actions-skipped-start; new-game with --scene-clock takes --action-after TARGET@SECONDS EDITOR_ID:STAGE (repeated); new-game takes --scene-recheck-variables, and with --next-blockers --setters-outer-calls; new-game takes --hit-events with --hit-init-registrations and --assume-hit PLUGIN:OBJECTID[@SECONDS], --combat-state-events with --assume-combat-state PLUGIN:OBJECTID TARGET STATE[@SECONDS], --death-events with --assume-death PLUGIN:OBJECTID KILLER[@SECONDS] (TARGET and KILLER a placed actor or player; repeated), --dead-state and --combat-state-natives (not with --assume-player-in-combat); new-game takes --global-values with --global-value-undetermined, --lock-state, --lock-events, --assume-unlock PLUGIN:OBJECTID[@SECONDS], --assume-item-count HOLDER|player ITEM N, --property-implicit-instance, --npc-response-lines, --timeline-deaths, --timeline-sits and --census-run-state; new-game takes --lock-events-script (with --lock-state), --terminal-fragments with --assume-terminal-item PLUGIN:OBJECTID ID[@SECONDS], --item-added-events with --inventory-filter-init, --item-added-counts, --assume-item-added HOLDER|player ITEM N player|none[@SECONDS], --perks-base (with --player-base-object), --factions-base and --assume-actor-value HOLDER|player AVIF N; new-game takes --alias-created-refs, --alias-external, --line-script-events, --timeline-sequences and --sit-end-walk UNITS (with --track and --sit-events), and the fight assumptions name the reference an alias holds as EDITOR_ID#ALIAS; new-game takes --quest-partial-previous, --container-change-events with --assume-container-change PLUGIN:OBJECTID HOLDER|player|none HOLDER|player|none[@SECONDS], --location-change-events with --assume-location-change PLUGIN:OBJECTID|EDITOR_ID#ALIAS|player LOCATION|none LOCATION|none[@SECONDS] (repeated), --location-parents and --timers-per-timer (with --timers N)";
    if a.len() != 3 {
        return Err(usage.into());
    }
    let mut a = a.into_iter();
    let (Some(manifest), Some(mode), Some(arg)) = (a.next(), a.next(), a.next()) else {
        return Err(usage.into());
    };
    Ok(Command {
        manifest: PathBuf::from(manifest),
        mode,
        arg,
        options: opts,
        then,
        settings: Settings {
            cond: (switches, assume),
            cond_for: assume_for,
            clock,
            line_audio,
            timeline,
            groups: None,
        },
    })
}

impl Command {
    /// Opens the manifest's data and runs the mode; a new-game run with the
    /// scene clock on is stepped by `stepper` when one is given.
    pub fn run(
        self,
        seconds: HeaderSeconds<'_>,
        stepper: Option<&mut dyn Stepper>,
    ) -> Result<(), E> {
        let Command {
            manifest,
            mode,
            arg,
            options: opts,
            then,
            mut settings,
        } = self;
        let m = Manifest::load(&manifest)?;
        let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
        let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
        if opts.scenes {
            settings.groups = Some(Rc::new(scene::Groups::read(&store)));
        }
        let mut clock = std::mem::take(&mut settings.clock);
        if then.scene_clock > 0 {
            if !opts.scenes {
                return Err("--scene-clock needs scenes on".into());
            }
            clock.on = true;
            if settings.line_audio {
                clock.voice = Some(Rc::new(load_voice(&m, &vfs, &store, seconds)?));
            }
            settings.clock = clock;
        }
        let out = Path::new(&arg);
        match mode.as_str() {
            "stages" => stages(&m, &vfs, &store, out, opts, &settings),
            "new-game" => game::new_game((&m, &vfs, &store), out, opts, &then, &settings, stepper),
            "fragments" => fragments(&m, &vfs, &store, out, opts, &settings),
            "scenes" => scenes(&m, &vfs, &store, out, opts, then.scene_clock, &settings),
            q => one(&m, &vfs, &store, q, arg.parse()?, opts, &settings),
        }
    }
}
