//! The play mode's switches in the window: `play_run`'s play options, read
//! by the one structure the play library keeps (`fo4_play::options::Play`,
//! `spec/play.md`), so the window and `play_run` parse, check and apply
//! them the same way. With the keyboard only the activation's
//! (`--play-reach`, `--play-pick-bounds`) and the live world's
//! (`--play-live-world`, `--play-nowait`) apply; with a scripted input
//! (`--play-input FILE`) the window's run is driven by
//! `fo4_play::walker::Walker` built from all of them
//! (`fo4_play::options::Play::walker`), as `play_run` builds it.
//!
//! Without `--features script-event` the window has no play mode: no play
//! option is read (each is then an unknown option).
#[cfg(feature = "script-event")]
pub use fo4_play::options::Play as Switches;

/// The named platforms (`--play-elevator`), whose drawn parts the scripted
/// window moves as the walker's collision moves.
#[cfg(feature = "script-event")]
pub fn platforms(s: &Switches) -> Vec<fo4_plugin::FormKey> {
    s.elevators.clone()
}

/// Without the interpreter: none.
#[cfg(not(feature = "script-event"))]
pub fn platforms(_: &Switches) -> Vec<fo4_plugin::FormKey> {
    Vec::new()
}

/// Whether the held doors' boxes stand in the scripted walker's world
/// (`--play-held-doors`), whose doors `--draw-held-doors` draws.
#[cfg(feature = "script-event")]
pub fn held_doors(s: &Switches) -> bool {
    s.held_doors
}

/// Without the interpreter: no.
#[cfg(not(feature = "script-event"))]
pub fn held_doors(_: &Switches) -> bool {
    false
}

/// Without the interpreter: no play option is read.
#[cfg(not(feature = "script-event"))]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Switches {
    _none: (),
}

#[cfg(not(feature = "script-event"))]
impl Switches {
    /// Reads nothing (0: not a play option).
    pub fn take(&mut self, _: &[String]) -> Result<usize, String> {
        Ok(0)
    }
    pub fn any(&self) -> bool {
        false
    }
    pub fn walker_only(&self) -> bool {
        false
    }
    pub fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Whether the person's mode (`--play-keys`) has what it needs: the
/// longest standing slice and E's reach.
#[cfg(feature = "script-event")]
pub fn keys_ready(s: &Switches) -> bool {
    s.stand.is_some() && s.reach.is_some()
}

/// Without the interpreter the play mode does not start (its own error).
#[cfg(not(feature = "script-event"))]
pub fn keys_ready(_: &Switches) -> bool {
    true
}
