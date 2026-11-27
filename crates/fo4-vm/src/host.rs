//! Native functions: the [`Host`] trait, and the [`RecordingHost`], which
//! implements no game behaviour. The interpreter writes every native call
//! to the effect log itself; a host only answers with a result.
use crate::{
    rules,
    world::{Clock, InstId, Timer, Val, World},
};

/// One call of a native function.
#[derive(Debug, Clone)]
pub struct NativeCall {
    /// Lower-case script that declares the function, and the function.
    pub script: String,
    pub name: String,
    /// The receiver (`None` for a global function).
    pub receiver: Option<Val>,
    /// The receiver's script instance, when the receiver is an object that
    /// carries one of the receiver slot's type (for `self`, the caller).
    pub instance: Option<InstId>,
    pub args: Vec<Val>,
    /// Declared parameter types, lower case.
    pub param_types: Vec<String>,
    /// Declared return type, lower case (`none` for no value).
    pub return_type: String,
}
impl NativeCall {
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.script, self.name)
    }
}

/// A host's answer.
#[derive(Debug, Clone)]
pub enum Reply {
    /// The call returns this value.
    Value(Val),
    /// The call returns a value nothing determines: the interpreter stores
    /// an undetermined value naming the native.
    Undetermined,
    /// The host does not provide this native: the run stops with
    /// `VmError::NativeNotProvided`.
    NotProvided,
}

pub trait Host {
    fn call(&mut self, call: &NativeCall, world: &mut World) -> Reply;
    /// Rules the host's answers rely on, for the run's rule list.
    fn rules(&self) -> &[&'static str] {
        &[]
    }
    /// Whether a placed reference is disabled in the run, as the host's
    /// `IsDisabled` would answer it, without counting a native call
    /// (`crate::track`): `None` when the host keeps no such state.
    fn reference_disabled(
        &mut self,
        _world: &mut World,
        _reference: &fo4_plugin::FormKey,
    ) -> Option<Result<bool, &'static str>> {
        None
    }
}

/// A host that provides no native.
pub struct NoHost;
impl Host for NoHost {
    fn call(&mut self, _: &NativeCall, _: &mut World) -> Reply {
        Reply::NotProvided
    }
}

/// The recording host (`spec/vm.md`, Recording host). Every native call is
/// logged by the interpreter; this host returns:
///
/// - none for a native declared to return none;
/// - for `game.getplayer`, the player handle: one opaque object, created on
///   first use, known to be of the declared return type, equal only to
///   itself (`vm_host_player_handle_v1`);
/// - for `scriptobject.starttimer`, `starttimergametime`, `canceltimer`
///   and `canceltimergametime`, none, after queueing or removing a timer of
///   the receiver's instance (`vm_timer_events_v1`);
/// - an undetermined value for every other native that returns a value.
#[derive(Default)]
pub struct RecordingHost {
    /// Rules relied on so far.
    pub used: Vec<&'static str>,
}

pub const PLAYER_LABEL: &str = "player";

impl RecordingHost {
    fn using(&mut self, r: &'static str) {
        if !self.used.contains(&r) {
            self.used.push(r);
        }
    }
}

impl Host for RecordingHost {
    fn call(&mut self, c: &NativeCall, w: &mut World) -> Reply {
        match (c.script.as_str(), c.name.as_str()) {
            ("game", "getplayer") => {
                self.using(rules::HOST_PLAYER);
                return Reply::Value(Val::Object(w.player(&c.return_type)));
            }
            ("scriptobject", "starttimer" | "starttimergametime") => {
                self.using(rules::TIMERS);
                if let Some(inst) = c.instance {
                    let clock = if c.name == "starttimer" {
                        Clock::Real
                    } else {
                        Clock::GameTime
                    };
                    // An interval that is negative or not finite has no
                    // reading here: the timer is queued undetermined, which
                    // stops the queue with a named error when it is due.
                    let interval = match c.args.first() {
                        Some(Val::Float(f)) if f.is_finite() && *f >= 0.0 => Some(f64::from(*f)),
                        _ => None,
                    };
                    let id = c.args.get(1).cloned().unwrap_or(Val::Int(0));
                    let now = w.now[clock as usize];
                    w.timer_seq += 1;
                    w.timers.push(Timer {
                        clock,
                        due: interval.map(|i| now + i),
                        seq: w.timer_seq,
                        instance: inst,
                        id,
                    });
                }
                return Reply::Value(Val::None);
            }
            ("scriptobject", "canceltimer" | "canceltimergametime") => {
                self.using(rules::TIMERS);
                if let Some(inst) = c.instance {
                    let clock = if c.name == "canceltimer" {
                        Clock::Real
                    } else {
                        Clock::GameTime
                    };
                    let id = c.args.first().cloned().unwrap_or(Val::Int(0));
                    w.timers
                        .retain(|t| !(t.instance == inst && t.clock == clock && t.id == id));
                }
                return Reply::Value(Val::None);
            }
            _ => {}
        }
        if c.return_type == "none" {
            Reply::Value(Val::None)
        } else {
            Reply::Undetermined
        }
    }
    fn rules(&self) -> &[&'static str] {
        &self.used
    }
}
