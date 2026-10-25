//! One command layer for driving the game from outside: the debug API and
//! the playtest runner both build `ControlCommand`s and go through `apply`,
//! so staging and scripting share one implementation.

use bevy::prelude::*;
use serde::Deserialize;

use crate::countdown::Countdown;
use crate::input::InputSource;
use crate::race::{Band, GAP_MAX_M, Ride, START_MATCHES};
use crate::radio::{Radio, current_vars};
use crate::scenario::Scenario;
use crate::sim::{RaceState, RiderInput};
use crate::terrain::Profile;
use crate::workout::{MAX_AUTOPILOT_BIAS_PCT, WorkoutSession, load_path};

/// Highest sim time scale.
pub const MAX_SPEED_MULTIPLIER: f32 = 50.0;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlCommand {
    /// Jump along the road; finite and >= 0.
    Teleport { distance_m: f32 },
    /// Both fields → override; either missing → clear the override.
    Input {
        power_w: Option<f32>,
        balance_left_pct: Option<f32>,
    },
    /// Sim time scale; `None` → 1.0, clamped to max 50, negative rejected.
    SetSpeed { multiplier: Option<f32> },
    /// Swap the course (`flat`, `hills`, `demo`) and restart at distance 0.
    /// Refused while a workout is loaded (it owns the course).
    SetProfile { name: String },
    /// Free-ride band; `0 < low_w < high_w <= 2000`. Never writes the
    /// settings file. Refused while a workout is loaded (it owns the band).
    SetBand { low_w: f32, high_w: f32 },
    /// Resets the ride (gap, matches, score, totals) and unloads a workout:
    /// free ride with the band and the profile from before it.
    RestartRide,
    /// Gap to the partner's wheel, 0..=60 m.
    SetGap { gap_m: f32 },
    /// Matches left, 0..=5.
    SetMatches { matches: u32 },
    /// Loads a workout file and restarts the ride at step 0 on the course
    /// generated from it (profile `workout`, distance 0). A relative path
    /// resolves against the repo's `workouts/`, then the config `workouts/`
    /// directory.
    LoadWorkout { path: String },
    /// Back to free ride with the band and the profile from before the
    /// workout (distance 0); the ride's totals stay.
    UnloadWorkout,
    /// Ends the current workout step now; the rider jumps to the next
    /// step's start on the course.
    SkipStep,
    /// Jumps the workout and the rider to workout time `elapsed_s`
    /// (`0..=total_s`).
    SeekWorkout { elapsed_s: f32 },
    /// 50..=600 W; re-resolves the current step's band. Never writes the
    /// settings file.
    SetFtp { ftp_w: f32 },
    /// The scripted mock rides the centre of the band (± 3 %).
    Autopilot { on: bool },
    /// The running breakaway's advantage, 0..=180 s (0 is caught on the
    /// next frame). Refused outside a breakaway set.
    SetAdvantage { seconds: f32 },
    /// The autopilot rides band centre × (1 + pct / 100), -50..=50.
    AutopilotBias { pct: f32 },
    /// Puts a line of the lines file on the radio, with the current band,
    /// rep, advantage and matches (refused for an unknown key; ignored while
    /// the radio is off or the key is cooling down).
    RadioSay { key: String },
    /// The radio on or off. Never writes the settings file.
    RadioEnabled { on: bool },
    /// The radio's voice on or off (off: text only, the clip on air stops).
    /// Never writes the settings file.
    VoiceEnabled { on: bool },
    /// The step countdown's beeps on or off (the numbers stay). Never
    /// writes the settings file.
    CountdownBeeps { on: bool },
}

/// Applies one command. `speed_multiplier` is the sim time scale; the Bevy
/// wrapper (`apply_to_world`) copies it into `Time<Virtual>`. On error
/// nothing is changed.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    cmd: &ControlCommand,
    race: &mut RaceState,
    ride: &mut Ride,
    session: &mut WorkoutSession,
    scenario: &mut Scenario,
    radio: &mut Radio,
    countdown: &mut Countdown,
    source: &mut InputSource,
    profile: &mut Profile,
    speed_multiplier: &mut f32,
) -> Result<(), String> {
    match cmd {
        ControlCommand::Teleport { distance_m } => {
            let d = *distance_m;
            if !(d.is_finite() && d >= 0.0) {
                return Err("body must be {\"distance_m\": f32}, finite and >= 0".into());
            }
            // Lateral is kept; gradient, speed and elevation follow on the next sim step.
            race.distance_m = d;
        }
        ControlCommand::Input {
            power_w,
            balance_left_pct,
        } => {
            source.override_input = match (power_w, balance_left_pct) {
                (Some(p), Some(b)) => Some(RiderInput {
                    power_w: *p,
                    balance_left_pct: *b,
                }),
                _ => None,
            };
        }
        ControlCommand::SetSpeed { multiplier } => {
            *speed_multiplier = match multiplier {
                None => 1.0,
                Some(m) if *m >= 0.0 => m.min(MAX_SPEED_MULTIPLIER),
                Some(_) => return Err("body must be {\"multiplier\": f32 >= 0} (max 50, null resets to 1)".into()),
            };
        }
        ControlCommand::SetProfile { name } => {
            let Some(p) = Profile::by_name(name) else {
                return Err(format!("unknown profile {name:?}; expected flat, hills or demo"));
            };
            if session.runner.is_some() {
                return Err("a loaded workout owns the course; unload it first".into());
            }
            *profile = p;
            race.distance_m = 0.0;
        }
        ControlCommand::SetBand { low_w, high_w } => {
            let band = Band::new(*low_w, *high_w)?;
            if session.runner.is_some() {
                return Err("a loaded workout owns the band; unload it first".into());
            }
            ride.band = band;
        }
        ControlCommand::RestartRide => {
            session.unload(ride, profile, race);
            ride.restart();
            radio.reset("");
        }
        ControlCommand::SetGap { gap_m } => {
            let g = *gap_m;
            if !(g.is_finite() && (0.0..=GAP_MAX_M).contains(&g)) {
                return Err(format!("body must be {{\"gap_m\": f32}}, 0..={GAP_MAX_M}"));
            }
            ride.gap_m = g;
        }
        ControlCommand::SetMatches { matches } => {
            if *matches > START_MATCHES {
                return Err(format!("body must be {{\"matches\": int}}, 0..={START_MATCHES}"));
            }
            ride.matches = *matches;
        }
        ControlCommand::LoadWorkout { path } => {
            let workout = load_path(path)?;
            radio.reset(&workout.name);
            session.load(workout, ride, profile, race);
        }
        ControlCommand::UnloadWorkout => {
            session.unload(ride, profile, race);
            radio.reset("");
        }
        ControlCommand::SkipStep => session.skip(ride, race)?,
        ControlCommand::SeekWorkout { elapsed_s } => session.seek(*elapsed_s, ride, race)?,
        ControlCommand::SetFtp { ftp_w } => session.set_ftp(*ftp_w, ride)?,
        ControlCommand::Autopilot { on } => session.autopilot = *on,
        ControlCommand::SetAdvantage { seconds } => scenario.set_advantage(*seconds, session, ride)?,
        ControlCommand::AutopilotBias { pct } => {
            if !(pct.is_finite() && pct.abs() <= MAX_AUTOPILOT_BIAS_PCT) {
                return Err(format!(
                    "body must be {{\"pct\": f32}}, -{MAX_AUTOPILOT_BIAS_PCT}..={MAX_AUTOPILOT_BIAS_PCT}"
                ));
            }
            session.autopilot_bias_pct = *pct;
        }
        ControlCommand::RadioSay { key } => radio.say(key, current_vars(ride, session, scenario))?,
        ControlCommand::RadioEnabled { on } => radio.set_enabled(*on),
        ControlCommand::VoiceEnabled { on } => radio.set_voice_enabled(*on),
        ControlCommand::CountdownBeeps { on } => countdown.beeps_enabled = *on,
    }
    Ok(())
}

/// `apply` against the live resources; the speed multiplier goes through
/// `Time<Virtual>`. The profile is only marked changed by the commands
/// that can swap it (`SetProfile`, loading, unloading, restarting).
#[allow(clippy::too_many_arguments)]
pub fn apply_to_world(
    cmd: &ControlCommand,
    race: &mut RaceState,
    ride: &mut Ride,
    session: &mut WorkoutSession,
    scenario: &mut Scenario,
    radio: &mut Radio,
    countdown: &mut Countdown,
    source: &mut InputSource,
    profile: &mut ResMut<Profile>,
    time: &mut Time<Virtual>,
) -> Result<(), String> {
    let mut speed = time.relative_speed();
    let result = apply(
        cmd,
        race,
        ride,
        session,
        scenario,
        radio,
        countdown,
        source,
        profile.bypass_change_detection(),
        &mut speed,
    );
    let swaps = matches!(
        cmd,
        ControlCommand::SetProfile { .. }
            | ControlCommand::LoadWorkout { .. }
            | ControlCommand::UnloadWorkout
            | ControlCommand::RestartRide
    );
    if swaps && result.is_ok() {
        profile.set_changed();
    }
    time.set_relative_speed(speed);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct World {
        race: RaceState,
        ride: Ride,
        session: WorkoutSession,
        scenario: Scenario,
        radio: Radio,
        countdown: Countdown,
        source: InputSource,
        profile: Profile,
        speed: f32,
    }

    impl World {
        fn new() -> Self {
            Self {
                race: RaceState::default(),
                ride: Ride::new(crate::settings::DEFAULT_BAND),
                session: WorkoutSession::new(200.0, crate::settings::DEFAULT_BAND),
                scenario: Scenario::default(),
                radio: crate::radio::test_radio(),
                countdown: Countdown::new(true),
                source: InputSource::default(),
                profile: Profile::demo(),
                speed: 1.0,
            }
        }

        fn apply(&mut self, cmd: ControlCommand) -> Result<(), String> {
            let s = self;
            let (scenario, radio, countdown) = (&mut s.scenario, &mut s.radio, &mut s.countdown);
            apply(
                &cmd,
                &mut s.race,
                &mut s.ride,
                &mut s.session,
                scenario,
                radio,
                countdown,
                &mut s.source,
                &mut s.profile,
                &mut s.speed,
            )
        }
    }

    #[test]
    fn teleport_validates() {
        let mut w = World::new();
        w.race.lateral_m = 1.5;
        assert!(w.apply(ControlCommand::Teleport { distance_m: 300.0 }).is_ok());
        assert_eq!((w.race.distance_m, w.race.lateral_m), (300.0, 1.5));
        for bad in [-1.0, f32::NAN, f32::INFINITY] {
            assert!(w.apply(ControlCommand::Teleport { distance_m: bad }).is_err());
        }
        assert_eq!(w.race.distance_m, 300.0);
    }

    #[test]
    fn input_needs_both_fields() {
        let mut w = World::new();
        w.apply(ControlCommand::Input {
            power_w: Some(200.0),
            balance_left_pct: Some(40.0),
        })
        .unwrap();
        assert_eq!(
            w.source.override_input,
            Some(RiderInput {
                power_w: 200.0,
                balance_left_pct: 40.0
            })
        );
        w.apply(ControlCommand::Input {
            power_w: Some(200.0),
            balance_left_pct: None,
        })
        .unwrap();
        assert_eq!(w.source.override_input, None);
    }

    #[test]
    fn set_speed_defaults_clamps_and_rejects() {
        let mut w = World::new();
        w.apply(ControlCommand::SetSpeed { multiplier: Some(80.0) }).unwrap();
        assert_eq!(w.speed, MAX_SPEED_MULTIPLIER);
        w.apply(ControlCommand::SetSpeed { multiplier: None }).unwrap();
        assert_eq!(w.speed, 1.0);
        w.apply(ControlCommand::SetSpeed { multiplier: Some(0.0) }).unwrap();
        assert_eq!(w.speed, 0.0);
        assert!(w.apply(ControlCommand::SetSpeed { multiplier: Some(-1.0) }).is_err());
        assert!(
            w.apply(ControlCommand::SetSpeed {
                multiplier: Some(f32::NAN)
            })
            .is_err()
        );
        assert_eq!(w.speed, 0.0);
    }

    #[test]
    fn set_profile_swaps_course_and_restarts() {
        let mut w = World::new();
        w.race.distance_m = 1234.0;
        w.apply(ControlCommand::SetProfile { name: "flat".into() }).unwrap();
        assert_eq!(w.profile.name(), "flat");
        assert_eq!(w.race.distance_m, 0.0);
        w.race.distance_m = 50.0;
        assert!(w.apply(ControlCommand::SetProfile { name: "nope".into() }).is_err());
        assert_eq!((w.profile.name(), w.race.distance_m), ("flat", 50.0));
    }

    #[test]
    fn set_band_validates_and_keeps_the_ride() {
        let mut w = World::new();
        w.ride.score = 12.0;
        w.apply(ControlCommand::SetBand {
            low_w: 180.0,
            high_w: 220.0,
        })
        .unwrap();
        assert_eq!(
            (w.ride.band, w.ride.score),
            (
                Band {
                    low_w: 180.0,
                    high_w: 220.0
                },
                12.0
            )
        );
        for (low_w, high_w) in [
            (220.0, 180.0),
            (200.0, 200.0),
            (0.0, 100.0),
            (100.0, 2001.0),
            (f32::NAN, 200.0),
        ] {
            assert!(
                w.apply(ControlCommand::SetBand { low_w, high_w }).is_err(),
                "{low_w} {high_w}"
            );
        }
        assert_eq!(
            w.ride.band,
            Band {
                low_w: 180.0,
                high_w: 220.0
            }
        );
    }

    #[test]
    fn ride_commands() {
        let mut w = World::new();
        w.apply(ControlCommand::SetGap { gap_m: 30.0 }).unwrap();
        w.apply(ControlCommand::SetMatches { matches: 0 }).unwrap();
        assert_eq!((w.ride.gap_m, w.ride.matches), (30.0, 0));
        for gap_m in [-0.1, 60.1, f32::NAN] {
            assert!(w.apply(ControlCommand::SetGap { gap_m }).is_err());
        }
        assert!(w.apply(ControlCommand::SetGap { gap_m: 60.0 }).is_ok());
        assert!(w.apply(ControlCommand::SetMatches { matches: 6 }).is_err());
        assert!(w.apply(ControlCommand::SetMatches { matches: 5 }).is_ok());
        w.ride.score = 40.0;
        w.apply(ControlCommand::SetBand {
            low_w: 180.0,
            high_w: 220.0,
        })
        .unwrap();
        w.apply(ControlCommand::RestartRide).unwrap();
        assert_eq!(
            w.ride,
            Ride::new(Band {
                low_w: 180.0,
                high_w: 220.0
            })
        );
    }

    fn load(path: &str) -> ControlCommand {
        ControlCommand::LoadWorkout { path: path.into() }
    }

    #[test]
    fn workout_commands() {
        let mut w = World::new();
        w.apply(ControlCommand::SetFtp { ftp_w: 200.0 }).unwrap();
        w.apply(ControlCommand::SetBand {
            low_w: 180.0,
            high_w: 220.0,
        })
        .unwrap();
        w.ride.score = 40.0;
        w.apply(load("samples/mit-sweetspot-6x5.json")).unwrap();
        assert_eq!(
            (w.ride.band, w.ride.score, w.ride.matches),
            (
                Band {
                    low_w: 90.0,
                    high_w: 110.0
                },
                0.0,
                5
            )
        );
        // The workout owns the band.
        assert!(
            w.apply(ControlCommand::SetBand {
                low_w: 100.0,
                high_w: 200.0
            })
            .is_err()
        );
        for _ in 0..6 {
            w.apply(ControlCommand::SkipStep).unwrap();
        }
        assert_eq!(
            w.ride.band,
            Band {
                low_w: 168.0,
                high_w: 188.0
            }
        );
        w.apply(ControlCommand::SetFtp { ftp_w: 250.0 }).unwrap();
        assert_eq!(
            w.ride.band,
            Band {
                low_w: 210.0,
                high_w: 235.0
            }
        );
        assert!(w.apply(ControlCommand::SetFtp { ftp_w: 700.0 }).is_err());
        w.apply(ControlCommand::Autopilot { on: true }).unwrap();
        assert!(w.session.autopilot);
        // A missing file changes nothing.
        let (ride, session) = (w.ride.clone(), w.session.clone());
        assert!(w.apply(load("samples/missing.json")).is_err());
        assert!(w.apply(load("/nonexistent/x.json")).is_err());
        assert_eq!((&w.ride, &w.session), (&ride, &session));
        // Restart: free ride with the band from before the workout.
        w.apply(ControlCommand::RestartRide).unwrap();
        assert!(w.session.runner.is_none());
        assert_eq!(
            w.ride,
            Ride::new(Band {
                low_w: 180.0,
                high_w: 220.0
            })
        );
        assert!(w.apply(ControlCommand::SkipStep).is_err());
        w.apply(load("samples/lit-1h.json")).unwrap();
        w.apply(ControlCommand::UnloadWorkout).unwrap();
        assert_eq!(
            w.ride.band,
            Band {
                low_w: 180.0,
                high_w: 220.0
            }
        );
        w.apply(ControlCommand::UnloadWorkout).unwrap();
    }

    #[test]
    fn a_workout_brings_its_course_and_takes_it_away_again() {
        let mut w = World::new();
        w.apply(ControlCommand::SetProfile { name: "flat".into() }).unwrap();
        w.race.distance_m = 700.0;
        w.apply(load("samples/mit-sweetspot-6x5.json")).unwrap();
        assert_eq!((w.profile.name(), w.race.distance_m), ("workout", 0.0));
        // The workout owns the course.
        assert!(w.apply(ControlCommand::SetProfile { name: "demo".into() }).is_err());
        assert_eq!(w.profile.name(), "workout");
        w.apply(ControlCommand::SeekWorkout { elapsed_s: 1185.0 }).unwrap();
        assert_eq!(w.session.runner.as_ref().unwrap().index, 5);
        assert!(w.race.distance_m > 0.0);
        for elapsed_s in [-0.1, 4200.1, f32::NAN] {
            assert!(w.apply(ControlCommand::SeekWorkout { elapsed_s }).is_err());
        }
        w.apply(ControlCommand::SkipStep).unwrap();
        assert_eq!(w.race.distance_m, w.session.course.as_ref().unwrap().step_start_m[6]);
        w.apply(ControlCommand::RestartRide).unwrap();
        assert_eq!((w.profile.name(), w.race.distance_m), ("flat", 0.0));
        assert!(w.apply(ControlCommand::SeekWorkout { elapsed_s: 10.0 }).is_err());
        w.apply(load("samples/lit-1h.json")).unwrap();
        w.apply(ControlCommand::UnloadWorkout).unwrap();
        assert_eq!(w.profile.name(), "flat");
    }

    #[test]
    fn scenario_commands() {
        let mut w = World::new();
        // Outside a set (no workout, or a step outside the sets): refused.
        assert!(w.apply(ControlCommand::SetAdvantage { seconds: 30.0 }).is_err());
        w.apply(load("samples/hit-30-30.json")).unwrap();
        assert!(w.apply(ControlCommand::SetAdvantage { seconds: 30.0 }).is_err());
        // Load, seek and set in one go (as a playtest stage does).
        w.apply(ControlCommand::SeekWorkout { elapsed_s: 1860.0 }).unwrap();
        w.apply(ControlCommand::SetAdvantage { seconds: 4.0 }).unwrap();
        assert_eq!(w.scenario.brk.map(|b| (b.set, b.advantage_s)), Some((1, 4.0)));
        for seconds in [-1.0, 180.5, f32::NAN] {
            assert!(w.apply(ControlCommand::SetAdvantage { seconds }).is_err(), "{seconds}");
        }
        assert_eq!(w.scenario.brk.map(|b| b.advantage_s), Some(4.0));
        // Autopilot bias.
        w.apply(ControlCommand::AutopilotBias { pct: -25.0 }).unwrap();
        assert_eq!(w.session.autopilot_bias_pct, -25.0);
        for pct in [-50.5, 51.0, f32::NAN] {
            assert!(w.apply(ControlCommand::AutopilotBias { pct }).is_err(), "{pct}");
        }
        w.apply(ControlCommand::AutopilotBias { pct: 50.0 }).unwrap();
        assert_eq!(w.session.autopilot_bias_pct, 50.0);
    }

    #[test]
    fn radio_commands() {
        let mut w = World::new();
        let say = |key: &str| ControlCommand::RadioSay { key: key.into() };
        assert!(w.apply(say("nope")).is_err());
        assert_eq!(w.radio.current(), None);
        // The running break's advantage and the current band fill it in.
        w.apply(load("samples/hit-30-30.json")).unwrap();
        w.apply(ControlCommand::SeekWorkout { elapsed_s: 1860.0 }).unwrap();
        w.apply(ControlCommand::SetAdvantage { seconds: 34.0 }).unwrap();
        w.apply(say("bunch_closing")).unwrap();
        let line = w.radio.current().unwrap();
        assert_eq!((line.key.as_str(), line.speaker.label()), ("bunch_closing", "ds"));
        assert!(line.text.contains("0:34"), "{}", line.text);
        // Off: silent, and says nothing; a load starts a fresh radio.
        w.apply(ControlCommand::RadioEnabled { on: false }).unwrap();
        assert_eq!(w.radio.current(), None);
        w.apply(say("pull_start")).unwrap();
        assert_eq!(w.radio.current(), None);
        w.apply(ControlCommand::RadioEnabled { on: true }).unwrap();
        w.apply(say("pull_start")).unwrap();
        assert_eq!(w.radio.current().map(|c| c.key.as_str()), Some("pull_start"));
        w.apply(load("samples/hit-30-30.json")).unwrap();
        assert_eq!(
            (w.radio.current(), w.radio.queue_len(), w.radio.enabled),
            (None, 0, true)
        );
        w.apply(say("halfway")).unwrap();
        w.apply(ControlCommand::RestartRide).unwrap();
        assert_eq!(w.radio.current(), None);
        // Voice: a clip with the line; off, the text alone (the clip on air
        // stops); a restart keeps the setting.
        w.apply(say("workout_start")).unwrap();
        assert!(
            w.radio
                .current()
                .unwrap()
                .clip
                .as_deref()
                .is_some_and(|c| c.starts_with("workout_start-"))
        );
        w.apply(ControlCommand::VoiceEnabled { on: false }).unwrap();
        assert_eq!(
            (
                w.radio.current().unwrap().key.as_str(),
                w.radio.current().unwrap().clip.as_deref()
            ),
            ("workout_start", None)
        );
        w.apply(ControlCommand::RestartRide).unwrap();
        w.apply(say("halfway")).unwrap();
        assert_eq!(
            (w.radio.voice_enabled, w.radio.current().unwrap().clip.as_deref()),
            (false, None)
        );
        w.apply(ControlCommand::VoiceEnabled { on: true }).unwrap();
        w.apply(ControlCommand::RestartRide).unwrap();
        w.apply(say("halfway")).unwrap();
        assert!(w.radio.current().unwrap().clip.is_some());
    }

    #[test]
    fn countdown_beeps_switch() {
        let mut w = World::new();
        assert!(w.countdown.beeps_enabled);
        w.apply(ControlCommand::CountdownBeeps { on: false }).unwrap();
        assert!(!w.countdown.beeps_enabled);
        w.apply(ControlCommand::CountdownBeeps { on: true }).unwrap();
        assert!(w.countdown.beeps_enabled);
    }
}
