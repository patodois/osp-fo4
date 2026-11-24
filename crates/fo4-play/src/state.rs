//! What a driver reads back from the run between slices: the references
//! the run's natives enabled or disabled (the visibility events of the
//! timeline, without the NoWait reading), and the lines being spoken.
use fo4_plugin::typed::Store;
use fo4_plugin::FormKey;
use fo4_scene::timeline::{Frame, Line};
use fo4_vm::Vm;
use std::collections::BTreeMap;

/// The last enable (`true`) or disable of each placed reference the run's
/// `Enable` and `Disable` natives (plain and NoWait) named, read from the
/// effect log as it grows.
#[derive(Debug, Clone, Default)]
pub struct Visibility {
    seen: usize,
    pub state: BTreeMap<FormKey, bool>,
}
impl Visibility {
    /// Reads the effects logged since the last call; whether a state
    /// changed.
    pub fn update(&mut self, vm: &Vm<'_>) -> bool {
        let mut changed = false;
        for e in vm.effects.iter().skip(self.seen) {
            let shown = match e.native.as_str() {
                "objectreference.enable" | "objectreference.enablenowait" => true,
                "objectreference.disable" | "objectreference.disablenowait" => false,
                _ => continue,
            };
            let key = e
                .receiver_object
                .and_then(|o| vm.world.objects.get(o))
                .and_then(|o| o.key.as_deref())
                .and_then(fo4_vm::data::key_record);
            if let Some(k) = key {
                changed |= self.state.insert(k, shown) != Some(shown);
            }
        }
        self.seen = vm.effects.len();
        changed
    }
    /// Whether the run disabled the reference last.
    pub fn disabled(&self, k: &FormKey) -> bool {
        self.state.get(k) == Some(&false)
    }
}

/// One subtitle: the line's text where the data settles it, else what the
/// timeline says about it.
pub fn subtitle(l: &Line) -> String {
    match &l.text {
        Some(t) => t.clone(),
        None => format!("({})", l.how),
    }
}

/// The subtitles of the lines a frame's second falls in, earliest first.
pub fn subtitles(f: &Frame) -> Vec<String> {
    f.spoken.iter().map(|(_, l)| subtitle(l)).collect()
}

/// The texts of the lines a frame's second falls in whose text the data
/// settles, earliest first: what a viewport shows as subtitles.
pub fn spoken_texts(f: &Frame) -> Vec<String> {
    f.spoken
        .iter()
        .filter_map(|(_, l)| l.text.clone())
        .collect()
}

/// What the timeline says about the lines a frame's second falls in whose
/// text the data does not settle (a title bar's notes, never a subtitle).
pub fn spoken_notes(f: &Frame) -> Vec<String> {
    f.spoken
        .iter()
        .filter(|(_, l)| l.text.is_none())
        .map(|(_, l)| subtitle(l))
        .collect()
}

/// The objective a frame shows: of the objectives the run displayed and
/// has neither completed nor failed since, the one displayed last (quest,
/// objective index).
pub fn current_objective(f: &Frame) -> Option<(FormKey, i32)> {
    use fo4_scene::timeline::ObjectiveKind as K;
    f.objectives
        .iter()
        .filter_map(|(k, v)| {
            let (shown, at) = v.get(&K::Displayed).copied()?;
            if shown != Some(true) {
                return None;
            }
            let ended = [K::Completed, K::Failed]
                .iter()
                .any(|e| v.get(e).is_some_and(|(x, t)| *x == Some(true) && *t >= at));
            (!ended).then(|| (at, k.clone()))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, k)| k)
}

/// Texts the records hold, read through the plugins' string tables when
/// they are localized (the run's manifest language), each table read once.
pub struct Texts<'a> {
    vfs: &'a fo4_vfs::Vfs,
    language: &'a str,
    cache: fo4_plugin::StringCache,
}
impl<'a> Texts<'a> {
    pub fn new(vfs: &'a fo4_vfs::Vfs, language: &'a str, b: &fo4_core::Budget) -> Texts<'a> {
        Texts {
            vfs,
            language,
            cache: fo4_plugin::StringCache::new(b),
        }
    }

    /// A text field of a record of `plugin` (a load order index): a string
    /// table id when the plugin is localized (the one table of the three
    /// that holds it), else the bytes before the NUL.
    fn field(&mut self, store: &Store, plugin: usize, data: &[u8]) -> Option<String> {
        let header = store.order().headers().get(plugin)?;
        if header.flags & fo4_plugin::LOCALIZED != 0 {
            let id = u32::from_le_bytes(data.try_into().ok()?);
            self.id(&header.name, id)
        } else {
            let d = data.strip_suffix(&[0]).unwrap_or(data);
            (!d.is_empty()).then(|| String::from_utf8_lossy(d).into_owned())
        }
    }

    fn id(&mut self, plugin: &str, id: u32) -> Option<String> {
        if id == 0 {
            return None;
        }
        let mut found = Vec::new();
        for kind in [
            fo4_plugin::StringKind::Strings,
            fo4_plugin::StringKind::DlStrings,
            fo4_plugin::StringKind::IlStrings,
        ] {
            if let Ok(t) = self
                .cache
                .resolve(self.vfs, plugin, self.language, kind, id)
            {
                found.push(t.to_string());
            }
        }
        match found.as_slice() {
            [one] if !one.is_empty() => Some(one.clone()),
            _ => None,
        }
    }

    /// A record's display name (its FULL), when it has one.
    pub fn display_name(&mut self, store: &Store, key: &FormKey) -> Option<String> {
        let b = fo4_core::Budget::new(store.limits.max_memory);
        let (r, plugin) = store.raw_with_plugin(key, &b).ok().flatten()?;
        let full = r.subrecords.iter().find(|s| s.signature == *b"FULL")?;
        let data = r.data(full).to_vec();
        self.field(store, plugin, &data)
    }

    /// The text of a quest's objective (its NNAM), when the record holds one.
    pub fn objective(&mut self, store: &Store, quest: &FormKey, objective: i32) -> Option<String> {
        let b = fo4_core::Budget::new(store.limits.max_memory);
        let (r, plugin) = store.raw_with_plugin(quest, &b).ok().flatten()?;
        let ctx = fo4_plugin::actor::Context {
            order: store.order(),
            plugin,
        };
        let q = fo4_plugin::quest::Quest::read(&r, ctx, &b).ok()?;
        let o = q.objective(u16::try_from(objective).ok()?)?;
        match o.text.as_ref()? {
            fo4_plugin::quest::Text::Id(id) => {
                let name = store.order().headers().get(plugin)?.name.clone();
                self.id(&name, *id)
            }
            fo4_plugin::quest::Text::Bytes(b) => {
                (!b.is_empty()).then(|| String::from_utf8_lossy(b).into_owned())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_scene::timeline::{Event, ObjectiveKind, Timeline, What};

    fn q(n: u32) -> FormKey {
        FormKey {
            plugin: "a.esm".into(),
            object_id: n,
        }
    }

    #[test]
    fn the_objective_shown_is_the_last_displayed_and_not_ended() {
        let ev = |t: f64, quest: u32, objective: i32, kind: ObjectiveKind| Event {
            t,
            what: What::Objective {
                quest: q(quest),
                objective,
                kind,
                value: Some(true),
            },
        };
        let t = Timeline {
            events: vec![
                ev(1.0, 1, 10, ObjectiveKind::Displayed),
                ev(2.0, 2, 5, ObjectiveKind::Displayed),
                ev(3.0, 2, 5, ObjectiveKind::Completed),
            ],
            ..Timeline::default()
        };
        assert_eq!(current_objective(&t.at(0.5)), None);
        assert_eq!(current_objective(&t.at(1.5)), Some((q(1), 10)));
        assert_eq!(current_objective(&t.at(2.5)), Some((q(2), 5)));
        // Completed: the one before shows again.
        assert_eq!(current_objective(&t.at(3.5)), Some((q(1), 10)));
    }
}
