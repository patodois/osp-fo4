#![forbid(unsafe_code)]
//! Coverage census: for every cell of the load order (one cell at a time,
//! the asset cache renewed every few cells), or for one test view (a cell,
//! optionally with the exterior cells around it, as the viewer loads it),
//! counts the placements by outcome with the viewer's own draw build:
//! drawn without `--under-hypotheses`, drawn only under hypotheses, hidden
//! by a registered rule, outside the visual slice, or omitted. Omissions are
//! broken down by exact reason (the uncovered blockers of the placement,
//! labelled by where they were raised, or the draw build's own refusal),
//! ranked by placement count and by an area proxy: the largest face of the
//! base's OBND box, times the reference scale squared, so that floors and
//! walls weigh more than clutter.
//!
//! Writes `summary.txt` (aggregates only, fit for `spec/`) and, outside the
//! repository, `bases.tsv` (reason by base record, with editor IDs and model
//! paths) and, for a view, `placements.tsv` (one line per placement).
//!
//! ```text
//! coverage_census MANIFEST OUT_DIR --all [--every N] [--limit N]
//! coverage_census MANIFEST OUT_DIR --view CELL_EDITOR_ID [--around N]
//! coverage_census MANIFEST OUT_DIR --views FILE [--mark FILE]
//! ```
//!
//! `--views FILE`: the views a run loads, one per line (`KEY AROUND
//! DESCRIPTION...`, the cell as a record key `plugin:XXXXXXXX`), each
//! counted alone into `OUT_DIR/view-K/` and all of them into `OUT_DIR`, with
//! `views.txt` (counts and reason labels only, fit for `spec/`): for each
//! view and the sum, the outcomes and the omitted placements' families and
//! exact reasons with the share each holds of the view's omitted
//! placements. `--mark FILE` (one record key per line): those placements'
//! outcomes and reasons are counted apart as well (the play run's named
//! platforms). `--render-child-flag-agrees`, `--render-static-fields`,
//! `--render-static-leaves`, `--render-reference-alpha` and
//! `--render-static-parts` (with `--views` only; off by default) read
//! render readiness under `xesp_child_flag_agrees_chain_v1`,
//! `refr_static_fields_render_unchanged_v1`,
//! `play_static_leaf_two_pose_v1`, `refr_xalp_render_unchanged_v1` and
//! `static_unmoved_parts_drawn_v1` (the viewer's `--draw-child-flag-agrees`,
//! `--draw-static-fields`, `--draw-static-leaves`, `--draw-reference-alpha`
//! and `--draw-static-parts`).
#[path = "coverage/detail.rs"]
mod detail;
use fo4_core::{Budget, Error, Manifest};
use fo4_plugin::{typed::Store, FormKey};
use fo4_render::draw::{BlockOptions, BuildOptions};
use fo4_scene::{
    hypotheses::{self, covering, status, Status},
    readiness::Origin,
    Instance, Loader, RuleLevel, State,
};
use fo4_vfs::Vfs;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::Write as _,
    path::Path,
};

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

/// The viewer's texture policy (`apps/fo4-view`, default `--texture-cap`).
const VIEWER_BLOCKS: BlockOptions = BlockOptions {
    max_dimension: 1024,
    max_alpha_tested_dimension: 2048,
    max_bytes: 512 << 20,
};

#[derive(Default, Clone, Copy)]
struct Tally {
    n: u64,
    area: f64,
}
impl Tally {
    fn add(&mut self, area: f64) {
        self.n += 1;
        self.area += area;
    }
}

/// One placement's outcome.
enum Outcome {
    Drawn,
    UnderHypotheses(Vec<&'static str>),
    Hidden(String),
    Outside(String),
    Omitted(Vec<String>),
}

#[derive(Default)]
struct Census {
    cells: u64,
    cell_failures: BTreeMap<String, u64>,
    placements: Tally,
    drawn: Tally,
    hypotheses: Tally,
    hidden: BTreeMap<String, Tally>,
    outside: BTreeMap<String, Tally>,
    omitted: Tally,
    /// Drawn with the root node's own transform left out
    /// (`nif_root_transform_replaced_v1`, under hypotheses).
    root_left_out: Tally,
    /// Placements with an XESP, by kind, the start state their chain gives
    /// under the enable parent rules and outcome (`spec/enable-parent.md`).
    enable: BTreeMap<String, Tally>,
    no_bounds: u64,
    /// Omitted placements whose reasons are this one only.
    sole: BTreeMap<String, Tally>,
    /// Every reason of an omitted placement, once per placement.
    any: BTreeMap<String, Tally>,
    /// The same by family of reasons ([`family`]).
    family_sole: BTreeMap<String, Tally>,
    family_any: BTreeMap<String, Tally>,
    family_sets: BTreeMap<String, Tally>,
    pending_rules: BTreeMap<String, Tally>,
    pending_sets: BTreeMap<String, Tally>,
    by_type: BTreeMap<(String, String), Tally>,
    elapsed: f64,
    exclusions: BTreeMap<String, u64>,
    deleted: u64,
    disabled: u64,
    /// (reason, base) -> tally, for the local table with real names.
    bases: BTreeMap<(String, Option<FormKey>), Tally>,
    base_info: BTreeMap<FormKey, (String, String, String)>,
    /// Sub-reasons of missing materials, swap scopes and texture transform
    /// controllers, and the UV fields of drawn placements, by (section,
    /// label) (`detail`).
    detail: BTreeMap<(String, String), Tally>,
    /// Texture-transform analysis by model path (not counted).
    texture_cache: BTreeMap<Vec<u8>, detail::TextureModel>,
    /// The caller's marked placements (`--mark`; keys with the plugin
    /// lower case), not counted themselves.
    marks: BTreeSet<FormKey>,
    /// The marked placements by kind, outcome and reasons.
    marked: BTreeMap<String, Tally>,
}

/// A label for `e` that carries no path, FormID or object name: the code,
/// and the `found` text only for codes whose `found` is a record, field,
/// block-type or rule label.
fn label(e: &Error) -> String {
    let labelled = matches!(
        e.code.as_str(),
        "UninterpretedVisualSubrecord"
            | "UnsupportedUsedFieldLayout"
            | "RuntimeRecordState"
            | "RuntimeCondition"
            | "RuntimeEnableParent"
            | "UnsupportedNifBlock"
            | "UnsupportedNifBlockSemantics"
            | "UnsupportedNifBlockTail"
            | "UnsupportedAnimationRuntime"
            | "UnsupportedMaterialFields"
            | "UnsupportedNifField"
            | "UnsupportedSkinning"
            | "UnsupportedRenderSwapField"
            | "UnsupportedColorOnlyEffectFeatures"
            | "UnsupportedEffectSurface"
            | "UnsettledEffectMaterialField"
            | "UnsupportedBlendFlags"
            | "UnsupportedRenderAlpha"
            | "UnsupportedNifVertexAttributes"
            | "UnsupportedNifVertexDesc"
            | "BaseTypeOutsideSlice"
            | "RenderSwapPair"
            | "AmbiguousMaterialSwap"
            | "UnsupportedRenderSwapScopes"
    );
    if e.code == "UnsupportedRenderAlpha" {
        // `Alpha` in Debug form: keep the blend and test words only.
        let word = |name: &str| {
            e.found
                .split(&format!("{name}: "))
                .nth(1)
                .and_then(|v| v.split([',', ' ', '}']).next())
                .unwrap_or("?")
                .to_string()
        };
        return format!(
            "{} enabled={} source={} destination={} test={} nif_function={} opacity={}",
            e.code,
            word("enabled"),
            word("source"),
            word("destination"),
            word("test_enabled"),
            word("nif_test_function"),
            word("opacity")
        );
    }
    if !labelled || e.found.is_empty() {
        return e.code.clone();
    }
    let found: String = e
        .found
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if found.contains(['/', '\\']) || found.len() > 96 {
        return e.code.clone();
    }
    format!("{} {}", e.code, found)
}

fn origin(o: Option<&Origin>) -> &'static str {
    match o {
        Some(Origin::Record) | None => "record",
        Some(Origin::MaterialSwap) => "swap",
        Some(Origin::Model) => "model",
        Some(Origin::ModelCitation) => "model citation",
        Some(Origin::RenderAsset) => "render asset",
    }
}

/// The reasons that keep a blocked placement out: its blockers that no
/// registered rule covers, or whose rule is refuted.
fn reasons(blockers: &[Error], origins: Option<&[Origin]>, prefix: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (k, e) in blockers.iter().enumerate() {
        let refuted = covering(e).filter(|h| status(h) == Some(Status::Refuted));
        if covering(e).is_some() && refuted.is_none() {
            continue;
        }
        let o = if prefix.is_empty() {
            origin(origins.and_then(|v| v.get(k)))
        } else {
            prefix
        };
        let mut key = format!("{o}: {}", label(e));
        if let Some(h) = refuted {
            let _ = write!(key, " (refuted {h})");
        }
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out.sort();
    out
}

/// A coarser grouping of a reason: every animation-controller block of a
/// model is one family, the particle-system blocks another, a record field
/// is named by its record and subrecord signatures only.
fn family(key: &str) -> String {
    let (o, rest) = key.split_once(": ").unwrap_or(("", key));
    let (code, found) = rest.split_once(' ').unwrap_or((rest, ""));
    let ty = found.split(';').next().unwrap_or("");
    let particle = ty.starts_with("NiPSys")
        || ty.starts_with("BSPSys")
        || ty == "NiParticleSystem"
        || ty.starts_with("NiPSysEmitter");
    match code {
        "UnsupportedAnimationRuntime" | "UnsupportedNifBlock" if particle => {
            format!("{o}: particle-system blocks")
        }
        // Split by what the blocks are at rest (`fo4_scene::rest`): the tag
        // names why the rest state is not decided.
        "UnsupportedAnimationRuntime" => match found.rsplit_once(";rest=") {
            Some((_, tag)) => format!("{o}: animation-controller blocks ({tag})"),
            None => format!("{o}: animation-controller blocks"),
        },
        "UninterpretedVisualSubrecord"
        | "UnsupportedUsedFieldLayout"
        | "RuntimeRecordState"
        | "RuntimeCondition" => {
            let mut parts = found.split('.');
            match (parts.next(), parts.next()) {
                (Some(a), Some(b)) => format!("{o}: {a}.{b}"),
                (Some(a), None) => format!("{o}: {a}"),
                _ => format!("{o}: {code}"),
            }
        }
        _ => key.split(" (refuted").next().unwrap_or(key).to_string(),
    }
}

fn families(reasons: &[String]) -> Vec<String> {
    let mut v: Vec<String> = reasons.iter().map(|r| family(r)).collect();
    v.sort();
    v.dedup();
    v
}

/// The rules a drawn placement relies on that are not supported by
/// measurement: those of its state and those the draw build added
/// (`Identity::hypotheses`).
fn pending(rules: &[&'static str]) -> Vec<&'static str> {
    let mut v: Vec<_> = rules
        .iter()
        .copied()
        // The root transform rule changes how a placement is drawn, not
        // whether: without it the placement is drawn with the root applied.
        .filter(|h| status(h) != Some(Status::Supported) && *h != hypotheses::ROOT_TRANSFORM)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Largest face of an OBND box, times the scale squared.
fn area(bounds: Option<[i16; 6]>, scale: Option<f32>) -> Option<f64> {
    let b = bounds?;
    let d = [0, 1, 2].map(|k| (f64::from(b[k + 3]) - f64::from(b[k])).abs());
    let s = f64::from(scale.unwrap_or(1.));
    Some((d[0] * d[1]).max(d[0] * d[2]).max(d[1] * d[2]) * s * s)
}

fn sig(s: &[u8; 4]) -> String {
    String::from_utf8_lossy(s).into_owned()
}

fn instance_outcome(
    i: &Instance,
    id: &fo4_render::draw::Identity,
    omission: Option<&fo4_render::draw::Omission>,
) -> Outcome {
    let Some(o) = omission else {
        let rules = pending(&id.hypotheses);
        return if rules.is_empty() {
            Outcome::Drawn
        } else {
            Outcome::UnderHypotheses(rules)
        };
    };
    let first = o.causes.first();
    if first.is_some_and(|e| e.code == "EditorMarkerHiddenUnderHypothesis") {
        return Outcome::Hidden(fo4_scene::hypotheses::MARKER.into());
    }
    if first.is_some_and(|e| e.code == "DisabledThroughEnableParent") {
        return Outcome::Hidden(hypotheses::ENABLE_FOLLOWS.into());
    }
    match &i.state {
        State::Outside => Outcome::Outside(
            i.base
                .as_ref()
                .map_or("no base".into(), |b| sig(&b.record.signature)),
        ),
        State::Blocked => Outcome::Omitted(reasons(&i.blockers, Some(&i.blocker_origins), "")),
        _ => Outcome::Omitted(vec![format!(
            "draw: {}",
            first.map_or("unknown".into(), label)
        )]),
    }
}

impl Census {
    #[allow(clippy::too_many_arguments)]
    fn count(
        &mut self,
        outcome: &Outcome,
        kind: &'static str,
        base: Option<&FormKey>,
        area: Option<f64>,
        rows: Option<&mut String>,
        reference: &FormKey,
        place: &str,
    ) {
        if area.is_none() {
            self.no_bounds += 1;
        }
        let a = area.unwrap_or(0.);
        self.placements.add(a);
        let (name, detail) = match outcome {
            Outcome::Drawn => {
                self.drawn.add(a);
                ("drawn", String::new())
            }
            Outcome::UnderHypotheses(rules) => {
                self.hypotheses.add(a);
                for r in rules {
                    self.pending_rules.entry((*r).into()).or_default().add(a);
                }
                let set = rules.join(" + ");
                self.pending_sets.entry(set.clone()).or_default().add(a);
                ("under hypotheses", set)
            }
            Outcome::Hidden(rule) => {
                self.hidden.entry(rule.clone()).or_default().add(a);
                ("hidden", rule.clone())
            }
            Outcome::Outside(t) => {
                self.outside.entry(t.clone()).or_default().add(a);
                ("outside", t.clone())
            }
            Outcome::Omitted(reasons) => {
                self.omitted.add(a);
                let reasons: Vec<String> = if reasons.is_empty() {
                    vec!["blocked: no uncovered reason".into()]
                } else {
                    reasons.clone()
                };
                if reasons.len() == 1 {
                    self.sole.entry(reasons[0].clone()).or_default().add(a);
                }
                for r in &reasons {
                    self.any.entry(r.clone()).or_default().add(a);
                    self.bases
                        .entry((r.clone(), base.cloned()))
                        .or_default()
                        .add(a);
                }
                let fam = families(&reasons);
                if fam.len() == 1 {
                    self.family_sole.entry(fam[0].clone()).or_default().add(a);
                }
                for f in &fam {
                    self.family_any.entry(f.clone()).or_default().add(a);
                }
                self.family_sets.entry(fam.join(" + ")).or_default().add(a);
                ("omitted", reasons.join(" + "))
            }
        };
        self.by_type
            .entry((kind.into(), name.into()))
            .or_default()
            .add(a);
        if let Some(rows) = rows {
            let base = base.map_or(String::new(), |k| {
                format!("{}:{:06x}", k.plugin, k.object_id)
            });
            let (edid, model, _) = base_key_info(self, &base);
            let _ = writeln!(
                rows,
                "{}:{:06x}\t{kind}\t{base}\t{edid}\t{model}\t{a:.0}\t{name}\t{detail}\t{place}",
                reference.plugin, reference.object_id
            );
        }
    }
}

/// The enable tally key of a placement with an XESP.
fn enable_key(kind: &str, p: &fo4_scene::enable::Presence, outcome: &Outcome) -> String {
    let start = match p.enabled {
        Some(true) => "chain says enabled",
        Some(false) => "chain says disabled",
        None => "chain not decided",
    };
    let o = match outcome {
        Outcome::Drawn => "drawn",
        Outcome::UnderHypotheses(_) => "drawn under hypotheses",
        Outcome::Hidden(rule) if rule == hypotheses::ENABLE_FOLLOWS => {
            "not drawn: disabled through the chain"
        }
        Outcome::Hidden(_) => "hidden by another rule",
        Outcome::Outside(_) => "outside",
        Outcome::Omitted(_) => "omitted",
    };
    format!("{kind}, {start}, {o}")
}

/// One placement's outcome as one line: its name and, omitted, its
/// reasons; drawn under hypotheses, its pending rules.
fn outcome_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Drawn => "drawn".into(),
        Outcome::UnderHypotheses(rules) => format!("drawn under hypotheses: {}", rules.join(" + ")),
        Outcome::Hidden(rule) => format!("hidden by {rule}"),
        Outcome::Outside(t) => format!("outside ({t})"),
        Outcome::Omitted(reasons) => format!("omitted: {}", reasons.join(" + ")),
    }
}

fn base_key_info(c: &Census, base: &str) -> (String, String, String) {
    c.base_info
        .iter()
        .find(|(k, _)| format!("{}:{:06x}", k.plugin, k.object_id) == base)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// The clock of the census's draw builds: the seconds of the game clock
/// (`BuildOptions::time`, one census per time) and the clock's switches
/// (`fo4_scene::clock::Switches`, off by default).
struct Clock {
    times: Vec<f64>,
    switches: fo4_scene::clock::Switches,
}

/// Loads one cell (and the cells around it) as the viewer does, builds the
/// viewer's draw list with actors at the hypotheses level at each time of
/// `clock` and counts every placement into that time's census. A failure
/// to load fails the cell for every time; a failure of one time's build is
/// that time's alone (`Ok` holds it).
#[allow(clippy::too_many_arguments)]
fn cell(
    cs: &mut [Census],
    clock: &Clock,
    loader: &mut Loader<'_>,
    store: &Store,
    m: &Manifest,
    vfs: &Vfs,
    spelling: &detail::Spelling,
    key: &FormKey,
    around: u32,
    mut rows: Option<&mut String>,
    b: &Budget,
) -> fo4_core::Result<Vec<Option<Error>>> {
    let mut scene = loader.cell(key, b)?;
    if around > 0 {
        let a = fo4_scene::grid::around(store, &scene.cell.key, around, b)?;
        fo4_scene::grid::absorb_around(loader, &mut scene, &a, b)?;
    }
    let actors = fo4_scene::actor::resolve(&scene, loader, b)?;
    let mut failures = Vec::new();
    for (c, time) in cs.iter_mut().zip(&clock.times) {
        // The rows of a view are those of its first time.
        let rows = if failures.is_empty() {
            rows.as_deref_mut()
        } else {
            None
        };
        failures.push(
            count(
                c,
                *time,
                clock.switches,
                &scene,
                &actors,
                loader,
                store,
                m,
                vfs,
                spelling,
                rows,
                b,
            )
            .err(),
        );
    }
    Ok(failures)
}

/// Builds the draw list of a loaded cell at `time` and counts every
/// placement into `c`.
#[allow(clippy::too_many_arguments)]
fn count(
    c: &mut Census,
    time: f64,
    switches: fo4_scene::clock::Switches,
    scene: &fo4_scene::Scene,
    actors: &fo4_scene::actor::Actors,
    loader: &mut Loader<'_>,
    store: &Store,
    m: &Manifest,
    vfs: &Vfs,
    spelling: &detail::Spelling,
    rows: Option<&mut String>,
    b: &Budget,
) -> fo4_core::Result<()> {
    // At a second of the game clock (0 unless asked): a model whose
    // free-running controllers the clock runs is drawn as it is then
    // (`BuildOptions::time`).
    let options = BuildOptions {
        time: Some(time),
        clock: switches,
        ..BuildOptions::with_blocks(RuleLevel::Hypotheses, VIEWER_BLOCKS)
    };
    let build = fo4_render::draw::build_with_actors(scene, actors, loader, &m.limits, options, b)?;
    let omitted: BTreeMap<u32, &fo4_render::draw::Omission> =
        build.omitted.iter().map(|o| (o.instance, o)).collect();
    // Whether any draw of an instance has a UV offset other than 0 or a UV
    // scale other than 1.
    let mut uv: BTreeMap<u32, (bool, bool)> = BTreeMap::new();
    for d in &build.list.draws {
        let e = uv.entry(d.instance).or_default();
        e.0 |= d.uv_offset != [0., 0.];
        e.1 |= d.uv_scale != [1., 1.];
    }
    let by_ref: BTreeMap<&FormKey, &Instance> = scene
        .instances
        .iter()
        .map(|i| (&i.reference.key, i))
        .collect();
    let mut rows = rows;
    for id in &build.instances {
        if build.actors.contains(&id.index) {
            continue;
        }
        let Some(i) = by_ref.get(&id.reference) else {
            continue;
        };
        if let Some(base) = &i.base {
            c.base_info.entry(base.key.clone()).or_insert_with(|| {
                (
                    base.common.editor_id.clone().unwrap_or_default(),
                    base.common.model.path.clone().unwrap_or_default(),
                    sig(&base.record.signature),
                )
            });
        }
        let outcome = instance_outcome(i, id, omitted.get(&id.index).copied());
        if id.hypotheses.contains(&hypotheses::ROOT_TRANSFORM) {
            c.root_left_out.add(
                area(
                    i.base.as_ref().and_then(|v| v.common.bounds),
                    i.reference.data.scale,
                )
                .unwrap_or(0.),
            );
        }
        let a = area(
            i.base.as_ref().and_then(|v| v.common.bounds),
            i.reference.data.scale,
        );
        details(
            c,
            i,
            &outcome,
            loader,
            vfs,
            spelling,
            uv.get(&id.index),
            a.unwrap_or(0.),
            b,
        )?;
        if let Some(p) = &i.enable {
            c.enable
                .entry(enable_key("reference", p, &outcome))
                .or_default()
                .add(a.unwrap_or(0.));
        }
        if c.marks.contains(&fo4_scene::timeline::norm(&id.reference)) {
            c.marked
                .entry(format!("reference, {}", outcome_text(&outcome)))
                .or_default()
                .add(a.unwrap_or(0.));
        }
        c.count(
            &outcome,
            "reference",
            i.base.as_ref().map(|v| &v.key),
            a,
            rows.as_deref_mut(),
            &id.reference,
            &format!(
                "{:?} {:?}",
                i.reference.data.transform, i.reference.data.scale
            ),
        );
    }
    for (a, index) in actors.actors.iter().zip(&build.actors) {
        let o = omitted.get(index).copied();
        let rules = build
            .instances
            .get(*index as usize)
            .map(|id| pending(&id.hypotheses))
            .unwrap_or_default();
        let outcome = match o {
            None if rules.is_empty() => Outcome::Drawn,
            None => Outcome::UnderHypotheses(rules),
            Some(o)
                if o.causes
                    .first()
                    .is_some_and(|e| e.code == "DisabledThroughEnableParent") =>
            {
                Outcome::Hidden(hypotheses::ENABLE_FOLLOWS.into())
            }
            Some(_) if a.plan.is_none() => {
                let note = a.notes.first().map_or("no plan", String::as_str);
                let note: String = note
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_ascii_digit())
                    .collect();
                Outcome::Omitted(vec![format!("actor: not resolved ({})", note.trim())])
            }
            Some(_) if matches!(a.state, State::Blocked) => {
                Outcome::Omitted(reasons(&a.blockers, None, "actor"))
            }
            Some(o) => Outcome::Omitted(vec![format!(
                "actor draw: {}",
                o.causes.first().map_or("unknown".into(), label)
            )]),
        };
        let mut bounds = None;
        if let Some(base) = &a.base {
            if let Ok(Some(r)) = store.raw(base, b) {
                for s in &r.subrecords {
                    let d = r.data(s);
                    if s.signature == *b"OBND" && d.len() == 12 {
                        bounds = Some(std::array::from_fn(|k| {
                            i16::from_le_bytes([d[2 * k], d[2 * k + 1]])
                        }));
                    }
                    if s.signature == *b"EDID" {
                        let edid = String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d));
                        c.base_info
                            .entry(base.clone())
                            .or_insert_with(|| (edid.into_owned(), String::new(), "NPC_".into()));
                    }
                }
            }
        }
        if let Some(p) = &a.enable {
            c.enable
                .entry(enable_key("actor", p, &outcome))
                .or_default()
                .add(area(bounds, a.scale).unwrap_or(0.));
        }
        c.count(
            &outcome,
            "actor",
            a.base.as_ref(),
            area(bounds, a.scale),
            rows.as_deref_mut(),
            &a.reference,
            &format!("{:?} {:?}", a.transform, a.scale),
        );
    }
    for e in &scene.exclusions {
        if e.child.signature == *b"ACHR" {
            continue;
        }
        *c.exclusions
            .entry(format!("{} {}", sig(&e.child.signature), e.code))
            .or_default() += 1;
    }
    c.deleted += scene.deleted.len() as u64;
    c.disabled += scene.disabled.len() as u64;
    c.cells += 1;
    Ok(())
}

/// Tallies the sub-reasons of one placement (`detail`).
#[allow(clippy::too_many_arguments)]
fn details(
    c: &mut Census,
    i: &Instance,
    outcome: &Outcome,
    loader: &Loader<'_>,
    vfs: &Vfs,
    spelling: &detail::Spelling,
    uv: Option<&(bool, bool)>,
    a: f64,
    b: &Budget,
) -> fo4_core::Result<()> {
    let add = |c: &mut Census, section: &str, label: String| {
        c.detail.entry((section.into(), label)).or_default().add(a);
    };
    let reasons = match outcome {
        Outcome::Omitted(r) => r,
        Outcome::Drawn | Outcome::UnderHypotheses(_) => {
            let kind = if matches!(outcome, Outcome::Drawn) {
                "drawn without hypotheses"
            } else {
                "drawn only under hypotheses"
            };
            let label = match uv.copied().unwrap_or_default() {
                (false, false) => "every draw at UV offset 0 and scale 1",
                (true, false) => "a draw with UV offset not 0",
                (false, true) => "a draw with UV scale not 1",
                (true, true) => "a draw with UV offset not 0 and scale not 1",
            };
            add(
                c,
                "UV fields of drawn placements",
                format!("{kind}; {label}"),
            );
            return Ok(());
        }
        _ => return Ok(()),
    };
    let sole = families(reasons).len() == 1;
    if reasons.iter().any(|r| r.ends_with(": MissingMaterial")) {
        let (set, files, spell) = detail::missing_labels(i, loader, spelling, b)?;
        if sole {
            add(c, "missing material, sole family, placements", set.clone());
        }
        add(c, "missing material, any, placements", set);
        for f in files {
            add(c, "missing material, any, files (per placement)", f);
        }
        for f in spell {
            add(
                c,
                "missing material, any, path classes (per placement and file)",
                f.into(),
            );
        }
    }
    if reasons.iter().any(|r| r.ends_with(": MissingTexture")) {
        let (set, files) = detail::missing_texture_labels(i, loader, spelling, b)?;
        if sole {
            add(c, "missing texture, sole family, placements", set.clone());
        }
        add(c, "missing texture, any, placements", set);
        for f in files {
            add(c, "missing texture, any, files (per placement)", f);
        }
    }
    if let Some(r) = reasons
        .iter()
        .find(|r| r.starts_with("draw: UnsupportedRenderSwapScopes"))
    {
        let found = r.trim_start_matches("draw: UnsupportedRenderSwapScopes");
        add(c, "swap scopes, placements", detail::scopes_label(i, found));
    }
    if reasons
        .iter()
        .any(|r| r.ends_with(";rest=texture-transform"))
    {
        let mut cache = std::mem::take(&mut c.texture_cache);
        let t = detail::texture_labels(i, loader, vfs, &mut cache, b);
        c.texture_cache = cache;
        let t = t?;
        let fam: Vec<String> = families(reasons);
        let only = fam.len() == 1;
        if only {
            add(
                c,
                "texture transform, sole family, placements",
                t.label.clone(),
            );
        }
        add(
            c,
            "texture transform, any, placements",
            format!("{}; other families {}", t.label, fam.len() - 1),
        );
        for x in t.controllers {
            add(
                c,
                if only {
                    "texture transform, sole family, controllers (per placement)"
                } else {
                    "texture transform, any, controllers (per placement)"
                },
                x,
            );
        }
        for x in t.shaders {
            add(
                c,
                if only {
                    "texture transform, sole family, controlled shaders (per placement)"
                } else {
                    "texture transform, any, controlled shaders (per placement)"
                },
                x,
            );
        }
    }
    Ok(())
}

fn pct(a: f64, b: f64) -> f64 {
    if b > 0. {
        100. * a / b
    } else {
        0.
    }
}

fn ranked(out: &mut String, title: &str, map: &BTreeMap<String, Tally>, total: Tally, top: usize) {
    let _ = writeln!(out, "\n## {title}\n");
    let _ = writeln!(out, "by placements:");
    let mut v: Vec<_> = map.iter().collect();
    v.sort_by(|a, b| b.1.n.cmp(&a.1.n).then(a.0.cmp(b.0)));
    for (k, t) in v.iter().take(top) {
        let _ = writeln!(
            out,
            "  {:>8} {:>6.2}%  area {:>6.2}%  {k}",
            t.n,
            pct(t.n as f64, total.n as f64),
            pct(t.area, total.area)
        );
    }
    if v.len() > top {
        let _ = writeln!(out, "  ... {} more", v.len() - top);
    }
    let _ = writeln!(out, "by area:");
    v.sort_by(|a, b| b.1.area.total_cmp(&a.1.area).then(a.0.cmp(b.0)));
    for (k, t) in v.iter().take(top) {
        let _ = writeln!(
            out,
            "  area {:>6.2}%  {:>8} {:>6.2}%  {k}",
            pct(t.area, total.area),
            t.n,
            pct(t.n as f64, total.n as f64)
        );
    }
}

fn summary(c: &Census, scope: &str, elapsed: f64) -> String {
    let mut out = String::new();
    let p = c.placements;
    let _ = writeln!(out, "# Coverage census ({scope})\n");
    let _ = writeln!(
        out,
        "cells {} read, {} failed ({:.0} s)",
        c.cells,
        c.cell_failures.values().sum::<u64>(),
        elapsed
    );
    for (k, n) in &c.cell_failures {
        let _ = writeln!(out, "  cell failure {k}: {n}");
    }
    let _ = writeln!(
        out,
        "placements {} (area proxy {:.4e} square units; {} without OBND, area 0); deleted {}, initially disabled {}",
        p.n, p.area, c.no_bounds, c.deleted, c.disabled
    );
    let line = |out: &mut String, name: &str, t: Tally| {
        let _ = writeln!(
            out,
            "  {name:<34} {:>8} {:>6.2}%  area {:>6.2}%",
            t.n,
            pct(t.n as f64, p.n as f64),
            pct(t.area, p.area)
        );
    };
    line(&mut out, "drawn without hypotheses", c.drawn);
    line(&mut out, "drawn only under hypotheses", c.hypotheses);
    for (k, t) in &c.hidden {
        line(&mut out, &format!("hidden by rule {k}"), *t);
    }
    let outside = c.outside.values().fold(Tally::default(), |mut a, t| {
        a.n += t.n;
        a.area += t.area;
        a
    });
    line(&mut out, "outside the visual slice", outside);
    line(&mut out, "omitted", c.omitted);
    line(
        &mut out,
        "drawn with the root transform left out",
        c.root_left_out,
    );
    let _ = writeln!(out, "\nby placement kind:");
    for ((kind, name), t) in &c.by_type {
        line(&mut out, &format!("{kind}, {name}"), *t);
    }
    let _ = writeln!(
        out,
        "\nwith an enable parent (XESP), by the start state the chain gives under its rules:"
    );
    for (k, t) in &c.enable {
        line(&mut out, k, *t);
    }
    let _ = writeln!(out, "\noutside, by base type:");
    for (k, t) in &c.outside {
        line(&mut out, k, *t);
    }
    ranked(
        &mut out,
        "Omitted, by family of reasons, sole family (resolving it alone would draw the placement under hypotheses)",
        &c.family_sole,
        c.omitted,
        40,
    );
    let multi = c.omitted.n - c.family_sole.values().map(|t| t.n).sum::<u64>();
    let _ = writeln!(out, "  (omitted with two or more families: {multi})");
    ranked(
        &mut out,
        "Omitted, every family of reasons (once per placement)",
        &c.family_any,
        c.omitted,
        40,
    );
    ranked(
        &mut out,
        "Omitted, full set of families",
        &c.family_sets,
        c.omitted,
        30,
    );
    ranked(
        &mut out,
        "Omitted, by exact sole reason",
        &c.sole,
        c.omitted,
        40,
    );
    ranked(
        &mut out,
        "Omitted, every exact reason (once per placement)",
        &c.any,
        c.omitted,
        80,
    );
    ranked(
        &mut out,
        "Drawn only under hypotheses, by pending rule",
        &c.pending_rules,
        c.hypotheses,
        40,
    );
    ranked(
        &mut out,
        "Drawn only under hypotheses, by set of pending rules",
        &c.pending_sets,
        c.hypotheses,
        20,
    );
    let mut section = String::new();
    for ((sec, label), t) in &c.detail {
        if *sec != section {
            let _ = writeln!(out, "\n## Sub-reasons: {sec}\n");
            section = sec.clone();
        }
        let _ = writeln!(
            out,
            "  {:>8}  area {:>6.2}% of omitted, {:>6.2}% of all  {label}",
            t.n,
            pct(t.area, c.omitted.area),
            pct(t.area, p.area)
        );
    }
    let _ = writeln!(out, "\n## Placed children not read as placements\n");
    for (k, n) in &c.exclusions {
        let _ = writeln!(out, "  {n:>8}  {k}");
    }
    if !c.marked.is_empty() {
        marked(&mut out, c);
    }
    out
}

/// The marked placements' section (`--mark`).
fn marked(out: &mut String, c: &Census) {
    let n: u64 = c.marked.values().map(|t| t.n).sum();
    let _ = writeln!(
        out,
        "\n## Marked placements ({n} of {} marked loaded), by kind, outcome and reasons\n",
        c.marks.len()
    );
    for (k, t) in &c.marked {
        let _ = writeln!(out, "  {:>6}  {k}", t.n);
    }
}

/// One view's (or the sum's) outcomes and the omitted placements' reasons,
/// each with its share of the omitted placements, by count and by area
/// proxy (`--views`; counts and labels only).
fn view_text(out: &mut String, title: &str, c: &Census, top: usize) {
    let p = c.placements;
    let o = c.omitted;
    let _ = writeln!(out, "## {title}\n");
    let _ = writeln!(
        out,
        "placements {} (area proxy {:.4e}); deleted {}, initially disabled {}",
        p.n, p.area, c.deleted, c.disabled
    );
    let line = |out: &mut String, name: &str, t: Tally| {
        let _ = writeln!(
            out,
            "  {name:<34} {:>6} {:>6.2}%  area {:>6.2}%",
            t.n,
            pct(t.n as f64, p.n as f64),
            pct(t.area, p.area)
        );
    };
    line(out, "drawn without hypotheses", c.drawn);
    line(out, "drawn only under hypotheses", c.hypotheses);
    for (k, t) in &c.hidden {
        line(out, &format!("hidden by rule {k}"), *t);
    }
    let outside = c.outside.values().fold(Tally::default(), |mut a, t| {
        a.n += t.n;
        a.area += t.area;
        a
    });
    line(out, "outside the visual slice", outside);
    line(out, "omitted", o);
    let ranked_share = |out: &mut String, name: &str, map: &BTreeMap<String, Tally>| {
        let _ = writeln!(
            out,
            "\n{name} (placements, share of the omitted; area share of the omitted):"
        );
        let mut v: Vec<_> = map.iter().collect();
        v.sort_by(|a, b| b.1.n.cmp(&a.1.n).then(a.0.cmp(b.0)));
        for (k, t) in v.iter().take(top) {
            let _ = writeln!(
                out,
                "  {:>6} {:>6.2}%  area {:>6.2}%  {k}",
                t.n,
                pct(t.n as f64, o.n as f64),
                pct(t.area, o.area)
            );
        }
        if v.len() > top {
            let rest = v[top..].iter().fold(Tally::default(), |mut a, (_, t)| {
                a.n += t.n;
                a.area += t.area;
                a
            });
            let _ = writeln!(out, "  ... {} more ({} placements)", v.len() - top, rest.n);
        }
    };
    ranked_share(
        out,
        "every family of reasons, once per placement",
        &c.family_any,
    );
    ranked_share(
        out,
        "sole family (resolving it alone would draw the placement)",
        &c.family_sole,
    );
    ranked_share(out, "every exact reason, once per placement", &c.any);
    if !c.marked.is_empty() {
        marked(out, c);
    }
    let _ = writeln!(out);
}

fn bases_table(c: &Census) -> String {
    let mut out = String::from("reason\tbase\tsignature\teditor_id\tmodel\tplacements\tarea\n");
    let mut v: Vec<_> = c.bases.iter().collect();
    v.sort_by(|a, b| {
        a.0 .0
            .cmp(&b.0 .0)
            .then(b.1.area.total_cmp(&a.1.area))
            .then(a.0 .1.cmp(&b.0 .1))
    });
    for ((reason, base), t) in v {
        let (edid, model, s) = base
            .as_ref()
            .and_then(|k| c.base_info.get(k))
            .cloned()
            .unwrap_or_default();
        let base = base.as_ref().map_or(String::new(), |k| {
            format!("{}:{:06x}", k.plugin, k.object_id)
        });
        let _ = writeln!(
            out,
            "{reason}\t{base}\t{s}\t{edid}\t{model}\t{}\t{:.0}",
            t.n, t.area
        );
    }
    out
}

/// Every tally as `section, key, count, area` lines, so that the counts of
/// several runs (`--shard`) can be added up (`--merge`).
fn dump(c: &Census) -> String {
    let mut out = String::new();
    let mut line = |sec: &str, key: &str, t: Tally| {
        let _ = writeln!(out, "{sec}\t{key}\t{}\t{:e}", t.n, t.area);
    };
    let n = |v: u64| Tally { n: v, area: 0. };
    line("cells", "", n(c.cells));
    line("no_bounds", "", n(c.no_bounds));
    line("deleted", "", n(c.deleted));
    line("disabled", "", n(c.disabled));
    line(
        "elapsed",
        "",
        Tally {
            n: 0,
            area: c.elapsed,
        },
    );
    line("placements", "", c.placements);
    line("drawn", "", c.drawn);
    line("hypotheses", "", c.hypotheses);
    line("omitted", "", c.omitted);
    line("root_left_out", "", c.root_left_out);
    for (k, v) in &c.cell_failures {
        line("cell_failures", k, n(*v));
    }
    for (k, v) in &c.exclusions {
        line("exclusions", k, n(*v));
    }
    for (sec, map) in [
        ("hidden", &c.hidden),
        ("enable", &c.enable),
        ("outside", &c.outside),
        ("sole", &c.sole),
        ("any", &c.any),
        ("family_sole", &c.family_sole),
        ("family_any", &c.family_any),
        ("family_sets", &c.family_sets),
        ("pending_rules", &c.pending_rules),
        ("pending_sets", &c.pending_sets),
        ("marked", &c.marked),
    ] {
        for (k, v) in map {
            line(sec, k, *v);
        }
    }
    for ((a, b), v) in &c.by_type {
        line("by_type", &format!("{a}\u{1f}{b}"), *v);
    }
    for ((r, b), v) in &c.bases {
        let b = b.as_ref().map_or(String::new(), |k| {
            format!("{}\u{1f}{}", k.plugin, k.object_id)
        });
        line("bases", &format!("{r}\u{1f}{b}"), *v);
    }
    for ((sec, label), v) in &c.detail {
        line("detail", &format!("{sec}\u{1f}{label}"), *v);
    }
    for (k, (e, m, sg)) in &c.base_info {
        let _ = writeln!(
            out,
            "base_info\t{}\u{1f}{}\u{1f}{e}\u{1f}{m}\u{1f}{sg}\t0\t0",
            k.plugin, k.object_id
        );
    }
    out
}

/// Adds the counts written by [`dump`] to `c`.
fn load(c: &mut Census, text: &str) -> AnyResult<()> {
    for row in text.lines() {
        let mut f = row.split('\t');
        let (Some(sec), Some(key), Some(n), Some(area)) = (f.next(), f.next(), f.next(), f.next())
        else {
            return Err(format!("CountsRow: {row}").into());
        };
        let t = Tally {
            n: n.parse()?,
            area: area.parse()?,
        };
        let add = |x: &mut Tally| {
            x.n += t.n;
            x.area += t.area;
        };
        let parts: Vec<&str> = key.split('\u{1f}').collect();
        let form = |p: &[&str]| -> AnyResult<Option<FormKey>> {
            Ok(match p {
                [plugin, id] => Some(FormKey {
                    plugin: (*plugin).into(),
                    object_id: id.parse()?,
                }),
                _ => None,
            })
        };
        match sec {
            "cells" => c.cells += t.n,
            "no_bounds" => c.no_bounds += t.n,
            "deleted" => c.deleted += t.n,
            "disabled" => c.disabled += t.n,
            "elapsed" => c.elapsed = c.elapsed.max(t.area),
            "placements" => add(&mut c.placements),
            "drawn" => add(&mut c.drawn),
            "hypotheses" => add(&mut c.hypotheses),
            "omitted" => add(&mut c.omitted),
            "root_left_out" => add(&mut c.root_left_out),
            "cell_failures" => *c.cell_failures.entry(key.into()).or_default() += t.n,
            "exclusions" => *c.exclusions.entry(key.into()).or_default() += t.n,
            "hidden" => add(c.hidden.entry(key.into()).or_default()),
            "enable" => add(c.enable.entry(key.into()).or_default()),
            "outside" => add(c.outside.entry(key.into()).or_default()),
            "sole" => add(c.sole.entry(key.into()).or_default()),
            "any" => add(c.any.entry(key.into()).or_default()),
            "family_sole" => add(c.family_sole.entry(key.into()).or_default()),
            "family_any" => add(c.family_any.entry(key.into()).or_default()),
            "family_sets" => add(c.family_sets.entry(key.into()).or_default()),
            "pending_rules" => add(c.pending_rules.entry(key.into()).or_default()),
            "pending_sets" => add(c.pending_sets.entry(key.into()).or_default()),
            "marked" => add(c.marked.entry(key.into()).or_default()),
            "by_type" if parts.len() == 2 => add(c
                .by_type
                .entry((parts[0].into(), parts[1].into()))
                .or_default()),
            "bases" if !parts.is_empty() => add(c
                .bases
                .entry((parts[0].into(), form(&parts[1..])?))
                .or_default()),
            "detail" if parts.len() == 2 => {
                if parts[0] == "data folder" {
                    let e = c
                        .detail
                        .entry((parts[0].into(), parts[1].into()))
                        .or_default();
                    e.n = e.n.max(t.n);
                } else {
                    add(c
                        .detail
                        .entry((parts[0].into(), parts[1].into()))
                        .or_default())
                }
            }
            "base_info" if parts.len() == 5 => {
                if let Some(k) = form(&parts[..2])? {
                    c.base_info
                        .entry(k)
                        .or_insert_with(|| (parts[2].into(), parts[3].into(), parts[4].into()));
                }
            }
            _ => return Err(format!("CountsRow: {row}").into()),
        }
    }
    Ok(())
}

fn run() -> AnyResult<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    const USAGE: &str = "coverage_census MANIFEST OUT_DIR (--all [--every N] [--limit N] [--shard K/N] | --view CELL_EDITOR_ID [--around N] | --views FILE [--mark FILE] [--render-child-flag-agrees] [--render-static-fields] [--render-static-leaves] [--render-reference-alpha] [--render-static-parts] | --merge DIR...) [--times S[,S...]] [--clock-emissive-multiple] [--clock-emissive-colour] [--clock-falloff] [--clock-translation] [--clock-rotation] [--clock-rotation-xyz] [--clock-scale]";
    if args.len() < 3 {
        return Err(USAGE.into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let out = Path::new(&args[1]);
    std::fs::create_dir_all(out)?;
    let mut all = false;
    let mut view = None;
    let mut views = None;
    let mut marks = BTreeSet::new();
    let mut render_agrees = false;
    let mut render_fields = false;
    let mut render_leaves = false;
    let mut render_alpha = false;
    let mut render_parts = false;
    let mut around = 0u32;
    let mut every = 32u64;
    let mut limit = u64::MAX;
    let mut shard = (0u64, 1u64);
    // Second 0 of the game clock unless `--times` names others; with more
    // than one time each time's outputs go to `OUT_DIR/t=S`.
    let mut clock = Clock {
        times: vec![0.],
        switches: fo4_scene::clock::Switches::default(),
    };
    if args[2] == "--merge" {
        let mut c = Census::default();
        // `--note TEXT` (a time of the clock, switches) joins the scope.
        let mut note = None;
        let mut dirs = Vec::new();
        let mut it = args[3..].iter();
        while let Some(d) = it.next() {
            if d == "--note" {
                note = Some(it.next().ok_or(USAGE)?.clone());
                continue;
            }
            dirs.push(d);
        }
        for d in &dirs {
            load(
                &mut c,
                &std::fs::read_to_string(Path::new(d).join("counts.tsv"))?,
            )?;
        }
        let mut scope = format!(
            "every cell of the load order, each alone; {} shards",
            dirs.len()
        );
        if let Some(n) = note {
            scope = format!("{scope}; {n}");
        }
        std::fs::write(out.join("summary.txt"), summary(&c, &scope, c.elapsed))?;
        std::fs::write(out.join("bases.tsv"), bases_table(&c))?;
        return Ok(());
    }
    let mut rest = args[2..].iter();
    while let Some(a) = rest.next() {
        if clock.switches.set_flag(a) {
            continue;
        }
        let mut num = || -> AnyResult<u64> { Ok(rest.next().ok_or(USAGE)?.parse::<u64>()?) };
        match a.as_str() {
            "--times" => {
                clock.times = rest
                    .next()
                    .ok_or(USAGE)?
                    .split(',')
                    .map(|t| t.parse::<f64>())
                    .collect::<Result<_, _>>()?;
                if clock.times.is_empty() || clock.times.iter().any(|t| !t.is_finite() || *t < 0.) {
                    return Err(USAGE.into());
                }
            }
            "--all" => all = true,
            "--render-child-flag-agrees" => render_agrees = true,
            "--render-static-fields" => render_fields = true,
            "--render-static-leaves" => render_leaves = true,
            "--render-reference-alpha" => render_alpha = true,
            "--render-static-parts" => render_parts = true,
            "--view" => view = Some(rest.next().ok_or(USAGE)?.clone()),
            "--views" => {
                let text = std::fs::read_to_string(rest.next().ok_or(USAGE)?)?;
                let mut v = Vec::new();
                for row in text.lines().filter(|r| !r.trim().is_empty()) {
                    let mut f = row.split_whitespace();
                    let key = f
                        .next()
                        .and_then(fo4_scene::timeline::parse_key)
                        .ok_or("ViewsRow: a record key first")?;
                    let around: u32 = f.next().ok_or("ViewsRow: an around second")?.parse()?;
                    v.push((key, around, f.collect::<Vec<_>>().join(" ")));
                }
                views = Some(v);
            }
            "--mark" => {
                let text = std::fs::read_to_string(rest.next().ok_or(USAGE)?)?;
                for row in text.lines().filter(|r| !r.trim().is_empty()) {
                    marks.insert(
                        fo4_scene::timeline::parse_key(row.trim())
                            .ok_or("MarkRow: a record key")?,
                    );
                }
            }
            "--around" => around = u32::try_from(num()?)?,
            "--every" => every = num()?.max(1),
            "--limit" => limit = num()?,
            "--shard" => {
                let v = rest.next().ok_or(USAGE)?;
                let (k, n) = v.split_once('/').ok_or(USAGE)?;
                shard = (k.parse()?, n.parse()?);
                if shard.1 == 0 || shard.0 >= shard.1 {
                    return Err(USAGE.into());
                }
            }
            _ => return Err(USAGE.into()),
        }
    }
    if [all, view.is_some(), views.is_some()]
        .iter()
        .filter(|x| **x)
        .count()
        != 1
        || (views.is_some() && clock.times.len() != 1)
        || (views.is_none()
            && (!marks.is_empty()
                || render_agrees
                || render_fields
                || render_leaves
                || render_alpha
                || render_parts))
    {
        return Err(USAGE.into());
    }
    let started = std::time::Instant::now();
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    if !store.diagnostics.is_empty() {
        return Err("PluginDiagnostics".into());
    }
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    vfs.check_complete()?;
    let spelling = detail::Spelling::build(&m, &vfs)?;
    let mut cs: Vec<Census> = clock.times.iter().map(|_| Census::default()).collect();
    let n = |v: u64| Tally { n: v, area: 0. };
    for c in &mut cs {
        c.detail.insert(
            (
                "data folder".into(),
                "archives the manifest does not mount".into(),
            ),
            n(spelling.unmounted_archives),
        );
        c.detail.insert(
            ("data folder".into(), "loose files".into()),
            n(spelling.loose_files),
        );
    }
    let cache = Budget::new(m.limits.max_memory);
    let mut loader = Loader::new(&store, &vfs, &m.language, RuleLevel::Hypotheses, &cache);
    if clock.switches.any() {
        loader.set_clock_switches(clock.switches, &cache);
    }
    // `xesp_child_flag_agrees_chain_v1` for render readiness (off by
    // default; with --views only).
    if render_agrees {
        loader.set_render_child_flag_agrees(true);
    }
    // `refr_static_fields_render_unchanged_v1` and
    // `play_static_leaf_two_pose_v1` for render readiness (off by default;
    // with --views only).
    loader.set_render_static_fields(render_fields);
    loader.set_render_static_leaves(render_leaves);
    loader.set_render_reference_alpha(render_alpha);
    loader.set_render_static_parts(render_parts);
    if let Some(views) = &views {
        let mut total = Census {
            marks: marks.clone(),
            ..Census::default()
        };
        let mut text = format!(
            "# Render readiness of the views a run loads (coverage_census --views)\n\n{}Each view loaded as the viewer loads it (the cell, and with around N the exterior cells about it), the viewer's draw list built with the placed actors at the hypotheses level; outcomes as in coverage-census.txt. Shares of the omitted placements by count and by the area proxy (the largest face of the base's OBND box times the scale squared).\n\n",
            [
                (
                    render_agrees,
                    "With --render-child-flag-agrees (xesp_child_flag_agrees_chain_v1 in render readiness). "
                ),
                (
                    render_fields,
                    "With --render-static-fields (refr_static_fields_render_unchanged_v1 in render readiness). "
                ),
                (
                    render_leaves,
                    "With --render-static-leaves (play_static_leaf_two_pose_v1 in render readiness). "
                ),
                (
                    render_alpha,
                    "With --render-reference-alpha (refr_xalp_render_unchanged_v1 in render readiness). "
                ),
                (
                    render_parts,
                    "With --render-static-parts (static_unmoved_parts_drawn_v1 in render readiness). "
                ),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, t)| *t)
            .collect::<String>()
        );
        for (k, (key, around, what)) in views.iter().enumerate() {
            let mut cs = vec![Census {
                marks: marks.clone(),
                ..Census::default()
            }];
            let mut rows = String::from(
                "reference\tkind\tbase\teditor_id\tmodel\tarea\toutcome\tdetail\tplacement\n",
            );
            let b = Budget::new(m.limits.max_memory.saturating_mul(8));
            let failures = cell(
                &mut cs,
                &clock,
                &mut loader,
                &store,
                &m,
                &vfs,
                &spelling,
                key,
                *around,
                Some(&mut rows),
                &b,
            )?;
            if let Some(e) = failures.into_iter().flatten().next() {
                return Err(e.into());
            }
            let c = &mut cs[0];
            c.elapsed = started.elapsed().as_secs_f64();
            let dir = out.join(format!("view-{}", k + 1));
            std::fs::create_dir_all(&dir)?;
            let scope = format!("view {}: {what}, around {around}", k + 1);
            std::fs::write(dir.join("summary.txt"), summary(c, &scope, c.elapsed))?;
            std::fs::write(dir.join("counts.tsv"), dump(c))?;
            std::fs::write(dir.join("bases.tsv"), bases_table(c))?;
            std::fs::write(dir.join("placements.tsv"), rows)?;
            view_text(&mut text, &scope, c, 15);
            load(&mut total, &dump(c))?;
            let _ = writeln!(
                std::io::stderr(),
                "view {}: {} placements, {} omitted, {:.0} s",
                k + 1,
                c.placements.n,
                c.omitted.n,
                c.elapsed
            );
        }
        total.elapsed = started.elapsed().as_secs_f64();
        view_text(
            &mut text,
            &format!(
                "the {} views together (a placement counted once per view that loads it)",
                views.len()
            ),
            &total,
            25,
        );
        std::fs::write(out.join("views.txt"), text)?;
        std::fs::write(
            out.join("summary.txt"),
            summary(&total, &format!("{} views", views.len()), total.elapsed),
        )?;
        std::fs::write(out.join("counts.tsv"), dump(&total))?;
        std::fs::write(out.join("bases.tsv"), bases_table(&total))?;
        return Ok(());
    }
    let scope;
    if let Some(edid) = &view {
        let key = store
            .find_cell(edid, &Budget::new(m.limits.max_memory))?
            .ok_or("MissingCellEditorId")?;
        let mut rows = String::from(
            "reference\tkind\tbase\teditor_id\tmodel\tarea\toutcome\tdetail\tplacement\n",
        );
        // A view with the cells around it holds more than one cell's read;
        // the viewer's own scene ceiling (`--scene-memory`) is larger too.
        let b = Budget::new(m.limits.max_memory.saturating_mul(8));
        let failures = cell(
            &mut cs,
            &clock,
            &mut loader,
            &store,
            &m,
            &vfs,
            &spelling,
            &key,
            around,
            Some(&mut rows),
            &b,
        )?;
        if let Some(e) = failures.into_iter().flatten().next() {
            return Err(e.into());
        }
        std::fs::write(out.join("placements.tsv"), rows)?;
        scope = format!("one view, around {around}");
    } else {
        let keys: Vec<FormKey> = store.cells().collect();
        let mut batch = 0;
        let mut progress = std::io::stderr();
        for (n, key) in keys.iter().enumerate() {
            if n as u64 >= limit {
                break;
            }
            if n as u64 % shard.1 != shard.0 {
                continue;
            }
            if batch == every {
                loader.reset_asset_cache(&cache);
                batch = 0;
            }
            batch += 1;
            // The viewer's scene ceiling, not one read's: the draw list of a
            // large cell holds more than the manifest's per-read limit.
            let b = Budget::new(m.limits.max_memory.saturating_mul(8));
            let failures = match cell(
                &mut cs,
                &clock,
                &mut loader,
                &store,
                &m,
                &vfs,
                &spelling,
                key,
                0,
                None,
                &b,
            ) {
                Ok(f) => f,
                Err(e) => vec![Some(e); cs.len()],
            };
            let mut failed = false;
            for (c, e) in cs.iter_mut().zip(failures) {
                if let Some(e) = e {
                    *c.cell_failures.entry(e.code.clone()).or_default() += 1;
                    failed = true;
                }
            }
            if failed {
                loader.reset_asset_cache(&cache);
                batch = 0;
            }
            if n % 500 == 0 {
                let _ = writeln!(
                    progress,
                    "{n}/{} cells, {:.0} s, {} placements",
                    keys.len(),
                    started.elapsed().as_secs_f64(),
                    cs[0].placements.n
                );
            }
        }
        scope = "every cell of the load order, each alone".into();
    }
    let several = cs.len() > 1;
    for (c, time) in cs.iter_mut().zip(&clock.times) {
        c.elapsed = started.elapsed().as_secs_f64();
        let dir = if several {
            out.join(format!("t={time}"))
        } else {
            out.to_path_buf()
        };
        std::fs::create_dir_all(&dir)?;
        // The default clock (second 0, every switch off) keeps the scope
        // as it was.
        let scope = if *time != 0. || clock.switches.any() {
            let mut s = format!("{scope}; second {time} of the game clock");
            for f in clock.switches.flags() {
                s = format!("{s}, {f}");
            }
            s
        } else {
            scope.clone()
        };
        std::fs::write(dir.join("summary.txt"), summary(c, &scope, c.elapsed))?;
        std::fs::write(dir.join("counts.tsv"), dump(c))?;
        std::fs::write(dir.join("bases.tsv"), bases_table(c))?;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
