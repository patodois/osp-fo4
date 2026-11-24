#![forbid(unsafe_code)]
//! The host interface of chosen movies, read statically from their AVM2
//! blocks with `fo4_ui::abc` (`spec/ui.md`, Host interface). Measurement
//! support.
//!
//! `ui_host MANIFEST OUT_DIR LABEL=PATH [LABEL=PATH ...]` writes
//! `ui-host.txt` (per label: counts and kinds only, the lines quoted in
//! `spec/ui.md`) and `ui-host-names.txt` (classes, members, host objects
//! and host calls by name; never versioned). Labels are the caller's words
//! for the movies; paths name archive entries and stay out of the repo.
use fo4_core::{Budget, Manifest};
use fo4_ui::abc::{self, Access, MemberKind, Via};
use fo4_ui::{ActionKind, Limits, Movie};
use fo4_vfs::Vfs;
use std::{collections::BTreeMap, collections::BTreeSet, fmt::Write as _, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        return Err("usage: ui_host MANIFEST OUT_DIR LABEL=PATH...".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let out = Path::new(&a[1]);
    std::fs::create_dir_all(out)?;
    let mut counts = String::new();
    let mut names = String::new();
    let _ = writeln!(
        counts,
        "# Host interface, static reading (ui_host; counts and kinds only)"
    );
    // FO4_UI_LISTING=TEXT: also write the op listing of the classes whose
    // name contains TEXT (notes only).
    let listing = std::env::var("FO4_UI_LISTING").ok();
    for arg in &a[2..] {
        let (label, rest) = arg.split_once('=').ok_or("LABEL=PATH expected")?;
        // PATH@WORD: also read the components whose class name contains WORD.
        let (path, focus) = match rest.split_once('@') {
            Some((p, f)) => (p, Some(f.to_ascii_lowercase())),
            None => (rest, None),
        };
        let b = Budget::new(512 << 20);
        let mv = Movie::open(&vfs, path, &Limits::default(), &b)?;
        let inst: BTreeSet<String> = mv.instance_names.iter().cloned().collect();
        let root = mv.root_class().unwrap_or("").to_string();
        let _ = writeln!(counts, "\n## {label}");
        let _ = writeln!(names, "\n## {label}\t{path}\troot class {root}");
        let mut c: BTreeMap<String, u64> = BTreeMap::new();
        let mut bump = |k: String, n: u64| *c.entry(k).or_default() += n;
        bump(
            "placements with an instance name".into(),
            mv.instance_names.len() as u64,
        );
        bump("text fields".into(), mv.texts.len() as u64);
        bump("class links".into(), mv.classes.len() as u64);
        for blk in mv.actions.iter().filter(|x| x.kind == ActionKind::Abc) {
            let bytes = mv.action_bytes(blk).ok_or("block outside the movie")?;
            let r = abc::read(bytes, path, &inst)?;
            if let Some(f) = &listing {
                let text = abc::listing(bytes, path, f)?;
                let file = format!(
                    "ui-listing-{}.txt",
                    label.replace(|c: char| !c.is_ascii_alphanumeric(), "-")
                );
                std::fs::write(out.join(file), text)?;
            }
            bump("ABC blocks".into(), 1);
            bump("classes".into(), r.classes.len() as u64);
            bump("method bodies".into(), r.method_bodies);
            bump("method bodies refused part-way".into(), r.refused_bodies);
            bump("property call sites".into(), r.call_sites);
            bump(
                "property call sites whose receiver the look-back cannot name".into(),
                r.unresolved_calls,
            );
            let _ = writeln!(
                names,
                "block {}\t{} bytes\tclasses {}",
                blk.name,
                blk.len,
                r.classes.len()
            );
            for cl in &r.classes {
                let is_root = cl.name == root;
                let public: Vec<_> = cl
                    .members
                    .iter()
                    .filter(|x| {
                        x.access == Access::Public && x.kind == MemberKind::Method && !x.is_static
                    })
                    .collect();
                if is_root {
                    bump("root class: public methods".into(), public.len() as u64);
                    let n = cl
                        .members
                        .iter()
                        .filter(|x| {
                            x.access == Access::Public
                                && matches!(
                                    x.kind,
                                    MemberKind::Slot | MemberKind::Getter | MemberKind::Setter
                                )
                                && !x.is_static
                        })
                        .count();
                    bump(
                        "root class: public properties (slots, getters, setters)".into(),
                        n as u64,
                    );
                } else {
                    bump("other classes: public methods".into(), public.len() as u64);
                }
                let _ = writeln!(
                    names,
                    "class {}{}\textends {}",
                    cl.name,
                    if is_root { " (root)" } else { "" },
                    cl.super_name
                );
                for x in &cl.members {
                    let _ = writeln!(
                        names,
                        "  {:?} {:?}{} {}({}) {}",
                        x.access,
                        x.kind,
                        if x.is_static { " static" } else { "" },
                        x.name,
                        x.params,
                        x.type_name
                    );
                }
            }
            if let Some(f) = &focus {
                let us = abc::uses(bytes, path)?;
                for cl in r
                    .classes
                    .iter()
                    .filter(|c| c.name.to_ascii_lowercase().contains(f.as_str()))
                {
                    bump("focus: components".into(), 1);
                    let ids: Vec<u16> = mv
                        .classes
                        .iter()
                        .filter(|(_, n)| *n == cl.name)
                        .map(|(id, _)| *id)
                        .collect();
                    for id in &ids {
                        for p in mv.paths_to(*id) {
                            bump(
                                format!(
                                    "focus: placement chains from the root, {} names deep",
                                    p.len()
                                ),
                                1,
                            );
                            let _ =
                                writeln!(names, "focus {} (id {id}) at {}", cl.name, p.join("."));
                        }
                    }
                    let pubs = cl
                        .members
                        .iter()
                        .filter(|x| x.access == Access::Public && !x.is_static)
                        .collect::<Vec<_>>();
                    for x in &pubs {
                        let _ = writeln!(
                            names,
                            "focus member {:?} {} {}",
                            x.kind, x.name, x.type_name
                        );
                        if x.kind == MemberKind::Slot && x.type_name == "TextField" {
                            bump("focus: public text field members".into(), 1);
                            let outside = us
                                .iter()
                                .filter(|u| u.name == x.name && u.class != cl.name)
                                .count();
                            let tw = us
                                .iter()
                                .filter(|u| u.name == x.name && u.kind == abc::UseKind::TextWrite)
                                .count();
                            bump(
                                "focus: their uses in other classes' code".into(),
                                outside as u64,
                            );
                            bump(
                                "focus: their text written by the block's code".into(),
                                tw as u64,
                            );
                            let _ = writeln!(
                                names,
                                "focus text field {}: uses outside {outside}, text writes {tw}",
                                x.name
                            );
                        } else if matches!(x.kind, MemberKind::Setter | MemberKind::Method) {
                            bump(format!("focus: public {:?} members", x.kind), 1);
                        }
                    }
                }
            }
            for hc in &r.host_classes {
                let pm = r.public_methods(hc);
                bump(
                    "classes the host talks to (declaring a host-filled member)".into(),
                    1,
                );
                bump(
                    "their public instance methods (own and inherited in the block)".into(),
                    pm.len() as u64,
                );
                for (decl, m) in pm {
                    let _ = writeln!(
                        names,
                        "entry point {hc}: {}({}) declared in {decl}",
                        m.name, m.params
                    );
                    bump(format!("entry points by parameter count: {}", m.params), 1);
                }
            }
            for (o, t) in &r.host_objects {
                bump(format!("host-filled members, type {t}"), 1);
                let _ = writeln!(names, "host object {o}: {t}");
            }
            let mut fns: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for h in &r.host_calls {
                let kind = match &h.via {
                    Via::External => "ExternalInterface.call".to_string(),
                    Via::Command => "fscommand".to_string(),
                    Via::Object(_) => "a host-filled member".to_string(),
                };
                fns.entry(kind.clone()).or_default().insert(h.name.clone());
                bump(format!("host call sites through {kind}"), 1);
                let _ = writeln!(
                    names,
                    "host call {:?} {}({} args) from {}.{}",
                    h.via, h.name, h.args, h.class, h.method
                );
            }
            for (k, v) in fns {
                bump(
                    format!("distinct host functions called through {k}"),
                    v.len() as u64,
                );
            }
        }
        for (k, v) in c {
            let _ = writeln!(counts, "{k}\t{v}");
        }
    }
    std::fs::write(out.join("ui-host.txt"), counts)?;
    std::fs::write(out.join("ui-host-names.txt"), names)?;
    Ok(())
}
