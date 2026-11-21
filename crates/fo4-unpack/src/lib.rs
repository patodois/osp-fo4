#![forbid(unsafe_code)]
//! Report-only CLI implementation. No extraction or writes to the input root.
mod assets;
mod bases;
mod causes;
mod cell_census;
mod check_cell;
mod collision;
mod families;
mod path_census;
mod references;
mod root_census;
use fo4_archive::GameArchive;
use fo4_core::{
    digest, io_error, within, Budget, Error, FileSource, Manifest, Mount, ReadAt, Result,
};
use fo4_plugin::{
    scan_bounded, Header, LoadOrder, Overrides, StringCache, StringKind, Value, LOCALIZED,
};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Debug)]
pub struct Options {
    /// Which rules beyond documented facts `check-cell` accepts.
    pub level: fo4_scene::RuleLevel,
    pub overlay_plugin: Option<PathBuf>,
    pub command: String,
    pub manifest: PathBuf,
    pub out: PathBuf,
    pub scope: Option<String>,
    pub path: Option<String>,
    pub items: bool,
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut args = args.into_iter();
        let command = args.next().ok_or_else(usage)?;
        if command == "check-cell" {
            let manifest = args.next().ok_or_else(usage)?.into();
            let path = Some(args.next().ok_or_else(usage)?);
            let mut out = PathBuf::from("reports/check-cell");
            let mut level = None;
            let mut overlay_plugin = None;
            let mut out_seen = false;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--out" if !out_seen => {
                        out = args.next().ok_or_else(usage)?.into();
                        out_seen = true;
                    }
                    "--overlay-plugin" if overlay_plugin.is_none() => {
                        overlay_plugin = Some(PathBuf::from(args.next().ok_or_else(usage)?))
                    }
                    "--under-hypotheses" if level.is_none() => {
                        level = Some(fo4_scene::RuleLevel::Hypotheses)
                    }
                    "--strict" if level.is_none() => level = Some(fo4_scene::RuleLevel::Facts),
                    _ => return Err(usage()),
                }
            }
            return Ok(Self {
                level: level.unwrap_or(fo4_scene::RuleLevel::Measured),
                overlay_plugin,
                command,
                manifest,
                out,
                scope: None,
                path,
                items: true,
            });
        }
        if !["inventory", "resolve", "sweep"].contains(&command.as_str()) {
            return Err(usage());
        }
        let mut manifest = None;
        let mut out = None;
        let mut scope = None;
        let mut path = None;
        let mut all = false;
        let mut items = false;
        while let Some(a) = args.next() {
            match a.as_str() {
                "nif" | "materials" | "textures" if command == "sweep" && scope.is_none() => {
                    scope = Some(a);
                }
                "--items" => {
                    if items {
                        return Err(usage());
                    }
                    items = true;
                }
                "--all" => {
                    if all {
                        return Err(usage());
                    }
                    all = true;
                }
                "--manifest" | "--out" | "--scope" | "--path" => {
                    let target = match a.as_str() {
                        "--manifest" => &mut manifest,
                        "--out" => &mut out,
                        "--scope" => &mut scope,
                        _ => &mut path,
                    };
                    if target.is_some() {
                        return Err(usage());
                    }
                    *target = Some(args.next().ok_or_else(usage)?);
                }
                _ => return Err(usage()),
            }
        }
        if command == "sweep"
            && !scope.as_deref().is_some_and(|s| {
                [
                    "archives",
                    "records",
                    "nif",
                    "materials",
                    "textures",
                    "cells",
                    "bases",
                    "shader-citations",
                    "unread",
                    "paths",
                    "roots",
                    "collision",
                ]
                .contains(&s)
            })
            || command != "sweep" && scope.is_some()
            || command != "resolve" && path.is_some()
            || path.is_some() && all
        {
            return Err(usage());
        }
        Ok(Self {
            level: fo4_scene::RuleLevel::Measured,
            overlay_plugin: None,
            command,
            items,
            manifest: manifest.ok_or_else(usage)?.into(),
            out: out.ok_or_else(usage)?.into(),
            scope,
            path,
        })
    }
}
fn usage() -> Error {
    Error::new("Usage","CLI",0,"inventory|resolve|sweep --manifest PROFILE.json --out DIR [--scope archives|records|nif|materials|textures|cells|bases|shader-citations|unread|paths|roots|collision] [--all|--path ASSET] [--items]; check-cell PROFILE.json EditorID [--out DIR] [--strict|--under-hypotheses] [--overlay-plugin FILE]","invalid or missing argument")
}
#[derive(Default, Serialize)]
pub struct Summary {
    pub game: fo4_core::Game,
    pub report_bytes: u64,
    pub max_report_bytes: u64,
    pub report_limit_exceeded: bool,
    pub archive_invalidation: Option<fo4_core::ArchiveInvalidation>,
    pub strings_by_language: BTreeMap<String, LanguageCounts>,
    pub records_by_plugin: BTreeMap<String, PluginCounts>,
    pub override_records: u64,
    pub unique_forms: u64,
    pub diagnostic_groups_omitted: u64,
    pub items_omitted: u64,
    pub root_census: root_census::Summary,
    pub cell_census: cell_census::CellTotals,
    pub base_census: bases::Summary,
    pub shader_citations: families::Summary,
    pub path_census: path_census::Summary,
    pub unread: cell_census::Unread,
    pub collision: collision::Summary,
    pub schema: u32,
    pub under_hypotheses: bool,
    /// The rule level the command applied, for the commands that classify readiness.
    pub rule_level: Option<fo4_scene::RuleLevel>,
    pub overlay_plugin: Option<PathBuf>,
    pub command: String,
    pub scope: Option<String>,
    pub installation_scope: String,
    pub language: String,
    pub build: Option<String>,
    pub files_enumerated: u64,
    pub files_attempted: u64,
    pub files_valid: u64,
    pub files_failed: u64,
    pub files_unavailable: u64,
    pub files_blocked: u64,
    pub divergences: u64,
    pub string_tables_loaded: u64,
    pub string_entries_enumerated: u64,
    pub string_entries_decoded: u64,
    pub string_entries_undecoded: u64,
    pub language_entries_without_breakdown: u64,
    pub override_memory: u64,
    pub vfs_memory: u64,
    pub other_memory: u64,
    pub archives: u64,
    pub blocks: u64,
    pub raw_blocks: u64,
    pub partially_read_blocks: u64,
    pub vertices: u64,
    pub triangles: u64,
    pub texture_surfaces: u64,
    pub decoded_texture_bytes: u64,
    pub shadowed_assets: u64,
    pub non_finite_values: u64,
    pub files_with_non_finite: u64,
    pub records_with_non_finite: u64,
    pub non_finite_by_attribute: BTreeMap<String, u64>,
    pub material_references: references::Counts,
    pub texture_references: references::Counts,
    pub entries_enumerated: u64,
    pub entries_attempted: u64,
    pub entries_valid: u64,
    pub entries_failed: u64,
    pub chunks_enumerated: u64,
    pub chunks_attempted: u64,
    pub chunks_valid: u64,
    pub chunks_failed: u64,
    pub payload_bytes: u64,
    pub groups: u64,
    pub records: u64,
    pub subrecords: u64,
    pub envelope_records: u64,
    pub interpreted_fields: u64,
    pub uninterpreted_fields: u64,
    pub header_uninterpreted_subrecords: u64,
    pub failed_fields: u64,
    pub partially_interpreted_fields: u64,
    pub unknown_groups: u64,
    pub deleted: u64,
    pub injected_records: u64,
    pub fingerprint_scope: Option<String>,
    pub disabled: u64,
    pub strings_resolved: u64,
    pub references: u64,
    pub vfs_keys: u64,
    pub vfs_candidates: u64,
    pub inactive_mounts: u64,
    pub fingerprint: Option<String>,
    pub diagnostics: u64,
    pub failures: u64,
    pub failure_codes: BTreeMap<String, u64>,
    pub failures_without_code: u64,
    pub non_enumerable_regions: u64,
    pub histograms: BTreeMap<String, u64>,
    pub status: u8,
}
#[derive(Default, Serialize)]
pub struct LanguageCounts {
    pub tables: u64,
    pub decoded: u64,
    pub undecoded: u64,
    pub bom_prefixed: u64,
    pub english_utf8_multibyte: u64,
}
#[derive(Default, Serialize)]
pub struct PluginCounts {
    pub non_finite_values: u64,
    pub records_with_non_finite: u64,
    pub records: u64,
    pub groups: u64,
    pub subrecords: u64,
    pub complete: bool,
    pub override_records: u64,
    pub injected_records: u64,
}
#[derive(Serialize)]
struct DiagnosticGroup {
    #[serde(flatten)]
    error: Error,
    count: u64,
    last_offset: u64,
    notice: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample: Option<serde_json::Value>,
}
struct OutputBudget {
    max: u64,
    used: u64,
    exhausted: bool,
}
const TERMINAL_RESERVE: u64 = 4 << 20;
const MIN_REPORT_BYTES: u64 = 8 << 20;
struct CountingWriter {
    count: u64,
    max: u64,
    exceeded: bool,
}
impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let next = self
            .count
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| std::io::Error::other("report length overflow"))?;
        if next > self.max {
            self.exceeded = true;
            return Err(std::io::Error::other("report byte limit"));
        }
        self.count = next;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn json_size(value: &impl Serialize, max: u64) -> Result<u64> {
    let mut sink = CountingWriter {
        count: 0,
        max,
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer(&mut sink, value) {
        return Err(Error::new(
            if sink.exceeded {
                "ReportLimitExceeded"
            } else {
                "ReportWrite"
            },
            "reports",
            0,
            format!("JSON bytes <= {max}"),
            error,
        ));
    }
    let size = fo4_core::add(sink.count, 1, "reports", 0)?;
    if size > max {
        return Err(Error::new("ReportLimitExceeded", "reports", 0, max, size));
    }
    Ok(size)
}
fn write_bounded(
    w: &mut impl Write,
    value: &impl Serialize,
    file: &str,
    b: &mut OutputBudget,
    terminal: bool,
) -> Result<()> {
    let reserve = if terminal { 0 } else { TERMINAL_RESERVE };
    let remaining = b.max.saturating_sub(b.used).saturating_sub(reserve);
    let size = match json_size(value, remaining) {
        Ok(size) => size,
        Err(error) => {
            if error.code == "ReportLimitExceeded" {
                b.exhausted = true;
            }
            return Err(error);
        }
    };
    b.used = fo4_core::add(b.used, size, file, 0)?;
    serde_json::to_writer(&mut *w, value)
        .map_err(|e| Error::new("ReportWrite", file, 0, "JSON", e))?;
    w.write_all(b"\n").map_err(|e| io_error(file, 0, e))
}
struct StagedReports {
    path: Option<PathBuf>,
    parents: Vec<PathBuf>,
    keep: bool,
}
impl StagedReports {
    fn rollback(&mut self) -> Result<()> {
        if let Some(path) = &self.path {
            match fs::remove_dir_all(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(Error::new(
                        "ReportRollbackFailed",
                        &path.to_string_lossy(),
                        0,
                        "remove newly created report directory",
                        e,
                    ))
                }
            }
            self.path = None;
        }
        while let Some(path) = self.parents.last() {
            match fs::remove_dir(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(Error::new(
                        "ReportRollbackFailed",
                        &path.to_string_lossy(),
                        0,
                        "remove empty newly created parent",
                        e,
                    ))
                }
            }
            self.parents.pop();
        }
        Ok(())
    }
}
impl Drop for StagedReports {
    fn drop(&mut self) {
        if !self.keep {
            let _ = self.rollback();
        }
    }
}
fn publish_reports(
    out: &Path,
    mut open: impl FnMut(&Path) -> Result<File>,
    mut verify: impl FnMut(&Path) -> Result<()>,
) -> Result<[BufWriter<File>; 3]> {
    // Validate the entire destination before even creating intermediate parents.
    verify(out)?;
    match fs::symlink_metadata(out) {
        Ok(_) => {
            return Err(Error::new(
                "ReportDirectoryExists",
                &out.to_string_lossy(),
                0,
                "output directory that does not exist",
                "existing destination; left unchanged",
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_error(&out.to_string_lossy(), 0, e)),
    }
    let parent = out.parent().ok_or_else(usage)?;
    let mut missing = Vec::new();
    let mut ancestor = parent;
    while !ancestor.exists() {
        missing.try_reserve(1).map_err(|e| {
            Error::new(
                "AllocationFailed",
                &out.to_string_lossy(),
                0,
                "output parents",
                e,
            )
        })?;
        missing.push(ancestor.to_owned());
        ancestor = ancestor.parent().ok_or_else(usage)?;
    }
    let mut guard = StagedReports {
        path: None,
        parents: Vec::new(),
        keep: false,
    };
    let attempt = (|| {
        for directory in missing.into_iter().rev() {
            verify(&directory)?;
            guard.parents.try_reserve(1).map_err(|e| {
                Error::new(
                    "AllocationFailed",
                    &out.to_string_lossy(),
                    0,
                    "created parents",
                    e,
                )
            })?;
            fs::create_dir(&directory).map_err(|e| io_error(&directory.to_string_lossy(), 0, e))?;
            guard.parents.push(directory);
            verify(guard.parents.last().ok_or_else(usage)?)?;
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = parent.join(format!(
            ".fo4-report-stage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        verify(&path)?;
        fs::create_dir(&path).map_err(|e| io_error(&path.to_string_lossy(), 0, e))?;
        guard.path = Some(path.clone());
        verify(&path)?;
        let summary = open(&path.join("summary.json"))?;
        verify(&path)?;
        let diagnostics = open(&path.join("diagnostics.jsonl"))?;
        verify(&path)?;
        let items = open(&path.join("items.jsonl"))?;
        verify(&path)?;
        verify(out)?;
        fs::rename(&path, out).map_err(|e| io_error(&out.to_string_lossy(), 0, e))?;
        // Keep the rollback guard armed until the final identity check succeeds.
        guard.path = Some(out.to_owned());
        verify(out)?;
        Ok([
            BufWriter::new(summary),
            BufWriter::new(diagnostics),
            BufWriter::new(items),
        ])
    })();
    match attempt {
        Ok(files) => {
            guard.keep = true;
            Ok(files)
        }
        Err(error) => {
            guard.rollback()?;
            Err(error)
        }
    }
}

struct Reports {
    items_enabled: bool,
    /// Optional text report written beside summary.json (collision census).
    tsv: Option<String>,
    out: PathBuf,
    groups: BTreeMap<(String, String, Option<String>, bool), DiagnosticGroup>,
    output: OutputBudget,
    summary_file: BufWriter<File>,
    diagnostics: BufWriter<File>,
    items: BufWriter<File>,
    summary: Summary,
}
impl Reports {
    fn new(o: &Options, m: &Manifest) -> Result<Self> {
        let root = m
            .root
            .canonicalize()
            .map_err(|e| io_error(&m.root.to_string_lossy(), 0, e))?;
        let out = safe_out(&o.out, &root)?;
        if m.limits.max_report_bytes < MIN_REPORT_BYTES {
            return Err(Error::new(
                "ReportBudgetTooSmall",
                "manifest",
                0,
                "max_report_bytes >= 8388608 (includes reserved census summary)",
                m.limits.max_report_bytes,
            ));
        }
        let [summary_file, diagnostics, items] = publish_reports(
            &out,
            |p| {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(p)
                    .map_err(|e| io_error(&p.to_string_lossy(), 0, e))
            },
            |p| check_output_identity(p, &root),
        )?;
        Ok(Self {
            tsv: None,
            out: out.clone(),
            items_enabled: o.items,
            groups: BTreeMap::new(),
            output: OutputBudget {
                max: m.limits.max_report_bytes,
                used: 0,
                exhausted: false,
            },
            summary_file,
            diagnostics,
            items,
            summary: Summary {
                game: m.game,
                schema: 3,
                max_report_bytes: m.limits.max_report_bytes,
                archive_invalidation: m.archive_invalidation.clone(),
                under_hypotheses: o.level == fo4_scene::RuleLevel::Hypotheses,
                rule_level: None,
                overlay_plugin: o.overlay_plugin.clone(),
                command: o.command.clone(),
                scope: o.scope.clone(),
                installation_scope: "explicit manifest subset; no game corpus certification".into(),
                language: m.language.clone(),
                build: m.build.clone(),
                ..Summary::default()
            },
        })
    }
    fn diagnostic(
        &mut self,
        e: &Error,
        notice: bool,
        sample: Option<serde_json::Value>,
    ) -> Result<()> {
        self.summary.diagnostics += 1;
        if !notice {
            self.summary.failures += 1;
            if let Some(count) = self.summary.failure_codes.get_mut(&e.code) {
                *count += 1;
            } else if self.summary.failure_codes.len() < 4096 && e.code.len() <= 128 {
                self.summary.failure_codes.insert(e.code.clone(), 1);
            } else {
                self.summary.failures_without_code += 1;
            }
        }
        let key = (
            e.code.clone(),
            e.source.file.clone(),
            e.source.archive_entry.clone(),
            notice,
        );
        if let Some(group) = self.groups.get_mut(&key) {
            group.count += 1;
            group.last_offset = e.source.offset;
            return Ok(());
        }
        if self.groups.len() >= 4096 {
            self.summary.diagnostic_groups_omitted += 1;
            return Ok(());
        }
        self.groups.insert(
            key,
            DiagnosticGroup {
                error: e.clone(),
                count: 1,
                last_offset: e.source.offset,
                notice,
                sample,
            },
        );
        Ok(())
    }
    fn error(&mut self, e: &Error) -> Result<()> {
        self.diagnostic(e, false, None)
    }
    fn notice(&mut self, e: &Error) -> Result<()> {
        self.diagnostic(e, true, None)
    }
    fn item(&mut self, item: &impl Serialize) -> Result<()> {
        if !self.items_enabled {
            return Ok(());
        }
        if self.output.exhausted {
            self.summary.items_omitted += 1;
            return Ok(());
        }
        match write_bounded(
            &mut self.items,
            item,
            "items.jsonl",
            &mut self.output,
            false,
        ) {
            Err(e) if e.code == "ReportLimitExceeded" => {
                self.summary.items_omitted += 1;
                Ok(())
            }
            result => result,
        }
    }
    fn numeric(&mut self, attributes: &[(&str, u64)], file: bool) -> Result<()> {
        let count: u64 = attributes.iter().map(|(_, n)| n).sum();
        if count == 0 {
            return Ok(());
        }
        self.summary.non_finite_values =
            fo4_core::add(self.summary.non_finite_values, count, "summary", 0)?;
        if file {
            self.summary.files_with_non_finite += 1;
        }
        for (name, count) in attributes {
            if *count > 0 {
                let n = self
                    .summary
                    .non_finite_by_attribute
                    .entry((*name).into())
                    .or_default();
                *n = fo4_core::add(*n, *count, "summary", 0)?;
            }
        }
        Ok(())
    }
    fn reference_notice(&mut self, kind: &str, path: &[u8], count: u64, code: &str) -> Result<()> {
        let budget = Budget::new(1 << 20);
        let display = fo4_core::decode_cp1252(path, "VFS reference", 0, &budget);
        let error = Error::new(
            code,
            "VFS reference",
            0,
            "referenced path has a supported mounted winner",
            fo4_core::HexBytes(path),
        );
        let row = serde_json::json!({"notice":true,"code":error.code,"source":error.source,"expected":error.expected,"found":error.found,"reference_kind":kind,"path_hex":fo4_core::HexBytes(path).to_string(),"display_cp1252":display.as_ref().ok(),"display_error":display.as_ref().err().map(|e|&e.code),"count":count});
        self.summary.diagnostics =
            fo4_core::add(self.summary.diagnostics, count, "diagnostics", 0)?;
        match write_bounded(
            &mut self.diagnostics,
            &row,
            "diagnostics.jsonl",
            &mut self.output,
            false,
        ) {
            Err(e) if e.code == "ReportLimitExceeded" => {
                self.summary.diagnostic_groups_omitted += 1;
                Ok(())
            }
            result => result,
        }
    }
    fn hist(&mut self, key: String) -> Result<()> {
        if !self.summary.histograms.contains_key(&key) && self.summary.histograms.len() >= 4096 {
            return Err(Error::new(
                "LimitExceeded",
                "histograms",
                0,
                "<=4096 distinct buckets",
                self.summary.histograms.len() + 1,
            ));
        }
        *self.summary.histograms.entry(key).or_default() += 1;
        Ok(())
    }
    fn string_diagnostic(
        &mut self,
        table: &fo4_plugin::StringTable,
        d: &fo4_plugin::StringDiagnostic,
    ) -> Result<()> {
        if !d.mark_reported() {
            return Ok(());
        }
        let key = (
            d.error.code.clone(),
            d.error.source.file.clone(),
            d.error.source.archive_entry.clone(),
            false,
        );
        let sample = if self.groups.contains_key(&key) {
            None
        } else {
            Some(
                serde_json::json!({"language":table.language,"id":d.id,"offset":d.offset,"length":d.length,"raw":&table.bytes[d.raw.clone()]}),
            )
        };
        self.diagnostic(&d.error, false, sample)
    }
    fn finish(mut self) -> Result<u8> {
        for group in self.groups.values() {
            if let Err(error) = write_bounded(
                &mut self.diagnostics,
                group,
                "diagnostics.jsonl",
                &mut self.output,
                false,
            ) {
                if error.code != "ReportLimitExceeded" {
                    return Err(error);
                }
                self.summary.diagnostic_groups_omitted += 1;
            }
        }
        if self.output.exhausted {
            self.summary.report_limit_exceeded = true;
            self.summary.failures += 1;
            self.summary.diagnostics += 1;
            let error = Error::new(
                "ReportLimitExceeded",
                "reports",
                self.output.used,
                format!("<= {} total bytes; census retained", self.output.max),
                "optional items or diagnostic samples omitted; enumeration continued",
            );
            write_bounded(
                &mut self.diagnostics,
                &error,
                "diagnostics.jsonl",
                &mut self.output,
                true,
            )?;
        }
        self.summary.status = if self.summary.failures > 0 { 1 } else { 0 };
        // Summary retains the complete census, including on optional-output exhaustion.
        let mut total = self.output.used;
        for _ in 0..8 {
            self.summary.report_bytes = total;
            let size = json_size(&self.summary, self.output.max - self.output.used)?;
            let next = self.output.used + size;
            if total == next {
                break;
            }
            total = next;
        }
        write_bounded(
            &mut self.summary_file,
            &self.summary,
            "summary.json",
            &mut self.output,
            true,
        )?;
        if let Some(text) = self.tsv.take() {
            // The directory was created and identity-checked by publish_reports.
            let path = self.out.join("collision.tsv");
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| io_error("collision.tsv", 0, e))?;
            file.write_all(text.as_bytes())
                .map_err(|e| io_error("collision.tsv", 0, e))?;
        }
        for (file, name) in [
            (&mut self.items, "items.jsonl"),
            (&mut self.diagnostics, "diagnostics.jsonl"),
            (&mut self.summary_file, "summary.json"),
        ] {
            file.flush().map_err(|e| io_error(name, 0, e))?;
        }
        Ok(self.summary.status)
    }
}
fn safe_out(out: &Path, root: &Path) -> Result<PathBuf> {
    let resolved = resolve_output_path(out)?;
    check_output_identity(&resolved, root)?;
    Ok(resolved)
}
fn resolve_output_path(out: &Path) -> Result<PathBuf> {
    // Resolve existing ancestors before creating anything; prohibit dot traversal.
    if out.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Error::new(
            "UnsafeReportPath",
            &out.to_string_lossy(),
            0,
            "output without ..",
            "parent traversal",
        ));
    }
    let absolute = if out.is_absolute() {
        out.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| io_error("cwd", 0, e))?
            .join(out)
    };
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                suffix.try_reserve(1).map_err(|e| {
                    Error::new(
                        "AllocationFailed",
                        &out.to_string_lossy(),
                        0,
                        "output suffix",
                        e,
                    )
                })?;
                suffix.push(ancestor.file_name().ok_or_else(usage)?.to_os_string());
                ancestor = ancestor.parent().ok_or_else(usage)?;
            }
            Err(e) => return Err(io_error(&ancestor.to_string_lossy(), 0, e)),
        }
    }
    let mut resolved = ancestor
        .canonicalize()
        .map_err(|e| io_error(&ancestor.to_string_lossy(), 0, e))?;
    for s in suffix.iter().rev() {
        resolved.push(s);
    }
    Ok(resolved)
}
fn check_output_identity(out: &Path, root: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let data = fs::metadata(root).map_err(|e| io_error(&root.to_string_lossy(), 0, e))?;
        let resolved = resolve_output_path(out)?;
        for ancestor in resolved.ancestors() {
            let metadata = match fs::metadata(ancestor) {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io_error(&ancestor.to_string_lossy(), 0, e)),
            };
            if (metadata.dev(), metadata.ino()) == (data.dev(), data.ino()) {
                return Err(Error::new(
                    "OutputInsideData",
                    &out.to_string_lossy(),
                    0,
                    "different directory identity from Data and descendants",
                    root.display(),
                ));
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        // No stable device/inode pair here: compare canonical paths instead.
        let data = fs::canonicalize(root).map_err(|e| io_error(&root.to_string_lossy(), 0, e))?;
        let resolved = resolve_output_path(out)?;
        for ancestor in resolved.ancestors() {
            let canonical = match fs::canonicalize(ancestor) {
                Ok(path) => path,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io_error(&ancestor.to_string_lossy(), 0, e)),
            };
            if canonical == data {
                return Err(Error::new(
                    "OutputInsideData",
                    &out.to_string_lossy(),
                    0,
                    "different directory from Data and descendants",
                    root.display(),
                ));
            }
        }
        Ok(())
    }
}
pub fn run(o: &Options) -> Result<u8> {
    run_selected(o, None)
}
/// Reinspect an explicit subset without presenting it as a complete base census.
pub fn run_base_subset(o: &Options, keys: &[fo4_plugin::FormKey]) -> Result<u8> {
    if o.command != "sweep" || o.scope.as_deref() != Some("bases") || keys.is_empty() {
        return Err(usage());
    }
    run_selected(o, Some(keys))
}
fn run_selected(o: &Options, keys: Option<&[fo4_plugin::FormKey]>) -> Result<u8> {
    let m = Manifest::load(&o.manifest)?;
    let mut r = Reports::new(o, &m)?;
    if m.build.is_none() {
        r.notice(&Error::new(
            "UnidentifiedBuild",
            &o.manifest.to_string_lossy(),
            0,
            "explicit build identity",
            "unknown; examination by format only",
        ))?;
    }
    let operation = match o.command.as_str() {
        "inventory" => inventory(&m, &mut r),
        "resolve" => resolve(&m, o, &mut r),
        "check-cell" => check_cell::run(
            &m,
            o.path.as_deref().ok_or_else(usage)?,
            &mut r,
            o.level,
            o.overlay_plugin.as_deref(),
        ),
        "sweep" => match o.scope.as_deref() {
            Some("archives") => archives(&m, &mut r),
            Some("records") => records(&m, &mut r),
            Some("roots") => root_census::sweep(&m, &mut r),
            Some("collision") => collision::sweep(&m, &mut r),
            Some("cells") => cell_census::cells(&m, &mut r),
            Some("bases") => bases::sweep(&m, &mut r, keys),
            Some("shader-citations") => families::sweep(&m, &mut r),
            Some("unread") => cell_census::unread(&m, &mut r, false),
            Some("paths") => cell_census::unread(&m, &mut r, true),
            Some(scope @ ("nif" | "materials" | "textures")) => assets::sweep(&m, &mut r, scope),
            _ => return Err(usage()),
        },
        _ => return Err(usage()),
    };
    if let Err(e) = operation {
        if e.code == "ReportLimitExceeded" {
            r.output.exhausted = true;
        } else if let Err(output_error) = r.error(&e) {
            if output_error.code != "ReportLimitExceeded" {
                return Err(output_error);
            }
            r.output.exhausted = true;
        }
    }
    r.finish()
}
fn inventory(m: &Manifest, r: &mut Reports) -> Result<()> {
    for required in &m.required_masters {
        if !m
            .plugins
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(required))
        {
            r.error(&Error::new(
                "MissingMaster",
                "manifest",
                0,
                required,
                "not declared",
            ))?;
        }
    }
    for excluded in &m.excluded {
        r.item(&serde_json::json!({"kind":"excluded","path":excluded}))?;
    }
    let paths = m
        .plugins
        .iter()
        .map(|p| p.path.clone())
        .chain(m.mounts.iter().flat_map(|mount| {
            match mount {
                Mount::Archive { path, .. } => vec![path.clone()],
                Mount::Loose { base, files, .. } => files
                    .iter()
                    .map(|p| {
                        base.as_ref()
                            .map_or_else(|| p.clone(), |base| format!("{base}/{p}"))
                    })
                    .collect(),
            }
        }));
    for path in paths {
        let path = path.as_str();
        r.summary.files_enumerated += 1;
        r.summary.files_attempted += 1;
        let attempt = (|| {
            let f = FileSource::open(&within(&m.root, path)?)?;
            let hash = digest(&f)?;
            r.item(&serde_json::json!({"kind":"file","path":path,"size":f.len(),"sha256":hash}))?;
            if let Some(plugin) = m.plugins.iter().find(|p| p.path == path) {
                let h = Header::read_for_game(
                    m.game,
                    &plugin.name,
                    &f,
                    &m.limits,
                    &Budget::new(m.limits.max_memory),
                )?;
                r.item(&h)?;
            } else if m
                .mounts
                .iter()
                .any(|mount| matches!(mount,Mount::Archive{path:p,..} if p==path))
            {
                let a =
                    GameArchive::read(m.game, &f, &m.limits, &Budget::new(m.limits.max_memory))?;
                r.item(&serde_json::json!({"kind":"archive_header","path":path,"version":a.version(),"tag":a.kind(),"entries":a.len()}))?;
            }
            Ok(())
        })();
        match attempt {
            Ok(()) => r.summary.files_valid += 1,
            Err(e) => {
                r.summary.files_failed += 1;
                r.error(&e)?;
            }
        }
    }
    Ok(())
}
fn archives(m: &Manifest, r: &mut Reports) -> Result<()> {
    for mount in &m.mounts {
        if let Mount::Archive { path, .. } = mount {
            r.summary.files_enumerated += 1;
            r.summary.files_attempted += 1;
            let failures_before = r.summary.failures;
            let attempt = (|| {
                let f = FileSource::open(&within(&m.root, path)?)?;
                let archive_budget = Budget::new(m.limits.max_memory);
                let a = GameArchive::read(m.game, &f, &m.limits, &archive_budget)?;
                r.summary.archives += 1;
                r.summary.uninterpreted_fields += a.uninterpreted_fields();
                r.hist(format!("{}.version.{}", a.format(), a.version()))?;
                r.hist(format!(
                    "{}.tag.{}",
                    a.format(),
                    String::from_utf8_lossy(&a.kind())
                ))?;
                r.summary.entries_enumerated += a.len() as u64;
                for i in 0..a.len() {
                    let e = a.entry(i)?;
                    r.summary.entries_attempted += 1;
                    r.summary.chunks_enumerated += e.chunks as u64;
                    let mut valid = e.diagnostics.is_empty();
                    for diagnostic in e.diagnostics {
                        r.error(diagnostic)?;
                    }
                    if let Some(t) = &e.texture {
                        r.hist(format!("dxgi.{}", t.format))?;
                    }
                    for j in 0..e.chunks {
                        r.summary.chunks_attempted += 1;
                        match a.decode_chunk(&f, i, j, &m.limits, &archive_budget.child()) {
                            Ok(bytes) => {
                                r.summary.chunks_valid += 1;
                                r.summary.payload_bytes += bytes.len() as u64;
                            }
                            Err(err) => {
                                valid = false;
                                r.summary.chunks_failed += 1;
                                r.error(&err)?;
                            }
                        }
                        r.hist(format!("codec.{}", a.codec(i, j)?))?;
                    }
                    if let Some(t) = &e.texture {
                        if e.diagnostics.is_empty() {
                            if let Err(err) = t.dds_header(f.name(), e.source.offset) {
                                valid = false;
                                r.error(&err)?;
                            }
                        }
                    }
                    if valid {
                        r.summary.entries_valid += 1;
                    } else {
                        r.summary.entries_failed += 1;
                    }
                    if r.items_enabled {
                        r.item(&serde_json::json!({"kind":"entry","archive":path,"entry":e.name,"name_encoding":e.name_encoding,"name_hex":fo4_core::HexBytes(e.original_name).to_string(),"chunks":e.chunks,"valid":valid}))?;
                    }
                }
                Ok(())
            })();
            match attempt {
                Ok(()) => {
                    if r.summary.failures > failures_before {
                        r.summary.files_failed += 1;
                    } else {
                        r.summary.files_valid += 1;
                    }
                }
                Err(e) => {
                    r.summary.files_failed += 1;
                    r.summary.non_enumerable_regions += 1;
                    r.error(&e)?;
                }
            }
        }
    }
    Ok(())
}
fn resolve(m: &Manifest, o: &Options, r: &mut Reports) -> Result<()> {
    let v = match Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory)) {
        Ok(v) => v,
        Err(e) => {
            r.error(&e)?;
            r.summary.non_enumerable_regions += 1;
            return Ok(());
        }
    };
    for diagnostic in &v.diagnostics {
        r.error(diagnostic)?;
    }
    r.summary.fingerprint = Some(v.fingerprint()?.to_owned());
    r.summary.fingerprint_scope = Some("assets".into());
    r.summary.inactive_mounts = v.inactive_mounts.len() as u64;
    if let Err(error) = v.check_complete() {
        r.error(&error)?;
        for c in &v.candidates {
            r.item(&serde_json::json!({"kind":"vfs","candidate":c,"winner":false,"resolution":"incomplete"}))?;
        }
        return Ok(());
    }
    let candidates = if let Some(p) = &o.path {
        match v.chain(p) {
            Ok([]) => {
                r.error(&Error::new("MissingPath", p, 0, "mounted key", "absent"))?;
                return Ok(());
            }
            Ok(chain) => chain,
            Err(e) => {
                r.error(&e)?;
                return Ok(());
            }
        }
    } else {
        v.candidates.as_slice()
    };
    let mut pos = 0;
    while pos < candidates.len() {
        let Some(key) = candidates[pos].key.as_deref() else {
            r.summary.vfs_candidates += 1;
            r.item(&serde_json::json!({"kind":"unkeyed_vfs","candidate":candidates[pos],"winner":false}))?;
            pos += 1;
            continue;
        };
        let end = pos + candidates[pos..].partition_point(|c| c.key.as_deref() == Some(key));
        r.summary.vfs_keys += 1;
        let resolved = match v.winner_bytes(key) {
            Ok(_) => true,
            Err(error) => {
                r.error(&error)?;
                false
            }
        };
        for (index, c) in candidates[pos..end].iter().enumerate() {
            r.summary.vfs_candidates += 1;
            let hash = if c.blocked || !r.items_enabled {
                None
            } else {
                match v.hash(c) {
                    Ok(hash) => Some(hash),
                    Err(error) => {
                        r.error(&error)?;
                        None
                    }
                }
            };
            if r.items_enabled {
                r.item(&serde_json::json!({"kind":"vfs","candidate":c,"original_name_hex":fo4_core::HexBytes(v.original_bytes(c)?).to_string(),"sha256":hash,"winner":resolved && index+1==end-pos,"selected":index+1==end-pos}))?;
            }
        }
        pos = end;
    }

    Ok(())
}
fn records(m: &Manifest, r: &mut Reports) -> Result<()> {
    let b = Budget::new(m.limits.max_memory);
    let vfs_budget = Budget::new(m.limits.max_vfs_memory);
    let overrides_budget = Budget::new(m.limits.max_override_memory);
    let mut string_cache = StringCache::new(&b);
    let mut headers = Vec::new();
    let mut sources = Vec::new();
    b.reserve(&mut headers, m.plugins.len(), "manifest", 0)?;
    b.reserve(&mut sources, m.plugins.len(), "manifest", 0)?;
    for p in &m.plugins {
        r.summary.files_enumerated += 1;
        r.summary.files_attempted += 1;
        let attempt = (|| {
            let source = FileSource::open(&within(&m.root, &p.path)?)?;
            let h = Header::read_for_game(m.game, &p.name, &source, &m.limits, &b)?;
            Ok((source, h))
        })();
        match attempt {
            Ok((s, h)) => {
                sources.push(s);
                headers.push(h);
            }
            Err(e) => {
                r.summary.files_failed += 1;
                r.summary.non_enumerable_regions += 1;
                r.error(&e)?;
            }
        }
    }
    if headers.len() != m.plugins.len() {
        r.summary.files_unavailable = m.plugins.len() as u64 - headers.len() as u64;
        r.summary.files_blocked = headers.len() as u64;
        r.notice(&Error::new(
            "UnavailableLoadOrder",
            "manifest",
            0,
            "all plugin headers readable",
            "not all headers available",
        ))?;
        return Ok(());
    }
    let order = match LoadOrder::new(headers, &b) {
        Ok(o) => o,
        Err(e) => {
            r.summary.files_blocked = m.plugins.len() as u64;
            r.error(&e)?;
            return Ok(());
        }
    };
    let mut plugin_hashes = Vec::new();
    b.reserve(&mut plugin_hashes, sources.len(), "plugin hashes", 0)?;
    for (h, source) in order.headers().iter().zip(&sources) {
        b.charge(h.name.len() as u64 + 64, source.name(), 0)?;
        plugin_hashes.push((h.name.clone(), digest(source)?));
    }
    let vfs = match Vfs::mount(m, &vfs_budget) {
        Ok(v) => Some(v),
        Err(e) => {
            r.error(&e)?;
            None
        }
    };
    if let Some(v) = &vfs {
        r.summary.fingerprint = Some(v.records_fingerprint(&plugin_hashes, &b)?);
        r.summary.fingerprint_scope = Some("assets+plugins".into());
        for diagnostic in &v.diagnostics {
            r.error(diagnostic)?;
        }
    }
    for h in order.headers() {
        for diagnostic in &h.diagnostics {
            r.notice(diagnostic)?;
        }
    }
    let mut overrides = Overrides::new();
    for (i, source) in sources.iter().enumerate() {
        let h = &order.headers()[i];
        r.summary.header_uninterpreted_subrecords += h.uninterpreted_subrecords;
        let mut last_group_offset = None;
        let groups_before = r.summary.groups;
        let failures_before = r.summary.failures;
        let mut index_complete = true;
        let records_before = r.summary.records;
        let subrecords_before = r.summary.subrecords;
        let overrides_before = overrides.history.len();
        let numeric_before = r.summary.non_finite_values;
        let numeric_records_before = r.summary.records_with_non_finite;
        let scanned = scan_bounded(source, h, &m.limits, &b, |record, groups, record_budget| {
            r.summary.records += 1;
            r.hist(format!("record.form_version.{}", record.version))?;
            r.summary.subrecords += record.subrecords.len() as u64;
            for subrecord in &record.subrecords {
                r.hist(format!(
                    "subrecord.{}",
                    String::from_utf8_lossy(&subrecord.signature)
                ))?;
            }
            let interpreted = m.game == fo4_core::Game::Fallout4
                && fo4_plugin::INTERPRETED_TYPES.contains(&record.signature);
            let reads_full = interpreted || m.game == fo4_core::Game::SkyrimSe;
            if !interpreted {
                r.summary.envelope_records += 1;
            }
            r.hist(format!(
                "record.{}",
                String::from_utf8_lossy(&record.signature)
            ))?;

            if let Err(e) = overrides.insert(&order, i, &record, &overrides_budget) {
                index_complete = false;
                r.error(&e)?;
            }
            if record.deleted() {
                r.summary.deleted += 1;
            }
            if record.disabled() {
                r.summary.disabled += 1;
            }
            let fields = match record.fields_for_game(m.game, &m.limits, record_budget) {
                Ok(fields) => fields,
                Err(e) => {
                    r.summary.failed_fields += record.subrecords.len() as u64;
                    r.error(&e)?;
                    Vec::new()
                }
            };
            let non_finite: u64 = fields.iter().map(|f| f.non_finite_values).sum();
            if non_finite > 0 {
                r.summary.records_with_non_finite += 1;
                r.numeric(&[("plugin_fields", non_finite)], false)?;
                let mut notice = Error::new(
                    "NonFiniteRecordValues",
                    source.name(),
                    record.source.offset,
                    "IEEE values preserved; consumer decides",
                    format!("record {:08X}: {non_finite}", record.form_id),
                );
                *notice.source = record.source.clone();
                r.notice(&notice)?;
            }
            for field in &fields {
                if field.signature == *b"FULL" && reads_full {
                    continue;
                }
                match field.value {
                    Value::Raw => r.summary.uninterpreted_fields += 1,
                    Value::LightingPrefix { .. } => {
                        r.summary.partially_interpreted_fields += 1;
                        r.summary.uninterpreted_fields += 1;
                    }
                    Value::FormId(id) | Value::EnableParent { id, .. } => {
                        r.summary.interpreted_fields += 1;
                        r.summary.references += 1;
                        let field_offset = record.subrecords[field.subrecord].offset;
                        match order.remap(i,id,source.name(),field_offset){
                            Ok(key)=>r.item(&serde_json::json!({"kind":"reference","plugin":h.name,"record":record.form_id,"field":field.signature,"key":key,"source":record.source,"field_offset":field_offset,"decompressed_offsets":record.payload_is_decompressed}))?,
                            Err(mut e)=>{if record.payload_is_decompressed{e.source.decompressed_offset=Some(field_offset);e.source.offset=record.source.offset+24;}r.error(&e)?;}
                        }
                    }
                    _ => r.summary.interpreted_fields += 1,
                }
            }
            for g in groups {
                if last_group_offset.is_none_or(|offset| g.source.offset > offset) {
                    r.summary.groups += 1;
                    if g.kind > 10 {
                        r.summary.unknown_groups += 1;
                    }
                    last_group_offset = Some(g.source.offset);
                    r.item(&serde_json::json!({"kind":"group","group":g}))?;
                }
            }
            match if reads_full && !fields.is_empty() {
                record.full_name(h.flags & LOCALIZED != 0, &m.limits, record_budget)
            } else {
                Ok(None)
            } {
                Ok(Some(Value::Localized(id))) => {
                    if let Some(v) = &vfs {
                        match string_cache.resolve(v, &h.name, &m.language, StringKind::Strings, id)
                        {
                            Ok(text) => {
                                r.summary.interpreted_fields += 1;
                                r.summary.strings_resolved += 1;
                                r.item(&serde_json::json!({"kind":"localized","plugin":h.name,"record":record.form_id,"id":id,"text":text}))?;
                            }
                            Err(e) => {
                                r.summary.uninterpreted_fields += 1;
                                if let Some((table, d)) = string_cache.diagnostic(
                                    &h.name,
                                    &m.language,
                                    StringKind::Strings,
                                    id,
                                ) {
                                    r.string_diagnostic(table, d)?;
                                } else {
                                    r.error(&e)?;
                                }
                                r.item(&serde_json::json!({"kind":"unresolved_string","plugin":h.name,"language":m.language,"id":id}))?;
                            }
                        }
                    } else {
                        r.summary.uninterpreted_fields += 1;
                        r.error(&Error::new(
                            "MissingStringTable",
                            source.name(),
                            record.source.offset,
                            "available VFS",
                            id,
                        ))?;
                    }
                }
                Ok(Some(Value::Text(_))) => r.summary.interpreted_fields += 1,
                Err(e) => {
                    r.summary.failed_fields += 1;
                    r.error(&e)?;
                }
                _ => {}
            }
            let parent = groups
                .iter()
                .rev()
                .find(|g| matches!(g.kind, 6 | 8 | 9 | 10))
                .map(|g| (g.kind, u32::from_le_bytes(g.label)));
            if let Some((_, id)) = parent {
                if let Err(e) = order.remap(i, id, source.name(), record.source.offset) {
                    r.error(&e)?;
                }
            }
            #[derive(Serialize)]
            struct RecordItem<'a> {
                kind: &'static str,
                plugin: &'a str,
                source: &'a fo4_core::SourceSpan,
                signature: [u8; 4],
                form_id: u32,
                flags: u32,
                revision: u32,
                version: u16,
                unknown: u16,
                decompressed_offsets: bool,
                fields: &'a [fo4_plugin::Field],
                subrecords: &'a [fo4_plugin::Subrecord],
                parent: Option<(u32, u32)>,
            }
            r.item(&RecordItem {
                kind: "record",
                plugin: &h.name,
                source: &record.source,
                signature: record.signature,
                form_id: record.form_id,
                flags: record.flags,
                revision: record.revision,
                version: record.version,
                unknown: record.unknown,
                decompressed_offsets: record.payload_is_decompressed,
                fields: &fields,
                subrecords: &record.subrecords,
                parent,
            })?;
            Ok(())
        });
        let complete = scanned.is_ok();
        match scanned {
            Ok(stats) => {
                if index_complete {
                    overrides.mark_complete(i, &order, &overrides_budget)?;
                }
                // Includes empty groups, which do not appear in the record visitor.
                r.summary.groups = groups_before + stats.groups;
                if !stats.count_matches {
                    r.summary.divergences += 1;
                    r.notice(&Error::new(
                        "HeaderRecordCountMismatch",
                        source.name(),
                        h.hedr_offset + 10,
                        h.declared_records,
                        stats.groups + stats.records,
                    ))?;
                }
                if r.summary.failures > failures_before {
                    r.summary.files_failed += 1;
                } else {
                    r.summary.files_valid += 1;
                }
            }
            Err(e) => {
                r.summary.files_failed += 1;
                r.summary.non_enumerable_regions += 1;
                r.error(&e)?;
            }
        }
        let history = &overrides.history[overrides_before..];
        let override_records = history.iter().filter(|v| v.previous.is_some()).count() as u64;
        let injected_records = history
            .iter()
            .filter(|v| v.injected && v.previous.is_none())
            .count() as u64;
        if r.summary.non_finite_values > numeric_before {
            r.summary.files_with_non_finite += 1;
        }
        r.summary.records_by_plugin.insert(
            h.name.clone(),
            PluginCounts {
                non_finite_values: r.summary.non_finite_values - numeric_before,
                records_with_non_finite: r.summary.records_with_non_finite - numeric_records_before,
                records: r.summary.records - records_before,
                groups: r.summary.groups - groups_before,
                subrecords: r.summary.subrecords - subrecords_before,
                complete,
                override_records,
                injected_records,
            },
        );
    }
    let mut counted = std::collections::BTreeSet::new();
    for table in string_cache.tables() {
        counted.insert(table.file.to_ascii_lowercase().replace('\\', "/"));
        count_string_table(r, table, &m.limits)?;
    }
    r.summary.string_tables_loaded = string_cache.loads;
    // Census all mounted localization tables belonging to explicit active plugins,
    // all observed languages and all three formats; one temporary table at a time.
    if let Some(v) = &vfs {
        if v.check_complete().is_ok() {
            for (index, candidate) in v.candidates.iter().enumerate() {
                let Some(key) = candidate.key.as_deref() else {
                    continue;
                };
                if v.candidates
                    .get(index + 1)
                    .is_some_and(|next| next.key.as_deref() == Some(key))
                {
                    continue;
                }
                let Ok(path) = std::str::from_utf8(key) else {
                    continue;
                };
                let Some((head, extension)) = path.rsplit_once('.') else {
                    continue;
                };
                let kind = match extension {
                    "strings" => StringKind::Strings,
                    "dlstrings" => StringKind::DlStrings,
                    "ilstrings" => StringKind::IlStrings,
                    _ => continue,
                };
                let mut language = None;
                for plugin in &m.plugins {
                    let stem = plugin
                        .name
                        .rsplit_once('.')
                        .map_or(plugin.name.as_str(), |x| x.0);
                    let prefix = format!("strings/{}_", stem.to_ascii_lowercase());
                    if let Some(token) = head.strip_prefix(&prefix) {
                        language = Some(token);
                        break;
                    }
                }
                let Some(language) = language else {
                    continue;
                };
                if counted.contains(path) {
                    continue;
                }
                if language.is_empty()
                    || language.len() > 64
                    || !language
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    r.error(&Error::new(
                        "UnsupportedLocalizationName",
                        path,
                        0,
                        "language token <=64 ASCII bytes",
                        language,
                    ))?;
                    continue;
                }
                let temporary = b.child();
                r.summary.string_tables_loaded += 1;
                match v.read_path_bytes(key, &temporary).and_then(|bytes| {
                    fo4_plugin::StringTable::read_for_language(
                        path, kind, bytes, language, &m.limits, &temporary,
                    )
                }) {
                    Ok(table) => count_string_table(r, &table, &m.limits)?,
                    Err(error) => r.error(&error)?,
                }
            }
        }
    }
    r.summary.override_memory = overrides_budget.used();
    r.summary.vfs_memory = vfs_budget.used();
    r.summary.other_memory = b.used();
    r.summary.injected_records = overrides
        .history
        .iter()
        .filter(|r| r.injected && r.previous.is_none())
        .count() as u64;
    r.summary.override_records = overrides
        .history
        .iter()
        .filter(|v| v.previous.is_some())
        .count() as u64;
    r.summary.unique_forms = overrides.history.len() as u64 - r.summary.override_records;
    if r.items_enabled {
        for item in &overrides.history {
            r.item(&serde_json::json!({"kind":"override","record":item,"form_key":overrides.form_key(item.key,&order),"source":fo4_core::SourceSpan::new(&order.headers()[item.plugin].source.file,item.offset,item.length),"winner":overrides.winner_id(item.key).is_some_and(|w|w.plugin==item.plugin),"runtime_id":order.runtime_id(&overrides.form_key(item.key,&order))?}))?;
        }
    }
    Ok(())
}

fn count_string_table(
    r: &mut Reports,
    table: &fo4_plugin::StringTable,
    limits: &fo4_core::Limits,
) -> Result<()> {
    let decoded = table
        .entries
        .iter()
        .filter(|(id, _)| table.get(*id, limits).is_ok())
        .count() as u64;
    let undecoded = table.entries.len() as u64 - decoded;
    r.summary.string_entries_enumerated += table.entries.len() as u64;
    r.summary.string_entries_decoded += decoded;
    r.summary.string_entries_undecoded += undecoded;
    if !r.summary.strings_by_language.contains_key(&table.language)
        && r.summary.strings_by_language.len() >= 4096
    {
        r.summary.language_entries_without_breakdown += table.entries.len() as u64;
        r.error(&Error::new(
            "LanguageSummaryLimitExceeded",
            &table.file,
            0,
            "<=4096 language buckets; global entry counts retained",
            &table.language,
        ))?;
        return Ok(());
    }
    let counts = r
        .summary
        .strings_by_language
        .entry(table.language.clone())
        .or_default();
    counts.tables += 1;
    counts.decoded += decoded;
    counts.undecoded += undecoded;
    counts.bom_prefixed += table.bom_prefixed;
    counts.english_utf8_multibyte += table.english_utf8_multibyte;
    for diagnostic in &table.diagnostics {
        r.string_diagnostic(table, diagnostic)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_of_second_or_third_create_publishes_nothing() {
        let base = fo4_core::TestDir::new("atomic-report");
        for fail_at in [2, 3] {
            let out = base.join(format!("out-{fail_at}"));
            let mut calls = 0;
            let result = publish_reports(
                &out,
                |path| {
                    calls += 1;
                    if calls == fail_at {
                        return Err(Error::new(
                            "InjectedIo",
                            &path.to_string_lossy(),
                            0,
                            "created file",
                            "test failure",
                        ));
                    }
                    fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(path)
                        .map_err(|e| io_error(&path.to_string_lossy(), 0, e))
                },
                |_| Ok(()),
            );
            assert_eq!(result.err().unwrap().code, "InjectedIo");
            assert!(!out.exists());
            assert_eq!(fs::read_dir(base.to_path_buf()).unwrap().count(), 0);
        }
        let out = base.join("ok");
        let files = publish_reports(
            &out,
            |path| {
                fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(path)
                    .map_err(|e| io_error(&path.to_string_lossy(), 0, e))
            },
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(fs::read_dir(&out).unwrap().count(), 3);
        drop(files);
    }
    #[test]
    fn identity_rejection_rolls_back_staging_and_publication() {
        for reject_staging in [false, true] {
            let base = fo4_core::TestDir::new("identity-rollback");
            let out = base.join("new/intermediate/reports");
            let opened = std::cell::Cell::new(0);
            let result = publish_reports(
                &out,
                |path| {
                    opened.set(opened.get() + 1);
                    fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(path)
                        .map_err(|e| io_error(&path.to_string_lossy(), 0, e))
                },
                |path| {
                    let staging = path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".fo4-report-stage-");
                    if (reject_staging && staging && path.exists())
                        || (!reject_staging && path == out && out.exists())
                    {
                        return Err(Error::new(
                            "OutputInsideData",
                            &path.to_string_lossy(),
                            0,
                            "safe identity",
                            "injected post-create rejection",
                        ));
                    }
                    Ok(())
                },
            );
            assert_eq!(result.err().unwrap().code, "OutputInsideData");
            assert_eq!(opened.get(), if reject_staging { 0 } else { 3 });
            assert!(!out.exists());
            assert_eq!(
                fs::read_dir(base.to_path_buf()).unwrap().count(),
                0,
                "rollback also removes only newly created parents"
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn canonical_identity_is_checked_before_creation_or_open() {
        let base = fo4_core::TestDir::new("identity-preflight");
        let root = base.join("Data");
        let sub = root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        let alias = base.join("alias");
        std::os::unix::fs::symlink(&sub, &alias).unwrap();
        for suffix in ["reports", "newdir/reports"] {
            let out = alias.join(suffix);
            assert_eq!(safe_out(&out, &root).unwrap_err().code, "OutputInsideData");
            let opened = std::cell::Cell::new(false);
            let result = publish_reports(
                &out,
                |_| {
                    opened.set(true);
                    Err(Error::new(
                        "UnexpectedOpen",
                        "test",
                        0,
                        "no file creation",
                        "open invoked",
                    ))
                },
                |p| check_output_identity(p, &root),
            );
            assert_eq!(result.err().unwrap().code, "OutputInsideData");
            assert!(!opened.get());
            assert_eq!(fs::read_dir(&sub).unwrap().count(), 0);
        }
    }
}
