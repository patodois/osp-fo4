//! What a driver reads back from the run between slices: the references
//! the run's natives enabled or disabled (the visibility events of the
//! timeline, without the NoWait reading), and the lines being spoken.
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
