//! A track that grows point by point (`spec/play.md`, the stepped run):
//! the crossings folded move by move (`track::Folded`, `Place::cut`,
//! `Place::extend`, `Talk::cut`, `Talk::extend`) against those of the
//! whole track (`track::intervals`, `talk::load_hello`), on synthetic
//! geometry (no game data).
use super::world::key;
use crate::{
    talk::{self, HelloCrossing},
    track::{self, Crossing, Folded, Place, PlacedActor, Point, Shape, Track, Volume},
    World,
};
use fo4_scene::transform::Transform;
use std::rc::Rc;

fn frame(x: f32, y: f32, z: f32, rz: f32) -> Transform {
    Transform::euler([x, y, z, 0.0, 0.0, rz], 1.0).unwrap()
}

fn volumes() -> Vec<Volume> {
    vec![
        Volume {
            key: key(10),
            space: key(1),
            frame: frame(100.0, 0.0, 0.0, 0.3),
            shape: Shape::Box([40.0, 30.0, 50.0]),
        },
        Volume {
            key: key(11),
            space: key(1),
            frame: frame(250.0, 40.0, 0.0, 0.0),
            shape: Shape::Sphere(60.0),
        },
        Volume {
            key: key(12),
            space: key(2),
            frame: frame(0.0, 0.0, 0.0, 0.0),
            shape: Shape::Box([10.0, 10.0, 10.0]),
        },
    ]
}

/// A walk in and out of both volumes, a stand, a stand inside, a jump to
/// another space and back, points sharing a second.
fn points() -> Vec<Point> {
    let p = |t: f64, s: u32, x: f64, y: f64| Point {
        t,
        space: key(s),
        pos: [x, y, 0.0],
        facing: None,
    };
    vec![
        p(0.0, 1, 0.0, 0.0),
        p(1.0, 1, 0.0, 0.0),
        p(2.0, 1, 110.0, 5.0),
        p(3.0, 1, 110.0, 5.0),
        p(3.0, 1, 112.0, 5.0),
        p(5.0, 1, 300.0, 40.0),
        p(6.0, 2, 0.0, 0.0),
        p(7.0, 2, 50.0, 0.0),
        p(8.0, 1, 250.0, 40.0),
        p(9.0, 1, 400.0, 40.0),
    ]
}

/// The crossings of a whole track as `Place::load_with` makes them.
fn whole(track: &Track, vols: &[Volume], body: Option<u32>) -> Vec<Crossing> {
    let mut out = Vec::new();
    for (i, v) in vols.iter().enumerate() {
        for (e, l) in track::intervals(track, v, body) {
            out.push(Crossing {
                t: e,
                volume: i,
                enter: true,
            });
            if l.is_finite() {
                out.push(Crossing {
                    t: l,
                    volume: i,
                    enter: false,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.t.total_cmp(&b.t)
            .then(a.enter.cmp(&b.enter))
            .then(a.volume.cmp(&b.volume))
    });
    out
}

#[test]
fn folding_a_track_point_by_point_gives_the_whole_tracks_crossings() {
    let vols = volumes();
    let points = points();
    for body in [None, Some(120)] {
        let mut folds = vec![Folded::default(); vols.len()];
        for n in 1..=points.len() {
            if n >= 2 {
                let mv = track::step_of(&points[n - 2], &points[n - 1]);
                for (f, v) in folds.iter_mut().zip(&vols) {
                    f.push(v, body, mv);
                }
            }
            let track = Track {
                points: points[..n].to_vec(),
            };
            let last = &points[n - 1];
            let mut got = Vec::new();
            for (i, (f, v)) in folds.iter().zip(&vols).enumerate() {
                for (e, l) in f.finish(v, body, last) {
                    got.push(Crossing {
                        t: e,
                        volume: i,
                        enter: true,
                    });
                    if l.is_finite() {
                        got.push(Crossing {
                            t: l,
                            volume: i,
                            enter: false,
                        });
                    }
                }
            }
            got.sort_by(|a, b| {
                a.t.total_cmp(&b.t)
                    .then(a.enter.cmp(&b.enter))
                    .then(a.volume.cmp(&b.volume))
            });
            assert_eq!(got, whole(&track, &vols, body), "prefix {n}, body {body:?}");
        }
        let all = whole(
            &Track {
                points: points.clone(),
            },
            &vols,
            body,
        );
        assert!(all.len() >= 6, "{all:?}");
    }
}

/// A place holding the volumes, the actors and the whole track, with its
/// crossings as `Place::load_with` computes them.
fn place(track: Track, body: Option<u32>) -> Place {
    let mut p = Place::default();
    p.on = true;
    p.volumes = volumes();
    p.sw.body = body;
    p.sw.actors = true;
    p.crossings = whole(&track, &p.volumes, body);
    p.track = Some(Rc::new(track));
    p.actors = vec![
        PlacedActor {
            key: key(20),
            space: key(1),
            pos: [120.0, 0.0, 0.0],
        },
        PlacedActor {
            key: key(21),
            space: key(2),
            pos: [40.0, 0.0, 0.0],
        },
    ];
    p
}

/// The hello crossings `talk::load_hello` gives for a world's track.
fn hello_whole(place: Place) -> Vec<HelloCrossing> {
    let mut w = World {
        place,
        ..World::default()
    };
    w.talk.sw.hello = true;
    w.talk.hello_distance = Some(30.0);
    talk::load_hello(&mut w);
    w.talk.crossings
}

#[test]
fn a_cut_track_extended_slice_by_slice_has_the_whole_tracks_crossings() {
    let points = points();
    for body in [None, Some(120)] {
        // Cut at the start, then grown in slices of one, two and three
        // points.
        for step in 1..=3 {
            let mut w = World {
                place: place(
                    Track {
                        points: points.clone(),
                    },
                    body,
                ),
                ..World::default()
            };
            w.talk.sw.hello = true;
            w.talk.hello_distance = Some(30.0);
            let (dropped, kept) = w.place.cut(0.0).expect("a track");
            assert_eq!((dropped, kept), (points.len() - 1, true));
            w.talk.cut(&w.place);
            let mut n = 1;
            while n < points.len() {
                let m = (n + step).min(points.len());
                let moves = w.place.extend(&points[n..m]).expect("in order");
                assert_eq!(moves.len(), m - n);
                w.talk.extend(&w.place, &moves);
                let prefix = Track {
                    points: points[..m].to_vec(),
                };
                assert_eq!(w.place.crossings, whole(&prefix, &w.place.volumes, body));
                assert_eq!(w.talk.crossings, hello_whole(place(prefix, body)));
                n = m;
            }
            assert_eq!(w.place.folds.as_ref().map(|f| f.prefix_changed), Some(0));
            assert_eq!(w.talk.prefix_changed, 0);
        }
    }
}

#[test]
fn a_point_before_the_tracks_last_second_is_refused() {
    let points = points();
    let mut p = place(
        Track {
            points: points[..3].to_vec(),
        },
        None,
    );
    p.cut(10.0).expect("a track");
    let back = Point {
        t: 1.5,
        ..points[3].clone()
    };
    let e = p.extend(&[back]).expect_err("back in time");
    assert!(e.starts_with("PlayTrackBack"), "{e}");
    assert_eq!(p.track.as_ref().map(|t| t.points.len()), Some(3));
}

#[test]
fn volumes_added_to_a_grown_track_are_folded_over_all_of_it() {
    let points = points();
    let mut p2 = place(
        Track {
            points: points[..1].to_vec(),
        },
        None,
    );
    let all = p2.volumes.clone();
    p2.volumes.truncate(1);
    p2.crossings = whole(
        &Track {
            points: points[..1].to_vec(),
        },
        &p2.volumes,
        None,
    );
    p2.cut(0.0).expect("a track");
    p2.extend(&points[1..]).expect("in order");
    assert_eq!(p2.add_volumes(all.clone()), 2);
    assert_eq!(
        p2.add_volumes(all.clone()),
        0,
        "held volumes are not added again"
    );
    p2.rebuild();
    assert_eq!(p2.crossings, whole(&Track { points }, &all, None));
}
