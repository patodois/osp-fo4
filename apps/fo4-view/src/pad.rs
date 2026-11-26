//! The pad in the person's mode (`--play-pad`, `spec/play.md`, The pad):
//! where a frame's [`PadPoll`] comes from. The key map it feeds
//! (`crate::person`: dead zones, sectors, run, look rates, the hold that
//! ends) is viewer policy, not a claim about the game: no rule.
//!
//! [`PadSource`] is read once a frame. [`FakePad`] replays the pad's
//! states by second of the play clock from a synthetic person's `pad`
//! lines (`--play-keys-from`), the only way to check the pad without one.
//! `GipPad` (`--features pad`) reads a GIP pad over USB through the
//! `gip-pad` crate's reader, on that crate's own thread; it is built only
//! when the window is started with `--play-pad`, never by a test.
use crate::person::PadPoll;

/// Where the pad's state comes from.
pub trait PadSource: Send + Sync {
    /// The pad as it stands at the play clock's second `clock`.
    fn poll(&mut self, clock: f64) -> PadPoll;
    /// Why there is no pad, in the source's words, when there is none.
    fn absent(&self) -> Option<String> {
        None
    }
}

/// A scripted pad: each state holds from its second of the play clock to
/// the next one; no pad before the first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FakePad {
    states: Vec<(f64, PadPoll)>,
    next: usize,
    now: PadPoll,
}

impl FakePad {
    /// States in time order (`person::split_pad`).
    pub fn new(states: Vec<(f64, PadPoll)>) -> FakePad {
        FakePad {
            states,
            ..FakePad::default()
        }
    }
}

impl PadSource for FakePad {
    fn poll(&mut self, clock: f64) -> PadPoll {
        while let Some((t, p)) = self.states.get(self.next) {
            if *t > clock {
                break;
            }
            self.now = *p;
            self.next += 1;
        }
        self.now
    }

    fn absent(&self) -> Option<String> {
        (!self.now.connected).then(|| "no scripted pad state yet, or pad off".to_string())
    }
}

/// The pad the window was asked for.
pub enum Wanted {
    /// A synthetic person's `pad` lines.
    Fake(FakePad),
    /// `--play-pad` (`reset`) or `--play-pad-no-reset`: the device.
    Device { reset: bool },
}

/// The pad's source, as asked for. The device needs a viewer built with
/// `--features pad`.
pub fn open(w: Wanted) -> Result<Box<dyn PadSource>, String> {
    match w {
        Wanted::Fake(f) => Ok(Box::new(f)),
        #[cfg(feature = "pad")]
        Wanted::Device { reset } => Ok(Box::new(GipPad::spawn(reset)?)),
        #[cfg(not(feature = "pad"))]
        Wanted::Device { reset } => Err(format!(
            "{} needs a viewer built with --features pad",
            if reset {
                "--play-pad"
            } else {
                "--play-pad-no-reset"
            }
        )),
    }
}

/// The pad's presence as the window last told it: one line at each
/// change, never one a frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    told: Option<(bool, Option<String>)>,
}

impl Status {
    /// The frame's presence (and, when absent, why): the line to print
    /// when it changed.
    pub fn update(&mut self, connected: bool, why: Option<String>) -> Option<String> {
        let now = (connected, if connected { None } else { why });
        if self.told.as_ref() == Some(&now) {
            return None;
        }
        let line = match &now {
            (true, _) => "play: pad: connected".to_string(),
            (false, Some(w)) => format!("play: pad: none ({w}); the keyboard plays"),
            (false, None) => "play: pad: none; the keyboard plays".to_string(),
        };
        self.told = Some(now);
        Some(line)
    }
}

/// The report bits of the buttons the key map reads, found by name.
#[cfg(any(feature = "pad", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Buttons {
    pub a: u16,
    pub b: u16,
    pub menu: u16,
    pub view: u16,
}

#[cfg(any(feature = "pad", test))]
impl Buttons {
    /// The bits of A, B, Menu (or Start) and View (or Back, Select) in a
    /// table of (bit, name), names compared without case; an error naming
    /// the table's names when one is missing.
    pub fn resolve(table: &[(u16, &str)]) -> Result<Buttons, String> {
        let find = |names: &[&str]| {
            table
                .iter()
                .find(|(_, n)| names.iter().any(|w| n.eq_ignore_ascii_case(w)))
                .map(|(bit, _)| *bit)
                .ok_or_else(|| {
                    let all: Vec<&str> = table.iter().map(|(_, n)| *n).collect();
                    format!(
                        "PadButtons: no button named {} among the pad library's {all:?}",
                        names.join(" or ")
                    )
                })
        };
        Ok(Buttons {
            a: find(&["A"])?,
            b: find(&["B"])?,
            menu: find(&["Menu", "Start"])?,
            view: find(&["View", "Back", "Select"])?,
        })
    }
}

/// A GIP pad read by `gip_pad::PadReader` on its own thread (it resets
/// the pad over USB unless asked not to, and claims it: one program at a
/// time). Dropped with the window's app, after the run is closed.
#[cfg(feature = "pad")]
pub struct GipPad {
    reader: gip_pad::PadReader,
    buttons: Buttons,
}

#[cfg(feature = "pad")]
impl GipPad {
    /// The button names first (nothing touches the device if one is
    /// missing), then the reader.
    pub fn spawn(reset: bool) -> Result<GipPad, String> {
        let buttons = Buttons::resolve(&gip_pad::BUTTONS)?;
        let reader = if reset {
            gip_pad::PadReader::spawn()
        } else {
            gip_pad::PadReader::spawn_without_reset()
        };
        Ok(GipPad { reader, buttons })
    }
}

#[cfg(feature = "pad")]
impl PadSource for GipPad {
    fn poll(&mut self, _clock: f64) -> PadPoll {
        let Some(p) = self.reader.latest() else {
            return PadPoll::default();
        };
        let axis = |v: i16| f64::from(gip_pad::stick(v, 0.0));
        let on = |bit: u16| p.buttons & bit != 0;
        let b = self.buttons;
        PadPoll {
            connected: true,
            left: [axis(p.left_stick.0), axis(p.left_stick.1)],
            right: [axis(p.right_stick.0), axis(p.right_stick.1)],
            activate: on(b.a),
            pause: on(b.menu),
            end: on(b.view) || on(b.b),
        }
    }

    fn absent(&self) -> Option<String> {
        self.reader.last_error()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_pad_replays_its_states_by_second() {
        let a = PadPoll {
            connected: true,
            left: [0.0, 1.0],
            ..PadPoll::default()
        };
        let b = PadPoll {
            activate: true,
            ..a
        };
        let mut f = FakePad::new(vec![
            (1.0, a),
            (2.0, b),
            (2.0, a),
            (3.0, PadPoll::default()),
        ]);
        assert_eq!(f.poll(0.5), PadPoll::default());
        assert!(f.absent().is_some());
        assert_eq!(f.poll(1.0), a);
        assert_eq!(f.absent(), None);
        assert_eq!(f.poll(1.9), a);
        // Two states at one second: the later holds.
        assert_eq!(f.poll(2.0), a);
        assert_eq!(f.poll(10.0), PadPoll::default());
        let mut w = open(Wanted::Fake(FakePad::new(vec![(0.0, b)]))).unwrap();
        assert_eq!(w.poll(0.0), b);
    }

    #[cfg(not(feature = "pad"))]
    #[test]
    fn the_device_needs_the_pad_feature() {
        let e = open(Wanted::Device { reset: true }).err().unwrap();
        assert!(e.contains("--features pad"), "{e}");
    }

    #[test]
    fn the_status_is_told_once_a_change() {
        let mut s = Status::default();
        assert_eq!(
            s.update(false, Some("not found".into())).as_deref(),
            Some("play: pad: none (not found); the keyboard plays")
        );
        assert_eq!(s.update(false, Some("not found".into())), None);
        assert!(s
            .update(false, Some("held by another program".into()))
            .is_some());
        assert_eq!(
            s.update(true, None).as_deref(),
            Some("play: pad: connected")
        );
        assert_eq!(s.update(true, Some("ignored".into())), None);
        assert!(s.update(false, None).is_some());
    }

    #[test]
    fn the_buttons_are_found_by_name() {
        let t = [(1, "a"), (2, "B"), (4, "Start"), (8, "Back"), (16, "X")];
        assert_eq!(
            Buttons::resolve(&t),
            Ok(Buttons {
                a: 1,
                b: 2,
                menu: 4,
                view: 8
            })
        );
        let e = Buttons::resolve(&[(1, "A"), (2, "B"), (4, "Menu")]).unwrap_err();
        assert!(
            e.contains("View or Back or Select") && e.contains("Menu"),
            "{e}"
        );
    }

    /// The pad library's own table names every button the key map reads
    /// (its constant only: no reader is built).
    #[cfg(feature = "pad")]
    #[test]
    fn the_pad_librarys_buttons_have_the_names() {
        let b = Buttons::resolve(&gip_pad::BUTTONS).unwrap();
        let bits = [b.a, b.b, b.menu, b.view];
        assert!(bits.iter().all(|x| x.count_ones() == 1), "{b:?}");
        assert!((1..4).all(|i| !bits[..i].contains(&bits[i])), "{b:?}");
    }
}
