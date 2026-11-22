//! Shared loading for the save examples: the save files of a folder and the
//! mapping from a runtime form id to the plugin data of the manifest.
use fo4_plugin::{typed::Store, FormKey};
use std::path::{Path, PathBuf};

/// Every `*.fos` file directly in `dir`, in name order. Only listed and
/// opened for reading.
#[allow(dead_code)]
pub fn saves(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("fos")))
        .collect();
    v.sort();
    Ok(v)
}

/// Where a runtime form id leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Top byte 0xFF.
    Created,
    /// Top byte 0xFE: a light plugin slot of the save's second list.
    Light { slot: u32, installed: bool },
    /// Top byte past the save's plugin list.
    PastList,
    /// Zero.
    Zero,
    /// A plugin of the save's list that the manifest does not load.
    NotLoaded,
    /// A plugin the manifest loads; the winning record's type, if any.
    Record(FormKey, Option<[u8; 4]>),
}

/// The save's plugin list against the manifest's load order.
pub struct Resolver<'a> {
    pub store: &'a Store,
    /// For each plugin of the save's first list, its name in lower case when
    /// the manifest loads it.
    pub loaded: Vec<Option<String>>,
    /// For each name of the second list, whether a file of that name is in
    /// the manifest's data folder.
    pub light_installed: Vec<bool>,
}

impl<'a> Resolver<'a> {
    pub fn new(store: &'a Store, save: &fo4_save::Save, data_root: &Path) -> Self {
        let names: Vec<String> = store
            .order()
            .headers()
            .iter()
            .map(|h| h.name.to_ascii_lowercase())
            .collect();
        let loaded = save
            .plugins
            .iter()
            .map(|p| {
                let p = String::from_utf8_lossy(p).to_ascii_lowercase();
                names.contains(&p).then_some(p)
            })
            .collect();
        let light_installed = save
            .light_plugins
            .iter()
            .map(|p| {
                fo4_core::within(data_root, &String::from_utf8_lossy(p)).is_ok_and(|x| x.is_file())
            })
            .collect();
        Self {
            store,
            loaded,
            light_installed,
        }
    }
    pub fn target(&self, id: u32) -> Target {
        if id == 0 {
            return Target::Zero;
        }
        let top = id >> 24;
        match top {
            0xff => Target::Created,
            0xfe => {
                let slot = (id >> 12) & 0xfff;
                Target::Light {
                    slot,
                    installed: self
                        .light_installed
                        .get(slot as usize)
                        .copied()
                        .unwrap_or(false),
                }
            }
            t => match self.loaded.get(t as usize) {
                None => Target::PastList,
                Some(None) => Target::NotLoaded,
                Some(Some(name)) => {
                    let key = FormKey {
                        plugin: name.clone(),
                        object_id: id & 0x00ff_ffff,
                    };
                    let sig = self.store.signature(&key);
                    Target::Record(key, sig)
                }
            },
        }
    }
}

/// A short class for counts: the record type, or what else the id names.
#[allow(dead_code)]
pub fn class(t: &Target) -> String {
    match t {
        Target::Created => "created (0xFF)".into(),
        Target::Light { installed, .. } => format!(
            "light slot (0xFE), plugin {}",
            if *installed {
                "installed"
            } else {
                "not installed"
            }
        ),
        Target::PastList => "top byte past the plugin list".into(),
        Target::Zero => "zero".into(),
        Target::NotLoaded => "plugin not loaded by the manifest".into(),
        Target::Record(_, Some(s)) => String::from_utf8_lossy(s).into_owned(),
        Target::Record(_, None) => "no winning record".into(),
    }
}
