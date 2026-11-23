#![forbid(unsafe_code)]
//! Sets quest stages and runs their fragments with `fo4_vm::quest`
//! (`spec/vm.md`, Quests). Every run is under pending rules; the modes,
//! switches and outputs are `fo4_vm::run`'s (its module docs).
//!
//! ```text
//! quest_run MANIFEST stages OUT_DIR      every quest, every stage with a fragment
//! quest_run MANIFEST new-game OUT_DIR    the quests started with a new game
//! quest_run MANIFEST QUEST STAGE         one stage of one quest (EditorID), effect log
//! ```
//!
//! The voice files' seconds are read here from their headers
//! (`fo4_audio::header_seconds`, `audio_xwma_dpds_seconds_v1`): the library
//! takes them from its caller.
use fo4_core::Budget;

fn seconds(bytes: &[u8], name: &str, b: &Budget) -> Option<f64> {
    fo4_audio::Sound::read(bytes, name, b)
        .ok()
        .and_then(|s| fo4_audio::header_seconds(&s.wave))
}

fn main() {
    if let Err(e) = fo4_vm::run::main_with(std::env::args().skip(1).collect(), &seconds, None) {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
