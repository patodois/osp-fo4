//! Candidate transformations are observations for the census, never scene lookup rules.
use crate::Reports;
use fo4_core::{add, limit, mul, path_key_bytes, Budget, Error, Result};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
#[derive(Debug, Default, Serialize)]
pub struct Outcomes {
    pub present: u64,
    pub present_citations: u64,
    pub absent: u64,
    pub absent_citations: u64,
    pub invalid: u64,
    pub invalid_citations: u64,
    pub ambiguous: u64,
    pub ambiguous_citations: u64,
}
impl Outcomes {
    fn add(&mut self, state: &str, citations: u64) {
        match state {
            "present" => {
                self.present += 1;
                self.present_citations += citations;
            }
            "absent" => {
                self.absent += 1;
                self.absent_citations += citations;
            }
            "ambiguous" => {
                self.ambiguous += 1;
                self.ambiguous_citations += citations;
            }
            _ => {
                self.invalid += 1;
                self.invalid_citations += citations;
            }
        }
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub distinct: u64,
    pub citations: u64,
    pub rejected_candidates: Outcomes,
    pub missing_candidates: Outcomes,
    pub strict_invalid: u64,
    pub strict_invalid_citations: u64,
    pub strict_absent: u64,
    pub strict_absent_citations: u64,
    pub strict_present: u64,
    pub strict_present_citations: u64,
    pub candidate_present: u64,
    pub candidate_present_citations: u64,
    pub candidate_absent: u64,
    pub candidate_absent_citations: u64,
    pub candidate_invalid: u64,
    pub candidate_ambiguous: u64,
}
#[derive(Debug, Default, Serialize)]
pub struct LegacyCohort {
    pub distinct: u64,
    pub citations: u64,
    pub candidates: Outcomes,
}
fn legacy_material(raw: &[u8]) -> bool {
    let tail = &raw[raw.len().saturating_sub(5)..];
    tail.eq_ignore_ascii_case(b".bgsm") || tail.eq_ignore_ascii_case(b".bgem")
}
#[derive(Debug, Serialize)]
pub struct Summary {
    pub classes: BTreeMap<String, Counts>,
    pub legacy_invalid: LegacyCohort,
    pub legacy_missing: LegacyCohort,
    pub nif_attempted: u64,
    pub nif_read: u64,
    pub nif_failed: u64,
}
impl Default for Summary {
    fn default() -> Self {
        Self {
            classes: [
                "developer_absolute",
                "leading_slash",
                "trailing_space",
                "changed_extension",
                "data_folder_prefix",
                "missing_folder_prefix",
                "slash_normalization",
                "uppercase",
                "other",
            ]
            .into_iter()
            .map(|s| (s.into(), Counts::default()))
            .collect(),
            legacy_invalid: LegacyCohort::default(),
            legacy_missing: LegacyCohort::default(),
            nif_attempted: 0,
            nif_read: 0,
            nif_failed: 0,
        }
    }
}
struct Citation {
    path: Vec<u8>,
    count: u64,
}
pub struct Census {
    paths: Vec<Citation>,
    index: HashMap<Vec<u8>, usize>,
    budget: Budget,
    max: u64,
}
impl Census {
    pub fn new(max: u64, memory: u64) -> Self {
        Self {
            paths: Vec::new(),
            index: HashMap::new(),
            budget: Budget::new(memory),
            max,
        }
    }
    pub fn cite(&mut self, path: &[u8]) -> Result<()> {
        if let Some(i) = self.index.get(path) {
            self.paths[*i].count = add(self.paths[*i].count, 1, "path census", 0)?;
            return Ok(());
        }
        limit(self.paths.len() as u64 + 1, self.max, "path census", 0)?;
        self.budget.charge(
            add(
                mul(path.len() as u64, 2, "path census", 0)?,
                128,
                "path census",
                0,
            )?,
            "path census",
            0,
        )?;
        self.index
            .try_reserve(1)
            .map_err(|e| Error::new("AllocationFailed", "path census", 0, "path index", e))?;
        self.budget.reserve(&mut self.paths, 1, "path census", 0)?;
        let mut value = Vec::new();
        self.budget
            .reserve(&mut value, path.len(), "path census", 0)?;
        value.extend_from_slice(path);
        self.index.insert(value.clone(), self.paths.len());
        self.paths.push(Citation {
            path: value,
            count: 1,
        });
        Ok(())
    }
    pub fn finish(self, v: &Vfs, r: &mut Reports) -> Result<()> {
        for p in self.paths {
            let work = Budget::new(v.limits().max_memory);
            let c = classify(&p.path, v, &work)?;
            r.item(&serde_json::json!({"kind":"path_candidate","path_hex":fo4_core::HexBytes(&p.path).to_string(),"citations":p.count,"class":c.class,"traits":c.traits,"strict_present":c.strict_present,"strict_state":c.strict_state,"candidate_state":c.state,"candidate_paths_hex":c.paths.iter().map(|v|fo4_core::HexBytes(v).to_string()).collect::<Vec<_>>(),"policy":"census_candidate_v2; never used by scene"}))?;
            let summary = &mut r.summary.path_census;
            if legacy_material(&p.path) {
                let cohort = match c.strict_state {
                    "invalid" => Some(&mut summary.legacy_invalid),
                    "absent" => Some(&mut summary.legacy_missing),
                    _ => None,
                };
                if let Some(cohort) = cohort {
                    cohort.distinct += 1;
                    cohort.citations += p.count;
                    cohort.candidates.add(c.state, p.count);
                }
            }
            let counts = summary.classes.entry(c.class.into()).or_default();
            counts.distinct += 1;
            counts.citations += p.count;
            if c.strict_state == "invalid" {
                counts.rejected_candidates.add(c.state, p.count);
                counts.strict_invalid += 1;
                counts.strict_invalid_citations += p.count;
            }
            if c.strict_state == "absent" {
                counts.missing_candidates.add(c.state, p.count);
                counts.strict_absent += 1;
                counts.strict_absent_citations += p.count;
            }
            if c.strict_present {
                counts.strict_present += 1;
                counts.strict_present_citations += p.count;
            }
            match c.state {
                "present" => {
                    counts.candidate_present += 1;
                    counts.candidate_present_citations += p.count;
                }
                "absent" => {
                    counts.candidate_absent += 1;
                    counts.candidate_absent_citations += p.count;
                }
                "ambiguous" => counts.candidate_ambiguous += 1,
                _ => counts.candidate_invalid += 1,
            }
        }
        Ok(())
    }
}
struct Classification {
    class: &'static str,
    traits: Vec<&'static str>,
    strict_present: bool,
    strict_state: &'static str,
    state: &'static str,
    paths: Vec<Vec<u8>>,
}
fn classify(raw: &[u8], v: &Vfs, b: &Budget) -> Result<Classification> {
    b.charge(
        add(
            mul(raw.len() as u64, 16, "path classification", 0)?,
            2048,
            "path classification",
            0,
        )?,
        "path classification",
        0,
    )?;
    let strict_state = if path_key_bytes(raw).is_err() {
        "invalid"
    } else {
        match v.winner_bytes(raw) {
            Ok(_) => "present",
            Err(e) if e.code == "MissingPath" => "absent",
            _ => "blocked",
        }
    };
    let strict_present = strict_state == "present";
    let mut path = raw
        .iter()
        .map(|c| {
            if *c == b'\\' {
                b'/'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>();
    let absolute = path.get(1) == Some(&b':') || path.starts_with(b"//");
    let leading = path.starts_with(b"/");
    let trailing = path.last() == Some(&b' ');
    let mut traits = Vec::new();
    if absolute {
        traits.push("developer_absolute");
        if let Some(i) = path
            .windows(10)
            .enumerate()
            .find(|(i, s)| *s == b"materials/" && (*i == 0 || path[*i - 1] == b'/'))
            .map(|(i, _)| i)
        {
            path.drain(..i);
        }
    }
    if leading {
        traits.push("leading_slash");
        if !absolute {
            while path.first() == Some(&b'/') {
                path.remove(0);
            }
        }
    }
    if trailing {
        traits.push("trailing_space");
        while path.last() == Some(&b' ') {
            path.pop();
        }
    }
    // Measurement hypothesis only: remove exactly Data/ before Materials/.
    // Scene citation rules still reject this authored form.
    let data_prefix = path.starts_with(b"data/materials/");
    if data_prefix {
        traits.push("data_folder_prefix");
        path.drain(..5);
    }
    let missing_prefix = !path.starts_with(b"materials/");
    if missing_prefix {
        traits.push("missing_folder_prefix");
        if path_key_bytes(&path).is_ok() {
            path.splice(..0, b"materials/".iter().copied());
        }
    }
    let extension = !(path.ends_with(b".bgsm") || path.ends_with(b".bgem"));
    if extension {
        traits.push("changed_extension");
    }
    if raw.contains(&b'\\') {
        traits.push("slash_normalization");
    }
    if raw.iter().any(u8::is_ascii_uppercase) {
        traits.push("uppercase");
    }
    let class = if absolute {
        "developer_absolute"
    } else if leading {
        "leading_slash"
    } else if trailing {
        "trailing_space"
    } else if extension {
        "changed_extension"
    } else if data_prefix {
        "data_folder_prefix"
    } else if missing_prefix {
        "missing_folder_prefix"
    } else if raw.contains(&b'\\') {
        "slash_normalization"
    } else if raw.iter().any(u8::is_ascii_uppercase) {
        "uppercase"
    } else {
        "other"
    };
    let mut paths = Vec::new();
    if extension {
        let start = path.iter().rposition(|c| *c == b'/').map_or(0, |i| i + 1);
        if let Some(i) = path[start..].iter().rposition(|c| *c == b'.') {
            path.truncate(start + i);
        }
        for ext in [b".bgsm", b".bgem"] {
            let mut p = path.clone();
            p.extend(ext);
            paths.push(p);
        }
    } else {
        paths.push(path);
    }
    let valid = paths.iter().filter(|p| path_key_bytes(p).is_ok()).count();
    let found = paths.iter().filter(|p| v.winner_bytes(p).is_ok()).count();
    let state = if found > 1 {
        "ambiguous"
    } else if found == 1 {
        "present"
    } else if valid == 0 {
        "invalid"
    } else {
        "absent"
    };
    Ok(Classification {
        class,
        traits,
        strict_present,
        strict_state,
        state,
        paths,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifier_does_not_authorize_repairs_or_choose_between_extensions() {
        let t = fo4_core::TestDir::new("path-candidates");
        std::fs::create_dir(t.join("materials")).unwrap();
        std::fs::write(t.join("materials/a.bgsm"), b"synthetic").unwrap();
        std::fs::write(t.join("materials/a.bgem"), b"synthetic").unwrap();
        let m = fo4_core::Manifest {
            game: fo4_core::Game::Fallout4,
            archive_invalidation: None,
            root: t.to_path_buf(),
            language: "english".into(),
            plugins: vec![fo4_core::PluginSpec {
                name: "a.esm".into(),
                path: "a.esm".into(),
            }],
            mounts: vec![fo4_core::Mount::Loose {
                plugin: None,
                base: None,
                files: vec!["materials/a.bgsm".into(), "materials/a.bgem".into()],
            }],
            required_masters: vec![],
            excluded: vec![],
            build: None,
            limits: fo4_core::Limits::default(),
        };
        let v = Vfs::mount(&m, &Budget::new(1 << 20)).unwrap();
        let b = Budget::new(1 << 20);
        let redundant = classify(b"Data\\Materials\\A.BGSM", &v, &b).unwrap();
        assert_eq!(redundant.strict_state, "absent");
        assert_eq!(redundant.class, "data_folder_prefix");
        assert_eq!(redundant.state, "present");
        assert_eq!(redundant.paths, vec![b"materials/a.bgsm".to_vec()]);
        assert!(v.winner("Data/Materials/A.BGSM").is_err());
        assert_eq!(
            classify(b"Data/Materials/../a.bgsm", &v, &b).unwrap().state,
            "invalid"
        );
        let c = classify(b"C:\\dev\\materials\\A.BGSM ", &v, &b).unwrap();
        assert_eq!(c.class, "developer_absolute");
        assert!(!c.strict_present);
        assert_eq!(c.state, "present");
        assert!(v.winner("C:\\dev\\materials\\A.BGSM ").is_err());
        assert_eq!(classify(b"a.nif", &v, &b).unwrap().state, "ambiguous");
        assert_eq!(classify(b"../a.bgsm", &v, &b).unwrap().state, "invalid");
        assert_eq!(
            classify(b"materials/A.BGSM", &v, &b).unwrap().class,
            "uppercase"
        );
        assert!(classify(b"a.bgsm", &v, &Budget::new(1)).is_err());
    }
}
