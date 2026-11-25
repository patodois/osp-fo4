//! What a scripted run writes beside `quest_run`'s outputs, the same for
//! every caller (`play_run`, the window's scripted run): the walker's lines
//! of `play.txt` ([`walker`]) and `play-track.txt` ([`track_text`]).
use crate::walker::Log;
use crate::Point;
use std::fmt::Write as _;

/// The run's track as `play-track.txt` holds it (readable by `--track`).
pub fn track_text(points: &[Point]) -> String {
    let mut s = String::from("# the run's track in the play mode: SECONDS SPACE X Y Z\n");
    for p in points {
        let _ = writeln!(
            s,
            "{} {} {} {} {}",
            p.t,
            fo4_vm::bind::record_key(&p.space),
            p.pos[0],
            p.pos[1],
            p.pos[2]
        );
    }
    s
}

/// The run's track to write: the caller's points up to second 0 (the
/// start), then the walker's.
pub fn track_points(caller: &fo4_vm::track::Track, l: &Log) -> Vec<Point> {
    let mut points: Vec<Point> = caller
        .points
        .iter()
        .filter(|p| p.t <= 0.0)
        .cloned()
        .collect();
    points.extend(l.track.iter().cloned());
    points
}

/// What the walker did, the lines of `play.txt` after the run.
pub fn walker(l: &Log, report: &mut String) {
    if let Some((p, feet, ticks)) = &l.start {
        let _ = writeln!(
            report,
            "start: track point {:?} at {} s; walker settled in {ticks} ticks at {:?}",
            p.pos, p.t, feet
        );
    }
    let _ = writeln!(
        report,
        "walker: {} ticks, {} slices ({} standing), {} points, {} blocked, {} stepped, {} airborne",
        l.ticks, l.slices, l.standing_slices, l.points, l.blocked, l.stepped, l.airborne
    );
    let _ = writeln!(
        report,
        "cost: walker ticks {:.6} s in all, {:.3} us a tick",
        l.tick_seconds,
        1e6 * l.tick_seconds / (l.ticks.max(1) as f64)
    );
    for (t, feet) in &l.arrivals {
        let _ = writeln!(report, "toward: arrived at {t} s at {feet:?}");
    }
    if let Some(s) = &l.live {
        let _ = writeln!(
            report,
            "live world: {} applies of the run's state, {} references entered (or came back), {} left, {} moved; {:.6} s in all, longest {:.6} s",
            s.applies, s.entered, s.left, s.moved, s.seconds, s.longest
        );
    }
    for (t, door, held) in &l.held_doors {
        let _ = writeln!(
            report,
            "held door: at {t} s the box of {} {} the walking world ({})",
            fo4_vm::bind::record_key(door),
            if *held {
                "entered (closed by the run, its activation blocked)"
            } else {
                "left"
            },
            crate::rules::HELD_DOORS
        );
    }
    for (t, cell, feet, secs) in &l.grid {
        let _ = writeln!(
            report,
            "grid: at {t} s the walking world built again about cell {} (feet {feet:?}) in {secs:.2} s",
            fo4_vm::bind::record_key(cell)
        );
    }
    for d in l.doors.iter().filter(|d| d.rule != crate::rules::LOAD_DOOR) {
        let _ = writeln!(
            report,
            "player {} followed at {} s: to {} in {}: cell {}, position {:?}, feet settled at {:?}; walking world built in {:.2} s\n{}",
            if d.rule == crate::rules::PLAYER_SIT { "sit" } else { "MoveTo" },
            d.t,
            fo4_vm::bind::record_key(&d.to.to),
            fo4_vm::bind::record_key(&d.to.space),
            fo4_vm::bind::record_key(&d.cell),
            d.to.pos,
            d.feet,
            d.seconds,
            d.report
        );
    }
    for (t, why) in &l.player_moves {
        let _ = writeln!(report, "player place at {t} s: {why}");
    }
    for d in l.doors.iter().filter(|d| d.rule == crate::rules::LOAD_DOOR) {
        let _ = writeln!(
            report,
            "load door at {} s: {} to {} in {}: cell {}, teleport {:?}, feet settled at {:?}; walking world built in {:.2} s\n{}",
            d.t,
            fo4_vm::bind::record_key(&d.door),
            fo4_vm::bind::record_key(&d.to.to),
            fo4_vm::bind::record_key(&d.to.space),
            fo4_vm::bind::record_key(&d.cell),
            d.to.pos,
            d.feet,
            d.seconds,
            d.report
        );
    }
    for (t, x) in &l.elevators {
        let _ = writeln!(report, "elevator at {t} s: {x}");
    }
    for r in &l.rules {
        let _ = writeln!(report, "rule relied on\t{r}");
    }
    for (t, h, p) in &l.looks {
        let _ = writeln!(report, "look at {t} s: heading {h} pitch {p}");
    }
    for (t, eye, dir, what) in &l.activations {
        let what = match what {
            Ok((k, d)) => format!("{} at {d:.1} units", fo4_vm::bind::record_key(k)),
            Err(why) => why.clone(),
        };
        let _ = writeln!(
            report,
            "activate at {t} s from {eye:?} along {dir:?}: {what}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_plugin::FormKey;

    #[test]
    fn the_play_track_reads_as_a_track() {
        let p = Point {
            t: 1.5,
            space: FormKey {
                plugin: "a.esm".into(),
                object_id: 0x3c,
            },
            pos: [1.0, -2.0, 3.25],
            facing: None,
        };
        assert_eq!(
            track_text(&[p]),
            "# the run's track in the play mode: SECONDS SPACE X Y Z\n1.5 a.esm:0000003C 1 -2 3.25\n"
        );
    }

    #[test]
    fn the_walker_lines_name_what_it_did() {
        let mut l = Log {
            ticks: 3,
            slices: 2,
            ..Log::default()
        };
        l.rules.insert(crate::rules::PLAYER_FEET);
        l.looks.push((1.0, 90.0, -5.0));
        let mut r = String::new();
        walker(&l, &mut r);
        assert!(
            r.starts_with("walker: 3 ticks, 2 slices (0 standing)"),
            "{r}"
        );
        assert!(r.contains("rule relied on\tplay_player_point_walker_feet_v1\n"));
        assert!(r.contains("look at 1 s: heading 90 pitch -5\n"));
    }
}
