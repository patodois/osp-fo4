//! Per-winning-base closure, independent of cell lighting and REFR runtime state.
use crate::{causes, Reports};
use fo4_core::{add, Budget, Error, Manifest, Result};
use fo4_plugin::{
    still::Class,
    typed::{CellKind, Refr, Store},
    FormKey,
};
use fo4_scene::{AssetKind, Base, Loader, PathPolicy, State};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default, Serialize)]
pub struct TypeCounts {
    pub attempted: u64,
    pub ready: u64,
    pub under_hypotheses: BTreeMap<String, ReadinessCounts>,
    pub blocked: u64,
    pub failed: u64,
    pub ready_references: u64,
    pub ready_interior_references: u64,
}
#[derive(Default, Serialize)]
pub struct ReadinessCounts {
    pub bases: u64,
    pub references: u64,
    pub interior_references: u64,
    pub active_references: u64,
}
impl ReadinessCounts {
    fn include(&mut self, r: &References) -> Result<()> {
        self.bases = add(self.bases, 1, "hypothesis census", 0)?;
        self.references = add(self.references, r.all, "hypothesis census", 0)?;
        self.interior_references =
            add(self.interior_references, r.interior, "hypothesis census", 0)?;
        self.active_references = add(self.active_references, r.active, "hypothesis census", 0)?;
        Ok(())
    }
}
#[derive(Default, Serialize)]
pub struct Cause {
    pub bases: u64,
    pub sole_bases: u64,
    pub references: u64,
    pub interior_references: u64,
    pub sole_references: u64,
}
#[derive(Default, Serialize)]
pub struct Summary {
    pub selected_bases: Option<u64>,
    pub strict_drawable: ReadinessCounts,
    pub hypothesis_drawable: BTreeMap<String, ReadinessCounts>,
    pub editor_markers: ReadinessCounts,
    pub strict_ready_editor_markers: ReadinessCounts,
    pub refused_families: BTreeMap<String, Cause>,
    pub attempted: u64,
    pub ready: u64,
    pub under_hypotheses: BTreeMap<String, ReadinessCounts>,
    pub blocked: u64,
    pub failed: u64,
    pub complete_index: bool,
    pub by_type: BTreeMap<String, TypeCounts>,
    pub causes: BTreeMap<String, Cause>,
    pub reference_winners: u64,
    pub references_read: u64,
    pub references_failed: u64,
    pub references_without_base: u64,
    pub ready_references: u64,
    pub ready_interior_references: u64,
    pub ready_active_references: u64,
    pub outside_fields: BTreeMap<String, u64>,
    pub outside_blocks: BTreeMap<String, u64>,
    pub material_components: BTreeMap<String, u64>,
    pub cycles: BTreeMap<String, u64>,
    pub cache_instances: u64,
    pub cache_peak_bytes: u64,
    pub cache_peak_entries: u64,
    pub declared_materials: u64,
    pub declared_models: u64,
    pub declared_textures: u64,
}
#[derive(Default)]
struct References {
    all: u64,
    interior: u64,
    active: u64,
}
#[derive(Serialize)]
struct Row<'a> {
    pending_live_proof: &'a BTreeSet<&'static str>,
    alpha_tuples: &'a BTreeSet<(u8, u32, u32)>,
    previously_blocking_alpha_tuples: &'a BTreeSet<(u8, u32, u32)>,
    kind: &'static str,
    key: &'a FormKey,
    record_type: &'a str,
    editor_id: &'a Option<String>,
    model: &'a Option<String>,
    state: &'a State,
    editor_marker: bool,
    remaining_causes: &'a BTreeSet<String>,
    shapes: u64,
    models: u64,
    materials: u64,
    textures: u64,
    references: u64,
    interior_references: u64,
    active_references: u64,
    causes: &'a BTreeSet<String>,
}
fn inc(map: &mut BTreeMap<String, u64>, key: &str, n: u64, b: &Budget) -> Result<()> {
    if !map.contains_key(key) {
        b.charge(key.len() as u64 + 128, "base totals", 0)?;
    }
    let x = map.entry(key.into()).or_default();
    *x = add(*x, n, "base totals", 0)?;
    Ok(())
}
fn retain<T: Ord + Clone>(set: &mut BTreeSet<T>, value: &T, bytes: u64, b: &Budget) -> Result<()> {
    if !set.contains(value) {
        b.charge(bytes + 128, "base distinct identities", 0)?;
        set.insert(value.clone());
    }
    Ok(())
}
pub(crate) fn outside_record(
    record: &fo4_plugin::Record,
    totals: &mut BTreeMap<String, u64>,
    b: &Budget,
) -> Result<()> {
    for raw in &record.subrecords {
        if fo4_plugin::still::classify_size(
            &record.signature,
            &raw.signature,
            record.data(raw).len(),
        ) == Class::Outside
        {
            inc(
                totals,
                &format!(
                    "{}.{}",
                    String::from_utf8_lossy(&record.signature),
                    String::from_utf8_lossy(&raw.signature)
                ),
                1,
                b,
            )?;
        }
    }
    Ok(())
}
pub(crate) fn outside(base: &Base, totals: &mut BTreeMap<String, u64>, b: &Budget) -> Result<()> {
    outside_record(&base.record, totals, b)?;
    for part in &base.parts {
        if let Some(base) = &part.resolved {
            outside(base, totals, b)?;
        }
    }
    Ok(())
}
pub fn sweep(m: &Manifest, r: &mut Reports, selected: Option<&[FormKey]>) -> Result<()> {
    r.items_enabled = true;
    let selection_budget = Budget::new(m.limits.max_memory);
    let selected = selected
        .map(|keys| {
            fo4_core::limit(keys.len() as u64, m.limits.max_entries, "base selection", 0)?;
            let mut set = BTreeSet::new();
            for key in keys {
                retain(
                    &mut set,
                    key,
                    key.plugin.len() as u64 + 32,
                    &selection_budget,
                )?;
            }
            Ok::<_, Error>(set)
        })
        .transpose()?;
    r.summary.base_census.selected_bases = selected.as_ref().map(|s| s.len() as u64);
    let index = Budget::new(m.limits.max_override_memory);
    let store = Store::open(m, &index)?;
    for e in &store.diagnostics {
        r.error(e)?;
    }
    let v = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    for e in &v.diagnostics {
        r.error(e)?;
    }
    v.check_complete()?;
    r.summary.fingerprint = Some(v.fingerprint()?.to_owned());
    r.summary.vfs_memory = v.memory_used();
    let aggregate = Budget::new(m.limits.max_memory);
    let mut interior = BTreeSet::new();
    for (record, kind) in store.placement_contexts() {
        if record.signature == *b"REFR" && kind == Some(CellKind::Interior) {
            retain(&mut interior, &record.key, 8, &aggregate)?;
        }
    }
    let mut references: BTreeMap<FormKey, References> = BTreeMap::new();
    r.summary.base_census.complete_index = store.diagnostics.is_empty();
    for winner in store.winners().filter(|w| w.signature == *b"REFR") {
        r.summary.base_census.reference_winners += 1;
        let key = store.overrides().form_key(winner.key, store.order());
        let work = Budget::new(m.limits.max_memory);
        match store.get::<Refr>(&key, &work) {
            Ok(Some(rf)) => {
                r.summary.base_census.references_read += 1;
                if let Some(key) = rf.data.base.as_ref().and_then(|r| r.key.as_ref()) {
                    if !references.contains_key(key) {
                        aggregate.charge(
                            key.plugin.len() as u64 + 192,
                            "base reference index",
                            0,
                        )?;
                    }
                    let x = references.entry(key.clone()).or_default();
                    x.all = add(x.all, 1, "base references", 0)?;
                    x.interior = add(
                        x.interior,
                        u64::from(interior.contains(&winner.key)),
                        "base references",
                        0,
                    )?;
                    x.active = add(
                        x.active,
                        u64::from(!winner.deleted && !winner.disabled),
                        "base references",
                        0,
                    )?;
                } else {
                    r.summary.base_census.references_without_base += 1;
                }
            }
            Ok(None) => {
                return Err(Error::new(
                    "MissingReferenceWinner",
                    &key.plugin,
                    winner.offset,
                    "indexed REFR",
                    key.object_id,
                ))
            }
            Err(e) => {
                r.summary.base_census.references_failed += 1;
                r.error(&e)?;
            }
        }
    }
    let cache = Budget::new(m.limits.max_memory);
    let mut loader = Loader::new(&store, &v, &m.language, PathPolicy::UnderHypotheses, &cache);
    let mut components: BTreeMap<String, BTreeSet<Vec<u8>>> = BTreeMap::new();
    let mut cycles = BTreeSet::new();
    let mut all_models = BTreeSet::new();
    let mut all_materials = BTreeSet::new();
    let mut all_textures = BTreeSet::new();
    let mut batch = 0;
    for winner in store
        .winners()
        .filter(|w| fo4_scene::BASE_TYPES.contains(&w.signature))
    {
        let key = store.overrides().form_key(winner.key, store.order());
        if selected.as_ref().is_some_and(|keys| !keys.contains(&key)) {
            continue;
        }
        if batch == 0
            || batch >= 256
            || cache.used() > m.limits.max_memory / 4 * 3
            || loader.assets.unavailable()
        {
            loader.reset_asset_cache(&cache);
            batch = 0;
            r.summary.base_census.cache_instances += 1;
        }
        batch += 1;
        let ty = String::from_utf8_lossy(&winner.signature);
        if !r.summary.base_census.by_type.contains_key(ty.as_ref()) {
            aggregate.charge(ty.len() as u64 + 192, "base types", 0)?;
        }
        r.summary.base_census.attempted += 1;
        r.summary
            .base_census
            .by_type
            .entry(ty.to_string())
            .or_default()
            .attempted += 1;
        let work = Budget::new(m.limits.max_memory);
        let result = loader.inspect_base(&key, &work);
        match result {
            Ok(inspect) => {
                if let Some(e) = inspect.blockers.iter().find(|e| causes::infrastructure(e)) {
                    r.summary.base_census.failed += 1;
                    r.summary
                        .base_census
                        .by_type
                        .entry(ty.to_string())
                        .or_default()
                        .failed += 1;
                    r.error(e)?;
                    continue;
                }
                let refs = references.get(&key);
                let empty = References::default();
                let refs = refs.unwrap_or(&empty);
                let mut labels = BTreeSet::new();
                for e in &inspect.blockers {
                    let label = causes::detail(e);
                    retain(&mut labels, &label, label.len() as u64, &work)?;
                }
                let mut remaining_labels = BTreeSet::new();
                for e in &inspect.blockers {
                    if fo4_scene::hypotheses::covering(e).is_none() {
                        let label = causes::detail(e);
                        retain(&mut remaining_labels, &label, label.len() as u64, &work)?;
                    }
                }
                if inspect.base.editor_marker {
                    r.summary.base_census.editor_markers.include(refs)?;
                    if matches!(inspect.state, State::Ready) {
                        r.summary
                            .base_census
                            .strict_ready_editor_markers
                            .include(refs)?;
                    }
                }
                let families = crate::families::instance_causes(
                    &loader.assets,
                    &inspect.assets,
                    &inspect.blockers,
                    &work,
                )?;
                for (family, n) in families {
                    if !r.summary.base_census.refused_families.contains_key(&family) {
                        aggregate.charge(family.len() as u64 + 192, "base family totals", 0)?;
                    }
                    let c = r
                        .summary
                        .base_census
                        .refused_families
                        .entry(family)
                        .or_default();
                    c.bases = add(c.bases, 1, "base family totals", 0)?;
                    c.references = add(c.references, refs.all, "base family totals", 0)?;
                    c.interior_references = add(
                        c.interior_references,
                        refs.interior,
                        "base family totals",
                        0,
                    )?;
                    if n.sole_cause_instances > 0 {
                        c.sole_bases += 1;
                        c.sole_references =
                            add(c.sole_references, refs.all, "base family totals", 0)?;
                    }
                }
                let sole = remaining_labels.len() == 1;
                for label in &remaining_labels {
                    if !r.summary.base_census.causes.contains_key(label) {
                        aggregate.charge(label.len() as u64 + 192, "base causes", 0)?;
                    }
                    let c = r
                        .summary
                        .base_census
                        .causes
                        .entry(label.clone())
                        .or_default();
                    c.bases = add(c.bases, 1, "base causes", 0)?;
                    c.references = add(c.references, refs.all, "base causes", 0)?;
                    c.interior_references =
                        add(c.interior_references, refs.interior, "base causes", 0)?;
                    if sole {
                        c.sole_bases += 1;
                        c.sole_references = add(c.sole_references, refs.all, "base causes", 0)?;
                    }
                }
                outside(
                    &inspect.base,
                    &mut r.summary.base_census.outside_fields,
                    &aggregate,
                )?;
                let (mut shapes, mut models, mut materials, mut textures) = (0, 0, 0, 0);
                let mut pending_live_proof = BTreeSet::new();
                let mut alpha_tuples = BTreeSet::new();
                let mut previously_blocking_alpha_tuples = BTreeSet::new();
                for id in &inspect.assets {
                    let node = &loader.assets.nodes[*id];
                    for rule in &node.pending_live_proof {
                        retain(&mut pending_live_proof, rule, rule.len() as u64, &work)?;
                    }
                    if let Some(alpha) = node.alpha {
                        let tuple = (alpha.enabled, alpha.source, alpha.destination);
                        retain(&mut alpha_tuples, &tuple, 16, &work)?;
                        if alpha.previously_blocked {
                            retain(&mut previously_blocking_alpha_tuples, &tuple, 16, &work)?;
                        }
                    }
                    for (ty, n) in &node.outside_blocks {
                        inc(
                            &mut r.summary.base_census.outside_blocks,
                            ty,
                            *n,
                            &aggregate,
                        )?;
                    }
                    if node.origin.is_some() {
                        match node.kind {
                            AssetKind::Model => {
                                models += 1;
                                shapes = add(shapes, node.shapes, "base shapes", 0)?;
                                let first = !all_models.contains(&node.path);
                                retain(
                                    &mut all_models,
                                    &node.path,
                                    node.path.len() as u64,
                                    &aggregate,
                                )?;
                                if first {
                                    let closure = loader.assets.closure(&[*id], &work)?;
                                    let mut model_materials = 0;
                                    let mut model_textures = 0;
                                    let mut unresolved = 0;
                                    for asset_id in closure {
                                        let asset = &loader.assets.nodes[asset_id];
                                        if asset.kind == AssetKind::Material {
                                            model_materials += 1;
                                        }
                                        if asset.kind == AssetKind::Texture {
                                            model_textures += 1;
                                        }
                                        unresolved += asset.blockers.len();
                                    }
                                    r.item(&serde_json::json!({"kind":"model_closure","path":fo4_core::HexBytes(&node.path).to_string(),"materials":model_materials,"textures":model_textures,"blockers":unresolved}))?;
                                }
                            }
                            AssetKind::Material => {
                                materials += 1;
                                retain(
                                    &mut all_materials,
                                    &node.path,
                                    node.path.len() as u64,
                                    &aggregate,
                                )?;
                            }
                            AssetKind::Texture => {
                                textures += 1;
                                retain(
                                    &mut all_textures,
                                    &node.path,
                                    node.path.len() as u64,
                                    &aggregate,
                                )?;
                            }
                        }
                    }
                    for e in &node.blockers {
                        if e.code == "UnsupportedMaterialFields" {
                            if !components.contains_key(&e.found) {
                                aggregate.charge(
                                    e.found.len() as u64 + 128,
                                    "material components",
                                    0,
                                )?;
                            }
                            retain(
                                components.entry(e.found.clone()).or_default(),
                                &node.path,
                                node.path.len() as u64,
                                &aggregate,
                            )?;
                        }
                    }
                    for cycle in &node.cycles {
                        // Cycles are canonicalized by lexicographically smallest rotated path+class edge sequence.
                        let mut edges = Vec::new();
                        work.reserve(&mut edges, cycle.edges.len(), "cycle census", 0)?;
                        for edge in &cycle.edges {
                            let from = &loader.assets.nodes[edge.from].path;
                            let to = &loader.assets.nodes[edge.to].path;
                            work.charge(
                                (from.len() + to.len()) as u64 * 2 + 128,
                                "cycle census",
                                edge.offset,
                            )?;
                            edges.push(format!(
                                "{}>{}>{}",
                                fo4_core::HexBytes(from),
                                edge.class.rule().0,
                                fo4_core::HexBytes(to)
                            ));
                        }
                        let mut variants = Vec::new();
                        work.reserve(&mut variants, edges.len(), "cycle canonical", 0)?;
                        for j in 0..edges.len() {
                            let label = edges[j..]
                                .iter()
                                .chain(&edges[..j])
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("|");
                            work.charge(label.len() as u64, "cycle canonical", 0)?;
                            variants.push(label);
                        }
                        if let Some(identity) = variants.iter().min() {
                            if !cycles.contains(identity) {
                                retain(&mut cycles, identity, identity.len() as u64, &aggregate)?;
                                let label = format!(
                                    "length={};edges={}",
                                    cycle.edges.len(),
                                    cycle
                                        .edges
                                        .iter()
                                        .map(|e| e.class.rule().0)
                                        .collect::<Vec<_>>()
                                        .join(",")
                                );
                                inc(&mut r.summary.base_census.cycles, &label, 1, &aggregate)?;
                                r.item(&serde_json::json!({"kind":"cycle","edges":edges}))?;
                            }
                        }
                    }
                }
                if shapes > 0 && !inspect.base.editor_marker {
                    match &inspect.state {
                        State::Ready => r.summary.base_census.strict_drawable.include(refs)?,
                        State::ReadyUnderHypotheses { hypotheses } => {
                            let set = hypotheses.join("+");
                            if !r.summary.base_census.hypothesis_drawable.contains_key(&set) {
                                aggregate.charge(set.len() as u64 + 256, "drawable bases", 0)?;
                            }
                            r.summary
                                .base_census
                                .hypothesis_drawable
                                .entry(set)
                                .or_default()
                                .include(refs)?;
                        }
                        _ => {}
                    }
                }
                let counts = r
                    .summary
                    .base_census
                    .by_type
                    .entry(ty.to_string())
                    .or_default();
                if matches!(inspect.state, State::Ready) {
                    counts.ready += 1;
                    r.summary.base_census.ready += 1;
                    counts.ready_references =
                        add(counts.ready_references, refs.all, "ready references", 0)?;
                    counts.ready_interior_references = add(
                        counts.ready_interior_references,
                        refs.interior,
                        "ready references",
                        0,
                    )?;
                    r.summary.base_census.ready_references = add(
                        r.summary.base_census.ready_references,
                        refs.all,
                        "ready references",
                        0,
                    )?;
                    r.summary.base_census.ready_interior_references = add(
                        r.summary.base_census.ready_interior_references,
                        refs.interior,
                        "ready references",
                        0,
                    )?;
                    r.summary.base_census.ready_active_references = add(
                        r.summary.base_census.ready_active_references,
                        refs.active,
                        "ready references",
                        0,
                    )?;
                } else if let State::ReadyUnderHypotheses { hypotheses } = &inspect.state {
                    let set = hypotheses.join("+");
                    if !counts.under_hypotheses.contains_key(&set) {
                        aggregate.charge(set.len() as u64 + 256, "hypothesis census", 0)?;
                    }
                    counts
                        .under_hypotheses
                        .entry(set.clone())
                        .or_default()
                        .include(refs)?;
                    if !r.summary.base_census.under_hypotheses.contains_key(&set) {
                        aggregate.charge(set.len() as u64 + 256, "hypothesis census", 0)?;
                    }
                    r.summary
                        .base_census
                        .under_hypotheses
                        .entry(set)
                        .or_default()
                        .include(refs)?;
                } else {
                    counts.blocked += 1;
                    r.summary.base_census.blocked += 1;
                }
                r.item(&Row {
                    pending_live_proof: &pending_live_proof,
                    alpha_tuples: &alpha_tuples,
                    previously_blocking_alpha_tuples: &previously_blocking_alpha_tuples,
                    kind: "base_census",
                    key: &key,
                    record_type: &ty,
                    editor_id: &inspect.base.common.editor_id,
                    model: &inspect.base.common.model.path,
                    state: &inspect.state,
                    editor_marker: inspect.base.editor_marker,
                    remaining_causes: &remaining_labels,
                    shapes,
                    models,
                    materials,
                    textures,
                    references: refs.all,
                    interior_references: refs.interior,
                    active_references: refs.active,
                    causes: &labels,
                })?;
            }
            Err(e) => {
                r.summary.base_census.failed += 1;
                r.summary
                    .base_census
                    .by_type
                    .entry(ty.to_string())
                    .or_default()
                    .failed += 1;
                r.error(&e)?;
            }
        }
        r.summary.base_census.cache_peak_bytes =
            r.summary.base_census.cache_peak_bytes.max(cache.used());
        r.summary.base_census.cache_peak_entries = r
            .summary
            .base_census
            .cache_peak_entries
            .max(loader.assets.nodes.len() as u64);
    }
    for (component, paths) in components {
        r.summary
            .base_census
            .material_components
            .insert(component, paths.len() as u64);
    }
    r.summary.base_census.declared_models = all_models.len() as u64;
    r.summary.base_census.declared_materials = all_materials.len() as u64;
    r.summary.base_census.declared_textures = all_textures.len() as u64;
    if let Some(selected) = &selected {
        if r.summary.base_census.attempted != selected.len() as u64 {
            return Err(Error::new(
                "MissingSelectedBase",
                "base selection",
                0,
                selected.len(),
                r.summary.base_census.attempted,
            ));
        }
    }
    r.summary.override_memory = index.used();
    r.summary.other_memory = aggregate.used() + r.summary.base_census.cache_peak_bytes;
    Ok(())
}
