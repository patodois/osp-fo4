use crate::{causes, path_census, Reports};
use fo4_core::{add, limit, Budget, Manifest, Result};
use fo4_plugin::{
    typed::{CellKind, Store},
    FormKey,
};
use fo4_scene::{AssetKind, Loader, PathPolicy, Scene, State};
use fo4_vfs::{Candidate, Vfs};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Serialize)]
struct Item<'a, T: Serialize> {
    kind: &'static str,
    data: &'a T,
}
#[derive(Serialize)]
struct InstanceItem<'a> {
    kind: &'static str,
    cell: &'a FormKey,
    cache_instance: u64,
    instance: &'a fo4_scene::Instance,
}
#[derive(Serialize)]
struct AssetItem<'a> {
    kind: &'static str,
    cell: &'a FormKey,
    cache_instance: u64,
    id: usize,
    asset: &'a fo4_scene::Asset,
}
#[derive(Default, Serialize)]
pub struct CellTotals {
    pub refused_families: BTreeMap<String, causes::Counts>,
    pub outside_fields: BTreeMap<String, u64>,
    pub outside_blocks: BTreeMap<String, u64>,
    pub attempted: u64,
    pub read: u64,
    pub failed: u64,
    pub interiors: u64,
    pub exteriors: u64,
    pub unknown_kind: u64,
    pub ready: u64,
    pub under_hypotheses: BTreeMap<String, u64>,
    pub editor_markers: u64,
    pub strict_drawable: u64,
    pub hypothesis_drawable: BTreeMap<String, u64>,
    pub blocked: u64,
    pub outside: u64,
    pub deleted: u64,
    pub disabled: u64,
    pub excluded: u64,
    pub references_by_base: BTreeMap<String, u64>,
    pub placed_types: BTreeMap<String, u64>,
    pub blockers: BTreeMap<String, u64>,
    pub cells_by_blocker: BTreeMap<String, u64>,
    pub candidates: Vec<CellRow>,
    pub complete_index: bool,
    pub detailed: BTreeMap<String, causes::Counts>,
    pub cell_causes: BTreeMap<String, causes::Counts>,
    pub ready_with_cell: u64,
    pub blocked_by_cell: u64,
    pub closest: Vec<NearCell>,
    pub infrastructure_failures: u64,
    pub data_failures: u64,
    pub cache_instances: u64,
    pub cache_batch_cells: u64,
    pub cache_peak_bytes: u64,
    pub cache_peak_entries: u64,
    pub cache_memory_limit: u64,
    pub cache_entry_limit: u64,
    pub distinct_models: u64,
    pub distinct_materials: u64,
    pub distinct_textures: u64,
}
#[derive(Serialize)]
pub struct CellRow {
    pub key: FormKey,
    pub editor_id: Option<String>,
    pub name: Option<String>,
    pub kind: Option<CellKind>,
    pub source: fo4_core::SourceSpan,
    pub references: u64,
    /// Active own placements, including unsupported active child types; excludes deleted/disabled.
    pub own_instances: u64,
    pub ready: u64,
    pub under_hypotheses: BTreeMap<String, u64>,
    pub editor_markers: u64,
    pub strict_drawable: u64,
    pub hypothesis_drawable: BTreeMap<String, u64>,
    pub blocked: u64,
    pub outside: u64,
    pub deleted: u64,
    pub disabled: u64,
    pub exclusions: u64,
    pub lights: u64,
    pub precombine: bool,
    pub previs: bool,
    pub models: u64,
    pub materials: u64,
    pub textures: u64,
    pub by_base: BTreeMap<String, u64>,
    pub blockers: BTreeMap<String, u64>,
    pub detailed: BTreeMap<String, u64>,
    pub candidate: bool,
    pub cell_causes: BTreeMap<String, u64>,
    pub ready_with_cell: u64,
    #[serde(skip)]
    allocation: Option<Budget>,
}
#[derive(Serialize)]
pub struct NearCell {
    pub key: FormKey,
    pub editor_id: Option<String>,
    pub name: Option<String>,
    pub references: u64,
    /// Active own placements, including unsupported active child types; excludes deleted/disabled.
    pub own_instances: u64,
    pub ready: u64,
    pub under_hypotheses: BTreeMap<String, u64>,
    pub editor_markers: u64,
    pub strict_drawable: u64,
    pub hypothesis_drawable: BTreeMap<String, u64>,
    pub blocked: u64,
    pub outside: u64,
    pub lights: u64,
    pub precombine: bool,
    pub cell_causes: BTreeMap<String, u64>,
    pub missing: BTreeMap<String, u64>,
    #[serde(skip)]
    _allocation: Budget,
}
impl NearCell {
    fn retain(row: &CellRow, b: &Budget) -> Result<Self> {
        let account = b.child();
        let n = 1024
            + row.key.plugin.len()
            + row.editor_id.as_ref().map_or(0, String::len)
            + row.name.as_ref().map_or(0, String::len)
            + row.cell_causes.keys().map(|s| s.len() + 128).sum::<usize>()
            + row.detailed.keys().map(|s| s.len() + 128).sum::<usize>();
        account.charge(n as u64, "closest cell", 0)?;
        Ok(Self {
            key: row.key.clone(),
            editor_id: row.editor_id.clone(),
            name: row.name.clone(),
            references: row.references,
            own_instances: row.own_instances,
            ready: row.ready,
            under_hypotheses: row.under_hypotheses.clone(),
            editor_markers: row.editor_markers,
            strict_drawable: row.strict_drawable,
            hypothesis_drawable: row.hypothesis_drawable.clone(),
            blocked: row.blocked,
            outside: row.outside,
            lights: row.lights,
            precombine: row.precombine,
            cell_causes: row.cell_causes.clone(),
            missing: row.detailed.clone(),
            _allocation: account,
        })
    }
}
fn near_order(a: &NearCell, b: &NearCell) -> std::cmp::Ordering {
    // Ranking only; serialized counts and fractions retain the two readiness classes.
    let left = (a.strict_drawable + a.hypothesis_drawable.values().sum::<u64>()) as u128
        * b.own_instances as u128;
    let right = (b.strict_drawable + b.hypothesis_drawable.values().sum::<u64>()) as u128
        * a.own_instances as u128;
    right
        .cmp(&left)
        .then(a.missing.len().cmp(&b.missing.len()))
        .then(a.references.cmp(&b.references))
        .then(a.key.cmp(&b.key))
}
#[derive(Default, Serialize)]
pub struct SizeCount {
    pub files: u64,
    pub bytes: u64,
}
#[derive(Default, Serialize)]
pub struct Unread {
    pub winners: u64,
    pub sizes_read: u64,
    pub sizes_failed: u64,
    pub rejected_names: u64,
    pub sampled_hkx: u64,
    pub failed_hkx: u64,
    pub extensions: BTreeMap<String, SizeCount>,
    pub readers: BTreeMap<String, String>,
    pub hkx_folders: BTreeMap<String, SizeCount>,
    pub hkx_prefixes: BTreeMap<String, u64>,
    pub hkx_version_text: BTreeMap<String, u64>,
    pub animtext_folders: BTreeMap<String, SizeCount>,
    pub animtext_extensions: BTreeMap<String, SizeCount>,
    pub animtext_content: BTreeMap<String, u64>,
    pub nif_animation_files: BTreeMap<String, u64>,
    pub nif_attempted: u64,
    pub nif_read: u64,
    pub nif_failed: u64,
    pub plugin_record_envelopes: BTreeMap<String, u64>,
    pub plugin_index_diagnostics: u64,
}
fn count(map: &mut BTreeMap<String, u64>, key: &str, n: u64, b: &Budget) -> Result<()> {
    if !map.contains_key(key) {
        b.charge(key.len() as u64 + 128, "census", 0)?;
    }
    let v = map.entry(key.into()).or_default();
    *v = add(*v, n, "census", 0)?;
    Ok(())
}
fn size(map: &mut BTreeMap<String, SizeCount>, key: &str, bytes: u64, b: &Budget) -> Result<()> {
    if !map.contains_key(key) {
        b.charge(key.len() as u64 + 128, "census", 0)?;
    }
    let v = map.entry(key.into()).or_default();
    v.files = add(v.files, 1, "census", 0)?;
    v.bytes = add(v.bytes, bytes, "census", 0)?;
    Ok(())
}
fn mount(m: &Manifest, r: &mut Reports) -> Result<Vfs> {
    let v = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    for e in &v.diagnostics {
        r.error(e)?;
    }
    v.check_complete()?;
    r.summary.vfs_memory = v.memory_used();
    r.summary.fingerprint = Some(v.fingerprint()?.to_owned());
    Ok(v)
}
pub fn cells(m: &Manifest, r: &mut Reports) -> Result<()> {
    let details = r.items_enabled;
    r.items_enabled = true;
    let index = Budget::new(m.limits.max_override_memory);
    let store = Store::open(m, &index)?;
    for e in &store.diagnostics {
        r.error(e)?;
    }
    let v = mount(m, r)?;

    let aggregate = Budget::new(m.limits.max_memory);
    r.summary.cell_census.complete_index = store.diagnostics.is_empty();
    r.summary.cell_census.cache_memory_limit = m.limits.max_memory;
    r.summary.cell_census.cache_entry_limit = m.limits.max_entries;
    const CACHE_CELLS: u64 = 32;
    r.summary.cell_census.cache_batch_cells = CACHE_CELLS;
    let cache = Budget::new(m.limits.max_memory);
    let mut loader = Loader::new(&store, &v, &m.language, PathPolicy::Strict, &cache);
    let mut batch_cells = 0;
    let mut renew = false;
    let mut all_models = BTreeSet::new();
    let mut all_materials = BTreeSet::new();
    let mut all_textures = BTreeSet::new();
    for key in store.cells() {
        r.summary.cell_census.attempted += 1;
        if renew || batch_cells == CACHE_CELLS {
            loader.assets = fo4_scene::Assets::new(&v, &cache);
            batch_cells = 0;
            renew = false;
        }
        if batch_cells == 0 {
            r.summary.cell_census.cache_instances += 1;
        }
        batch_cells += 1;
        let work = Budget::new(m.limits.max_memory);
        let attempt = loader.cell(&key, &work).and_then(|scene| {
            if causes::infrastructure_scene(&scene) || loader.assets.unavailable() {
                let mut e = fo4_core::Error::new(
                    "CellInfrastructureFailure",
                    &scene.cell.record.source.file,
                    scene.cell.record.source.offset,
                    "cell within cache/parser budgets",
                    "unavailable cache",
                );
                if let Some(cause) = causes::first_infrastructure(&scene) {
                    e.source = cause.source.clone();
                    e.expected = format!("{}: {}", cause.code, cause.expected);
                    e.found = cause.found.clone();
                }
                return Err(e);
            }
            let mut row = cell_row(&scene, &loader, &work)?;
            let local = causes::observe(&scene, &work)?;
            for (key, value) in &local {
                work.charge(key.len() as u64 + 128, "cell cause labels", 0)?;
                row.detailed.insert(
                    key.clone(),
                    value.instances.max(u64::from(value.cell_declarations > 0)),
                );
            }
            Ok((scene, row, local))
        });
        match attempt {
            Ok((scene, mut row, local)) => {
                if let Err(e) =
                    causes::merge(&local, &mut r.summary.cell_census.detailed, &aggregate)
                {
                    r.summary.cell_census.failed += 1;
                    r.summary.cell_census.infrastructure_failures += 1;
                    renew = true;
                    r.error(&e)?;
                    r.summary.cell_census.cache_peak_bytes =
                        r.summary.cell_census.cache_peak_bytes.max(cache.used());
                    r.summary.cell_census.cache_peak_entries = r
                        .summary
                        .cell_census
                        .cache_peak_entries
                        .max(loader.assets.nodes.len() as u64);
                    continue;
                }
                let cell_causes = causes::observe_cell(&scene, &work)?;
                causes::merge(
                    &cell_causes,
                    &mut r.summary.cell_census.cell_causes,
                    &aggregate,
                )?;
                crate::bases::outside_record(
                    &scene.cell.record,
                    &mut r.summary.cell_census.outside_fields,
                    &aggregate,
                )?;
                if let Some(template) = &scene.lighting_template {
                    crate::bases::outside_record(
                        &template.record,
                        &mut r.summary.cell_census.outside_fields,
                        &aggregate,
                    )?;
                }
                let mut family_rows = BTreeMap::new();
                for instance in &scene.instances {
                    let scratch = work.child();
                    let families = crate::families::instance_causes(
                        &loader.assets,
                        &instance.assets,
                        &instance.blockers,
                        &scratch,
                    )?;
                    causes::merge(&families, &mut family_rows, &work)?;
                    if let Some(base) = &instance.base {
                        crate::bases::outside(
                            base,
                            &mut r.summary.cell_census.outside_fields,
                            &aggregate,
                        )?;
                    }
                    crate::bases::outside_record(
                        &instance.reference.record,
                        &mut r.summary.cell_census.outside_fields,
                        &aggregate,
                    )?;
                    for id in &instance.assets {
                        for (ty, n) in &loader.assets.nodes[*id].outside_blocks {
                            count(
                                &mut r.summary.cell_census.outside_blocks,
                                ty,
                                *n,
                                &aggregate,
                            )?;
                        }
                    }
                }
                for value in family_rows.values_mut() {
                    value.cells = 1;
                }
                causes::merge(
                    &family_rows,
                    &mut r.summary.cell_census.refused_families,
                    &aggregate,
                )?;
                row.candidate &= r.summary.cell_census.complete_index;
                for e in &scene.diagnostics {
                    r.error(e)?;
                }
                r.item(&Item {
                    kind: "cell_census",
                    data: &row,
                })?;
                for instance in scene.instances.iter().filter(|_| details) {
                    r.item(&InstanceItem {
                        kind: "cell_instance",
                        cell: &key,
                        cache_instance: r.summary.cell_census.cache_instances,
                        instance,
                    })?;
                }
                for exclusion in &scene.exclusions {
                    r.item(exclusion)?;
                }
                for (i, asset) in loader.assets.nodes.iter().enumerate() {
                    if asset.origin.is_some() {
                        let set = match asset.kind {
                            AssetKind::Model => &mut all_models,
                            AssetKind::Material => &mut all_materials,
                            AssetKind::Texture => &mut all_textures,
                        };
                        if !set.contains(&asset.path) {
                            aggregate.charge(
                                asset.path.len() as u64 + 128,
                                "global asset paths",
                                0,
                            )?;
                            set.insert(asset.path.clone());
                        }
                    }
                    if details {
                        r.item(&AssetItem {
                            kind: "scene_asset",
                            cell: &key,
                            cache_instance: r.summary.cell_census.cache_instances,
                            id: i,
                            asset,
                        })?;
                    }
                }
                let total = &mut r.summary.cell_census;
                if row.kind == Some(CellKind::Interior) && row.own_instances >= 50 {
                    aggregate.reserve_capped(&mut total.closest, 1, 21, "closest cells", 0)?;
                    total.closest.push(NearCell::retain(&row, &aggregate)?);
                    total.closest.sort_by(near_order);
                    total.closest.truncate(20);
                }
                total.read += 1;
                match scene.kind {
                    Some(CellKind::Interior) => total.interiors += 1,
                    Some(CellKind::Exterior) => total.exteriors += 1,
                    None => total.unknown_kind += 1,
                }
                total.ready += row.ready;
                total.editor_markers += row.editor_markers;
                total.strict_drawable += row.strict_drawable;
                for (set, n) in &row.under_hypotheses {
                    count(&mut total.under_hypotheses, set, *n, &aggregate)?;
                }
                for (set, n) in &row.hypothesis_drawable {
                    count(&mut total.hypothesis_drawable, set, *n, &aggregate)?;
                }
                total.blocked += row.blocked;
                total.outside += row.outside;
                total.ready_with_cell += row.ready_with_cell;
                total.blocked_by_cell += row.ready - row.ready_with_cell;
                total.deleted += row.deleted;
                total.disabled += row.disabled;
                total.excluded += row.exclusions;
                for (tag, n) in &row.by_base {
                    count(&mut total.references_by_base, tag, *n, &aggregate)?;
                }
                for (cause, n) in &row.blockers {
                    count(&mut total.blockers, cause, *n, &aggregate)?;
                    count(&mut total.cells_by_blocker, cause, 1, &aggregate)?;
                }
                for child in scene.deleted.iter().chain(&scene.disabled) {
                    count(
                        &mut total.placed_types,
                        &tag(&child.signature),
                        1,
                        &aggregate,
                    )?;
                }
                count(
                    &mut total.placed_types,
                    "REFR",
                    scene.instances.len() as u64,
                    &aggregate,
                )?;
                for exclusion in &scene.exclusions {
                    count(
                        &mut total.placed_types,
                        &tag(&exclusion.child.signature),
                        1,
                        &aggregate,
                    )?;
                }
                if row.candidate && total.complete_index {
                    let account = aggregate.child();
                    let retained = std::mem::size_of::<CellRow>()
                        + row.key.plugin.len()
                        + row.editor_id.as_ref().map_or(0, String::len)
                        + row.name.as_ref().map_or(0, String::len)
                        + row.source.file.len()
                        + row.by_base.keys().map(|s| s.len() + 128).sum::<usize>()
                        + row.blockers.keys().map(|s| s.len() + 128).sum::<usize>()
                        + row.cell_causes.keys().map(|s| s.len() + 128).sum::<usize>()
                        + row.detailed.keys().map(|s| s.len() + 128).sum::<usize>();
                    account.charge(retained as u64, "cell candidate", 0)?;
                    row.allocation = Some(account);
                    aggregate.reserve_capped(&mut total.candidates, 1, 21, "cell census", 0)?;
                    total.candidates.push(row);
                    total
                        .candidates
                        .sort_by(|a, b| a.references.cmp(&b.references).then(a.key.cmp(&b.key)));
                    total.candidates.truncate(20);
                }
            }
            Err(e) => {
                r.summary.cell_census.failed += 1;
                if e.code == "CellInfrastructureFailure" || causes::infrastructure(&e) {
                    r.summary.cell_census.infrastructure_failures += 1;
                    renew = true;
                } else {
                    r.summary.cell_census.data_failures += 1;
                }
                r.error(&e)?;
                r.item(&serde_json::json!({"kind":"cell_failed","key":key,"error":e}))?;
            }
        }
        if cache.used() > m.limits.max_memory / 4 * 3
            || loader.assets.nodes.len() as u64 > m.limits.max_entries / 4 * 3
        {
            renew = true;
        }
        r.summary.cell_census.cache_peak_bytes =
            r.summary.cell_census.cache_peak_bytes.max(cache.used());
        r.summary.cell_census.cache_peak_entries = r
            .summary
            .cell_census
            .cache_peak_entries
            .max(loader.assets.nodes.len() as u64);
    }
    r.summary.cell_census.distinct_models = all_models.len() as u64;
    r.summary.cell_census.distinct_materials = all_materials.len() as u64;
    r.summary.cell_census.distinct_textures = all_textures.len() as u64;
    r.summary.override_memory = index.used();
    r.summary.other_memory = r.summary.cell_census.cache_peak_bytes;
    Ok(())
}
fn cell_row(scene: &Scene, loader: &Loader<'_>, b: &Budget) -> Result<CellRow> {
    let mut by_base = BTreeMap::new();
    let mut blockers = BTreeMap::new();
    let mut ready = 0;
    let mut under_hypotheses = BTreeMap::new();
    let mut editor_markers = 0;
    let mut strict_drawable = 0;
    let mut hypothesis_drawable = BTreeMap::new();
    let mut blocked = 0;
    let mut outside = 0;
    let mut cell_causes = BTreeMap::new();
    let mut lights = 0;
    for i in &scene.instances {
        if i.editor_marker {
            editor_markers += 1;
        }
        match &i.state {
            State::Ready => {
                ready += 1;
                if !i.editor_marker
                    && i.assets
                        .iter()
                        .any(|id| loader.assets.nodes[*id].shapes > 0)
                {
                    strict_drawable += 1;
                }
            }
            State::ReadyUnderHypotheses { hypotheses } => {
                count(&mut under_hypotheses, &hypotheses.join("+"), 1, b)?;
                if !i.editor_marker
                    && i.assets
                        .iter()
                        .any(|id| loader.assets.nodes[*id].shapes > 0)
                {
                    count(&mut hypothesis_drawable, &hypotheses.join("+"), 1, b)?;
                }
            }
            State::Blocked => blocked += 1,
            State::Outside => outside += 1,
        }
        let tag = i
            .reference
            .data
            .base
            .as_ref()
            .and_then(|r| r.key.as_ref())
            .and_then(|key| loader.plugins.overrides().winner(key))
            .map(|v| String::from_utf8_lossy(&v.signature).into_owned())
            .unwrap_or_else(|| "unresolved".into());
        count(&mut by_base, &tag, 1, b)?;
        if tag == "LIGH" {
            lights += 1;
        }
        let mut instance_causes = BTreeSet::new();
        for e in i
            .blockers
            .iter()
            .filter(|e| fo4_scene::hypotheses::covering(e).is_none())
        {
            if instance_causes.insert(&e.code) {
                count(&mut blockers, &e.code, 1, b)?;
            }
        }
    }
    for child in scene.deleted.iter().chain(&scene.disabled) {
        let temporary = b.child();
        let tag = if child.signature == *b"REFR" {
            match loader
                .plugins
                .get::<fo4_plugin::typed::Refr>(&child.key, &temporary)
            {
                Ok(Some(r)) => r
                    .data
                    .base
                    .as_ref()
                    .and_then(|r| r.key.as_ref())
                    .and_then(|k| loader.plugins.overrides().winner(k))
                    .map(|w| String::from_utf8_lossy(&w.signature).into_owned()),
                _ => None,
            }
        } else {
            None
        };
        count(
            &mut by_base,
            tag.as_deref().unwrap_or("unresolved_inactive"),
            1,
            b,
        )?;
    }
    for e in &scene.exclusions {
        count(
            &mut by_base,
            &format!(
                "unresolved_excluded_{}",
                String::from_utf8_lossy(&e.child.signature)
            ),
            1,
            b,
        )?;
    }
    for e in &scene.exclusions {
        count(&mut blockers, e.code, 1, b)?;
    }
    for e in &scene.diagnostics {
        count(&mut cell_causes, &causes::detail(e), 1, b)?;
    }
    let mut models = 0;
    let mut materials = 0;
    let mut textures = 0;
    for i in &scene.assets {
        if loader.assets.nodes[*i].origin.is_none() {
            continue;
        }
        match loader.assets.nodes[*i].kind {
            AssetKind::Model => models += 1,
            AssetKind::Material => materials += 1,
            AssetKind::Texture => textures += 1,
        }
    }
    let precombine = !scene.visibility.precombine.is_empty()
        || scene
            .instances
            .iter()
            .any(|i| !i.visibility.precombine.is_empty());
    let previs = !scene.visibility.previs.is_empty()
        || scene
            .instances
            .iter()
            .any(|i| !i.visibility.previs.is_empty());
    let candidate = scene.kind == Some(CellKind::Interior)
        && blocked == 0
        && under_hypotheses.is_empty()
        && !precombine
        && lights > 0
        && scene.exclusions.is_empty()
        && scene.diagnostics.is_empty();
    b.charge(2048, "cell census", 0)?;
    Ok(CellRow {
        key: scene.cell.key.clone(),
        editor_id: scene.cell.data.common.editor_id.clone(),
        name: scene.name.clone(),
        kind: scene.kind,
        source: scene.cell.record.source.clone(),
        own_instances: (scene.instances.len() + scene.exclusions.len()) as u64,
        references: (scene.instances.len()
            + scene.deleted.len()
            + scene.disabled.len()
            + scene.exclusions.len()) as u64,
        ready,
        under_hypotheses,
        editor_markers,
        strict_drawable,
        hypothesis_drawable,
        blocked,
        outside,
        cell_causes,
        ready_with_cell: if scene.diagnostics.is_empty() {
            ready
        } else {
            0
        },
        deleted: scene.deleted.len() as u64,
        disabled: scene.disabled.len() as u64,
        exclusions: scene.exclusions.len() as u64,
        lights,
        precombine,
        previs,
        models,
        materials,
        textures,
        by_base,
        blockers,
        candidate,
        detailed: BTreeMap::new(),
        allocation: None,
    })
}
fn tag(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.into(),
        Err(_) => format!("hex:{}", fo4_core::HexBytes(bytes)),
    }
}
fn extension(key: &[u8]) -> String {
    let leaf = key.rsplit(|c| *c == b'/').next().unwrap_or(key);
    leaf.iter().rposition(|c| *c == b'.').map_or_else(
        || "<none>".into(),
        |i| {
            String::from_utf8(leaf[i..].to_vec())
                .unwrap_or_else(|_| format!("hex:{}", fo4_core::HexBytes(&leaf[i..])))
        },
    )
}
fn reader(ext: &str) -> &'static str {
    match ext {
        ".nif" => "fo4-nif (documented block subset)",
        ".bgsm" | ".bgem" => "fo4-material (binary family)",
        ".dds" => "fo4-texture",
        ".strings" | ".dlstrings" | ".ilstrings" => "fo4-plugin strings",
        ".esm" | ".esp" | ".esl" => "fo4-plugin (envelopes + visual slice)",
        ".ba2" => "fo4-archive",
        _ => "none",
    }
}
fn version_text(bytes: &[u8]) -> Option<String> {
    bytes
        .split(|c| !matches!(c, 0x20..=0x7e))
        .find(|s| s.starts_with(b"hk_") || s.starts_with(b"Havok-"))
        .map(|s| String::from_utf8_lossy(s).into_owned())
}
fn text_class(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        if !(bytes.len() - 2).is_multiple_of(2) {
            return "binary_or_invalid_utf16";
        }
        let units = bytes[2..].as_chunks::<2>().0.iter().map(|c| {
            if little {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        });
        if std::char::decode_utf16(units)
            .all(|c| c.is_ok_and(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r')))
        {
            return if little { "utf16le_bom" } else { "utf16be_bom" };
        }
        return "binary_or_invalid_utf16";
    }
    if std::str::from_utf8(bytes).is_ok_and(|s| {
        s.chars()
            .all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
    }) {
        "utf8"
    } else {
        "binary_or_other_encoding"
    }
}
pub fn unread(m: &Manifest, r: &mut Reports, paths_only: bool) -> Result<()> {
    r.items_enabled = true;
    let v = mount(m, r)?;
    let aggregate = Budget::new(m.limits.max_memory);
    let mut paths = path_census::Census::new(m.limits.max_entries, m.limits.max_memory);
    for chain in v.candidates.chunk_by(|a, b| a.key == b.key) {
        let Some(c) = chain.last() else { continue };
        let Some(key) = &c.key else {
            r.summary.unread.rejected_names += chain.len() as u64;
            continue;
        };
        let work = Budget::new(m.limits.max_memory);
        // Verify priority policy; never choose a supported loser when the winner is blocked.
        if let Err(e) = v.chain_bytes(key) {
            r.error(&e)?;
            continue;
        }
        if !paths_only {
            inventory_entry(&v, c, key, r, &aggregate, &work)?;
        }
        if key.ends_with(b".nif") {
            r.summary.unread.nif_attempted += 1;
            r.summary.path_census.nif_attempted += 1;
            let result = (|| {
                let bytes = v.read_candidate(c, &work)?;
                let n = fo4_nif::Nif::read(&bytes, &c.original, m.game, &m.limits, &work)?;
                let mut types = BTreeSet::new();
                for block in &n.blocks {
                    let name = &n.block_types[block.type_index as usize];
                    if name.contains("Controller")
                        || name.contains("Sequence")
                        || name.contains("Interpolator")
                        || matches!(
                            name.as_str(),
                            "NiTransformData"
                                | "NiKeyframeData"
                                | "NiTextKeyExtraData"
                                | "BSAnimNotes"
                        )
                    {
                        types.insert(name);
                    }
                    if matches!(
                        name.as_str(),
                        "BSLightingShaderProperty" | "BSEffectShaderProperty"
                    ) {
                        if let Some(fo4_nif::Data::Fields(fields)) = &block.decoded {
                            for f in fields.iter().filter(|f| f.name == "Name") {
                                if let fo4_nif::Value::StringIndex(i) = f.value {
                                    if let Some(path) = n.strings.get(i as usize) {
                                        if !path.is_empty() {
                                            paths.cite(path)?;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                for ty in types {
                    count(&mut r.summary.unread.nif_animation_files, ty, 1, &aggregate)?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    r.summary.unread.nif_read += 1;
                    r.summary.path_census.nif_read += 1;
                }
                Err(e) => {
                    r.summary.unread.nif_failed += 1;
                    r.summary.path_census.nif_failed += 1;
                    r.error(&e)?;
                }
            }
        }
    }
    if !paths_only {
        let store = Store::open(m, &Budget::new(m.limits.max_override_memory))?;
        for (tag, n) in &store.record_types {
            count(
                &mut r.summary.unread.plugin_record_envelopes,
                &self::tag(tag),
                *n,
                &aggregate,
            )?;
        }
        r.summary.unread.plugin_index_diagnostics = store.diagnostics.len() as u64;
        for e in &store.diagnostics {
            r.error(e)?;
        }
    }
    paths.finish(&v, r)?;
    Ok(())
}
fn inventory_entry(
    v: &Vfs,
    c: &Candidate,
    key: &[u8],
    r: &mut Reports,
    aggregate: &Budget,
    work: &Budget,
) -> Result<()> {
    r.summary.unread.winners += 1;
    let n = match v.payload_size(c) {
        Ok(n) => n,
        Err(e) => {
            r.summary.unread.sizes_failed += 1;
            r.error(&e)?;
            return Ok(());
        }
    };
    let ext = extension(key);
    let u = &mut r.summary.unread;
    u.sizes_read += 1;
    size(&mut u.extensions, &ext, n, aggregate)?;
    if !u.readers.contains_key(&ext) {
        aggregate.charge(ext.len() as u64 + 256, "census", 0)?;
        u.readers.insert(ext.clone(), reader(&ext).into());
    }
    if ext == ".hkx" {
        let path = String::from_utf8_lossy(key);
        let folders: Vec<_> = path
            .rsplit_once('/')
            .map_or("", |(p, _)| p)
            .split('/')
            .take(2)
            .collect();
        for depth in 1..=folders.len() {
            size(
                &mut u.hkx_folders,
                &folders[..depth].join("/"),
                n,
                aggregate,
            )?;
        }
        match v.read_prefix(c, 256, work) {
            Ok(prefix) => {
                u.sampled_hkx += 1;
                count(
                    &mut u.hkx_prefixes,
                    &fo4_core::HexBytes(&prefix.bytes[..prefix.bytes.len().min(16)]).to_string(),
                    1,
                    aggregate,
                )?;
                count(
                    &mut u.hkx_version_text,
                    &version_text(&prefix.bytes).unwrap_or_else(|| "<none observed>".into()),
                    1,
                    aggregate,
                )?;
                r.item(&serde_json::json!({"kind":"hkx_prefix","path_hex":fo4_core::HexBytes(key).to_string(),"origin":v.provenance(c)?,"prefix":prefix}))?;
            }
            Err(e) => {
                u.failed_hkx += 1;
                r.error(&e)?;
            }
        }
    }
    if key.starts_with(b"meshes/animtextdata/") {
        let path = String::from_utf8_lossy(key);
        let folders: Vec<_> = path
            .rsplit_once('/')
            .map_or("", |(p, _)| p)
            .split('/')
            .collect();
        limit(folders.len() as u64, v.limits().max_depth, &c.original, 0)?;
        let u = &mut r.summary.unread;
        for depth in 2..=folders.len() {
            size(
                &mut u.animtext_folders,
                &folders[..depth].join("/"),
                n,
                aggregate,
            )?;
        }
        size(&mut u.animtext_extensions, &ext, n, aggregate)?;
        match v.read_candidate(c, work) {
            Ok(bytes) => count(&mut u.animtext_content, text_class(&bytes), 1, aggregate)?,
            Err(e) => {
                count(
                    &mut u.animtext_content,
                    "unchecked_read_failure",
                    1,
                    aggregate,
                )?;
                r.error(&e)?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_classifies_bytes_without_guessing_version() {
        assert_eq!(
            version_text(b"\x00hk_2014.1.0-r1\0xx").as_deref(),
            Some("hk_2014.1.0-r1")
        );
        assert_eq!(version_text(b"\x01\x02\x03"), None);
        assert_eq!(text_class(b"hello\n\tworld"), "utf8");
        assert_eq!(text_class(b"a\0b"), "binary_or_other_encoding");
        assert_eq!(text_class(&[255, 254, b'a', 0, 10, 0]), "utf16le_bom");
        assert_eq!(text_class(&[255, 254, 0, 216]), "binary_or_invalid_utf16");
    }
}
