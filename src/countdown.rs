//! Step countdown (pure): the seconds before a workout step ends, as the
//! number on screen and the beep each one gets. 10–6 white, 5–1 red and
//! growing with louder ticks, one `Go` beep as the step ends. Neutral
//! cueing: it says when the step changes, never how hard to ride.
//!
//! `Countdown::step` is fed the workout session and the events the
//! workout's own advance produced this frame (not skips or seeks), so a
//! jump never beeps `Go`; a number that rises or disappears is silent.

use bevy::prelude::Resource;

use crate::workout::{WorkoutEvent, WorkoutSession};

/// Steps of at least `SHORT_STEP_S` count from 10, shorter ones from 5
/// (the radio's step-warning rule).
pub const LONG_START_S: f32 = 10.0;
pub const SHORT_START_S: f32 = 5.0;
pub const SHORT_STEP_S: f32 = 20.0;

/// Numbers above this are white and small; from here down red and growing.
pub const RED_FROM: u32 = 5;
pub const WHITE_PX: f32 = 72.0;
pub const RED_PX_AT_5: f32 = 96.0;
pub const RED_PX_AT_1: f32 = 160.0;

/// Beep levels (× the voice volume).
pub const TICK_LEVEL_WHITE: f32 = 0.35;
pub const TICK_LEVEL_AT_5: f32 = 0.5;
pub const TICK_LEVEL_AT_1: f32 = 1.0;
pub const GO_LEVEL: f32 = 1.0;

/// Seconds the countdown of a step of `step_duration_s` starts at.
pub fn start_s(step_duration_s: f32) -> f32 {
    if step_duration_s >= SHORT_STEP_S {
        LONG_START_S
    } else {
        SHORT_START_S
    }
}

/// The number on screen: `ceil(remaining)` while `0 < remaining <= start`.
pub fn number(remaining_s: f32, step_duration_s: f32) -> Option<u32> {
    (remaining_s > 0.0 && remaining_s <= start_s(step_duration_s)).then(|| remaining_s.ceil() as u32)
}

/// How a number is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub px: f32,
    pub red: bool,
}

/// 10–6: white, 72 px; 5–1: red, 96 px at 5 growing linearly to 160 px at 1.
pub fn look(n: u32) -> Look {
    if n > RED_FROM {
        Look {
            px: WHITE_PX,
            red: false,
        }
    } else {
        Look {
            px: RED_PX_AT_5 + red_share(n) * (RED_PX_AT_1 - RED_PX_AT_5),
            red: true,
        }
    }
}

/// 0 at 5, 1 at 1 (and below).
fn red_share(n: u32) -> f32 {
    (RED_FROM.saturating_sub(n.max(1))) as f32 / (RED_FROM - 1) as f32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beep {
    /// The number changed to `n`.
    Tick { n: u32 },
    /// The step ended while a number was showing.
    Go,
}

impl Beep {
    /// Level × the voice volume: ticks 10–6 at 0.35, 5–1 rising from 0.5
    /// to 1.0, `Go` at 1.0.
    pub fn level(self) -> f32 {
        match self {
            Beep::Tick { n } if n > RED_FROM => TICK_LEVEL_WHITE,
            Beep::Tick { n } => TICK_LEVEL_AT_5 + red_share(n) * (TICK_LEVEL_AT_1 - TICK_LEVEL_AT_5),
            Beep::Go => GO_LEVEL,
        }
    }

    /// The clip's path under `assets/`.
    pub fn clip(self) -> &'static str {
        match self {
            Beep::Tick { .. } => "sfx/tick.ogg",
            Beep::Go => "sfx/go.ogg",
        }
    }
}

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Countdown {
    /// Beeps on (the `countdown_beeps` setting); numbers show regardless.
    pub beeps_enabled: bool,
    shown: Option<u32>,
    /// `WorkoutSession::loads` the shown number belongs to.
    loads: u64,
}

impl Countdown {
    pub fn new(beeps_enabled: bool) -> Countdown {
        Countdown {
            beeps_enabled,
            shown: None,
            loads: 0,
        }
    }

    /// The number on screen.
    pub fn number(&self) -> Option<u32> {
        self.shown
    }

    /// Follows the workout: `advanced` are the events of this frame's
    /// workout advance. The beep due this frame, if any (at most one;
    /// `Go` wins over a tick of the next step). Nothing without a workout,
    /// after the finish, or when the number rises or disappears (skip,
    /// seek, load, unload).
    pub fn step(&mut self, session: &WorkoutSession, advanced: &[WorkoutEvent]) -> Option<Beep> {
        let now = session
            .runner
            .as_ref()
            .filter(|r| !r.finished)
            .and_then(|r| number(r.step_remaining_s(), r.current().duration_s));
        let same_workout = self.loads == session.loads;
        let was = self.shown;
        self.shown = now;
        self.loads = session.loads;
        if !same_workout {
            return None;
        }
        if !advanced.is_empty() && was.is_some() {
            return Some(Beep::Go);
        }
        match (was, now) {
            (None, Some(n)) => Some(Beep::Tick { n }),
            (Some(p), Some(n)) if n < p => Some(Beep::Tick { n }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::{BandState, Ride};
    use crate::settings::DEFAULT_BAND;
    use crate::sim::RaceState;
    use crate::terrain::Profile;
    use crate::workout::{Workout, load_path, step_band};

    #[test]
    fn number_follows_the_remaining_seconds() {
        assert_eq!(number(10.0, 60.0), Some(10));
        assert_eq!(number(9.01, 60.0), Some(10));
        assert_eq!(number(9.0, 60.0), Some(9));
        assert_eq!(number(0.2, 60.0), Some(1));
        assert_eq!(number(0.0, 60.0), None);
        assert_eq!(number(10.5, 60.0), None);
        // Short steps count from 5.
        assert_eq!(number(5.0, 15.0), Some(5));
        assert_eq!(number(5.5, 15.0), None);
        assert_eq!(number(10.0, 20.0), Some(10));
    }

    #[test]
    fn look_and_levels() {
        assert_eq!(look(10), Look { px: 72.0, red: false });
        assert_eq!(look(6), Look { px: 72.0, red: false });
        assert_eq!(look(5), Look { px: 96.0, red: true });
        assert_eq!(look(3), Look { px: 128.0, red: true });
        assert_eq!(look(1), Look { px: 160.0, red: true });
        assert_eq!(Beep::Tick { n: 10 }.level(), 0.35);
        assert_eq!(Beep::Tick { n: 6 }.level(), 0.35);
        assert_eq!(Beep::Tick { n: 5 }.level(), 0.5);
        assert_eq!(Beep::Tick { n: 3 }.level(), 0.75);
        assert_eq!(Beep::Tick { n: 1 }.level(), 1.0);
        assert_eq!(Beep::Go.level(), 1.0);
        assert_eq!(
            (Beep::Go.clip(), Beep::Tick { n: 2 }.clip()),
            ("sfx/go.ogg", "sfx/tick.ogg")
        );
    }

    /// Two 60 s steps then a 15 s one, FTP 200, the ride in band.
    fn session() -> (WorkoutSession, Ride, Profile, RaceState) {
        let w = Workout::parse(
            r#"{"name": "cd", "date": "2026-10-03", "source": "test", "steps": [
                {"name": "a", "duration_s": 60, "low_pct": 60, "high_pct": 70, "class": "active", "block": 0, "block_kind": "single", "rep": 1, "reps": 1, "cadence_low": null, "cadence_high": null, "fixed_rest": false},
                {"name": "b", "duration_s": 60, "low_pct": 60, "high_pct": 70, "class": "active", "block": 1, "block_kind": "single", "rep": 1, "reps": 1, "cadence_low": null, "cadence_high": null, "fixed_rest": false},
                {"name": "c", "duration_s": 15, "low_pct": 60, "high_pct": 70, "class": "active", "block": 2, "block_kind": "single", "rep": 1, "reps": 1, "cadence_low": null, "cadence_high": null, "fixed_rest": false}
            ]}"#,
        )
        .unwrap();
        let mut session = WorkoutSession::new(200.0, DEFAULT_BAND);
        let (mut ride, mut profile, mut race) = (Ride::new(DEFAULT_BAND), Profile::demo(), RaceState::default());
        session.load(w, &mut ride, &mut profile, &mut race);
        (session, ride, profile, race)
    }

    /// Rides at the band centre for `secs` at 60 fps; collects the beeps.
    fn ride_for(cd: &mut Countdown, session: &mut WorkoutSession, ride: &mut Ride, secs: f32) -> Vec<Beep> {
        let mut beeps = Vec::new();
        let frames = (secs * 60.0).round() as usize;
        for _ in 0..frames {
            let band = step_band(session.runner.as_ref().unwrap().current(), session.ftp_w);
            crate::race::step(ride, (band.low_w + band.high_w) / 2.0, 1.0 / 60.0);
            let advanced = session.advance(ride, 1.0 / 60.0);
            beeps.extend(cd.step(session, &advanced));
        }
        beeps
    }

    #[test]
    fn ticks_ten_to_one_then_go_at_the_step_end() {
        let (mut session, mut ride, _p, _r) = session();
        let mut cd = Countdown::new(true);
        assert_eq!(cd.step(&session, &[]), None);
        assert_eq!(cd.number(), None);
        let beeps = ride_for(&mut cd, &mut session, &mut ride, 61.0);
        let expected: Vec<Beep> = (1..=10).rev().map(|n| Beep::Tick { n }).chain([Beep::Go]).collect();
        assert_eq!(beeps, expected);
        assert_eq!(session.runner.as_ref().unwrap().index, 1);
        // The 15 s step counts from 5; the last step ends with Go too.
        let beeps = ride_for(&mut cd, &mut session, &mut ride, 60.0 + 15.5);
        let second: Vec<Beep> = (1..=10).rev().map(|n| Beep::Tick { n }).chain([Beep::Go]).collect();
        let third: Vec<Beep> = (1..=5).rev().map(|n| Beep::Tick { n }).chain([Beep::Go]).collect();
        assert_eq!(beeps, [second, third].concat());
        assert!(session.runner.as_ref().unwrap().finished);
        assert_eq!(cd.number(), None);
    }

    #[test]
    fn nothing_while_idle() {
        let (mut session, mut ride, _p, _r) = session();
        let mut cd = Countdown::new(true);
        cd.step(&session, &[]);
        session.seek(52.0, &mut ride, &mut RaceState::default()).unwrap();
        assert_eq!(cd.step(&session, &[]), Some(Beep::Tick { n: 8 }));
        for _ in 0..600 {
            crate::race::step(&mut ride, 0.0, 1.0 / 60.0);
            let advanced = session.advance(&mut ride, 1.0 / 60.0);
            assert_eq!(cd.step(&session, &advanced), None);
        }
        assert_eq!((ride.state, cd.number()), (BandState::Idle, Some(8)));
    }

    #[test]
    fn seeks_and_skips_tick_down_but_never_go() {
        let (mut session, mut ride, _p, _r) = session();
        let mut race = RaceState::default();
        let mut cd = Countdown::new(true);
        cd.step(&session, &[]);
        // Into the last 3 s: one tick for 3, then 2, 1, Go.
        session.seek(57.0, &mut ride, &mut race).unwrap();
        assert_eq!(cd.step(&session, &[]), Some(Beep::Tick { n: 3 }));
        let beeps = ride_for(&mut cd, &mut session, &mut ride, 3.5);
        assert_eq!(beeps, [Beep::Tick { n: 2 }, Beep::Tick { n: 1 }, Beep::Go]);
        // Away while a number shows: no Go; the next step's number appears
        // silently after a skip into it.
        session.seek(118.0, &mut ride, &mut race).unwrap();
        assert_eq!(cd.step(&session, &[]), Some(Beep::Tick { n: 2 }));
        session.seek(30.0, &mut ride, &mut race).unwrap();
        assert_eq!((cd.step(&session, &[]), cd.number()), (None, None));
        session.seek(118.0, &mut ride, &mut race).unwrap();
        cd.step(&session, &[]);
        session.skip(&mut ride, &mut race).unwrap();
        // The 15 s step starts at 15 s remaining: no number yet.
        assert_eq!((cd.step(&session, &[]), cd.number()), (None, None));
        // A rising number is silent.
        session.seek(133.0, &mut ride, &mut race).unwrap();
        assert_eq!(cd.step(&session, &[]), Some(Beep::Tick { n: 2 }));
        session.seek(131.0, &mut ride, &mut race).unwrap();
        assert_eq!((cd.step(&session, &[]), cd.number()), (None, Some(4)));
        // Unload: silent, nothing shown.
        let mut profile = Profile::demo();
        session.unload(&mut ride, &mut profile, &mut race);
        assert_eq!((cd.step(&session, &[]), cd.number()), (None, None));
    }

    #[test]
    fn every_sample_gets_one_go_per_step() {
        for name in [
            "hit-30-30",
            "mit-sweetspot-6x5",
            "hidits-4x12",
            "lit-1h",
            "lactate-elimination",
        ] {
            let w = load_path(&format!("samples/{name}.json")).unwrap();
            let steps = w.steps.len();
            let total = w.total_s();
            let mut session = WorkoutSession::new(200.0, DEFAULT_BAND);
            let (mut ride, mut profile, mut race) = (Ride::new(DEFAULT_BAND), Profile::demo(), RaceState::default());
            session.load(w, &mut ride, &mut profile, &mut race);
            let mut cd = Countdown::new(true);
            // 4 fps is enough: every tick is still seen.
            let dt = 0.25;
            let mut gos = 0;
            let mut ticks = 0;
            let mut t = 0.0;
            while t < total + 1.0 {
                let band = step_band(session.runner.as_ref().unwrap().current(), session.ftp_w);
                crate::race::step(&mut ride, (band.low_w + band.high_w) / 2.0, dt);
                let advanced = session.advance(&mut ride, dt);
                match cd.step(&session, &advanced) {
                    Some(Beep::Go) => gos += 1,
                    Some(Beep::Tick { .. }) => ticks += 1,
                    None => {}
                }
                t += dt;
            }
            assert_eq!(gos, steps, "{name}");
            assert!(ticks >= 5 * steps, "{name}: {ticks} ticks");
        }
    }
}
