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
                (false, false) => return None,
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
