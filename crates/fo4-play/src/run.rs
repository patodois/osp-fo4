//! `quest_run`'s new-game code, compiled from `fo4-vm`'s example by
//! `build.rs` (patched only to take its arguments from the caller and to
//! call [`play::play_loop`] in place of its clock loop), and the stepped
//! loop that feeds it input ([`play`]).
#![allow(dead_code)]

include!(concat!(env!("OUT_DIR"), "/quest_run.rs"));

pub(crate) mod play;

/// Runs `quest_run` with these arguments (`MANIFEST new-game OUT_DIR
/// [switches...]`), the play driver installed for its clock loop when one
/// is given ([`play::install`]).
pub(crate) fn run_args(args: Vec<String>) -> Result<(), E> {
    run_with(args)
}
