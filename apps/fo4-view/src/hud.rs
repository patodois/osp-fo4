//! What the play window shows in its viewport (`--play-hud`, on with
//! `--play-keys`; `spec/play.md`, On screen): a small crosshair at the
//! centre, under it one line naming what E would activate when a target is
//! in reach (a short action and a plain name) or, for a moment, what an
//! activation did, the subtitle being spoken (wrapped to three lines), the
//! objective the run shows in the top left corner, and "paused",
//! "loading" or, once the person's input is over, "ending". Viewer policy, no rule: presentation only, nothing here
//! reaches the run.
//!
//! The text is drawn with the viewer's own 5 by 7 ASCII font
//! ([`crate::hud_font`]; the pinned Bevy compiles no text renderer and no
//! font) into small images, each shown by a quad of a camera of its own
//! that draws after the scene (`crate::gpu`'s `hud_*` systems).
use crate::hud_font::{GLYPHS, HEIGHT, WIDTH};

/// The pixels between two glyphs, and the box's margin around the text.
const GAP: usize = 1;
const MARGIN: usize = 2;

/// An RGBA8 image (sRGB), rows top first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raster {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

/// One of the HUD's places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    Crosshair,
    /// Under the crosshair: what E would activate.
    Prompt,
    /// Near the bottom: the line being spoken.
    Subtitle,
    /// Near the top: paused, loading.
    Status,
    /// The top left corner: the objective the run shows.
    Objective,
}

impl Slot {
    pub const ALL: [Slot; 5] = [
        Slot::Crosshair,
        Slot::Prompt,
        Slot::Subtitle,
        Slot::Status,
        Slot::Objective,
    ];
}

/// The lines a subtitle may take at most.
pub const SUBTITLE_LINES: usize = 3;

/// `text` broken at spaces into lines of `columns` characters at most
/// (a word longer than a line cut), `most` lines at most, the last marked
/// by three dots when text is left over.
pub fn wrap(text: &str, columns: usize, most: usize) -> Vec<String> {
    let columns = columns.max(4);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        loop {
            let n = line.chars().count();
            let w = word.chars().count();
            if n == 0 && w > columns {
                let head: String = word.chars().take(columns).collect();
                word = word.chars().skip(columns).collect();
                lines.push(head);
                continue;
            }
            if n > 0 && n + 1 + w > columns {
                lines.push(std::mem::take(&mut line));
                continue;
            }
            if n > 0 {
                line.push(' ');
            }
            line.push_str(&word);
            break;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.len() > most {
        lines.truncate(most);
        if let Some(last) = lines.last_mut() {
            *last = fit(&format!("{last}..."), columns);
            if !last.ends_with("...") {
                *last = format!("{}...", last.chars().take(columns - 3).collect::<String>());
            }
        }
    }
    lines
}

/// The text as the font can draw it: printable ASCII, the common
/// typographic quotes and dashes turned into theirs, any other character
/// a question mark, line breaks and tabs a space.
pub fn ascii(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            ' '..='~' => c,
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{2032}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{2033}' => '"',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '\u{2026}' => '.',
            '\n' | '\r' | '\t' => ' ',
            _ => '?',
        })
        .collect()
}

/// `text` cut to `most` characters, the cut marked by three dots.
pub fn fit(text: &str, most: usize) -> String {
    let n = text.chars().count();
    if n <= most {
        return text.to_string();
    }
    let keep = most.saturating_sub(3);
    text.chars()
        .take(keep)
        .chain("...".chars())
        .take(most)
        .collect()
}

/// One line of text in white on a box of translucent black, one texel per
/// font pixel; `None` for an empty line.
#[cfg(test)]
pub fn text(line: &str) -> Option<Raster> {
    lines(&[line.to_string()], false)
}

/// Lines of text in white on one box of translucent black, each centred
/// (or from the left with `left`), one texel per font pixel; `None` when
/// every line is empty.
pub fn lines(lines: &[String], left: bool) -> Option<Raster> {
    let lines: Vec<String> = lines.iter().map(|l| ascii(l)).collect();
    let most = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    if most == 0 {
        return None;
    }
    let w = MARGIN * 2 + most * (WIDTH + GAP) - GAP;
    let row_h = HEIGHT + 2 * GAP;
    let h = MARGIN * 2 + lines.len() * row_h - 2 * GAP;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for _ in 0..w * h {
        rgba.extend_from_slice(&[0, 0, 0, 150]);
    }
    for (r, line) in lines.iter().enumerate() {
        let n = line.chars().count();
        let x_start = if left {
            0
        } else {
            (most - n) * (WIDTH + GAP) / 2
        };
        for (i, c) in line.chars().enumerate() {
            let g = &GLYPHS[(c as usize) - 32];
            let x0 = MARGIN + x_start + i * (WIDTH + GAP);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..WIDTH {
                    if bits & (1 << (WIDTH - 1 - col)) != 0 {
                        let at = ((MARGIN + r * row_h + row) * w + x0 + col) * 4;
                        rgba[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }
    }
    Some(Raster {
        size: [w as u32, h as u32],
        rgba,
    })
}

/// The crosshair: a plus of white strokes edged in black, `arm` texels
/// from the centre each way, with a gap at the centre.
pub fn crosshair(arm: usize) -> Raster {
    let s = 2 * arm + 3;
    let mut rgba = vec![0u8; s * s * 4];
    let c = arm + 1;
    let mut put = |x: usize, y: usize, v: [u8; 4]| {
        let at = (y * s + x) * 4;
        if rgba[at + 3] < v[3] || v[0] == 255 {
            rgba[at..at + 4].copy_from_slice(&v);
        }
    };
    let stroke = |k: usize| (k + 2 <= c || k >= c + 2) && k >= 1 && k + 1 < s;
    // The edge first, then the white over it.
    for pass in 0..2 {
        for k in 0..s {
            if !stroke(k) {
                continue;
            }
            for (x, y) in [(k, c), (c, k)] {
                if pass == 0 {
                    for (dx, dy) in [(0, 1), (2, 1), (1, 0), (1, 2)] {
                        let (xx, yy) = (x + dx, y + dy);
                        if (1..=s).contains(&xx) && (1..=s).contains(&yy) {
                            put(xx - 1, yy - 1, [0, 0, 0, 200]);
                        }
                    }
                } else {
                    put(x, y, [255, 255, 255, 255]);
                }
            }
        }
    }
    Raster {
        size: [s as u32, s as u32],
        rgba,
    }
}

/// What the HUD shows this frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shown {
    /// What E would activate (a short action and a plain name), `None`
    /// when nothing usable is in reach.
    pub prompt: Option<String>,
    /// What the last activation did, shown in the prompt's place for a
    /// moment after it.
    pub confirm: Option<String>,
    pub subtitle: Option<String>,
    /// The objective the run shows.
    pub objective: Option<String>,
    pub paused: bool,
    pub loading: bool,
    /// The person's input is over (Escape): the run goes on to its end and
    /// writes its outputs while the play clock stands; the status line
    /// says so instead of "loading".
    pub ending: bool,
    /// A named platform near the walker is moving under a sequence the
    /// run started, or has just stopped ([`platform_status`]): the status
    /// line says so when nothing else holds it.
    pub platform: Option<String>,
}

/// A named platform as the status line reads it: the boxes of its moving
/// parts' collision where they stand now, whether one carries the walker,
/// and that part's own visible motion
/// (`fo4_play::elevator::Platform::carried_moving`, `carried_starts`,
/// `carried_until`): whether it moves now, the second it sets off when it
/// stands and a started sequence moves it later, the second its motion
/// ends.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformNear {
    pub boxes: Vec<([f64; 3], [f64; 3])>,
    pub carries: bool,
    pub carried_moving: bool,
    pub carried_starts: Option<f64>,
    pub carried_until: Option<f64>,
}

/// How near (units, horizontally from the walker's feet to a moving
/// part's box, and vertically within the same reach) a platform must be
/// for the status line to speak of it. Viewer policy.
pub const PLATFORM_NEAR: f64 = 300.0;
/// How long "platform ready" shows after the motion ended. Viewer policy.
pub const PLATFORM_READY_SECONDS: f64 = 4.0;

/// The status line's platform cue at second `t` for a walker whose feet
/// stand at `feet`. It speaks only of a platform one of whose parts
/// carries the walker, and only of that part's own motion: "platform
/// moving - ready in N s" while the part moves (N: whole seconds to the
/// end of its motion, rounded up); "platform starts in N s" while it
/// stands and a sequence the run started moves it later (N: whole seconds
/// to that start); "platform ready" for [`PLATFORM_READY_SECONDS`] after
/// `ended` (the end second the cue last counted down to, kept by the
/// caller) while the walker is still on it; `None` otherwise, and always
/// when no part carries the walker (a platform beside the walker, even one
/// its activation set off, says nothing). Returns the cue and the end
/// second to keep. Viewer policy: the seconds are the sequences' own keys
/// read as visible motion (`fo4_play::elevator::CUE_STILL`), nothing about
/// the game.
pub fn platform_status(
    near: &[PlatformNear],
    feet: [f64; 3],
    t: f64,
    ended: Option<f64>,
) -> (Option<String>, Option<f64>) {
    let close = |b: &([f64; 3], [f64; 3])| {
        let d = |i: usize| (b.0[i] - feet[i]).max(feet[i] - b.1[i]).max(0.0);
        d(0).hypot(d(1)) <= PLATFORM_NEAR && d(2) <= PLATFORM_NEAR
    };
    let ours: Vec<&PlatformNear> = near
        .iter()
        .filter(|p| p.carries && p.boxes.iter().any(close))
        .collect();
    let moving = ours
        .iter()
        .filter(|p| p.carried_moving)
        .filter_map(|p| p.carried_until.filter(|e| *e > t))
        .fold(None, |m: Option<f64>, e| Some(m.map_or(e, |m| m.max(e))));
    if let Some(e) = moving {
        let n = (e - t).ceil().max(1.0);
        return (
            Some(format!("platform moving - ready in {n:.0} s")),
            Some(e),
        );
    }
    let starts = ours
        .iter()
        .filter_map(|p| p.carried_starts.filter(|s| *s > t))
        .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.min(s))));
    if let Some(s) = starts {
        let n = (s - t).ceil().max(1.0);
        return (Some(format!("platform starts in {n:.0} s")), ended);
    }
    match ended {
        Some(e) if t >= e && t - e < PLATFORM_READY_SECONDS => (
            (!ours.is_empty()).then(|| "platform ready".to_string()),
            Some(e),
        ),
        Some(e) if t < e => (None, Some(e)),
        _ => (None, None),
    }
}

/// How long (seconds) the walker must be held against a blocking shape
/// before the status line says so. Viewer policy.
pub const HELD_SECONDS: f64 = 1.0;
/// The height above the feet (units) at or below which the top of what
/// holds the walker is a "low obstacle": two fifths of the 120-unit eye
/// height, about knee height. Viewer policy, nothing about the game.
pub const HELD_LOW: f64 = 48.0;

/// The status line's held cue (`--play-held-cue`, a viewer aid that
/// claims nothing about the game): `held`, the second the walker's hold
/// against a blocking shape began and how far the top of the primitive it
/// last met stands above its feet (`fo4_play::walker::Held::since` and
/// `top`). After [`HELD_SECONDS`] without progress, "blocked - low
/// obstacle" when the top of
/// the primitive it last met stands at most [`HELD_LOW`] above its feet,
/// "blocked" otherwise (or when the top is not known); `None` before. No
/// object is named: the cue says why the walker stands when the cause may
/// lie below the eye's frame.
pub fn held_status(held: Option<(f64, Option<f64>)>, t: f64) -> Option<String> {
    let (_, top) = held.filter(|h| t - h.0 >= HELD_SECONDS)?;
    Some(match top {
        Some(top) if top <= HELD_LOW => "blocked - low obstacle".to_string(),
        _ => "blocked".to_string(),
    })
}

impl Shown {
    /// The text of a one-line slot (`None`: the slot is hidden; a subtitle
    /// as its lines joined). `columns`: the characters a line may hold.
    #[cfg(test)]
    pub fn text(&self, slot: Slot, columns: usize) -> Option<String> {
        self.lines(slot, columns).map(|l| l.join(" "))
    }

    /// The lines of a slot (`None`: the slot is hidden): one line, cut to
    /// `columns` characters, but the subtitle, wrapped to
    /// [`SUBTITLE_LINES`] lines.
    pub fn lines(&self, slot: Slot, columns: usize) -> Option<Vec<String>> {
        let t = match slot {
            Slot::Crosshair => return Some(vec![String::new()]),
            Slot::Prompt => match &self.confirm {
                Some(c) => format!("E  {c}"),
                None => format!("E  {}", self.prompt.as_ref()?),
            },
            Slot::Subtitle => {
                let l = wrap(&ascii(self.subtitle.as_ref()?), columns, SUBTITLE_LINES);
                return (!l.is_empty()).then_some(l);
            }
            Slot::Objective => self.objective.clone()?,
            Slot::Status if self.ending => "ending".into(),
            Slot::Status => match (self.paused, self.loading) {
                (true, true) => "paused - loading".into(),
                (true, false) => "paused".into(),
                (false, true) => "loading".into(),
                (false, false) => self.platform.clone()?,
            },
        };
        let t = fit(&ascii(&t), columns);
        (!t.trim().is_empty()).then(|| vec![t])
    }
}

/// The size of one font pixel on screen (window pixels) for a window
/// `height` pixels high: 2 at least, larger in tall windows.
pub fn scale(height: f32) -> f32 {
    (height / 360.0).round().max(2.0)
}

/// Where a slot's quad is centred (window pixels from the centre, y up) in
/// a window `size` with font pixels of `k`, for a one-line raster.
#[cfg(test)]
pub fn place(slot: Slot, size: [f32; 2], k: f32) -> [f32; 2] {
    place_sized(slot, size, k, [0, (HEIGHT + 2 * MARGIN) as u32])
}

/// Where a slot's quad of `raster` texels is centred: the subtitle's
/// bottom edge where a one-line subtitle's is (more lines grow upward),
/// the objective in the top left corner, the others as [`place`] says.
pub fn place_sized(slot: Slot, size: [f32; 2], k: f32, raster: [u32; 2]) -> [f32; 2] {
    let line = (HEIGHT + 2 * MARGIN) as f32 * k;
    let [rw, rh] = [raster[0] as f32 * k, raster[1] as f32 * k];
    match slot {
        Slot::Crosshair => [0.0, 0.0],
        Slot::Prompt => [0.0, -(2.0 * line)],
        Slot::Subtitle => [0.0, -size[1] / 2.0 + 1.5 * line + rh / 2.0],
        Slot::Status => [0.0, size[1] / 2.0 - 2.0 * line],
        Slot::Objective => [
            -size[0] / 2.0 + line + rw / 2.0,
            size[1] / 2.0 - line - rh / 2.0,
        ],
    }
}

/// How many characters a line may hold in a window `width` pixels wide.
pub fn columns(width: f32, k: f32) -> usize {
    let each = (WIDTH + GAP) as f32 * k;
    ((width * 0.9 / each).floor() as usize).max(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_printable_code_has_a_glyph_and_text_is_drawn_in_it() {
        assert_eq!(GLYPHS.len(), 95);
        // Space is empty; every other glyph has a pixel.
        assert!(GLYPHS[0].iter().all(|r| *r == 0));
        for (i, g) in GLYPHS.iter().enumerate().skip(1) {
            assert!(g.iter().any(|r| *r != 0), "code {}", i + 32);
            assert!(g.iter().all(|r| *r < 32), "code {}", i + 32);
        }
        let r = text("E  door").unwrap();
        assert_eq!(r.size, [2 * 2 + 7 * 6 - 1, 11]);
        assert_eq!(r.rgba.len(), (r.size[0] * r.size[1] * 4) as usize);
        // The E's top row: five white pixels from the margin.
        let row = 2 * r.size[0] as usize;
        let white = |x: usize| r.rgba[(row + x) * 4..(row + x) * 4 + 4] == [255, 255, 255, 255];
        assert!((2..7).all(white));
        assert!(!white(7));
        assert!(text("").is_none());
    }

    #[test]
    fn text_is_made_ascii_and_cut_to_the_line() {
        assert_eq!(
            ascii("It\u{2019}s \u{201c}fine\u{201d} \u{2014} caf\u{e9}\n"),
            "It's \"fine\" - caf? "
        );
        assert_eq!(fit("abcdefghij", 8), "abcde...");
        assert_eq!(fit("abc", 8), "abc");
        let s = Shown {
            prompt: Some("DOOR front door".into()),
            subtitle: Some("You think?".into()),
            paused: true,
            ..Shown::default()
        };
        assert_eq!(s.text(Slot::Prompt, 80).unwrap(), "E  DOOR front door");
        assert_eq!(s.text(Slot::Prompt, 10).unwrap(), "E  DOOR...");
        assert_eq!(s.text(Slot::Status, 80).unwrap(), "paused");
        assert_eq!(s.text(Slot::Subtitle, 80).unwrap(), "You think?");
        let none = Shown::default();
        assert!(none.text(Slot::Prompt, 80).is_none());
        assert!(none.text(Slot::Status, 80).is_none());
        assert!(none.text(Slot::Subtitle, 80).is_none());
        assert_eq!(none.text(Slot::Crosshair, 80), Some(String::new()));
        let loading = Shown {
            loading: true,
            ..Shown::default()
        };
        assert_eq!(loading.text(Slot::Status, 80).unwrap(), "loading");
        // After the input's end the standing clock is the run finishing,
        // not a load.
        let ending = Shown {
            ending: true,
            ..loading
        };
        assert_eq!(ending.text(Slot::Status, 80).unwrap(), "ending");
        // A platform cue shows only when nothing else holds the line.
        let platform = Shown {
            platform: Some("platform ready".into()),
            ..Shown::default()
        };
        assert_eq!(platform.text(Slot::Status, 80).unwrap(), "platform ready");
        let held = Shown {
            loading: true,
            ..platform
        };
        assert_eq!(held.text(Slot::Status, 80).unwrap(), "loading");
    }

    /// A deck under the walker: its own motion's seconds.
    fn deck(moving: bool, starts: Option<f64>, until: Option<f64>) -> PlatformNear {
        PlatformNear {
            boxes: vec![([0.0, 0.0, 50.0], [100.0, 100.0, 140.0])],
            carries: true,
            carried_moving: moving,
            carried_starts: starts,
            carried_until: until,
        }
    }

    /// The 30th sessions: the walker stood against shapes below the eye's
    /// frame (a rail at the shin, a tipped panel, a bank). A synthetic hold:
    /// nothing for the first second, then "blocked", with "low obstacle"
    /// when what holds it is no higher than the knee.
    #[test]
    fn a_walker_held_a_second_says_blocked_and_whether_low() {
        let held = |top: Option<f64>| (10.0, top);
        assert_eq!(held_status(None, 20.0), None);
        assert_eq!(held_status(Some(held(Some(24.0))), 10.5), None);
        assert_eq!(
            held_status(Some(held(Some(24.0))), 11.0).as_deref(),
            Some("blocked - low obstacle")
        );
        assert_eq!(
            held_status(Some(held(Some(72.0))), 11.2).as_deref(),
            Some("blocked")
        );
        assert_eq!(
            held_status(Some(held(None)), 11.2).as_deref(),
            Some("blocked")
        );
        assert_eq!(
            held_status(Some(held(Some(HELD_LOW))), 12.0).as_deref(),
            Some("blocked - low obstacle")
        );
    }

    #[test]
    fn a_moving_deck_under_the_walker_counts_down_to_the_end_of_its_motion() {
        let feet = [50.0, 50.0, 140.0];
        let (cue, kept) = platform_status(&[deck(true, None, Some(43.3))], feet, 10.0, None);
        assert_eq!(cue.as_deref(), Some("platform moving - ready in 34 s"));
        assert_eq!(kept, Some(43.3));
        // Far away (a box the walker is not near): nothing.
        let far = [150.0 + PLATFORM_NEAR + 1.0, 50.0, 0.0];
        assert_eq!(
            platform_status(&[deck(true, None, Some(43.3))], far, 10.0, None),
            (None, None)
        );
        // Stopped: "platform ready" for a few seconds after the kept end.
        let (cue, kept) = platform_status(&[deck(false, None, None)], feet, 44.0, Some(43.3));
        assert_eq!(cue.as_deref(), Some("platform ready"));
        assert_eq!(kept, Some(43.3));
        let late = 43.3 + PLATFORM_READY_SECONDS + 0.1;
        assert_eq!(
            platform_status(&[deck(false, None, None)], feet, late, Some(43.3)),
            (None, None)
        );
    }

    /// The 30th sessions (a): on the surface lift's upper deck the count
    /// ran while the deck stood (its first sequence holds the deck's pose,
    /// with noise in its keys). A deck that stands says when it sets off,
    /// when that is known, and nothing else; the count starts when it moves.
    #[test]
    fn a_standing_deck_under_the_walker_says_when_it_starts_not_that_it_moves() {
        let feet = [50.0, 50.0, 140.0];
        let (cue, kept) = platform_status(&[deck(false, Some(12.4), Some(31.0))], feet, 10.0, None);
        assert_eq!(cue.as_deref(), Some("platform starts in 3 s"));
        assert_eq!(kept, None);
        // Standing with no start known: nothing.
        assert_eq!(
            platform_status(&[deck(false, None, None)], feet, 10.0, None),
            (None, None)
        );
        // Then moving: the count to its own end.
        let (cue, _) = platform_status(&[deck(true, None, Some(31.0))], feet, 12.5, None);
        assert_eq!(cue.as_deref(), Some("platform moving - ready in 19 s"));
    }

    /// The 30th sessions (b): beside the exit ramp, on the fixed floor, the
    /// count ran for the ramp the person's activation set moving. And the
    /// 28th: platforms the person had nothing to do with. A platform none
    /// of whose parts carries the walker says nothing, moving or stopped.
    #[test]
    fn a_platform_not_under_the_walker_says_nothing() {
        let beside = |moving: bool, until: Option<f64>| PlatformNear {
            carries: false,
            ..deck(moving, None, until)
        };
        let feet = [150.0, 50.0, 0.0];
        assert_eq!(
            platform_status(&[beside(true, Some(43.3))], feet, 10.0, None),
            (None, None)
        );
        assert_eq!(
            platform_status(&[beside(false, None)], feet, 44.0, Some(43.3)).0,
            None
        );
        // Beside the walker's own deck, the other's later end is not
        // counted down to.
        let own = deck(true, None, Some(20.0));
        let (cue, kept) = platform_status(
            &[beside(true, Some(43.3)), own],
            [50.0, 50.0, 140.0],
            10.0,
            None,
        );
        assert_eq!(cue.as_deref(), Some("platform moving - ready in 10 s"));
        assert_eq!(kept, Some(20.0));
    }

    /// The 30th sessions (c): just after the load door that puts the walker
    /// on the rising surface deck, the count is the deck's own rise (it
    /// moves from its sequence's first key): it shows, and goes down.
    #[test]
    fn the_deck_a_load_door_puts_the_walker_on_counts_its_own_rise() {
        let feet = [50.0, 50.0, 140.0];
        let rising = deck(true, None, Some(1703.9));
        let (cue, kept) = platform_status(&[rising.clone()], feet, 1692.5, Some(1692.1));
        assert_eq!(cue.as_deref(), Some("platform moving - ready in 12 s"));
        assert_eq!(kept, Some(1703.9));
        let (cue, _) = platform_status(&[rising], feet, 1698.0, kept);
        assert_eq!(cue.as_deref(), Some("platform moving - ready in 6 s"));
    }

    /// The 29th sessions: on the lower deck, settled under the walker
    /// while the run's scene held it, the cue counted down to the end of
    /// the deck's sequence, whose other parts still moved. The carrying
    /// part's own motion is all that counts.
    #[test]
    fn a_settled_deck_under_the_walker_counts_nothing_while_its_other_parts_move() {
        let feet = [50.0, 50.0, 140.0];
        let (cue, kept) = platform_status(&[deck(true, None, Some(43.3))], feet, 40.0, None);
        assert_eq!(cue.as_deref(), Some("platform moving - ready in 4 s"));
        assert_eq!(kept, Some(43.3));
        let (cue, _) = platform_status(&[deck(false, None, None)], feet, 46.7, Some(43.3));
        assert_eq!(cue.as_deref(), Some("platform ready"));
        assert_eq!(
            platform_status(&[deck(false, None, None)], feet, 50.0, Some(43.3)),
            (None, None)
        );
    }

    #[test]
    fn the_crosshair_is_a_plus_with_an_open_centre() {
        let r = crosshair(5);
        assert_eq!(r.size, [13, 13]);
        let px = |x: usize, y: usize| {
            let at = (y * 13 + x) * 4;
            r.rgba[at..at + 4].to_vec()
        };
        assert_eq!(px(6, 6)[3], 0, "the centre is open");
        assert_eq!(px(6, 2), [255, 255, 255, 255]);
        assert_eq!(px(2, 6), [255, 255, 255, 255]);
        assert_eq!(px(0, 0)[3], 0);
        assert_eq!(scale(720.0), 2.0);
        assert_eq!(scale(1440.0), 4.0);
        assert!(columns(1280.0, 2.0) >= 90);
        let p = place(Slot::Subtitle, [1280.0, 720.0], 2.0);
        assert!(p[1] < -300.0);
        // The objective in the top left corner.
        let o = place_sized(Slot::Objective, [1280.0, 720.0], 2.0, [100, 11]);
        assert!(o[0] < -500.0 && o[1] > 300.0, "{o:?}");
    }

    #[test]
    fn long_subtitles_wrap_to_three_lines_and_confirmations_take_the_prompt() {
        let w = wrap("one two three four five six seven", 10, 3);
        assert_eq!(w, ["one two", "three four", "five si..."]);
        assert_eq!(
            wrap("one two three four five six", 12, 2),
            ["one two", "three fou..."]
        );
        assert_eq!(wrap("short", 10, 3), ["short"]);
        assert_eq!(wrap("abcdefghijklmn", 6, 3), ["abcdef", "ghijkl", "mn"]);
        assert!(wrap("", 10, 3).is_empty());
        let s = Shown {
            prompt: Some("open DOOR".into()),
            confirm: Some("opened".into()),
            subtitle: Some("a b c d e f g h i j k l m n o p q r s t".into()),
            objective: Some("Leave the place".into()),
            ..Shown::default()
        };
        assert_eq!(s.text(Slot::Prompt, 80).unwrap(), "E  opened");
        let l = s.lines(Slot::Subtitle, 12).unwrap();
        assert_eq!(l.len(), 3);
        assert!(l[2].ends_with("..."));
        assert_eq!(s.text(Slot::Objective, 80).unwrap(), "Leave the place");
        let r = lines(&l, false).unwrap();
        assert_eq!(r.size[1] as usize, 2 * 2 + 3 * (7 + 2) - 2);
        let without = Shown { confirm: None, ..s };
        assert_eq!(without.text(Slot::Prompt, 80).unwrap(), "E  open DOOR");
    }
}
