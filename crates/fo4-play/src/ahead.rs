//! A walking world built ahead on a thread of its own (`--play-build-ahead`,
//! a play-mode policy that changes no outcome; `spec/play.md`, the
//! thirty-second-session step): while the walker walks the loaded squares,
//! the square it nears is built on this thread from the thread's own
//! records and files, and handed over through a channel when the walker
//! crosses into it. Nothing is shared between the threads: a request is
//! sent, an owned result comes back. A build that is not finished when it
//! is needed is waited for; one that was never asked is built where it is
//! needed, as before. Either way the world is the one a build in place
//! would give, and which way it came depends only on what was asked, so a
//! run is the same with and without it, however fast the thread was.
use std::sync::mpsc::{channel, Receiver, Sender};

/// What a build gives back: the round it was asked in, the request and its
/// result.
type Done<K, T> = (u64, K, Result<T, String>);

/// What the thread tells: a build begun, a build done.
enum Told<K, T> {
    Begun(u64, K),
    Done(Done<K, T>),
}

/// What the thread answers for a request a newer one replaced before its
/// build began.
pub const SKIPPED: &str = "PlayBuildAheadSkipped";

/// One thread that builds the newest of what it is asked. A request a newer
/// one replaced is answered as skipped, and one still queued behind another
/// build when it is needed is not waited for: the caller builds it in place.
/// The world handed over is the same either way, only the wait differs.
pub struct Ahead<K, T> {
    to: Option<Sender<(u64, K)>>,
    from: Receiver<Told<K, T>>,
    /// The request of this round whose build has begun and not ended.
    begun: Option<K>,
    /// The round: one hand-over ([`Ahead::take`]) ends it, and what was
    /// asked in it and not taken is dropped.
    round: u64,
    /// Requests of this round not answered yet.
    asked: Vec<K>,
    /// Answers of this round not taken yet.
    done: Vec<(K, Result<T, String>)>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Builds asked, answers taken finished, answers waited for.
    pub counts: [u64; 3],
}

impl<K, T> Ahead<K, T>
where
    K: PartialEq + Clone + Send + 'static,
    T: Send + 'static,
{
    /// Starts the thread: `make` runs on it first and gives the builder
    /// (which may then hold what cannot cross threads: records and files
    /// opened there).
    pub fn spawn<M, G>(make: M) -> Result<Self, String>
    where
        M: FnOnce() -> G + Send + 'static,
        G: FnMut(&K) -> Result<T, String>,
    {
        let (to, requests) = channel::<(u64, K)>();
        let (answers, from) = channel::<Told<K, T>>();
        let thread = std::thread::Builder::new()
            .name("play build ahead".into())
            .spawn(move || {
                let mut build = make();
                while let Ok(mut next) = requests.recv() {
                    // Only the newest request is built; the older ones are
                    // answered as skipped, for the caller to build in place.
                    while let Ok(newer) = requests.try_recv() {
                        let (round, k) = std::mem::replace(&mut next, newer);
                        let skipped = Told::Done((round, k, Err(SKIPPED.into())));
                        if answers.send(skipped).is_err() {
                            return;
                        }
                    }
                    let (round, k) = next;
                    if answers.send(Told::Begun(round, k.clone())).is_err() {
                        break;
                    }
                    let r = build(&k);
                    if answers.send(Told::Done((round, k, r))).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("PlayBuildAhead: {e}"))?;
        Ok(Self {
            to: Some(to),
            from,
            begun: None,
            round: 0,
            asked: Vec::new(),
            done: Vec::new(),
            thread: Some(thread),
            counts: [0; 3],
        })
    }

    /// Keeps what the thread told of this round.
    fn keep(&mut self, told: Told<K, T>) {
        match told {
            Told::Begun(round, k) => {
                if round == self.round {
                    self.begun = Some(k);
                }
            }
            Told::Done((round, k, r)) => {
                if round != self.round {
                    return;
                }
                if self.begun.as_ref() == Some(&k) {
                    self.begun = None;
                }
                if let Some(i) = self.asked.iter().position(|a| *a == k) {
                    self.asked.remove(i);
                }
                self.done.push((k, r));
            }
        }
    }

    fn poll(&mut self) {
        while let Ok(d) = self.from.try_recv() {
            self.keep(d);
        }
    }

    /// Asks for `k` unless it was asked already in this round.
    pub fn ask(&mut self, k: K) {
        self.poll();
        if self.asked.contains(&k) || self.done.iter().any(|d| d.0 == k) {
            return;
        }
        if let Some(to) = &self.to {
            if to.send((self.round, k.clone())).is_ok() {
                self.asked.push(k);
                self.counts[0] += 1;
            }
        }
    }

    /// Whether the answer for `k` has arrived.
    pub fn ready(&mut self, k: &K) -> bool {
        self.poll();
        self.done.iter().any(|d| d.0 == *k)
    }

    /// The hand-over: the build of `k` if it was asked in this round and
    /// has ended (the answer taken) or begun (the answer waited for); `None`
    /// when it was not asked, was replaced by a newer request or still waits
    /// behind another build (the caller builds it in place). Ends the round:
    /// every other request and answer of it is dropped.
    pub fn take(&mut self, k: &K) -> Option<Result<T, String>> {
        self.poll();
        let got = if let Some(i) = self.done.iter().position(|d| d.0 == *k) {
            self.counts[1] += 1;
            Some(self.done.swap_remove(i).1)
        } else if self.begun.as_ref() == Some(k) {
            let mut got = None;
            while let Ok(told) = self.from.recv() {
                match told {
                    Told::Done((round, d, r)) if round == self.round && d == *k => {
                        got = Some(r);
                        break;
                    }
                    other => self.keep(other),
                }
            }
            if got.is_some() {
                self.counts[2] += 1;
            }
            got
        } else {
            None
        };
        let got = got.filter(|r| !matches!(r, Err(e) if e == SKIPPED));
        self.round += 1;
        self.asked.clear();
        self.done.clear();
        self.begun = None;
        got
    }
}

impl<K, T> Drop for Ahead<K, T> {
    fn drop(&mut self) {
        self.to = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fo4_walk::World;
    use std::sync::{Arc, Mutex};

    /// A synthetic world for a square: one floor triangle at the square's
    /// height, as a build would give it.
    fn square_world(k: &[i32; 2]) -> Result<World, String> {
        let b = fo4_core::Budget::new(1 << 20);
        let mut w = World::new();
        let (x, y) = (f64::from(k[0]) * 100.0, f64::from(k[1]) * 100.0);
        w.add_triangles(
            &[[
                [x, y, 1.0],
                [x + 100.0, y, 1.0],
                [x + 100.0, y + 100.0, 1.0],
            ]],
            "floor",
            &b,
        )
        .map_err(|e| e.to_string())?;
        Ok(w)
    }

    /// The handover: a world asked ahead comes back whole, built on the
    /// other thread; one being built is waited for; one never asked, or
    /// queued behind another build, is left to the caller; a hand-over drops
    /// the round's other requests.
    #[test]
    fn a_world_built_ahead_is_handed_over_whole() {
        let gate = Arc::new(Mutex::new(()));
        let (started_tx, started) = std::sync::mpsc::channel::<[i32; 2]>();
        let g = Arc::clone(&gate);
        let held = gate.lock().unwrap();
        let mut a: Ahead<[i32; 2], World> = Ahead::spawn(move || {
            move |k: &[i32; 2]| {
                let _ = started_tx.send(*k);
                // Each build waits for the test to open the gate.
                drop(g.lock().unwrap());
                square_world(k)
            }
        })
        .unwrap();
        a.ask([1, 0]);
        a.ask([1, 0]);
        assert_eq!(a.counts[0], 1);
        assert_eq!(started.recv().unwrap(), [1, 0]);
        a.ask([0, 1]);
        assert!(!a.ready(&[1, 0]));
        drop(held);
        // Being built: waited for, and the same world a build in place
        // gives.
        let w = a.take(&[1, 0]).unwrap().unwrap();
        let fresh = square_world(&[1, 0]).unwrap();
        assert_eq!(w.bounds, fresh.bounds);
        assert_eq!(w.entries.len(), fresh.entries.len());
        // The other request of that round was dropped with it.
        assert!(a.take(&[0, 1]).is_none());
        // Not asked: the caller builds it.
        assert!(a.take(&[5, 5]).is_none());
        // Queued behind another build (and replaced by a newer request):
        // the caller builds it.
        let held = gate.lock().unwrap();
        a.ask([1, 1]);
        while started.try_recv().ok() != Some([1, 1]) {
            std::thread::yield_now();
        }
        a.ask([3, 3]);
        a.ask([4, 4]);
        drop(held);
        assert!(a.take(&[3, 3]).is_none());
        // Asked and finished before it is needed: taken ready.
        a.ask([2, 0]);
        while !a.ready(&[2, 0]) {
            std::thread::yield_now();
        }
        let w2 = a.take(&[2, 0]).unwrap().unwrap();
        assert_eq!(w2.bounds, square_world(&[2, 0]).unwrap().bounds);
        // Taken once only.
        assert!(a.take(&[2, 0]).is_none());
    }

    /// The square built ahead is the one beyond the nearest edge, once the
    /// feet come within the lead of it; none in a square's middle.
    #[test]
    fn the_square_ahead_is_beyond_the_nearest_edge() {
        use crate::walker::{ahead_square, AHEAD_LEAD};
        let side = crate::world::CELL_SIZE;
        assert_eq!(ahead_square([side / 2.0, side / 2.0, 0.0], [0, 0]), None);
        assert_eq!(
            ahead_square([side - 10.0, side / 2.0, 0.0], [0, 0]),
            Some([1, 0])
        );
        assert_eq!(ahead_square([10.0, side / 2.0, 0.0], [0, 0]), Some([-1, 0]));
        assert_eq!(
            ahead_square([side / 2.0, 1.0 - AHEAD_LEAD, 0.0], [0, -1]),
            Some([0, 0])
        );
        assert_eq!(
            ahead_square([side / 2.0, AHEAD_LEAD + 1.0, 0.0], [0, 0]),
            None
        );
    }
}
