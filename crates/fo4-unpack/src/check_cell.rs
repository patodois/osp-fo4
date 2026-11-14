use crate::Reports;
use fo4_core::{Budget, Error, Manifest, Result};
use fo4_plugin::typed::Store;
use fo4_scene::{Loader, PathPolicy, State};
use fo4_vfs::Vfs;
pub fn run(
    m: &Manifest,
    editor_id: &str,
    reports: &mut Reports,
    under_hypotheses: bool,
    overlay: Option<&std::path::Path>,
) -> Result<()> {
    reports.items_enabled = true;
    let store = Store::open_overlay(m, overlay, &Budget::new(m.limits.max_override_memory))?;
    for error in &store.diagnostics {
        reports.error(error)?;
    }
    let vfs = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    vfs.check_complete()?;
    reports.summary.fingerprint = Some(vfs.fingerprint()?.to_owned());
    let budget = Budget::new(m.limits.max_memory);
    let mut loader = Loader::new(
        &store,
        &vfs,
        &m.language,
        if under_hypotheses {
            PathPolicy::UnderHypotheses
        } else {
            PathPolicy::Strict
        },
        &budget,
    );
    let scene = loader.by_editor_id(editor_id, &budget)?;
    // The command's strict contract is instance readiness. CELL/LGTM causes
    // remain explicitly reported; they do not certify a renderer or lighting.
    reports.item(&serde_json::json!({"kind":"cell_context", "cell":scene.cell.key,"causes":scene.diagnostics,"deleted":scene.deleted,"disabled":scene.disabled,"exclusions":scene.exclusions}))?;
    for instance in &scene.instances {
        let readiness_cause = (!(matches!(instance.state, State::Ready)
            || under_hypotheses && instance.state.drawable_readiness()))
        .then(|| {
            Error::new(
                "CellInstanceNotReady",
                &instance.reference.record.source.file,
                instance.reference.record.source.offset,
                "Ready",
                format!("{:?}", instance.state),
            )
        });
        let row = serde_json::json!({"kind":"checked_instance","reference":instance.reference.key,"base":instance.reference.data.base,"transform":instance.reference.data.transform,"position":instance.reference.data.transform.as_ref().map(|t|&t[..3]),"rotation":instance.reference.data.transform.as_ref().map(|t|&t[3..]),"scale":instance.reference.data.scale,"state":instance.state,"editor_marker":instance.editor_marker,"hypotheses":instance.state.hypotheses(),"visibility_hypotheses":instance.visibility_hypotheses,"causes":instance.blockers,"readiness_cause":readiness_cause});
        reports.item(&row)?;
        println!("{row}");
        reports.summary.files_attempted += 1;
        if matches!(instance.state, State::Ready)
            || under_hypotheses && instance.state.drawable_readiness()
        {
            reports.summary.files_valid += 1;
        } else {
            reports.summary.files_blocked += 1;
            for error in &instance.blockers {
                reports.error(error)?;
            }
            if let Some(error) = readiness_cause {
                reports.error(&error)?;
            }
        }
    }
    Ok(())
}
