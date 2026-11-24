//! The scripted input of the headless play mode: what a player would do
//! with the keyboard and the mouse, at seconds of the play clock.
//!
//! One command per line, `SECONDS COMMAND [VALUE]`; `#` starts a comment.
//! Seconds never go back. Commands:
//!
//! - `forward`, `back`, `left`, `right`, `run` with `on` or `off`: a key held
//!   down or let go (`left` and `right` strafe);
//! - `face DEGREES`: the heading, degrees from +X towards +Y (the mouse
//!   turned to it); `turn DEGREES`: the heading changed by that much;
//! - `pitch DEGREES`: the eye's angle above the horizontal (up positive);
//! - `toward X Y`: walk to a point: at each of the walker's ticks the
//!   heading turns to the point (horizontally) and forward is held, until
//!   the walker is within half a tick's move of it (a player steering by
//!   eye; `face`, `turn` or `forward off` ends it and lets forward go);
//! - `activate`: the activation key, on what the crosshair points at;
//! - `end`: the input is over at that second.
use std::fmt;

/// A key that can be held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Forward,
    Back,
    Left,
    Right,
    Run,
}

/// One command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Key(Key, bool),
    Face(f64),
    Turn(f64),
    Pitch(f64),
    /// Walk to a point (x, y).
    Toward(f64, f64),
    Activate,
    End,
}

/// A command at its second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    pub t: f64,
    pub command: Command,
}

impl fmt::Display for Command {
    /// The command as a line of the input holds it after its second.
    /// Numbers are written as Rust writes an `f64` (the shortest text that
    /// reads back to the same value), so a command written and read again
    /// is the same command bit for bit.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let on = |b: bool| if b { "on" } else { "off" };
        match self {
            Command::Key(k, b) => {
                let k = match k {
                    Key::Forward => "forward",
                    Key::Back => "back",
                    Key::Left => "left",
                    Key::Right => "right",
                    Key::Run => "run",
                };
                write!(f, "{k} {}", on(*b))
            }
            Command::Face(d) => write!(f, "face {d}"),
            Command::Turn(d) => write!(f, "turn {d}"),
            Command::Pitch(p) => write!(f, "pitch {p}"),
            Command::Toward(x, y) => write!(f, "toward {x} {y}"),
            Command::Activate => write!(f, "activate"),
            Command::End => write!(f, "end"),
        }
    }
}

impl fmt::Display for Event {
    /// The event as one line of the input (`SECONDS COMMAND [VALUE]`), the
    /// second written as the numbers of [`Command`]'s text are.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.t, self.command)
    }
}

/// The input, in time order (the file's order on a tie).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Script {
    pub events: Vec<Event>,
}

/// A line the parser refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub line: usize,
    pub why: String,
}
impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PlayInput: line {}: {}", self.line, self.why)
    }
}
impl std::error::Error for Refused {}

impl Script {
    pub fn parse(text: &str) -> Result<Script, Refused> {
        let mut events: Vec<Event> = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let bad = |why: &str| Refused {
                line: n + 1,
                why: why.to_string(),
            };
            let w: Vec<&str> = line.split_whitespace().collect();
            let num = |s: &str| -> Result<f64, Refused> {
                s.parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| bad("a number that is not finite"))
            };
            let t = num(w[0])?;
            if t < 0.0 {
                return Err(bad("a second before 0"));
            }
            if events.last().is_some_and(|e| e.t > t) {
                return Err(bad("time goes back"));
            }
            let value = |i: usize| w.get(i).copied().ok_or_else(|| bad("a value is missing"));
            let command = match w.get(1).copied() {
                Some(k @ ("forward" | "back" | "left" | "right" | "run")) => {
                    let key = match k {
                        "forward" => Key::Forward,
                        "back" => Key::Back,
                        "left" => Key::Left,
                        "right" => Key::Right,
                        _ => Key::Run,
                    };
                    let on = match value(2)? {
                        "on" => true,
                        "off" => false,
                        _ => return Err(bad("a key is on or off")),
                    };
                    Command::Key(key, on)
                }
                Some("face") => Command::Face(num(value(2)?)?),
                Some("turn") => Command::Turn(num(value(2)?)?),
                Some("pitch") => {
                    let p = num(value(2)?)?;
                    if !(-90.0..=90.0).contains(&p) {
                        return Err(bad("pitch is between -90 and 90 degrees"));
                    }
                    Command::Pitch(p)
                }
                Some("toward") => Command::Toward(num(value(2)?)?, num(value(3)?)?),
                Some("activate") => Command::Activate,
                Some("end") => Command::End,
                _ => return Err(bad("expected SECONDS forward|back|left|right|run on|off, face|turn|pitch DEGREES, toward X Y, activate or end")),
            };
            let extra = match command {
                Command::Toward(..) => 4,
                Command::Key(..) | Command::Face(_) | Command::Turn(_) | Command::Pitch(_) => 3,
                Command::Activate | Command::End => 2,
            };
            if w.len() != extra {
                return Err(bad("extra words"));
            }
            events.push(Event { t, command });
        }
        Ok(Script { events })
    }
    /// The second of the first `end`, if any.
    pub fn end(&self) -> Option<f64> {
        self.events
            .iter()
            .find(|e| e.command == Command::End)
            .map(|e| e.t)
    }
}

/// The keys held and where the player looks, changed command by command.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Controls {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub run: bool,
    /// Degrees from +X towards +Y.
    pub heading: f64,
    /// Degrees above the horizontal.
    pub pitch: f64,
    /// The point a `toward` command walks to.
    pub target: Option<[f64; 2]>,
}
impl Controls {
    /// Applies a command; `Activate` and `End` change nothing here.
    pub fn apply(&mut self, c: Command) {
        match c {
            Command::Key(k, on) => match k {
                Key::Forward => {
                    self.forward = on;
                    if !on {
                        self.target = None;
                    }
                }
                Key::Back => self.back = on,
                Key::Left => self.left = on,
                Key::Right => self.right = on,
                Key::Run => self.run = on,
            },
            Command::Face(d) => {
                self.heading = d;
                self.drop_target();
            }
            Command::Turn(d) => {
                self.heading += d;
                self.drop_target();
            }
            Command::Pitch(p) => self.pitch = p,
            Command::Toward(x, y) => {
                self.target = Some([x, y]);
                self.forward = true;
            }
            Command::Activate | Command::End => {}
        }
    }
    /// The walker's input: forward and strafe in [-1, 1], heading in
    /// radians.
    pub fn walker(&self) -> fo4_walk::Input {
        let axis = |plus: bool, minus: bool| f64::from(u8::from(plus)) - f64::from(u8::from(minus));
        fo4_walk::Input {
            forward: axis(self.forward, self.back),
            right: axis(self.right, self.left),
            heading: self.heading.to_radians(),
            run: self.run,
        }
    }
    /// Ends a walk to a point: its forward key is let go.
    fn drop_target(&mut self) {
        if self.target.take().is_some() {
            self.forward = false;
        }
    }
    /// Steers to the target from the walker's feet: the heading toward it,
    /// and when it is within `arrive` units, forward let go and the target
    /// dropped. Returns whether it arrived.
    pub fn steer(&mut self, feet: [f64; 3], arrive: f64) -> bool {
        let Some([x, y]) = self.target else {
            return false;
        };
        let (dx, dy) = (x - feet[0], y - feet[1]);
        if dx.hypot(dy) <= arrive {
            self.target = None;
            self.forward = false;
            return true;
        }
        self.heading = dy.atan2(dx).to_degrees();
        false
    }
    /// The eye's unit direction.
    pub fn look(&self) -> [f64; 3] {
        let (h, p) = (self.heading.to_radians(), self.pitch.to_radians());
        [h.cos() * p.cos(), h.sin() * p.cos(), p.sin()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands_in_time_order() {
        let s = Script::parse(
            "# a walk\n0 face 90\n1.5 forward on\n2 turn -45 # left\n2 pitch -10\n3 forward off\n3 activate\n4 end\n",
        )
        .unwrap();
        assert_eq!(s.events.len(), 7);
        assert_eq!(s.events[1].command, Command::Key(Key::Forward, true));
        assert_eq!(s.end(), Some(4.0));
        let mut c = Controls::default();
        for e in &s.events {
            c.apply(e.command);
        }
        assert_eq!(c.heading, 45.0);
        assert_eq!(c.pitch, -10.0);
        assert!(!c.forward);
    }

    #[test]
    fn refuses_bad_lines() {
        for (text, line) in [
            ("1 forward\n", 1),
            ("2 face 0\n1 face 1\n", 2),
            ("0 jump on\n", 1),
            ("0 pitch 120\n", 1),
            ("0 activate now\n", 1),
            ("0 toward 1\n", 1),
            ("-1 end\n", 1),
            ("x face 0\n", 1),
        ] {
            assert_eq!(Script::parse(text).unwrap_err().line, line, "{text}");
        }
    }

    #[test]
    fn controls_give_the_walker_input() {
        let mut c = Controls {
            heading: 90.0,
            ..Controls::default()
        };
        c.apply(Command::Key(Key::Forward, true));
        c.apply(Command::Key(Key::Left, true));
        let i = c.walker();
        assert_eq!((i.forward, i.right), (1.0, -1.0));
        assert!((i.heading - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let l = Controls {
            pitch: 90.0,
            ..Controls::default()
        }
        .look();
        assert!((l[2] - 1.0).abs() < 1e-12);
    }
}

#[cfg(test)]
mod text_tests {
    use super::*;

    #[test]
    fn a_command_written_reads_back_bit_for_bit() {
        // In time order, as an input holds them.
        let seconds = [
            0.0,
            f64::MIN_POSITIVE,
            1e-7,
            0.1 + 0.2,
            1.0 / 3.0,
            17.614,
            123_456.789_012_345_67,
        ];
        let commands = [
            Command::Key(Key::Forward, true),
            Command::Key(Key::Back, false),
            Command::Key(Key::Left, true),
            Command::Key(Key::Right, false),
            Command::Key(Key::Run, true),
            Command::Face(-0.0),
            Command::Face(359.999_999_999_999_9),
            Command::Turn(-1.0 / 7.0),
            Command::Pitch(-89.0 + 1e-12),
            Command::Toward(-80_443.83, 90_985.980_000_000_01),
            Command::Activate,
            Command::End,
        ];
        let mut events = Vec::new();
        for (i, c) in commands.iter().enumerate() {
            events.push(Event {
                t: seconds[i * seconds.len() / commands.len()],
                command: *c,
            });
        }
        let text: String = events.iter().map(|e| format!("{e}\n")).collect();
        let back = Script::parse(&text).unwrap();
        assert_eq!(back.events.len(), events.len());
        for (a, b) in events.iter().zip(&back.events) {
            assert_eq!(a.t.to_bits(), b.t.to_bits(), "{text}");
            let bits = |c: &Command| match *c {
                Command::Face(d) | Command::Turn(d) | Command::Pitch(d) => vec![d.to_bits()],
                Command::Toward(x, y) => vec![x.to_bits(), y.to_bits()],
                _ => Vec::new(),
            };
            assert_eq!(a.command, b.command);
            assert_eq!(bits(&a.command), bits(&b.command));
        }
    }
}

#[cfg(test)]
mod steer_tests {
    use super::*;

    #[test]
    fn toward_steers_to_the_point_and_stops_there() {
        let s = Script::parse("0 toward 100 100\n").unwrap();
        let mut c = Controls::default();
        c.apply(s.events[0].command);
        assert!(c.forward && c.target == Some([100.0, 100.0]));
        assert!(!c.steer([0.0, 0.0, 0.0], 1.0));
        assert!((c.heading - 45.0).abs() < 1e-12);
        assert!(c.steer([99.5, 100.0, 0.0], 1.0));
        assert!(!c.forward && c.target.is_none());
        // A face command ends the walk to a point.
        c.apply(Command::Toward(5.0, 0.0));
        c.apply(Command::Face(10.0));
        assert!(c.target.is_none() && !c.forward);
    }
}
