//! Which clip a character's behaviour graph reaches from its root when
//! nothing has happened yet (`spec/character-graph.md`), each reading
//! under its pending rule behind its own switch ([`Switches`], all off by
//! default):
//!
//! - the race's behaviour project: the MODL text after the race's NAM3
//!   marker in the actor's section (MNAM 0, FNAM 1), as the skeleton's ANAM
//!   is read (`actor_acbs_bit0_second_section_v1`); the project, character
//!   and graph files as `crate::anim_graph` follows them;
//! - [`hypotheses::CHAR_GRAPH_START`] (`--actor-start-clip`): from the
//!   graph's root, each state machine's start state (or the stored initial
//!   value of the variable its start state is bound to), the generator each
//!   wrapper holds, a blender's child of the highest weight (the first of
//!   equal weights; a weight bound to a variable takes the variable's
//!   stored initial value), a manual selector's stored index (or its
//!   variable's), a behaviour reference's file, down to a clip generator;
//!   anything the data does not store refuses the actor with a named
//!   reason ([`Refusal`]);
//! - [`hypotheses::CHAR_GRAPH_SUBGRAPH`] (`--actor-subgraph-keywords`): a
//!   swap generator plays the race's subgraph entry (from the subgraph
//!   race SRAC names, else the race) of flag word 0 with no target keyword
//!   whose actor keywords all are the actor's own (its base's KWDA and its
//!   race's), the one with the most such keywords, a tie refused;
//! - [`hypotheses::CHAR_GRAPH_CLIP_FOLDER`] (`--actor-clip-folder`): the
//!   clip text's file name, with `.hkx`, in the first of the subgraph
//!   entry's animation folders (SAPT, under `meshes`) that holds it; a clip
//!   reached outside a subgraph is its text with `.hkx` under the project's
//!   folder.
//!
//! The pose a clip gives and the time it is drawn at are the caller's
//! (`crate::actor::clip`, [`hypotheses::CHAR_GRAPH_CLIP_LOOP`]).
use crate::anim_graph::{folder_of, key_under};
use crate::hypotheses;
use fo4_core::{Budget, Result};
use fo4_havok as hk;
use fo4_plugin::{typed::Store, FormKey, Record};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::BTreeSet;

/// Most generators followed from a root to a clip.
pub const MAX_STEPS: usize = 512;
/// Most graph files one walk opens.
pub const MAX_FILES: usize = 16;
/// The blender flag bit refused by the walk: set on every blender whose
/// children's stored weights run outside 0 to 1 (`spec/character-graph.md`).
pub const BLEND_PARAMETRIC_FLAG: u16 = 0x10;

/// The readings that are off by default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Switches {
    /// [`hypotheses::CHAR_GRAPH_START`] (`--actor-start-clip`).
    pub start: bool,
    /// [`hypotheses::CHAR_GRAPH_SUBGRAPH`] (`--actor-subgraph-keywords`).
    pub subgraph: bool,
    /// [`hypotheses::CHAR_GRAPH_CLIP_FOLDER`] (`--actor-clip-folder`).
    pub folder: bool,
    /// [`hypotheses::CHAR_GRAPH_CLIP_LOOP`] (`--actor-start-clip-loop`).
    pub cycle: bool,
    /// [`hypotheses::CHAR_GRAPH_FURNITURE`] (`--actor-sit-clip`): a seated
    /// actor ([`crate::timeline::Pose::Seated`]) posed by its furniture's
    /// clip ([`furniture_clip`]).
    pub sit: bool,
}
impl Switches {
    pub fn any(&self) -> bool {
        self.start || self.subgraph || self.folder || self.cycle || self.sit
    }
    /// Turns on the switch a command-line flag names; `false` for any
    /// other argument.
    pub fn set_flag(&mut self, arg: &str) -> bool {
        let slot = match arg {
            "--actor-start-clip" => &mut self.start,
            "--actor-subgraph-keywords" => &mut self.subgraph,
            "--actor-clip-folder" => &mut self.folder,
            "--actor-start-clip-loop" => &mut self.cycle,
            "--actor-sit-clip" => &mut self.sit,
            _ => return false,
        };
        *slot = true;
        true
    }
}

/// One generator on the way from a root to a clip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub file: String,
    pub object: usize,
    pub class: String,
    /// How the walk left it (presentation).
    pub how: String,
}

/// One subgraph entry of a race: SAKD keywords, STKD keywords, SGNM text,
/// SAPT texts, SRAF word, in stored order (closed by its SRAF).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub index: usize,
    pub actor_keywords: Vec<FormKey>,
    pub target_keywords: Vec<FormKey>,
    pub behavior: String,
    pub folders: Vec<String>,
    pub flags: u32,
}

/// The clip a walk reached.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StartClip {
    pub project: String,
    pub behavior: String,
    pub subgraph: Option<Entry>,
    /// The clip generator's text and fields.
    pub generator: hk::ClipGenerator,
    /// The archive key of the clip file, under [`Switches::folder`].
    pub clip: String,
    pub steps: Vec<Step>,
    /// Blenders whose highest weight more than one child shares.
    pub ties: u32,
    pub rules: BTreeSet<&'static str>,
}

/// Why a walk stops before a clip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub detail: String,
    pub steps: Vec<Step>,
}

fn text(r: &Record, at: usize) -> String {
    let d = r.data(&r.subrecords[at]);
    String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned()
}

fn form(store: &Store, r: &Record, plugin: usize, at: usize) -> Result<Option<FormKey>> {
    let s = &r.subrecords[at];
    let d = r.data(s);
    let Some(w) = d.get(..4) else {
        return Err(fo4_core::Error::new(
            "InvalidRecordField",
            &r.source.file,
            s.offset,
            "4-byte FormID",
            d.len(),
        ));
    };
    store.order().remap(
        plugin,
        u32::from_le_bytes([w[0], w[1], w[2], w[3]]),
        &r.source.file,
        s.offset,
    )
}

/// The race's behaviour project text of a section: the MODL after NAM3
/// and the section's marker (MNAM for 0, FNAM for 1).
pub fn race_project(r: &Record, section: usize) -> Option<String> {
    let start = r.subrecords.iter().position(|s| s.signature == *b"NAM3")?;
    let marker: [u8; 4] = if section == 0 { *b"MNAM" } else { *b"FNAM" };
    let mut inside = false;
    for (i, s) in r.subrecords.iter().enumerate().skip(start + 1) {
        match &s.signature {
            x if *x == marker => inside = true,
            b"MNAM" | b"FNAM" => inside = false,
            b"MODL" if inside => return Some(text(r, i)),
            b"MODT" => {}
            _ if inside => return None,
            _ => {}
        }
    }
    None
}

/// The race whose subgraph entries apply (the one SRAC names, else the
/// race itself) and its entries.
pub fn subgraphs(
    store: &Store,
    race: &FormKey,
    b: &Budget,
) -> Result<Option<(FormKey, Vec<Entry>)>> {
    let Some((r, plugin)) = store.raw_with_plugin(race, b)? else {
        return Ok(None);
    };
    let mut key = race.clone();
    let mut held = (r, plugin);
    if let Some(at) = held
        .0
        .subrecords
        .iter()
        .position(|s| s.signature == *b"SRAC")
    {
        if let Some(k) = form(store, &held.0, held.1, at)? {
            let Some(x) = store.raw_with_plugin(&k, b)? else {
                return Ok(None);
            };
            key = k;
            held = x;
        }
    }
    let (r, plugin) = held;
    let mut out = Vec::new();
    let mut cur: Option<Entry> = None;
    for i in 0..r.subrecords.len() {
        let sig = r.subrecords[i].signature;
        if !matches!(&sig, b"SAKD" | b"STKD" | b"SGNM" | b"SAPT" | b"SRAF") {
            continue;
        }
        let e = cur.get_or_insert_with(|| Entry {
            index: out.len(),
            actor_keywords: Vec::new(),
            target_keywords: Vec::new(),
            behavior: String::new(),
            folders: Vec::new(),
            flags: 0,
        });
        match &sig {
            b"SAKD" => e.actor_keywords.extend(form(store, &r, plugin, i)?),
            b"STKD" => e.target_keywords.extend(form(store, &r, plugin, i)?),
            b"SGNM" => e.behavior = text(&r, i),
            b"SAPT" => e.folders.push(text(&r, i)),
            _ => {
                let d = r.data(&r.subrecords[i]);
                e.flags = d
                    .get(..4)
                    .map_or(0, |w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
                b.reserve(&mut out, 1, &r.source.file, r.subrecords[i].offset)?;
                out.extend(cur.take());
            }
        }
    }
    Ok(Some((key, out)))
}

/// The keywords of a record's KWDA (FormIDs remapped through its plugin).
pub fn record_keywords(store: &Store, r: &Record, plugin: usize) -> Result<Vec<FormKey>> {
    let mut out = Vec::new();
    for s in r.subrecords.iter().filter(|s| s.signature == *b"KWDA") {
        let d = r.data(s);
        for (k, w) in d.as_chunks::<4>().0.iter().enumerate() {
            let id = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
            if let Some(f) =
                store
                    .order()
                    .remap(plugin, id, &r.source.file, s.offset + 4 * k as u64)?
            {
                out.push(f);
            }
        }
    }
    Ok(out)
}

/// The actor's own keywords for [`hypotheses::CHAR_GRAPH_SUBGRAPH`]: its
/// base's KWDA and its race's; a base with a template (TPLT) is refused
/// (which template supplies keywords is not read).
pub fn actor_keywords(
    store: &Store,
    base: &FormKey,
    race: &FormKey,
    b: &Budget,
) -> Result<std::result::Result<BTreeSet<FormKey>, String>> {
    npc_keywords(store, base, race, false, b)
}

/// [`actor_keywords`] of the NPC_ `base`; with `template_read` (the
/// template bit reading, `crate::actor::Readings::template_bits`, chose
/// this NPC_ for the keywords) its own template does not refuse them.
pub fn npc_keywords(
    store: &Store,
    base: &FormKey,
    race: &FormKey,
    template_read: bool,
    b: &Budget,
) -> Result<std::result::Result<BTreeSet<FormKey>, String>> {
    let Some((r, p)) = store.raw_with_plugin(base, b)? else {
        return Ok(Err("the base has no winning record".into()));
    };
    if r.signature != *b"NPC_" {
        return Ok(Err(format!(
            "the base is a {}, not an NPC_",
            String::from_utf8_lossy(&r.signature)
        )));
    }
    if !template_read && r.subrecords.iter().any(|s| s.signature == *b"TPLT") {
        return Ok(Err(
            "the base has a template (TPLT): its keywords are not read".into(),
        ));
    }
    let mut out: BTreeSet<FormKey> = record_keywords(store, &r, p)?.into_iter().collect();
    if let Some((rr, rp)) = store.raw_with_plugin(race, b)? {
        out.extend(record_keywords(store, &rr, rp)?);
    }
    Ok(Ok(out))
}

/// The entry [`hypotheses::CHAR_GRAPH_SUBGRAPH`] picks for `keywords`, or why none.
pub fn pick_entry<'e>(
    entries: &'e [Entry],
    keywords: &BTreeSet<FormKey>,
) -> std::result::Result<&'e Entry, String> {
    let fits: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.flags == 0 && e.target_keywords.is_empty())
        .filter(|e| e.actor_keywords.iter().all(|k| keywords.contains(k)))
        .collect();
    let Some(most) = fits.iter().map(|e| e.actor_keywords.len()).max() else {
        return Err(
            "no entry of flag word 0 without target keywords fits the actor's keywords".into(),
        );
    };
    let best: Vec<&&Entry> = fits
        .iter()
        .filter(|e| e.actor_keywords.len() == most)
        .collect();
    match best.as_slice() {
        [one] => Ok(one),
        _ => Err(format!(
            "{} entries tie with {most} of the actor's keywords",
            best.len()
        )),
    }
}

/// The archive key of a clip text under [`hypotheses::CHAR_GRAPH_CLIP_FOLDER`].
pub fn clip_key(
    vfs: &Vfs,
    clip: &str,
    project: &str,
    entry: Option<&Entry>,
) -> std::result::Result<String, String> {
    let stem = clip
        .rsplit_once('.')
        .map_or(clip, |(s, _)| s)
        .replace('\\', "/");
    let exists = |k: &str| vfs.chain(k).is_ok_and(|c| !c.is_empty());
    match entry {
        Some(e) => {
            let name = stem.rsplit('/').next().unwrap_or(&stem);
            for f in &e.folders {
                if let Some(k) = key_under("meshes", &format!("{f}/{name}.hkx")) {
                    if exists(&k) {
                        return Ok(k);
                    }
                }
            }
            Err(format!(
                "the clip's file name is in none of the entry's {} folders",
                e.folders.len()
            ))
        }
        None => match key_under(folder_of(project), &format!("{stem}.hkx")) {
            Some(k) if exists(&k) => Ok(k),
            Some(k) => Err(format!("{k} is not mounted")),
            None => Err("the clip text is not a valid path".into()),
        },
    }
}

/// The actor's inputs to a walk.
pub struct Actor<'k> {
    pub race: &'k FormKey,
    pub section: usize,
    /// The actor's keywords ([`actor_keywords`]), or why they are not read.
    pub keywords: std::result::Result<&'k BTreeSet<FormKey>, &'k str>,
}

struct Walk<'s> {
    steps: Vec<Step>,
    rules: BTreeSet<&'static str>,
    ties: u32,
    subgraph: Option<Entry>,
    switches: &'s Switches,
}
impl Walk<'_> {
    fn step(&mut self, file: &str, object: usize, class: &str, how: impl Into<String>) {
        self.steps.push(Step {
            file: file.to_string(),
            object,
            class: class.to_string(),
            how: how.into(),
        });
    }
    fn refuse(&mut self, code: &'static str, detail: impl Into<String>) -> Refusal {
        Refusal {
            code,
            detail: detail.into(),
            steps: std::mem::take(&mut self.steps),
        }
    }
}

/// The stored value a binding of `path` on `object` gives, as a number:
/// `Ok(None)` without such a binding.
fn bound(
    c: &hk::Container<'_>,
    object: usize,
    path: &str,
    vars: &hk::Variables,
    file: &str,
    b: &Budget,
) -> Result<std::result::Result<Option<f64>, (&'static str, String)>> {
    let all = hk::bindings(c, object, file, 0, b)?;
    let Some(x) = all.iter().find(|x| x.path == path) else {
        return Ok(Ok(None));
    };
    if x.kind != 0 {
        return Ok(Err((
            "GraphBindingCharacterProperty",
            format!(
                "{path} is bound to character property {} (a value the actor holds at run time)",
                x.variable
            ),
        )));
    }
    if x.bit != -1 {
        return Ok(Err((
            "GraphBindingBit",
            format!("{path} is bound to bit {} of a variable", x.bit),
        )));
    }
    Ok(match vars.number(x.variable) {
        Some(v) => Ok(Some(v)),
        None => Err((
            "GraphVariableType",
            format!(
                "{path} is bound to variable {} of type {:?}",
                x.variable,
                vars.types.get(x.variable.max(0) as usize)
            ),
        )),
    })
}

/// Walks the graph of the actor's race from its root (see the module).
/// `Ok(Err(_))` is a named refusal; `Err` an unreadable file.
pub fn start_clip(
    vfs: &Vfs,
    store: &Store,
    actor: &Actor<'_>,
    switches: &Switches,
    b: &Budget,
) -> Result<std::result::Result<StartClip, Refusal>> {
    let mut w = Walk {
        steps: Vec::new(),
        rules: BTreeSet::new(),
        ties: 0,
        subgraph: None,
        switches,
    };
    if !switches.start {
        return Ok(Err(w.refuse(
            "GraphStartSwitchOff",
            format!(
                "{} is off (--actor-start-clip)",
                hypotheses::CHAR_GRAPH_START
            ),
        )));
    }
    w.rules.insert(hypotheses::CHAR_GRAPH_START);
    w.rules.insert(hypotheses::ACTOR_SECTION);
    let Some(race) = store.raw(actor.race, b)? else {
        return Ok(Err(w.refuse("GraphRace", "the race has no winning record")));
    };
    let Some(project_text) = race_project(&race, actor.section) else {
        return Ok(Err(w.refuse(
            "GraphRaceProject",
            format!("no MODL after NAM3 in section {}", actor.section),
        )));
    };
    let chain = match project_chain(vfs, &project_text, b) {
        Ok(x) => x,
        Err(e) => return Ok(Err(w.refuse("GraphProjectChain", e))),
    };
    let (project, behavior) = chain;
    let file = behavior.clone();
    walk(vfs, store, actor, &mut w, project, behavior, file, b)
}

/// The walk from the root of graph file `file` down to a clip (see the
/// module); `project` and `behavior` are the race's, for the clip's folder
/// and the result.
#[allow(clippy::too_many_arguments)]
fn walk(
    vfs: &Vfs,
    store: &Store,
    actor: &Actor<'_>,
    w: &mut Walk<'_>,
    project: String,
    behavior: String,
    mut file: String,
    b: &Budget,
) -> Result<std::result::Result<StartClip, Refusal>> {
    let mut files = 0usize;
    'files: loop {
        files += 1;
        if files > MAX_FILES {
            return Ok(Err(w.refuse(
                "GraphFileLimit",
                format!("more than {MAX_FILES} graph files"),
            )));
        }
        let scope = b.child();
        let bytes = vfs.read(&file, &scope)?;
        let c = hk::Container::read(&bytes, &file, 0, &scope)?;
        let vars = hk::Variables::read(&c, hk::graph_data(&c, &file, 0)?, &file, 0, &scope)?;
        let g = hk::only_object(&c, hk::BEHAVIOR_GRAPH).ok_or_else(|| {
            fo4_core::Error::new(
                "InvalidHavokBehaviorGraph",
                &file,
                0,
                "one behaviour graph",
                "none",
            )
        })?;
        let Some(mut at) = hk::BehaviorGraph::read(&c, g, &file, 0)?.root else {
            return Ok(Err(
                w.refuse("GraphNoRoot", format!("{file}: no root generator"))
            ));
        };
        loop {
            if w.steps.len() >= MAX_STEPS {
                return Ok(Err(w.refuse(
                    "GraphStepLimit",
                    format!("more than {MAX_STEPS} generators"),
                )));
            }
            let node = hk::Node::read(&c, at, &file, 0, &scope)?;
            let class = c.objects[at].class.to_string();
            macro_rules! value {
                ($obj:expr, $path:expr) => {
                    match bound(&c, $obj, $path, &vars, &file, &scope)? {
                        Ok(v) => v,
                        Err((code, why)) => return Ok(Err(w.refuse(code, why))),
                    }
                };
            }
            let next = match node {
                hk::Node::Machine(m) => {
                    let start = match value!(at, "startStateId") {
                        Some(v) => v as i32,
                        None => m.start,
                    };
                    let Some(state) = hk::machine_state(&c, &m, start, &file, 0)? else {
                        return Ok(Err(w.refuse(
                            "GraphStartStateMissing",
                            format!(
                                "start state {start} is none of the machine's {} states",
                                m.states.len()
                            ),
                        )));
                    };
                    let mut eventless = 0;
                    for t in [state.transitions, m.wildcards].into_iter().flatten() {
                        eventless += hk::transitions(&c, t, &file, 0, &scope)?
                            .iter()
                            .filter(|x| x.event < 0)
                            .count();
                    }
                    if eventless > 0 {
                        return Ok(Err(w.refuse(
                            "GraphTransitionWithoutEvent",
                            format!("{eventless} transitions out of the start state wait on no event (their conditions are not read)"),
                        )));
                    }
                    w.step(&file, at, &class, format!("start state {start}"));
                    state.generator
                }
                hk::Node::Wrapper { generator } => {
                    w.step(&file, at, &class, "its generator");
                    generator
                }
                hk::Node::Offset { generator, .. } => {
                    w.step(
                        &file,
                        at,
                        &class,
                        "its generator (the offset clip is not drawn)",
                    );
                    generator
                }
                hk::Node::Blender {
                    flags, children, ..
                } => {
                    if flags & BLEND_PARAMETRIC_FLAG != 0 {
                        return Ok(Err(w.refuse(
                            "GraphBlendFlag",
                            format!("blender flag word {flags:#06x} has bit {BLEND_PARAMETRIC_FLAG:#x} (weights read as positions, not read)"),
                        )));
                    }
                    let mut best: Option<(f64, Option<usize>)> = None;
                    let mut tie = false;
                    for ch in &children {
                        let weight = match value!(ch.object, "weight") {
                            Some(v) => v,
                            None => f64::from(ch.weight),
                        };
                        match best {
                            Some((bw, _)) if weight < bw => {}
                            Some((bw, _)) if weight == bw => tie = true,
                            _ => {
                                best = Some((weight, ch.generator));
                                tie = false;
                            }
                        }
                    }
                    let Some((weight, generator)) = best else {
                        return Ok(Err(
                            w.refuse("GraphBlendEmpty", "a blender without children")
                        ));
                    };
                    if tie {
                        w.ties += 1;
                    }
                    w.step(
                        &file,
                        at,
                        &class,
                        format!(
                            "child of weight {weight}{} of {}",
                            if tie {
                                " (the first of equal weights)"
                            } else {
                                ""
                            },
                            children.len()
                        ),
                    );
                    generator
                }
                hk::Node::Selector {
                    generators,
                    selected,
                } => {
                    let index = match value!(at, "selectedGeneratorIndex") {
                        Some(v) => v as i64,
                        None => i64::from(selected),
                    };
                    let Some(g) = usize::try_from(index).ok().and_then(|i| generators.get(i))
                    else {
                        return Ok(Err(w.refuse(
                            "GraphSelectorIndex",
                            format!("index {index} of {} generators", generators.len()),
                        )));
                    };
                    w.step(
                        &file,
                        at,
                        &class,
                        format!("generator {index} of {}", generators.len()),
                    );
                    Some(*g)
                }
                hk::Node::Reference { behavior } => {
                    w.step(&file, at, &class, "its behaviour file");
                    match key_under(folder_of(&project), &behavior) {
                        Some(k) => {
                            file = k;
                            continue 'files;
                        }
                        None => {
                            return Ok(Err(w.refuse(
                                "GraphReferencePath",
                                "the behaviour text is not a valid path",
                            )))
                        }
                    }
                }
                hk::Node::Swap => {
                    if !w.switches.subgraph {
                        return Ok(Err(w.refuse(
                            "GraphSwapAtRunTime",
                            format!(
                                "a swap generator plays a subgraph the run chooses ({} is off)",
                                hypotheses::CHAR_GRAPH_SUBGRAPH
                            ),
                        )));
                    }
                    if w.subgraph.is_some() {
                        return Ok(Err(w.refuse(
                            "GraphSwapNested",
                            "a second swap generator below a subgraph",
                        )));
                    }
                    let keywords = match actor.keywords {
                        Ok(k) => k,
                        Err(why) => return Ok(Err(w.refuse("GraphActorKeywords", why))),
                    };
                    let Some((_, entries)) = subgraphs(store, actor.race, &scope)? else {
                        return Ok(Err(w.refuse(
                            "GraphSubgraphRace",
                            "the subgraph race has no winning record",
                        )));
                    };
                    let e = match pick_entry(&entries, keywords) {
                        Ok(e) => e.clone(),
                        Err(why) => return Ok(Err(w.refuse("GraphSubgraphEntry", why))),
                    };
                    w.step(
                        &file,
                        at,
                        &class,
                        format!("subgraph entry {} of {}", e.index, entries.len()),
                    );
                    w.rules.insert(hypotheses::CHAR_GRAPH_SUBGRAPH);
                    let Some(k) = key_under("meshes", &e.behavior) else {
                        return Ok(Err(w.refuse(
                            "GraphSubgraphPath",
                            "the entry's behaviour text is not a valid path",
                        )));
                    };
                    w.subgraph = Some(e);
                    file = k;
                    continue 'files;
                }
                hk::Node::Clip(g) => {
                    w.step(&file, at, &class, "the clip");
                    if !w.switches.folder {
                        return Ok(Err(w.refuse(
                            "GraphClipFolderSwitchOff",
                            format!(
                                "which file a clip text names is {} (off)",
                                hypotheses::CHAR_GRAPH_CLIP_FOLDER
                            ),
                        )));
                    }
                    let clip = match clip_key(vfs, &g.clip, &project, w.subgraph.as_ref()) {
                        Ok(k) => k,
                        Err(why) => return Ok(Err(w.refuse("GraphClipFile", why))),
                    };
                    w.rules.insert(hypotheses::CHAR_GRAPH_CLIP_FOLDER);
                    return Ok(Ok(StartClip {
                        project,
                        behavior,
                        subgraph: w.subgraph.take(),
                        generator: g,
                        clip,
                        steps: std::mem::take(&mut w.steps),
                        ties: w.ties,
                        rules: std::mem::take(&mut w.rules),
                    }));
                }
                hk::Node::Unread { class } => {
                    return Ok(Err(
                        w.refuse("GraphUnreadClass", format!("{class} is not read"))
                    ))
                }
            };
            let Some(n) = next else {
                return Ok(Err(w.refuse(
                    "GraphNoGenerator",
                    format!("a {class} without its generator link"),
                )));
            };
            at = n;
        }
    }
}

/// The subgraph entry [`hypotheses::CHAR_GRAPH_FURNITURE`] picks for a
/// furniture's keywords: an entry of flag word 2 without actor keywords
/// whose target keywords all are the furniture's, the one with the most; a
/// tie or none refused.
pub fn pick_furniture_entry<'e>(
    entries: &'e [Entry],
    keywords: &BTreeSet<FormKey>,
) -> std::result::Result<&'e Entry, String> {
    let fits: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.flags == 2 && e.actor_keywords.is_empty())
        .filter(|e| !e.target_keywords.is_empty())
        .filter(|e| e.target_keywords.iter().all(|k| keywords.contains(k)))
        .collect();
    let Some(most) = fits.iter().map(|e| e.target_keywords.len()).max() else {
        return Err(
            "no entry of flag word 2 without actor keywords has target keywords all the furniture's"
                .into(),
        );
    };
    let best: Vec<&&Entry> = fits
        .iter()
        .filter(|e| e.target_keywords.len() == most)
        .collect();
    match best.as_slice() {
        [one] => Ok(one),
        _ => Err(format!(
            "{} entries tie with {most} of the furniture's keywords",
            best.len()
        )),
    }
}

/// The clip a sitter is posed by (`--actor-sit-clip`,
/// [`hypotheses::CHAR_GRAPH_FURNITURE`]): the race's subgraph entry
/// [`pick_furniture_entry`] picks for the furniture base's keywords (KWDA),
/// walked from its behaviour file's root with the start clip readings
/// ([`start_clip`]; [`Switches::start`] and [`Switches::sit`] must be on).
pub fn furniture_clip(
    vfs: &Vfs,
    store: &Store,
    actor: &Actor<'_>,
    furniture: &FormKey,
    switches: &Switches,
    b: &Budget,
) -> Result<std::result::Result<StartClip, Refusal>> {
    let mut w = Walk {
        steps: Vec::new(),
        rules: BTreeSet::new(),
        ties: 0,
        subgraph: None,
        switches,
    };
    if !(switches.start && switches.sit) {
        return Ok(Err(w.refuse(
            "GraphStartSwitchOff",
            format!(
                "{} and {} must be on (--actor-start-clip, --actor-sit-clip)",
                hypotheses::CHAR_GRAPH_START,
                hypotheses::CHAR_GRAPH_FURNITURE
            ),
        )));
    }
    w.rules.insert(hypotheses::CHAR_GRAPH_START);
    w.rules.insert(hypotheses::CHAR_GRAPH_FURNITURE);
    w.rules.insert(hypotheses::ACTOR_SECTION);
    let Some(race) = store.raw(actor.race, b)? else {
        return Ok(Err(w.refuse("GraphRace", "the race has no winning record")));
    };
    let Some(project_text) = race_project(&race, actor.section) else {
        return Ok(Err(w.refuse(
            "GraphRaceProject",
            format!("no MODL after NAM3 in section {}", actor.section),
        )));
    };
    let (project, behavior) = match project_chain(vfs, &project_text, b) {
        Ok(x) => x,
        Err(e) => return Ok(Err(w.refuse("GraphProjectChain", e))),
    };
    let Some((fr, fp)) = store.raw_with_plugin(furniture, b)? else {
        return Ok(Err(w.refuse(
            "GraphFurniture",
            "the furniture base has no winning record",
        )));
    };
    let keywords: BTreeSet<FormKey> = record_keywords(store, &fr, fp)?.into_iter().collect();
    let Some((_, entries)) = subgraphs(store, actor.race, b)? else {
        return Ok(Err(w.refuse(
            "GraphSubgraphRace",
            "the subgraph race has no winning record",
        )));
    };
    let e = match pick_furniture_entry(&entries, &keywords) {
        Ok(e) => e.clone(),
        Err(why) => return Ok(Err(w.refuse("GraphFurnitureEntry", why))),
    };
    let Some(file) = key_under("meshes", &e.behavior) else {
        return Ok(Err(w.refuse(
            "GraphSubgraphPath",
            "the entry's behaviour text is not a valid path",
        )));
    };
    w.step(
        &file,
        0,
        "furniture entry",
        format!("subgraph entry {} of {}", e.index, entries.len()),
    );
    w.subgraph = Some(e);
    walk(vfs, store, actor, &mut w, project, behavior, file, b)
}

/// The project, character and graph files of a project text (under
/// `meshes`), as `crate::anim_graph` follows them; the project key and
/// the graph key, or why not.
pub fn project_chain(
    vfs: &Vfs,
    project: &str,
    b: &Budget,
) -> std::result::Result<(String, String), String> {
    let pk = key_under("meshes", project).ok_or("the project path is not valid")?;
    let pb = vfs
        .read(&pk, &b.child())
        .map_err(|e| format!("{pk}: {}", e.code))?;
    let pc = hk::Container::read(&pb, &pk, 0, &b.child()).map_err(|e| e.to_string())?;
    let i = hk::only_object(&pc, hk::PROJECT_STRINGS).ok_or("not one project string object")?;
    let cs = hk::project_characters(&pc, i, &pk, 0, b).map_err(|e| e.to_string())?;
    let [ch] = cs.as_slice() else {
        return Err(format!("the project names {} characters", cs.len()));
    };
    let ck = key_under(folder_of(&pk), &String::from_utf8_lossy(ch))
        .ok_or("the character path is not valid")?;
    let cb = vfs
        .read(&ck, &b.child())
        .map_err(|e| format!("{ck}: {}", e.code))?;
    let cc = hk::Container::read(&cb, &ck, 0, &b.child()).map_err(|e| e.to_string())?;
    let i = hk::only_object(&cc, hk::CHARACTER_STRINGS).ok_or("not one character string object")?;
    let beh = hk::character_behavior(&cc, i, &ck, 0).map_err(|e| e.to_string())?;
    let bk = key_under(folder_of(&pk), &String::from_utf8_lossy(beh))
        .ok_or("the behaviour path is not valid")?;
    Ok((pk, bk))
}

/// The clip time a frame poses at: the generator's start time, or under
/// [`hypotheses::CHAR_GRAPH_CLIP_LOOP`] the frame's second from it, folded
/// into the clip's length when the generator's mode is 1 and held at the
/// end when it is 0; another mode, or a playback speed other than 1, is
/// refused under that switch.
pub fn clip_time(
    g: &hk::ClipGenerator,
    duration: f64,
    second: f64,
    cycle: bool,
) -> std::result::Result<f64, String> {
    let start = f64::from(g.start_time);
    if !cycle {
        return Ok(start);
    }
    if g.playback_speed != 1.0 {
        return Err(format!(
            "playback speed {} (only 1 is read)",
            g.playback_speed
        ));
    }
    if duration.is_nan() || duration <= 0.0 {
        return Err(format!("clip length {duration}"));
    }
    let t = start + second;
    match g.mode {
        1 => Ok(t.rem_euclid(duration)),
        0 => Ok(t.min(duration)),
        m => Err(format!("clip mode {m} (only 0 and 1 are read)")),
    }
}

#[cfg(test)]
mod tests;
