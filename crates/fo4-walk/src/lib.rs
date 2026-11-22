#![forbid(unsafe_code)]
//! A first walking controller over the placed collision of a cell
//! (`fo4_scene::collision`): a vertical capsule with gravity, ground
//! detection, sliding along walls, step-up and a walkable slope limit, run
//! at a fixed time step in f64. Pure and deterministic: no GPU, no clock,
//! no randomness; the same world, start and inputs give the same trace.
//!
//! The controller is this crate's own design from general collide-and-slide
//! geometry; it does not claim to reproduce the game's. Every number that
//! describes how the player moves (capsule size, step height, slope limit,
//! gravity, speeds, eye height) is game behaviour that no source settles:
//! each is a named `pending` rule in `spec/hypotheses.tsv` ([`rules`]) and
//! the values live in one [`Params`] struct whose default,
//! [`Params::PROVISIONAL`], is provisional. See `spec/walk.md`.
pub mod controller;
pub mod geometry;
pub mod set;
pub mod terrain;
pub mod world;

pub use controller::{Character, Ground, Input, Settings, State};
pub use set::{Space, WorldSet};
pub use world::{Contact, Prim, RayHit, Source, World};

use fo4_core::{Error, Result};
use fo4_scene::hypotheses::Status;
use fo4_scene::RuleLevel;
use serde::Serialize;

/// Names of the controller's rules in `spec/hypotheses.tsv`.
pub mod rules {
    pub const CAPSULE_RADIUS: &str = "walk_capsule_radius_v1";
    pub const CAPSULE_HEIGHT: &str = "walk_capsule_height_v1";
    pub const EYE_HEIGHT: &str = "walk_eye_height_v1";
    pub const STEP_HEIGHT: &str = "walk_step_height_v1";
    pub const MAX_SLOPE: &str = "walk_max_slope_v1";
    pub const GRAVITY: &str = "walk_gravity_v1";
    pub const WALK_SPEED: &str = "walk_speed_walk_v1";
    pub const RUN_SPEED: &str = "walk_speed_run_v1";
    pub const AIR_CONTROL: &str = "walk_no_air_control_v1";
    pub const TRIANGLE_SIDES: &str = "walk_triangles_block_both_sides_v1";
    /// Every rule of [`crate::Params`] (the triangle rule belongs to the
    /// world, [`crate::World::rules`]).
    pub const PARAMS: [&str; 9] = [
        CAPSULE_RADIUS,
        CAPSULE_HEIGHT,
        EYE_HEIGHT,
        STEP_HEIGHT,
        MAX_SLOPE,
        GRAVITY,
        WALK_SPEED,
        RUN_SPEED,
        AIR_CONTROL,
    ];
}

/// How the player moves. Every field is a registered rule; none is a fact.
/// World units, seconds and degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Params {
    /// Radius of the vertical capsule (`walk_capsule_radius_v1`).
    pub radius: f64,
    /// Total height of the capsule, feet to top (`walk_capsule_height_v1`).
    pub height: f64,
    /// Camera height above the feet (`walk_eye_height_v1`).
    pub eye_height: f64,
    /// Highest ledge walked onto without jumping, and the largest drop
    /// followed without leaving the ground (`walk_step_height_v1`).
    pub step_height: f64,
    /// Steepest walkable surface, degrees from horizontal (`walk_max_slope_v1`).
    pub max_slope_degrees: f64,
    /// Downward acceleration, units per second squared (`walk_gravity_v1`).
    pub gravity: f64,
    /// Units per second (`walk_speed_walk_v1`).
    pub walk_speed: f64,
    /// Units per second (`walk_speed_run_v1`).
    pub run_speed: f64,
    /// In the air the horizontal velocity keeps its value from the last
    /// grounded tick (`walk_no_air_control_v1`); `false` lets input steer.
    pub no_air_control: bool,
}

/// Where a parameter value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// A round placeholder chosen for this controller, pending measurement.
    Provisional,
    /// Set by the caller (command line, viewer).
    Caller,
}

impl Params {
    /// PROVISIONAL placeholders, not game values: round numbers that make a
    /// human-sized capsule, pending the measurements in `spec/walk.md`.
    /// Gravity is 9.81 m/s^2 times the pending unit rule
    /// `havok_collision_units_69_99125_v1` (69.99125 units per metre).
    pub const PROVISIONAL: Self = Self {
        radius: 16.0,
        height: 128.0,
        eye_height: 120.0,
        step_height: 20.0,
        max_slope_degrees: 45.0,
        gravity: 9.81 * 69.99125,
        walk_speed: 100.0,
        run_speed: 350.0,
        no_air_control: true,
    };

    /// Each field with its rule, its value and where the value comes from.
    pub fn provenance(&self) -> Vec<(&'static str, &'static str, f64, Origin)> {
        let p = Self::PROVISIONAL;
        let origin = |a: f64, b: f64| {
            if a.to_bits() == b.to_bits() {
                Origin::Provisional
            } else {
                Origin::Caller
            }
        };
        let flag = |x: bool| f64::from(u8::from(x));
        vec![
            (
                "radius",
                rules::CAPSULE_RADIUS,
                self.radius,
                origin(self.radius, p.radius),
            ),
            (
                "height",
                rules::CAPSULE_HEIGHT,
                self.height,
                origin(self.height, p.height),
            ),
            (
                "eye_height",
                rules::EYE_HEIGHT,
                self.eye_height,
                origin(self.eye_height, p.eye_height),
            ),
            (
                "step_height",
                rules::STEP_HEIGHT,
                self.step_height,
                origin(self.step_height, p.step_height),
            ),
            (
                "max_slope_degrees",
                rules::MAX_SLOPE,
                self.max_slope_degrees,
                origin(self.max_slope_degrees, p.max_slope_degrees),
            ),
            (
                "gravity",
                rules::GRAVITY,
                self.gravity,
                origin(self.gravity, p.gravity),
            ),
            (
                "walk_speed",
                rules::WALK_SPEED,
                self.walk_speed,
                origin(self.walk_speed, p.walk_speed),
            ),
            (
                "run_speed",
                rules::RUN_SPEED,
                self.run_speed,
                origin(self.run_speed, p.run_speed),
            ),
            (
                "no_air_control",
                rules::AIR_CONTROL,
                flag(self.no_air_control),
                origin(flag(self.no_air_control), flag(p.no_air_control)),
            ),
        ]
    }

    /// Refuses values the controller cannot run with.
    pub fn validate(&self) -> Result<()> {
        let bad = |what: &str, v: f64| {
            Err(Error::new(
                "InvalidWalkParameter",
                "walk parameters",
                0,
                what.to_owned(),
                v,
            ))
        };
        let positive = [
            ("radius > 0", self.radius),
            ("walk_speed > 0", self.walk_speed),
            ("run_speed > 0", self.run_speed),
        ];
        for (what, v) in positive {
            if !(v.is_finite() && v > 0.0) {
                return bad(what, v);
            }
        }
        if !(self.height.is_finite() && self.height >= 2.0 * self.radius) {
            return bad("height >= 2 radius", self.height);
        }
        if !(self.eye_height.is_finite()
            && self.eye_height >= 0.0
            && self.eye_height <= self.height)
        {
            return bad("0 <= eye_height <= height", self.eye_height);
        }
        if !(self.step_height.is_finite()
            && self.step_height >= 0.0
            && self.step_height < self.height - self.radius)
        {
            return bad("0 <= step_height < height - radius", self.step_height);
        }
        if !(self.max_slope_degrees.is_finite()
            && self.max_slope_degrees > 0.0
            && self.max_slope_degrees < 85.0)
        {
            return bad("0 < max_slope_degrees < 85", self.max_slope_degrees);
        }
        if !(self.gravity.is_finite() && self.gravity >= 0.0) {
            return bad("gravity >= 0", self.gravity);
        }
        Ok(())
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::PROVISIONAL
    }
}

/// Whether every rule in `rules` is accepted at `level` under `status`; the
/// error names the rules that are not.
pub fn accept(
    rules: &[&'static str],
    level: RuleLevel,
    status: impl Fn(&str) -> Option<Status>,
) -> Result<()> {
    let state = fo4_scene::collision::rules_state(rules, &status);
    if state.ready_at(level) {
        return Ok(());
    }
    let open: Vec<&str> = rules
        .iter()
        .copied()
        .filter(|r| {
            let s = status(r);
            match level {
                RuleLevel::Facts => true,
                RuleLevel::Measured => s != Some(Status::Supported),
                RuleLevel::Hypotheses => !matches!(s, Some(Status::Supported | Status::Pending)),
            }
        })
        .collect();
    Err(Error::new(
        "WalkRuleNotAccepted",
        "walk",
        0,
        format!("every rule accepted at {level:?}"),
        open.join(","),
    ))
}

#[cfg(test)]
mod tests;
