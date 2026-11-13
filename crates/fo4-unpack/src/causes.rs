//! Detailed data causes; each placement contributes once per distinct detailed cause.
use fo4_core::{add, Budget, Error, Result};
use fo4_scene::Scene;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default, Serialize)]
pub struct Counts {
    pub instances: u64,
    pub cells: u64,
    pub sole_cause_instances: u64,
    pub citations: u64,
    pub cell_declarations: u64,
    pub distinct_paths: u64,
}
pub fn path_cause(code: &str) -> bool {
    matches!(
        code,
        "MissingModel" | "MissingMaterial" | "MissingTexture" | "MissingPath" | "RejectedVfsPath"
    )
}
pub fn detail(e: &Error) -> String {
    let specialized = matches!(
        e.code.as_str(),
        "UninterpretedVisualSubrecord"
            | "UnsupportedUsedFieldLayout"
            | "RuntimeRecordState"
            | "UnsupportedMaterialFields"
            | "AssetDependencyCycle"
            | "UnsupportedAnimationRuntime"
            | "BaseTypeOutsideSlice"
            | "UnsupportedSkinning"
            | "UnsupportedNifBlockSemantics"
            | "UnsupportedNifBlock"
            | "UnsupportedNifBlockTail"
            | "UnsupportedNifField"
    ) || path_cause(&e.code);
    if specialized {
        format!("{}:{}", e.code, e.found)
    } else {
        e.code.clone()
    }
}
fn entry<'a>(
    map: &'a mut BTreeMap<String, Counts>,
    key: &str,
    path: bool,
    b: &Budget,
) -> Result<&'a mut Counts> {
    if !map.contains_key(key) {
        b.charge(key.len() as u64 + 192, "detailed causes", 0)?;
        map.insert(
            key.into(),
            Counts {
                distinct_paths: u64::from(path),
                ..Counts::default()
            },
        );
    }
    map.get_mut(key)
        .ok_or_else(|| Error::new("MissingCauseEntry", "causes", 0, "inserted cause", key))
}
fn local(map: &mut BTreeMap<String, u64>, key: &str, b: &Budget) -> Result<()> {
    if !map.contains_key(key) {
        b.charge(key.len() as u64 + 128, "cell causes", 0)?;
    }
    let n = map.entry(key.into()).or_default();
    *n = add(*n, 1, "cell causes", 0)?;
    Ok(())
}
pub fn observe(scene: &Scene, work: &Budget) -> Result<BTreeMap<String, Counts>> {
    let mut stored = BTreeMap::new();
    let total = &mut stored;
    let aggregate = work;
    let mut cell = BTreeMap::new();
    for i in &scene.instances {
        let temp = work.child();
        let mut unique = BTreeSet::new();
        for e in i
            .blockers
            .iter()
            .filter(|e| fo4_scene::hypotheses::covering(e).is_none())
        {
            let key = detail(e);
            let stat = entry(total, &key, path_cause(&e.code), aggregate)?;
            stat.citations = add(stat.citations, 1, "detailed causes", 0)?;
            temp.charge(key.len() as u64 + 128, "instance causes", 0)?;
            unique.insert(key);
        }
        let sole = unique.len() == 1;
        for key in unique {
            let stat = entry(total, &key, false, aggregate)?;
            stat.instances += 1;
            if sole {
                stat.sole_cause_instances += 1;
            }
            local(&mut cell, &key, work)?;
        }
    }
    for e in &scene.exclusions {
        let temp = work.child();
        let tag = String::from_utf8(e.child.signature.to_vec())
            .unwrap_or_else(|_| format!("hex:{}", fo4_core::HexBytes(&e.child.signature)));
        let key = format!("{}:{tag}", e.code);
        let mut keys = BTreeSet::new();
        temp.charge(key.len() as u64 + 128, "excluded causes", 0)?;
        keys.insert(key);
        for diagnostic in &e.diagnostics {
            let key = detail(diagnostic);
            temp.charge(key.len() as u64 + 128, "excluded causes", 0)?;
            keys.insert(key);
        }
        let sole = keys.len() == 1;
        for key in keys {
            let stat = entry(total, &key, false, aggregate)?;
            stat.instances += 1;
            stat.citations += 1;
            if sole {
                stat.sole_cause_instances += 1;
            }
            local(&mut cell, &key, work)?;
        }
    }
    for key in cell.keys() {
        entry(total, key, false, aggregate)?.cells += 1;
    }
    Ok(stored)
}
pub fn observe_cell(scene: &Scene, work: &Budget) -> Result<BTreeMap<String, Counts>> {
    let mut result = BTreeMap::new();
    for e in &scene.diagnostics {
        let key = detail(e);
        let stat = entry(&mut result, &key, path_cause(&e.code), work)?;
        stat.citations = add(stat.citations, 1, "cell layer", 0)?;
        stat.cell_declarations = add(stat.cell_declarations, 1, "cell layer", 0)?;
        stat.cells = 1;
    }
    let sole = result.len() == 1;
    for stat in result.values_mut() {
        stat.instances = scene.instances.len() as u64;
        if sole {
            stat.sole_cause_instances = scene
                .instances
                .iter()
                .filter(|i| matches!(i.state, fo4_scene::State::Ready))
                .count() as u64;
        }
    }
    Ok(result)
}
pub fn merge(
    local: &BTreeMap<String, Counts>,
    total: &mut BTreeMap<String, Counts>,
    b: &Budget,
) -> Result<()> {
    let mut reserve = 0;
    for key in local.keys() {
        if !total.contains_key(key) {
            reserve = add(reserve, key.len() as u64 + 192, "detailed cause merge", 0)?;
        }
    }
    b.charge(reserve, "detailed cause merge", 0)?;
    // All retained allocations are budgeted before committing any cell counters.
    for (key, value) in local {
        let target = total.entry(key.clone()).or_default();
        target.instances = add(target.instances, value.instances, "detailed causes", 0)?;
        target.cells = add(target.cells, value.cells, "detailed causes", 0)?;
        target.sole_cause_instances = add(
            target.sole_cause_instances,
            value.sole_cause_instances,
            "detailed causes",
            0,
        )?;
        target.citations = add(target.citations, value.citations, "detailed causes", 0)?;
        target.cell_declarations = add(
            target.cell_declarations,
            value.cell_declarations,
            "detailed causes",
            0,
        )?;
        target.distinct_paths = target.distinct_paths.max(value.distinct_paths);
    }
    Ok(())
}
pub fn infrastructure(e: &Error) -> bool {
    matches!(
        e.code.as_str(),
        "AllocationFailed" | "LimitExceeded" | "AssetCacheUnavailable"
    )
}
pub fn first_infrastructure(s: &Scene) -> Option<&Error> {
    s.diagnostics
        .iter()
        .chain(s.instances.iter().flat_map(|i| i.blockers.iter()))
        .chain(s.exclusions.iter().flat_map(|e| e.diagnostics.iter()))
        .find(|e| infrastructure(e))
}
pub fn infrastructure_scene(s: &Scene) -> bool {
    s.exclusions
        .iter()
        .any(|e| e.diagnostics.iter().any(infrastructure))
        || s.diagnostics.iter().any(infrastructure)
        || s.instances
            .iter()
            .any(|i| i.blockers.iter().any(infrastructure))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_reservation_failure_does_not_commit_partial_cell_counts() {
        let mut local = BTreeMap::new();
        local.insert(
            "MissingTexture:aabb".into(),
            Counts {
                instances: 2,
                cells: 1,
                ..Counts::default()
            },
        );
        let mut global = BTreeMap::new();
        assert_eq!(
            merge(&local, &mut global, &Budget::new(1))
                .unwrap_err()
                .code,
            "LimitExceeded"
        );
        assert!(global.is_empty());
    }
    #[test]
    fn detail_preserves_record_tag_block_field_and_path() {
        for (code, found) in [
            ("UninterpretedVisualSubrecord", "REFR.XLKR"),
            ("BaseTypeOutsideSlice", "CONT"),
            ("UnsupportedNifField", "NiNode.Extra Data"),
            ("UnsupportedSkinning", "BSTriShape"),
            ("MissingTexture", "aabb"),
        ] {
            let e = Error::new(code, "source", 9, "supported", found);
            assert_eq!(detail(&e), format!("{code}:{found}"));
        }
        assert!(infrastructure(&Error::new(
            "AssetCacheUnavailable",
            "cache",
            0,
            "available",
            "aborted"
        )));
    }
}
