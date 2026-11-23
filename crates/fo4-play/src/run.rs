//! The play mode's side of a stepped run: `fo4_vm::run`'s new-game run
//! (the code of `quest_run new-game`) with a [`Driver`] giving it its
//! input slice by slice through [`fo4_vm::run::NewGame::advance_to`].
use crate::{Driver, Next, Options, View};
use fo4_core::Budget;
use fo4_vm::run::{NewGame, Stepper, Stepping, E};

/// The voice files' seconds from their headers
/// (`fo4_audio::header_seconds`, `audio_xwma_dpds_seconds_v1`), as
/// `quest_run` reads them.
fn seconds(bytes: &[u8], name: &str, b: &Budget) -> Option<f64> {
    fo4_audio::Sound::read(bytes, name, b)
        .ok()
        .and_then(|s| fo4_audio::header_seconds(&s.wave))
}

/// A driver stepping a run, and the input counts it took back.
struct Play {
    driver: Box<dyn Driver>,
    options: Options,
    counts: crate::Counts,
}

impl Stepper for Play {
    fn step(&mut self, run: &mut NewGame<'_, '_>) -> Result<(), E> {
        run.begin_input(Stepping {
            region: self.options.region.then_some(crate::rules::REGION),
            recheck_marks: self.options.recheck_marks,
        })?;
        while !run.ended() {
            let next = {
                let timeline = || run.timeline_now();
                let view = View {
                    vm: run.vm(),
                    store: run.store(),
                    sources: run.sources(),
                    loaded: run.loaded_track(),
                    timeline: &timeline,
                };
                self.driver.next(&view)
            };
            match next {
                Next::Slice(s) => {
                    let actions: Vec<_> = s
                        .actions
                        .iter()
                        .map(|a| (a.at, a.action, a.reference.clone()))
                        .collect();
                    run.advance_to(s.until, &s.points, &actions)?;
                }
                Next::Finish => run.finish_input()?,
                Next::Stop => run.stop(),
            }
        }
        self.counts = run.take_input_counts();
        Ok(())
    }
}

/// Runs `quest_run` with these arguments (`MANIFEST new-game OUT_DIR
/// [switches...]`), its clock loop stepped by the driver when one is given.
pub(crate) fn new_game(
    args: Vec<String>,
    driver: Option<Box<dyn Driver>>,
    options: Options,
) -> Result<crate::Played, Box<dyn std::error::Error>> {
    let mut play = driver.map(|driver| Play {
        driver,
        options,
        counts: crate::Counts::new(),
    });
    let out = fo4_vm::run::main_with(args, &seconds, play.as_mut().map(|p| p as &mut dyn Stepper));
    let (driver, counts) = match play {
        Some(p) => (Some(p.driver), p.counts),
        None => (None, crate::Counts::new()),
    };
    out.map(|()| (driver, counts))
}
