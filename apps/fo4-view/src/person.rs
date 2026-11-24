//! The person's mode (`--play-keys`, `spec/play.md`, The person's input):
//! the keyboard and the mouse turned into the scripted input's commands
//! (`fo4_play::input::Command`) at the second of the frame they happen in,
//! for `play_run`'s walker on the run's thread. The key map, the mouse's
//! turn per pixel, the pause and the crosshair are viewer policy, not
//! claims about the game: no rule.
//!
//! The key map: W, S, A, D hold forward, back, left, right (left and right
//! strafe); Shift holds run, whether it arrives as a key of its own or only
//! as a modifier of the movement key ([`shift_of_modifiers`]); the mouse
//! turns (heading) and looks up or down (pitch) while the left button is
//! held, from raw device motion or, when there is none, from the window's
//! cursor ([`Drag`]); the arrow keys turn and look up or down at a fixed
//! rate ([`ARROW_TURN_DEGREES_PER_SECOND`]); E activates; Escape ends the
//! input; P pauses (no command: the window's clock stops). The walker has
//! no jump and `toward` (a scripted steering aid) has no key.
//!
//! [`Synthetic`] is a person for checks and measurements only: timed key,
//! button and mouse events from a file, injected by the window where a real
//! key press arrives, so they go through the same key map.
use fo4_play::input::{Command, Key};

/// The pitch the person can look up or down to (degrees): the viewer's
/// free-fly limit.
pub const MAX_PITCH_DEGREES: f64 = 89.0;

/// How fast the left and right arrow keys turn the view (degrees a second
/// of the window's frames): viewer policy.
pub const ARROW_TURN_DEGREES_PER_SECOND: f64 = 90.0;
/// How fast the up and down arrow keys look up or down (degrees a second).
pub const ARROW_PITCH_DEGREES_PER_SECOND: f64 = 60.0;

/// The keys a person holds: forward, back, left, right, run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Held {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub run: bool,
}

/// What the person does at the window, and what the walker was told.
#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    /// Where the person looks (degrees from +X towards +Y; above the
    /// horizontal): the window's camera, ahead of the walker.
    pub heading: f64,
    pub pitch: f64,
    pub held: Held,
    /// E and Escape pressed since the commands were last taken.
    activate: bool,
    quit: bool,
    pub paused: bool,
    /// Escape was sent: no command after it.
    pub ended: bool,
    /// What the walker was told: keys, heading, pitch (the walker starts
    /// with no key held, heading 0 and pitch 0).
    sent: (Held, f64, f64),
    /// The play clock as the window gave it: the sum of the frames'
    /// seconds while the run went on.
    pub clock: f64,
    /// Degrees of turn per pixel of mouse motion.
    pub degrees_per_pixel: f64,
    /// Commands given, looks among them.
    pub given: (u64, u64),
}

impl Person {
    /// Looking as the window starts (`--eye`, `--look`).
    pub fn new(heading: f64, pitch: f64, degrees_per_pixel: f64) -> Person {
        Person {
            heading: heading.rem_euclid(360.0),
            pitch: pitch.clamp(-MAX_PITCH_DEGREES, MAX_PITCH_DEGREES),
            held: Held::default(),
            activate: false,
            quit: false,
            paused: false,
            ended: false,
            sent: (Held::default(), 0.0, 0.0),
            clock: 0.0,
            degrees_per_pixel,
            given: (0, 0),
        }
    }

    /// The arrow keys held for a frame of `dt` seconds: left and right
    /// turn, up and down look up and down, at a fixed rate (none while
    /// paused).
    pub fn arrows(&mut self, left: bool, right: bool, up: bool, down: bool, dt: f64) {
        if self.paused || !(dt.is_finite() && dt > 0.0) {
            return;
        }
        let way = |a: bool, b: bool| f64::from(i8::from(a) - i8::from(b));
        let turn = way(left, right) * ARROW_TURN_DEGREES_PER_SECOND * dt;
        let tilt = way(up, down) * ARROW_PITCH_DEGREES_PER_SECOND * dt;
        if turn != 0.0 {
            self.heading = (self.heading + turn).rem_euclid(360.0);
        }
        if tilt != 0.0 {
            self.pitch = (self.pitch + tilt).clamp(-MAX_PITCH_DEGREES, MAX_PITCH_DEGREES);
        }
    }

    /// Mouse motion in window pixels while the left button is held (x
    /// right, y down): mouse right turns right, mouse up looks up.
    pub fn look(&mut self, d: [f64; 2]) {
        if self.paused || d == [0.0; 2] || !d.iter().all(|x| x.is_finite()) {
            return;
        }
        self.heading = (self.heading - d[0] * self.degrees_per_pixel).rem_euclid(360.0);
        self.pitch = (self.pitch - d[1] * self.degrees_per_pixel)
            .clamp(-MAX_PITCH_DEGREES, MAX_PITCH_DEGREES);
    }

    /// E was pressed.
    pub fn press_activate(&mut self) {
        if !self.paused {
            self.activate = true;
        }
    }

    /// Escape was pressed.
    pub fn press_quit(&mut self) {
        self.quit = true;
    }

    /// P was pressed.
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// The commands for a frame in which the play clock runs: the look
    /// first (so an activation in the same frame aims where the person
    /// looks), then the keys that changed, the activation, the end.
    pub fn commands(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        if self.ended {
            self.activate = false;
            self.quit = false;
            return out;
        }
        let (held, heading, pitch) = self.sent;
        if self.heading != heading {
            out.push(Command::Face(self.heading));
        }
        if self.pitch != pitch {
            out.push(Command::Pitch(self.pitch));
        }
        let looks = out.len() as u64;
        let h = self.held;
        for (key, now, was) in [
            (Key::Forward, h.forward, held.forward),
            (Key::Back, h.back, held.back),
            (Key::Left, h.left, held.left),
            (Key::Right, h.right, held.right),
            (Key::Run, h.run, held.run),
        ] {
            if now != was {
                out.push(Command::Key(key, now));
            }
        }
        if std::mem::take(&mut self.activate) {
            out.push(Command::Activate);
        }
        if std::mem::take(&mut self.quit) {
            out.push(Command::End);
            self.ended = true;
        }
        self.sent = (h, self.heading, self.pitch);
        self.given.0 += out.len() as u64;
        self.given.1 += looks;
        out
    }
}

/// What the window's pointer did, in the order the window system sent it
/// (the window's ordered events, not the per-kind messages): the cursor at
/// a position (window pixels), the left button pressed or let go, the
/// cursor out of the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointer {
    Moved([f64; 2]),
    Button(bool),
    Left,
}

/// The left button's drags as the window's cursor makes them: the
/// cursor's movement between its positions while the button is down, in
/// the order the events came. A tool that moves the cursor without raw
/// device motion, or sends the press, the motion and the release within
/// one frame, or carries the motion on the release itself (as the window
/// system does: a release brings the cursor's position before the
/// button's change), still turns the view. Viewer policy, no rule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drag {
    held: bool,
    at: Option<[f64; 2]>,
    sum: [f64; 2],
    during: bool,
}

impl Drag {
    /// One pointer event, in order.
    pub fn feed(&mut self, p: Pointer) {
        match p {
            Pointer::Moved(q) => {
                if self.held {
                    if let Some(a) = self.at {
                        self.sum[0] += q[0] - a[0];
                        self.sum[1] += q[1] - a[1];
                    }
                }
                self.at = Some(q);
            }
            Pointer::Button(down) => {
                self.during |= down || self.held;
                self.held = down;
            }
            Pointer::Left => self.at = None,
        }
    }

    /// The frame's cursor motion while the button was down, and whether
    /// the button was down at any time in the frame; then a new frame.
    pub fn take(&mut self) -> ([f64; 2], bool) {
        let out = (self.sum, self.during || self.held);
        self.sum = [0.0; 2];
        self.during = false;
        out
    }
}

/// The motion a frame looks by: the raw device motion when there is any,
/// else the cursor's movement while the button was held; both are never
/// added.
pub fn look_motion(raw: [f64; 2], cursor: [f64; 2]) -> [f64; 2] {
    if raw != [0.0; 2] {
        raw
    } else {
        cursor
    }
}

/// Whether Shift is on in a window system's modifier change, read from
/// its text as the pinned window library prints it
/// (`ModifiersChanged(Modifiers { state: ModifiersState(SHIFT | ...), ..
/// })`): `None` for any other event. A tool that sends Shift only as a
/// flag on the movement key's press (no Shift key event of its own, so no
/// left or right Shift is attested) is seen here. Viewer policy.
pub fn shift_of_modifiers(text: &str) -> Option<bool> {
    let rest = text.strip_prefix("ModifiersChanged(")?;
    let state = rest.split_once("state: ModifiersState(")?.1;
    let flags = state.split_once(')')?.0;
    Some(flags.split('|').any(|f| f.trim() == "SHIFT"))
}

/// Run is held: Shift as a key of its own, or Shift as the modifier of a
/// movement key while one is held.
pub fn run_held(shift_key: bool, shift_modifier: bool, held: Held) -> bool {
    shift_key || (shift_modifier && (held.forward || held.back || held.left || held.right))
}

/// A key the synthetic person presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyName {
    W,
    A,
    S,
    D,
    E,
    P,
    Escape,
    Shift,
    /// The arrow keys.
    Left,
    Right,
    Up,
    Down,
}

impl KeyName {
    fn parse(s: &str) -> Option<KeyName> {
        Some(match s {
            "W" => KeyName::W,
            "A" => KeyName::A,
            "S" => KeyName::S,
            "D" => KeyName::D,
            "E" => KeyName::E,
            "P" => KeyName::P,
            "Escape" => KeyName::Escape,
            "Shift" => KeyName::Shift,
            "Left" => KeyName::Left,
            "Right" => KeyName::Right,
            "Up" => KeyName::Up,
            "Down" => KeyName::Down,
            _ => return None,
        })
    }
}

/// A key or button state change: down, up, or down then up the next frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Down,
    Up,
    Tap,
}

/// One thing the synthetic person does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    Key(KeyName, Press),
    /// The left mouse button.
    Button(Press),
    /// Mouse motion in pixels (x right, y down).
    Mouse(f64, f64),
    /// A drag by the window's cursor alone, as a desktop tool sends one:
    /// the cursor at a point, the left button pressed, the cursor moved by
    /// (DX, DY) and the button let go, all within one frame, with no raw
    /// device motion.
    Drag(f64, f64),
    /// Turn (with the left button held for that frame) until the view's
    /// heading is this many degrees, as a person turns to a heading.
    TurnTo(f64),
    /// Look up or down to this pitch the same way.
    PitchTo(f64),
    /// Walk to a point: W held, the mouse turning the view toward the
    /// point from where the walker is drawn each frame, W let go near it
    /// (a person steering by eye). A later `steer` replaces the point;
    /// `stop`, `turn-to` and W let go end it.
    Steer(f64, f64),
    Stop,
    /// Let W go and look at a point (x, y, z) from the walker's eye as
    /// drawn, again each frame (a person keeping the thing in the middle
    /// of the view) until another look, `steer` or `stop`.
    AimAt(f64, f64, f64),
}

/// One timed event of the synthetic person's file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timed {
    pub t: f64,
    pub act: Act,
}

/// Reads the synthetic person's file: one event a line, `SECONDS EVENT`,
/// seconds of the play clock in time order; `#` starts a comment. Events:
/// `key W|A|S|D|E|P|Escape|Shift|Left|Right|Up|Down down|up|tap`,
/// `button down|up|tap`, `mouse DX DY`, `drag DX DY`, `turn-to DEGREES`,
/// `pitch-to DEGREES`, `steer X Y`, `aim-at X Y Z`, `stop`.
pub fn parse_synthetic(text: &str) -> Result<Vec<Timed>, String> {
    let mut out: Vec<Timed> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let bad = |why: &str| format!("PlayKeysFrom: line {}: {why}", n + 1);
        let w: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| -> Result<f64, String> {
            w.get(i)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .ok_or_else(|| bad("a finite number is missing"))
        };
        let press = |i: usize| -> Result<Press, String> {
            match w.get(i).copied() {
                Some("down") => Ok(Press::Down),
                Some("up") => Ok(Press::Up),
                Some("tap") => Ok(Press::Tap),
                _ => Err(bad("down, up or tap")),
            }
        };
        let t = num(0)?;
        if t < 0.0 || out.last().is_some_and(|e| e.t > t) {
            return Err(bad("seconds are 0 or more and never go back"));
        }
        let (act, words) =
            match w.get(1).copied() {
                Some("key") => (
                    Act::Key(
                        w.get(2).and_then(|k| KeyName::parse(k)).ok_or_else(|| {
                            bad("a key: W A S D E P Escape Shift Left Right Up Down")
                        })?,
                        press(3)?,
                    ),
                    4,
                ),
                Some("button") => (Act::Button(press(2)?), 3),
                Some("mouse") => (Act::Mouse(num(2)?, num(3)?), 4),
                Some("drag") => (Act::Drag(num(2)?, num(3)?), 4),
                Some("turn-to") => (Act::TurnTo(num(2)?), 3),
                Some("pitch-to") => {
                    let p = num(2)?;
                    if p.abs() > MAX_PITCH_DEGREES {
                        return Err(bad("pitch-to is within 89 degrees"));
                    }
                    (Act::PitchTo(p), 3)
                }
                Some("steer") => (Act::Steer(num(2)?, num(3)?), 4),
                Some("aim-at") => (Act::AimAt(num(2)?, num(3)?, num(4)?), 5),
                Some("stop") => (Act::Stop, 2),
                _ => return Err(bad(
                    "expected key, button, mouse, drag, turn-to, pitch-to, steer, aim-at or stop",
                )),
            };
        if w.len() != words {
            return Err(bad("extra words"));
        }
        out.push(Timed { t, act });
    }
    Ok(out)
}

/// What the synthetic person injects in one frame: keys pressed (held keys
/// again each frame, so a lost focus does not let them go) and let go, the
/// left button's press or release, the mouse motion.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inject {
    pub down: Vec<KeyName>,
    pub up: Vec<KeyName>,
    pub button: Option<bool>,
    pub motion: [f64; 2],
    /// A drag by the cursor alone ([`Act::Drag`]), its motion summed.
    pub drag: Option<[f64; 2]>,
}

/// The synthetic person's state.
#[derive(Debug, Clone, Default)]
pub struct Synthetic {
    events: Vec<Timed>,
    next: usize,
    held: std::collections::BTreeSet<KeyName>,
    tapped: Vec<KeyName>,
    button: bool,
    button_tap: bool,
    button_was: bool,
    steer: Option<[f64; 2]>,
    aim: Option<[f64; 3]>,
    /// Points steered to and reached: (clock, feet).
    pub arrived: Vec<(f64, [f64; 2])>,
}

/// A difference of headings in (-180, 180].
fn wrap(d: f64) -> f64 {
    let r = (d + 180.0).rem_euclid(360.0) - 180.0;
    if r == -180.0 {
        180.0
    } else {
        r
    }
}

impl Synthetic {
    pub fn new(events: Vec<Timed>) -> Synthetic {
        Synthetic {
            events,
            ..Synthetic::default()
        }
    }

    /// Whether every event was given and nothing is held.
    pub fn done(&self) -> bool {
        self.next >= self.events.len() && self.steer.is_none()
    }

    /// One frame at the play clock `clock`: the events due, in order (a
    /// tap ends the frame's events, so what follows it at the same second
    /// comes a frame later), then the steering. `view`: the window's
    /// heading and pitch; `degrees_per_pixel`: the mouse's turn;
    /// `walker`: the walker's feet as drawn and its eye's height above
    /// them; `arrive`: how near the point the person lets W go (units).
    pub fn frame(
        &mut self,
        clock: f64,
        view: (f64, f64),
        degrees_per_pixel: f64,
        walker: Option<([f64; 3], f64)>,
        arrive: f64,
    ) -> Inject {
        let feet = walker.map(|w| w.0);
        let mut out = Inject {
            up: std::mem::take(&mut self.tapped),
            ..Inject::default()
        };
        let mut look = false;
        if std::mem::take(&mut self.button_tap) {
            self.button = false;
        }
        let dpp = degrees_per_pixel;
        // The view after this frame's motion so far.
        let heading = |m: [f64; 2]| view.0 - m[0] * dpp;
        let pitch = |m: [f64; 2]| view.1 - m[1] * dpp;
        while let Some(e) = self.events.get(self.next).copied() {
            if e.t > clock {
                break;
            }
            self.next += 1;
            let mut tap = false;
            match e.act {
                Act::Key(k, Press::Down) => {
                    self.held.insert(k);
                }
                Act::Key(k, Press::Up) => {
                    self.held.remove(&k);
                    out.up.push(k);
                    if k == KeyName::W {
                        self.steer = None;
                    }
                }
                Act::Key(k, Press::Tap) => {
                    out.down.push(k);
                    self.tapped.push(k);
                    tap = true;
                }
                Act::Button(Press::Down) => self.button = true,
                Act::Button(Press::Up) => self.button = false,
                Act::Button(Press::Tap) => {
                    self.button = true;
                    self.button_tap = true;
                    tap = true;
                }
                Act::Mouse(dx, dy) => {
                    out.motion[0] += dx;
                    out.motion[1] += dy;
                }
                Act::Drag(dx, dy) => {
                    let d = out.drag.get_or_insert([0.0; 2]);
                    d[0] += dx;
                    d[1] += dy;
                }
                Act::TurnTo(h) => {
                    self.aim = None;
                    if self.steer.take().is_some() {
                        self.held.remove(&KeyName::W);
                        out.up.push(KeyName::W);
                    }
                    out.motion[0] += wrap(heading(out.motion) - h) / dpp;
                    look = true;
                }
                Act::PitchTo(p) => {
                    self.aim = None;
                    out.motion[1] += (pitch(out.motion) - p) / dpp;
                    look = true;
                }
                Act::Steer(x, y) => {
                    self.aim = None;
                    self.steer = Some([x, y]);
                    self.held.insert(KeyName::W);
                }
                Act::Stop => {
                    self.aim = None;
                    if self.steer.take().is_some() {
                        self.held.remove(&KeyName::W);
                        out.up.push(KeyName::W);
                    }
                }
                Act::AimAt(x, y, z) => {
                    if self.steer.take().is_some() {
                        self.held.remove(&KeyName::W);
                        out.up.push(KeyName::W);
                    }
                    self.aim = Some([x, y, z]);
                }
            }
            if tap {
                break;
            }
        }
        if let (Some([x, y, z]), Some((f, eye))) = (self.aim, walker) {
            let d = [x - f[0], y - f[1], z - f[2] - eye];
            if d[0].hypot(d[1]) > 1e-6 {
                let h = d[1].atan2(d[0]).to_degrees();
                let p = d[2]
                    .atan2(d[0].hypot(d[1]))
                    .to_degrees()
                    .clamp(-MAX_PITCH_DEGREES, MAX_PITCH_DEGREES);
                let turn = wrap(heading(out.motion) - h);
                let tilt = pitch(out.motion) - p;
                if turn != 0.0 || tilt != 0.0 {
                    out.motion[0] += turn / dpp;
                    out.motion[1] += tilt / dpp;
                    look = true;
                }
            }
        }
        if let (Some([x, y]), Some(f)) = (self.steer, feet) {
            let (dx, dy) = (x - f[0], y - f[1]);
            if dx.hypot(dy) <= arrive {
                self.steer = None;
                self.held.remove(&KeyName::W);
                out.up.push(KeyName::W);
                self.arrived.push((clock, [f[0], f[1]]));
            } else {
                let want = dy.atan2(dx).to_degrees();
                let turn = wrap(heading(out.motion) - want);
                if turn != 0.0 {
                    out.motion[0] += turn / dpp;
                    look = true;
                }
            }
        }
        out.down.extend(self.held.iter().copied());
        out.down.sort();
        out.down.dedup();
        out.up.retain(|k| !out.down.contains(k));
        let button = self.button || look;
        if button != self.button_was || button {
            out.button = Some(button);
        }
        self.button_was = button;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_looks_become_the_scripted_inputs_commands() {
        let mut p = Person::new(90.0, 0.0, 0.5);
        // The start's heading is told first.
        assert_eq!(p.commands(), [Command::Face(90.0)]);
        assert!(p.commands().is_empty());
        p.held.forward = true;
        p.held.run = true;
        // Mouse right turns right (the heading falls), mouse up looks up.
        p.look([2.0, -1.0]);
        p.press_activate();
        assert_eq!(
            p.commands(),
            [
                Command::Face(89.0),
                Command::Pitch(0.5),
                Command::Key(Key::Forward, true),
                Command::Key(Key::Run, true),
                Command::Activate,
            ]
        );
        p.held.forward = false;
        p.look([0.0, -1e9]);
        assert_eq!(p.pitch, MAX_PITCH_DEGREES);
        p.press_quit();
        assert_eq!(
            p.commands(),
            [
                Command::Pitch(MAX_PITCH_DEGREES),
                Command::Key(Key::Forward, false),
                Command::End
            ]
        );
        // Nothing after the end; the heading wraps.
        p.look([-9100.0, 0.0]);
        assert_eq!(p.heading, 319.0);
        p.press_activate();
        assert!(p.commands().is_empty());
        // Paused: no look and no activation.
        let mut q = Person::new(0.0, 0.0, 0.01);
        q.toggle_pause();
        q.look([10.0, 10.0]);
        q.press_activate();
        assert!(q.commands().is_empty());
    }

    #[test]
    fn the_synthetic_file_reads_and_refuses() {
        let e = parse_synthetic(
            "# a walk\n1 key W down\n1 mouse 3 -2\n2 turn-to 90 # east\n2 pitch-to -15\n2 key E tap\n3 steer 10 20\n4 stop\n5 button tap\n6 key Escape tap\n",
        )
        .unwrap();
        assert_eq!(e.len(), 9);
        assert_eq!(e[0].act, Act::Key(KeyName::W, Press::Down));
        assert_eq!(e[5].act, Act::Steer(10.0, 20.0));
        for bad in [
            "1 key Q down\n",
            "1 key W\n",
            "2 stop\n1 stop\n",
            "1 pitch-to 95\n",
            "1 steer 1\n",
            "1 jump\n",
            "1 stop now\n",
            "1 aim-at 1 2\n",
        ] {
            assert!(parse_synthetic(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_synthetic_person_turns_taps_and_steers_through_the_mouse() {
        let events = parse_synthetic(
            "1 turn-to 90\n1 pitch-to -15\n1 key E tap\n1 pitch-to 0\n2 steer 100 0\n",
        )
        .unwrap();
        let mut s = Synthetic::new(events);
        let dpp = 0.1;
        // Nothing before 1 s.
        assert_eq!(s.frame(0.5, (0.0, 0.0), dpp, None, 1.0), Inject::default());
        // At 1 s: the turn and the look down with the button, then E; the
        // look back up waits for the next frame.
        let f = s.frame(1.0, (0.0, 0.0), dpp, None, 1.0);
        assert!((f.motion[0] - (-900.0)).abs() < 1e-9, "{f:?}");
        assert!((f.motion[1] - 150.0).abs() < 1e-9);
        assert_eq!(f.down, [KeyName::E]);
        assert_eq!(f.button, Some(true));
        let f = s.frame(1.02, (90.0, -15.0), dpp, None, 1.0);
        assert_eq!(f.up, [KeyName::E]);
        assert!((f.motion[1] - (-150.0)).abs() < 1e-9);
        // The button is let go once nothing looks.
        let f = s.frame(1.04, (90.0, 0.0), dpp, None, 1.0);
        assert_eq!(f.button, Some(false));
        let f = s.frame(1.06, (90.0, 0.0), dpp, None, 1.0);
        assert_eq!(f.button, None);
        // Steering: W held, the view turned to the point (east of the feet).
        let at = |x: f64| Some(([x, 0.0, 0.0], 100.0));
        let f = s.frame(2.0, (90.0, 0.0), dpp, at(0.0), 1.0);
        assert_eq!(f.down, [KeyName::W]);
        assert!((f.motion[0] - 900.0).abs() < 1e-9, "{f:?}");
        let f = s.frame(2.1, (0.0, 0.0), dpp, at(50.0), 1.0);
        assert_eq!(f.down, [KeyName::W]);
        assert_eq!(f.motion, [0.0, 0.0]);
        // Near the point W is let go.
        let f = s.frame(2.2, (0.0, 0.0), dpp, at(99.5), 1.0);
        assert_eq!(f.up, [KeyName::W]);
        assert!(f.down.is_empty() && s.done());
        assert_eq!(s.arrived.len(), 1);
        // Aiming at a point: W let go, the view on the point from the eye,
        // again each frame until another look.
        let mut s = Synthetic::new(
            parse_synthetic("0 steer 500 0\n1 aim-at 100 100 100\n2 pitch-to 0\n").unwrap(),
        );
        assert_eq!(
            s.frame(0.0, (0.0, 0.0), 1.0, at(0.0), 1.0).down,
            [KeyName::W]
        );
        let f = s.frame(1.0, (0.0, 0.0), 1.0, at(0.0), 1.0);
        assert_eq!(f.up, [KeyName::W]);
        assert!(
            (f.motion[0] - (-45.0)).abs() < 1e-9 && f.motion[1].abs() < 1e-9,
            "{f:?}"
        );
        let f = s.frame(1.5, (40.0, 0.0), 1.0, at(0.0), 1.0);
        assert!((f.motion[0] - (-5.0)).abs() < 1e-9, "{f:?}");
        let f = s.frame(2.0, (45.0, 0.0), 1.0, at(0.0), 1.0);
        assert_eq!(f.motion, [0.0, 0.0]);
        assert_eq!(wrap(-190.0), 170.0);
        assert_eq!(wrap(180.0), 180.0);
        assert_eq!(wrap(-180.0), 180.0);
    }

    #[test]
    fn the_arrow_keys_turn_and_look_at_a_fixed_rate() {
        let mut p = Person::new(10.0, 0.0, 0.1);
        // Left for half a second turns left (the heading grows).
        p.arrows(true, false, false, false, 0.5);
        assert!((p.heading - (10.0 + ARROW_TURN_DEGREES_PER_SECOND * 0.5)).abs() < 1e-9);
        // Right for a second turns right past 0.
        p.arrows(false, true, false, false, 1.0);
        assert!(
            (p.heading - (10.0 + 45.0 - 90.0 + 360.0)).abs() < 1e-9,
            "{}",
            p.heading
        );
        // Up looks up, down looks down, within the limit; both cancel.
        p.arrows(false, false, true, false, 0.25);
        assert!((p.pitch - 15.0).abs() < 1e-9);
        p.arrows(false, false, true, true, 0.25);
        assert!((p.pitch - 15.0).abs() < 1e-9);
        p.arrows(false, false, false, true, 10.0);
        assert_eq!(p.pitch, -MAX_PITCH_DEGREES);
        // The look goes to the walker as face and pitch.
        let c = p.commands();
        assert!(
            matches!(c[..], [Command::Face(_), Command::Pitch(_)]),
            "{c:?}"
        );
        // Not while paused, not for a frame without time.
        p.toggle_pause();
        let before = (p.heading, p.pitch);
        p.arrows(true, false, true, false, 1.0);
        p.toggle_pause();
        p.arrows(true, false, true, false, 0.0);
        assert_eq!((p.heading, p.pitch), before);
    }

    #[test]
    fn a_drag_by_the_cursor_alone_turns_the_view() {
        // A tool's drag: the cursor warped to the start, the press, then
        // the release carrying the cursor's end position (the window system
        // sends the position before the button's change), in one frame.
        let mut d = Drag::default();
        d.feed(Pointer::Moved([100.0, 100.0]));
        d.feed(Pointer::Button(true));
        d.feed(Pointer::Moved([160.0, 80.0]));
        d.feed(Pointer::Button(false));
        assert_eq!(d.take(), ([60.0, -20.0], true));
        // Nothing held: nothing in the next frame, motion without the
        // button does not count.
        d.feed(Pointer::Moved([300.0, 300.0]));
        assert_eq!(d.take(), ([0.0, 0.0], false));
        // A drag over several frames: the press in one, moves in the next,
        // the release with its motion in the last.
        d.feed(Pointer::Button(true));
        assert_eq!(d.take(), ([0.0, 0.0], true));
        d.feed(Pointer::Moved([310.0, 300.0]));
        assert_eq!(d.take(), ([10.0, 0.0], true));
        d.feed(Pointer::Moved([315.0, 290.0]));
        d.feed(Pointer::Button(false));
        assert_eq!(d.take(), ([5.0, -10.0], true));
        // The cursor leaving the window forgets its place: no jump.
        d.feed(Pointer::Button(true));
        d.feed(Pointer::Left);
        d.feed(Pointer::Moved([0.0, 0.0]));
        d.feed(Pointer::Moved([2.0, 0.0]));
        assert_eq!(d.take(), ([2.0, 0.0], true));
        // Raw motion wins when there is any; never both.
        assert_eq!(look_motion([3.0, 4.0], [60.0, 0.0]), [3.0, 4.0]);
        assert_eq!(look_motion([0.0, 0.0], [60.0, 0.0]), [60.0, 0.0]);
        let mut p = Person::new(0.0, 0.0, 0.5);
        p.look(look_motion([0.0; 2], [60.0, -20.0]));
        assert_eq!((p.heading, p.pitch), (330.0, 10.0));
    }

    #[test]
    fn shift_as_the_movement_keys_modifier_runs() {
        let on = "ModifiersChanged(Modifiers { state: ModifiersState(SHIFT), pressed_mods: ModifiersKeys(0x0) })";
        let both = "ModifiersChanged(Modifiers { state: ModifiersState(SHIFT | CONTROL), pressed_mods: ModifiersKeys(LSHIFT) })";
        let off = "ModifiersChanged(Modifiers { state: ModifiersState(0x0), pressed_mods: ModifiersKeys(LSHIFT) })";
        assert_eq!(shift_of_modifiers(on), Some(true));
        assert_eq!(shift_of_modifiers(both), Some(true));
        // A left Shift attested in the pressed keys is not the state.
        assert_eq!(shift_of_modifiers(off), Some(false));
        assert_eq!(shift_of_modifiers("CursorMoved { position: (1, 2) }"), None);
        let w = Held {
            forward: true,
            ..Held::default()
        };
        assert!(run_held(false, true, w));
        assert!(run_held(true, false, Held::default()));
        // The modifier alone, no movement key: no run.
        assert!(!run_held(false, true, Held::default()));
        let mut p = Person::new(0.0, 0.0, 0.1);
        p.held = Held {
            run: run_held(false, true, w),
            ..w
        };
        assert_eq!(
            p.commands(),
            [
                Command::Key(Key::Forward, true),
                Command::Key(Key::Run, true)
            ]
        );
    }

    #[test]
    fn the_synthetic_person_drags_and_holds_the_arrow_keys() {
        let e =
            parse_synthetic("1 drag 30 -10\n1 drag 5 0\n2 key Left down\n3 key Left up\n").unwrap();
        let mut s = Synthetic::new(e);
        let f = s.frame(1.0, (0.0, 0.0), 0.1, None, 1.0);
        assert_eq!(f.drag, Some([35.0, -10.0]));
        assert_eq!(f.motion, [0.0, 0.0]);
        assert_eq!(f.button, None);
        let f = s.frame(2.0, (0.0, 0.0), 0.1, None, 1.0);
        assert_eq!((f.drag, f.down.as_slice()), (None, &[KeyName::Left][..]));
        let f = s.frame(3.0, (0.0, 0.0), 0.1, None, 1.0);
        assert_eq!(f.up, [KeyName::Left]);
        assert!(parse_synthetic("1 drag 3\n").is_err());
        assert!(parse_synthetic("1 key Home down\n").is_err());
    }
}
