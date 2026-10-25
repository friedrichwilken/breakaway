//! Planned workouts: the JSON file format (flat steps in riding order,
//! targets in percent of FTP), its loader, the `WorkoutRunner` that walks
//! the steps and the `WorkoutSession` that sets the ride's band per step
//! and puts the rider on the course generated from the workout.
//! Pure logic: `dt` comes in as a parameter, no Bevy systems.

use std::path::{Path, PathBuf};

use bevy::prelude::{Message, Resource};
use serde::{Deserialize, Serialize};

use crate::course::{Course, TimeTable};
use crate::race::{Band, BandState, Ride};
use crate::settings::{MAX_FTP_W, MIN_FTP_W, valid_ftp};
use crate::sim::{self, RaceState, RiderInput};
use crate::terrain::Profile;
use crate::tuning::Tuning;

/// Highest accepted target, percent of FTP.
pub const MAX_PCT: f32 = 300.0;
/// The autopilot's bias is within ± this (percent of the band centre).
pub const MAX_AUTOPILOT_BIAS_PCT: f32 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepClass {
    WarmUp,
    Active,
    Rest,
    CoolDown,
}

impl StepClass {
    /// `warm_up`, `active`, `rest` or `cool_down`.
    pub fn label(self) -> &'static str {
        match self {
            StepClass::WarmUp => "warm_up",
            StepClass::Active => "active",
            StepClass::Rest => "rest",
            StepClass::CoolDown => "cool_down",
        }
    }

    /// For a step without a name: `Warm up`, `Active`, `Rest`, `Cool down`.
    pub fn title(self) -> &'static str {
        match self {
            StepClass::WarmUp => "Warm up",
            StepClass::Active => "Active",
            StepClass::Rest => "Rest",
            StepClass::CoolDown => "Cool down",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Single,
    Repetition,
    Ramp,
}

impl BlockKind {
    /// `single`, `repetition` or `ramp`.
    pub fn label(self) -> &'static str {
        match self {
            BlockKind::Single => "single",
            BlockKind::Repetition => "repetition",
            BlockKind::Ramp => "ramp",
        }
    }
}

/// One step. Repetition blocks are already expanded: `block` numbers the
/// source blocks from 0, `rep` / `reps` are the 1-based repetition and the
/// block's count (1 / 1 outside repetitions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub name: String,
    pub duration_s: f32,
    pub low_pct: f32,
    pub high_pct: f32,
    pub class: StepClass,
    pub block: u32,
    pub block_kind: BlockKind,
    pub rep: u32,
    pub reps: u32,
    /// Stored, not used yet.
    #[serde(default)]
    pub cadence_low: Option<f32>,
    #[serde(default)]
    pub cadence_high: Option<f32>,
    /// A rest that cannot be extended (used by a later task).
    #[serde(default)]
    pub fixed_rest: bool,
}

impl Step {
    /// The step's name, or its class when the name is empty.
    pub fn display_name(&self) -> &str {
        if self.name.trim().is_empty() {
            self.class.title()
        } else {
            &self.name
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workout {
    pub name: String,
    /// `YYYY-MM-DD`, informational.
    #[serde(default)]
    pub date: Option<String>,
    /// Where it came from, e.g. `trainingpeaks:100000005`.
    #[serde(default)]
    pub source: Option<String>,
    pub steps: Vec<Step>,
}

impl Workout {
    /// Parses and validates a workout file's text.
    pub fn parse(text: &str) -> Result<Workout, String> {
        let workout: Workout = serde_json::from_str(text).map_err(|e| e.to_string())?;
        workout.validate()?;
        Ok(workout)
    }

    pub fn load(path: &Path) -> Result<Workout, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Workout::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// At least one step; every duration finite and > 0; every target
    /// `0 < low <= high <= 300` percent of FTP.
    pub fn validate(&self) -> Result<(), String> {
        if self.steps.is_empty() {
            return Err("workout has no steps".into());
        }
        for (i, s) in self.steps.iter().enumerate() {
            if !(s.duration_s.is_finite() && s.duration_s > 0.0) {
                return Err(format!("step {i} ({:?}): duration_s must be > 0", s.name));
            }
            let pct_ok = s.low_pct.is_finite() && s.high_pct.is_finite();
            if !(pct_ok && 0.0 < s.low_pct && s.low_pct <= s.high_pct && s.high_pct <= MAX_PCT) {
                return Err(format!(
                    "step {i} ({:?}): need 0 < low_pct <= high_pct <= {MAX_PCT}",
                    s.name
                ));
            }
        }
        Ok(())
    }

    pub fn total_s(&self) -> f32 {
        self.steps.iter().map(|s| s.duration_s).sum()
    }
}

/// A band narrower than this is widened symmetrically to it.
pub const MIN_STEP_BAND_W: f32 = 20.0;

/// The watts band for a step: `low_pct / 100 × ftp_w` to the same for high,
/// widened symmetrically to `MIN_STEP_BAND_W` when narrower (a 1.5 s
/// smoothed power needs about that much room to settle into an easy band).
pub fn step_band(step: &Step, ftp_w: f32) -> Band {
    let (low_w, high_w) = (step.low_pct * ftp_w / 100.0, step.high_pct * ftp_w / 100.0);
    if high_w - low_w >= MIN_STEP_BAND_W {
        return Band { low_w, high_w };
    }
    let centre = (low_w + high_w) / 2.0;
    // Never at or below 0 W (only reachable with tiny targets).
    let low_w = (centre - MIN_STEP_BAND_W / 2.0).max(0.1);
    Band {
        low_w,
        high_w: low_w + MIN_STEP_BAND_W,
    }
}

#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkoutEvent {
    StepStarted { index: usize },
    Finished,
}

/// Walks a workout's steps; time comes in as `dt`. Times are `f64` so a
/// long ride of small frame steps does not drift.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkoutRunner {
    pub workout: Workout,
    pub index: usize,
    pub step_elapsed_s: f64,
    /// Position in the workout: the start of the current step plus
    /// `step_elapsed_s` (a skip jumps it to the next step's start).
    pub elapsed_s: f64,
    pub finished: bool,
}

impl WorkoutRunner {
    /// At the start of step 0. `workout` must have steps (`Workout::validate`).
    pub fn new(workout: Workout) -> WorkoutRunner {
        WorkoutRunner {
            workout,
            index: 0,
            step_elapsed_s: 0.0,
            elapsed_s: 0.0,
            finished: false,
        }
    }

    pub fn current(&self) -> &Step {
        &self.workout.steps[self.index]
    }

    /// The step after the current one; `None` on the last step.
    pub fn next(&self) -> Option<&Step> {
        self.workout.steps.get(self.index + 1)
    }

    /// 0 once finished.
    pub fn step_remaining_s(&self) -> f32 {
        if self.finished {
            0.0
        } else {
            (f64::from(self.current().duration_s) - self.step_elapsed_s).max(0.0) as f32
        }
    }

    pub fn total_s(&self) -> f32 {
        self.workout.total_s()
    }

    /// Advances by `dt`; may cross several steps, each emitting its
    /// `StepStarted` in order, then `Finished` after the last.
    pub fn advance(&mut self, dt: f32) -> Vec<WorkoutEvent> {
        let mut events = Vec::new();
        if self.finished || !(dt.is_finite() && dt > 0.0) {
            return events;
        }
        self.step_elapsed_s += f64::from(dt);
        self.elapsed_s += f64::from(dt);
        while !self.finished && self.step_elapsed_s >= f64::from(self.current().duration_s) {
            self.step_elapsed_s -= f64::from(self.current().duration_s);
            self.next_step(&mut events);
        }
        events
    }

    /// Ends the current step now.
    pub fn skip(&mut self) -> Vec<WorkoutEvent> {
        let mut events = Vec::new();
        if !self.finished {
            self.step_elapsed_s = 0.0;
            self.next_step(&mut events);
        }
        events
    }

    /// Jumps to workout time `elapsed_s` (clamped to `0..=total`): into the
    /// step it falls in (`StepStarted`), or `Finished` at the total.
    pub fn seek(&mut self, elapsed_s: f64) -> Vec<WorkoutEvent> {
        let total = f64::from(self.total_s());
        let elapsed_s = elapsed_s.clamp(0.0, total);
        if elapsed_s >= total {
            self.index = self.workout.steps.len() - 1;
            self.step_elapsed_s = f64::from(self.current().duration_s);
            self.elapsed_s = total;
            self.finished = true;
            return vec![WorkoutEvent::Finished];
        }
        let mut start = 0.0;
        for (i, s) in self.workout.steps.iter().enumerate() {
            let end = start + f64::from(s.duration_s);
            if elapsed_s < end {
                self.index = i;
                self.step_elapsed_s = elapsed_s - start;
                break;
            }
            start = end;
        }
        self.elapsed_s = elapsed_s;
        self.finished = false;
        vec![WorkoutEvent::StepStarted { index: self.index }]
    }

    fn next_step(&mut self, events: &mut Vec<WorkoutEvent>) {
        if self.index + 1 < self.workout.steps.len() {
            self.index += 1;
            self.elapsed_s = self.start_of(self.index) + self.step_elapsed_s;
            events.push(WorkoutEvent::StepStarted { index: self.index });
        } else {
            self.finished = true;
            self.step_elapsed_s = f64::from(self.current().duration_s);
            self.elapsed_s = self.start_of(self.workout.steps.len());
            events.push(WorkoutEvent::Finished);
        }
    }

    /// Seconds from the workout's start to the start of step `index`.
    fn start_of(&self, index: usize) -> f64 {
        self.workout.steps[..index]
            .iter()
            .map(|s| f64::from(s.duration_s))
            .sum()
    }
}

/// The repo's `workouts/` directory, baked in at build time.
pub fn repo_workouts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("workouts")
}

/// `<config dir>/breakaway/workouts/`, where `just fetch-workout` writes.
pub fn config_workouts_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("breakaway").join("workouts"))
}

/// The directories a relative workout path is looked up in, in order.
pub fn search_dirs() -> Vec<PathBuf> {
    std::iter::once(repo_workouts_dir())
        .chain(config_workouts_dir())
        .collect()
}

/// An absolute path as it is; a relative one in the first of `dirs` where
/// the file exists.
pub fn resolve(path: &str, dirs: &[PathBuf]) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if path.is_empty() {
        return Err("empty workout path".into());
    }
    if p.is_absolute() {
        return Ok(p.to_owned());
    }
    dirs.iter().map(|d| d.join(p)).find(|c| c.is_file()).ok_or_else(|| {
        let looked: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
        format!("workout {path:?} not found in {}", looked.join(", "))
    })
}

/// Resolves (`search_dirs`) and loads a workout file.
pub fn load_path(path: &str) -> Result<Workout, String> {
    Workout::load(&resolve(path, &search_dirs())?)
}

/// How the world moves this frame (see `WorkoutSession::pace`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pace<'a> {
    /// At the terrain's scripted speed: no workout, or it is finished.
    Free,
    /// Auto-pause: the world does not move.
    Paused,
    /// At the terrain's speed, kept on `table` at workout time `elapsed_s`
    /// (the end of this frame).
    Locked { table: &'a TimeTable, elapsed_s: f64 },
}

/// The loaded workout (if any), its course, the FTP and the autopilot
/// switch. A loaded workout owns the ride's band (each step's band is set as
/// it starts) and the course (time is the master: the distance follows the
/// workout's time).
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct WorkoutSession {
    pub runner: Option<WorkoutRunner>,
    pub ftp_w: f32,
    /// The scripted mock rides the centre of the band (± 3 %).
    pub autopilot: bool,
    /// The autopilot rides band centre × (1 + pct / 100), -50..=50.
    pub autopilot_bias_pct: f32,
    /// The free-ride band, restored when the workout is unloaded.
    pub free_band: Band,
    /// The course generated from the loaded workout.
    pub course: Option<Course>,
    /// The profile active before the workout, restored when it is unloaded.
    pub free_profile: Option<Profile>,
    /// Counts loads and unloads, so a reload of the same workout is seen as
    /// new (the scenario layer replans on a change).
    pub loads: u64,
    /// The tuning file's values (speed factor, lateral range); the world
    /// moves and the course is solved with them.
    pub tuning: Tuning,
}

impl WorkoutSession {
    pub fn new(ftp_w: f32, free_band: Band) -> WorkoutSession {
        WorkoutSession {
            runner: None,
            ftp_w,
            autopilot: false,
            autopilot_bias_pct: 0.0,
            free_band,
            course: None,
            free_profile: None,
            loads: 0,
            tuning: Tuning::default(),
        }
    }

    /// Restarts the ride (matches, score, totals) at step 0 of `workout`
    /// and puts the rider at distance 0 of the workout's course, which
    /// becomes the active profile.
    pub fn load(&mut self, workout: Workout, ride: &mut Ride, profile: &mut Profile, race: &mut RaceState) {
        let course = crate::course::generate(&workout, self.tuning.speed_factor);
        if self.runner.is_none() {
            self.free_band = ride.band;
            self.free_profile = Some(profile.clone());
        }
        let runner = WorkoutRunner::new(workout);
        ride.band = step_band(runner.current(), self.ftp_w);
        ride.restart();
        ride.begin_grace();
        self.runner = Some(runner);
        *profile = course.profile.clone();
        race.distance_m = 0.0;
        self.course = Some(course);
        self.loads += 1;
    }

    /// Back to free ride with the band and the profile from before the
    /// workout (distance 0); the ride's totals stay.
    pub fn unload(&mut self, ride: &mut Ride, profile: &mut Profile, race: &mut RaceState) {
        if self.runner.take().is_some() {
            self.loads += 1;
            ride.band = self.free_band;
            ride.finished = false;
            self.course = None;
            if let Some(p) = self.free_profile.take() {
                *profile = p;
                race.distance_m = 0.0;
            }
        }
    }

    /// 50..=600 W; re-resolves the current step's band.
    pub fn set_ftp(&mut self, ftp_w: f32, ride: &mut Ride) -> Result<(), String> {
        if !valid_ftp(ftp_w) {
            return Err(format!("ftp_w must be {MIN_FTP_W}..={MAX_FTP_W}"));
        }
        self.ftp_w = ftp_w;
        if let Some(r) = self.runner.as_ref().filter(|r| !r.finished) {
            ride.band = step_band(r.current(), ftp_w);
        }
        Ok(())
    }

    /// Ends the current step; the rider jumps to the next step's start on
    /// the course (the course's end after the last step).
    pub fn skip(&mut self, ride: &mut Ride, race: &mut RaceState) -> Result<(), String> {
        let Some(runner) = self.runner.as_mut() else {
            return Err("no workout loaded".into());
        };
        if runner.finished {
            return Err("workout already finished".into());
        }
        let events = runner.skip();
        self.apply_events(&events, ride);
        if let (Some(r), Some(c)) = (&self.runner, &self.course) {
            race.distance_m = if r.finished {
                c.length_m()
            } else {
                c.step_start_m[r.index]
            };
        }
        Ok(())
    }

    /// Jumps the workout and the rider to workout time `elapsed_s`
    /// (`0..=total`); the step it falls in starts afresh (band, grace). The
    /// ride's totals stay.
    pub fn seek(&mut self, elapsed_s: f32, ride: &mut Ride, race: &mut RaceState) -> Result<(), String> {
        let Some(runner) = self.runner.as_mut() else {
            return Err("no workout loaded".into());
        };
        let total = runner.total_s();
        if !(elapsed_s.is_finite() && (0.0..=total).contains(&elapsed_s)) {
            return Err(format!("elapsed_s must be 0..={total}"));
        }
        let events = runner.seek(f64::from(elapsed_s));
        if !runner.finished {
            ride.finished = false;
            ride.state = BandState::Idle;
        }
        self.apply_events(&events, ride);
        if let Some(c) = &self.course {
            race.distance_m = c.expected_distance.at(f64::from(elapsed_s));
        }
        Ok(())
    }

    /// Advances the workout by `dt` unless the ride is `Idle` (auto-pause).
    pub fn advance(&mut self, ride: &mut Ride, dt: f32) -> Vec<WorkoutEvent> {
        let Some(runner) = self.runner.as_mut() else {
            return Vec::new();
        };
        if ride.state == BandState::Idle || runner.finished {
            return Vec::new();
        }
        let events = runner.advance(dt);
        self.apply_events(&events, ride);
        events
    }

    /// How the world moves: locked to the workout's time while it runs,
    /// still while the ride is `Idle`, free otherwise (no workout, or after
    /// the finish: rolling on along the flat end of the course).
    pub fn pace(&self, ride: &Ride) -> Pace<'_> {
        match (&self.runner, &self.course) {
            (Some(r), Some(c)) if !r.finished => {
                if ride.state == BandState::Idle {
                    Pace::Paused
                } else {
                    Pace::Locked {
                        table: &c.expected_distance,
                        elapsed_s: r.elapsed_s,
                    }
                }
            }
            _ => Pace::Free,
        }
    }

    /// Moves the world by `dt` after the ride and the workout have been
    /// stepped this frame, at the pace `pace` gives.
    pub fn move_world(&self, ride: &Ride, race: &mut RaceState, input: RiderInput, profile: &Profile, dt: f32) {
        match self.pace(ride) {
            Pace::Free => sim::step(race, input, profile, &self.tuning, dt),
            Pace::Paused => sim::step_paused(race, input, profile, &self.tuning, dt),
            Pace::Locked { table, elapsed_s } => {
                // The workout already advanced by `dt` this frame.
                let from = elapsed_s - f64::from(dt);
                sim::step_locked(race, input, profile, &self.tuning, dt, |s| {
                    table.at(from + f64::from(s))
                });
            }
        }
    }

    /// Where the workout expects the rider minus where they are (m);
    /// `None` without a workout.
    pub fn drift_m(&self, distance_m: f32) -> Option<f32> {
        let (r, c) = (self.runner.as_ref()?, self.course.as_ref()?);
        Some(c.expected_distance.at(r.elapsed_s) - distance_m)
    }

    fn apply_events(&self, events: &[WorkoutEvent], ride: &mut Ride) {
        let Some(runner) = &self.runner else { return };
        for e in events {
            match e {
                WorkoutEvent::StepStarted { index } => {
                    ride.band = step_band(&runner.workout.steps[*index], self.ftp_w);
                    ride.begin_grace();
                }
                WorkoutEvent::Finished => ride.finish(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::Grace;

    const ONE: &str = r#"{
        "name": "Sweet spot - 6x5 min", "date": "2026-01-09", "source": "trainingpeaks:100000005",
        "steps": [
            { "name": "Warm up", "duration_s": 120, "low_pct": 40, "high_pct": 50,
              "class": "warm_up", "block": 0, "block_kind": "single",
              "rep": 1, "reps": 1, "cadence_low": null, "cadence_high": null,
              "fixed_rest": false }
        ]
    }"#;

    #[test]
    fn parses_the_spec_example() {
        let w = Workout::parse(ONE).unwrap();
        assert_eq!(
            (w.name.as_str(), w.date.as_deref(), w.steps.len()),
            ("Sweet spot - 6x5 min", Some("2026-01-09"), 1)
        );
        let s = &w.steps[0];
        assert_eq!(
            (s.duration_s, s.low_pct, s.high_pct, s.class, s.block_kind),
            (120.0, 40.0, 50.0, StepClass::WarmUp, BlockKind::Single)
        );
        assert_eq!(w.total_s(), 120.0);
        // `fixed_rest`, cadence, date and source may be left out.
        let lean = ONE
            .replace(r#", "fixed_rest": false"#, "")
            .replace(r#""cadence_low": null, "cadence_high": null,"#, "");
        let lean = lean.replace(r#""date": "2026-01-09", "source": "trainingpeaks:100000005","#, "");
        let w = Workout::parse(&lean).unwrap();
        assert!(!w.steps[0].fixed_rest && w.steps[0].cadence_low.is_none() && w.source.is_none());
    }

    #[test]
    fn rejects_bad_files() {
        let cases = [
            ONE.replace(r#""steps": ["#, r#""steps": [], "x": ["#),
            ONE.replace(r#""duration_s": 120"#, r#""duration_s": 0"#),
            ONE.replace(r#""duration_s": 120"#, r#""duration_s": -5"#),
            ONE.replace(r#""low_pct": 40"#, r#""low_pct": 0"#),
            ONE.replace(r#""low_pct": 40"#, r#""low_pct": 60"#),
            ONE.replace(r#""high_pct": 50"#, r#""high_pct": 301"#),
            ONE.replace(r#""warm_up""#, r#""sprint""#),
            ONE.replace(r#""single""#, r#""pyramid""#),
            ONE.replace(
                r#""name": "Sweet spot - 6x5 min","#,
                r#""name": "Sweet spot - 6x5 min", "coach": "x","#,
            ),
            ONE.replace(r#""fixed_rest": false"#, r#""fixed_rest": false, "power_w": 200"#),
            "not json".to_owned(),
        ];
        for (i, text) in cases.iter().enumerate() {
            assert!(Workout::parse(text).is_err(), "case {i}");
        }
        // No steps at all.
        let empty = r#"{"name": "x", "steps": []}"#;
        assert_eq!(Workout::parse(empty).unwrap_err(), "workout has no steps");
        assert!(
            Workout::load(Path::new("/nonexistent/w.json"))
                .unwrap_err()
                .contains("/nonexistent/w.json")
        );
    }

    #[test]
    fn every_sample_parses() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("workouts").join("samples");
        let mut found: Vec<(String, usize, f32)> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .map(|p| {
                let w = Workout::load(&p).unwrap_or_else(|e| panic!("{e}"));
                (
                    p.file_stem().unwrap().to_string_lossy().into_owned(),
                    w.steps.len(),
                    w.total_s(),
                )
            })
            .collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let expected = [
            ("hidits-4x12", 65, 7200.0),
            ("hit-30-30", 100, 8400.0),
            ("lactate-elimination", 17, 10440.0),
            ("lit-1h", 2, 3600.0),
            ("mit-sweetspot-6x5", 19, 4200.0),
        ];
        let expected: Vec<(String, usize, f32)> = expected.iter().map(|(n, c, t)| (n.to_string(), *c, *t)).collect();
        assert_eq!(found, expected);
    }

    fn step(name: &str, duration_s: f32, low_pct: f32, high_pct: f32) -> Step {
        Step {
            name: name.into(),
            duration_s,
            low_pct,
            high_pct,
            class: StepClass::Active,
            block: 0,
            block_kind: BlockKind::Single,
            rep: 1,
            reps: 1,
            cadence_low: None,
            cadence_high: None,
            fixed_rest: false,
        }
    }

    /// 10 s, 20 s, 30 s.
    fn three() -> WorkoutRunner {
        let steps = vec![
            step("a", 10.0, 50.0, 60.0),
            step("b", 20.0, 90.0, 100.0),
            step("c", 30.0, 40.0, 50.0),
        ];
        WorkoutRunner::new(Workout {
            name: "w".into(),
            date: None,
            source: None,
            steps,
        })
    }

    #[test]
    fn runner_walks_the_steps_in_order() {
        let mut r = three();
        assert_eq!(
            (r.current().name.as_str(), r.next().map(|s| s.name.as_str())),
            ("a", Some("b"))
        );
        assert_eq!(r.advance(9.5), vec![]);
        assert_eq!(r.step_remaining_s(), 0.5);
        assert_eq!(r.advance(0.5), vec![WorkoutEvent::StepStarted { index: 1 }]);
        assert_eq!((r.index, r.step_elapsed_s, r.elapsed_s), (1, 0.0, 10.0));
        assert_eq!(r.step_remaining_s(), 20.0);
        let mut events = Vec::new();
        for _ in 0..(50 * 60) {
            events.extend(r.advance(1.0 / 60.0));
        }
        assert_eq!(
            events,
            vec![WorkoutEvent::StepStarted { index: 2 }, WorkoutEvent::Finished]
        );
        assert!(r.finished);
        assert_eq!((r.index, r.elapsed_s, r.step_remaining_s()), (2, 60.0, 0.0));
        assert!(r.next().is_none());
        // Nothing moves after the finish.
        assert_eq!(r.advance(5.0), vec![]);
        assert_eq!(r.elapsed_s, 60.0);
    }

    #[test]
    fn a_large_dt_crosses_several_steps() {
        let mut r = three();
        assert_eq!(
            r.advance(35.0),
            vec![
                WorkoutEvent::StepStarted { index: 1 },
                WorkoutEvent::StepStarted { index: 2 }
            ]
        );
        assert_eq!(
            (r.index, r.step_elapsed_s, r.elapsed_s, r.step_remaining_s()),
            (2, 5.0, 35.0, 25.0)
        );
        let mut r = three();
        let all = r.advance(1000.0);
        assert_eq!(all.last(), Some(&WorkoutEvent::Finished));
        assert_eq!((all.len(), r.elapsed_s), (3, 60.0));
        // Zero, negative or NaN dt do nothing.
        let mut r = three();
        for dt in [0.0, -1.0, f32::NAN] {
            assert!(r.advance(dt).is_empty());
        }
        assert_eq!(r, three());
    }

    #[test]
    fn skip_jumps_to_the_next_step_and_finishes_after_the_last() {
        let mut r = three();
        r.advance(4.0);
        assert_eq!(r.skip(), vec![WorkoutEvent::StepStarted { index: 1 }]);
        assert_eq!((r.index, r.step_elapsed_s, r.elapsed_s), (1, 0.0, 10.0));
        assert_eq!(r.skip(), vec![WorkoutEvent::StepStarted { index: 2 }]);
        assert_eq!(r.elapsed_s, 30.0);
        assert_eq!(r.skip(), vec![WorkoutEvent::Finished]);
        assert!(r.finished && r.elapsed_s == 60.0);
        assert!(r.skip().is_empty());
    }

    #[test]
    fn band_from_percent_of_ftp_at_least_twenty_watts_wide() {
        let s = step("x", 60.0, 65.0, 75.0);
        assert_eq!(
            step_band(&s, 200.0),
            Band {
                low_w: 130.0,
                high_w: 150.0
            }
        );
        // FTP change scales it.
        assert_eq!(
            step_band(&s, 250.0),
            Band {
                low_w: 162.5,
                high_w: 187.5
            }
        );
        // 110-112 % of 200 = 220-224: widened around 222.
        assert_eq!(
            step_band(&step("x", 60.0, 110.0, 112.0), 200.0),
            Band {
                low_w: 212.0,
                high_w: 232.0
            }
        );
        // A single value (low = high) too.
        assert_eq!(
            step_band(&step("x", 60.0, 50.0, 50.0), 200.0),
            Band {
                low_w: 90.0,
                high_w: 110.0
            }
        );
        // 50-55 % (10 W) is widened; exactly 20 W stays.
        assert_eq!(
            step_band(&step("x", 60.0, 50.0, 55.0), 200.0),
            Band {
                low_w: 95.0,
                high_w: 115.0
            }
        );
        assert_eq!(
            step_band(&step("x", 60.0, 50.0, 60.0), 200.0),
            Band {
                low_w: 100.0,
                high_w: 120.0
            }
        );
        // Every band is valid for the ride.
        for (lo, hi) in [(0.5, 0.5), (1.0, 300.0), (300.0, 300.0)] {
            for ftp in [50.0, 200.0, 600.0] {
                let b = step_band(&step("x", 60.0, lo, hi), ftp);
                assert!(
                    Band::new(b.low_w, b.high_w).is_ok() && b.high_w - b.low_w >= MIN_STEP_BAND_W - 1e-3,
                    "{b:?}"
                );
            }
        }
    }

    fn complete(s: &WorkoutSession) -> bool {
        s.runner.as_ref().is_some_and(|r| r.finished)
    }

    /// The demo course at distance 0.
    fn road() -> (Profile, RaceState) {
        (Profile::demo(), RaceState::default())
    }

    fn session() -> (WorkoutSession, Ride) {
        let free = Band::new(180.0, 220.0).unwrap();
        (WorkoutSession::new(200.0, free), Ride::new(free))
    }

    fn mit() -> Workout {
        load_path("samples/mit-sweetspot-6x5.json").unwrap()
    }

    #[test]
    fn loading_restarts_the_ride_at_step_zero_and_owns_the_band() {
        let (mut s, mut ride) = session();
        let (mut profile, mut race) = road();
        ride.score = 80.0;
        ride.matches = 2;
        s.load(mit(), &mut ride, &mut profile, &mut race);
        assert_eq!(
            ride.band,
            Band {
                low_w: 90.0,
                high_w: 110.0
            }
        );
        assert_eq!((ride.score, ride.matches), (0.0, 5));
        assert_eq!(
            ride.grace,
            Some(Grace {
                elapsed_s: 0.0,
                entered_at_s: None
            })
        );
        assert_eq!(
            s.runner.as_ref().map(|r| (r.index, r.workout.steps.len())),
            Some((0, 19))
        );
        // Six skips: first sweet-spot rep, 168-188 W.
        for _ in 0..6 {
            s.skip(&mut ride, &mut race).unwrap();
        }
        let r = s.runner.as_ref().unwrap();
        assert_eq!(
            (r.current().name.as_str(), r.current().rep, r.current().reps),
            ("Sweet spot", 1, 6)
        );
        assert_eq!(
            ride.band,
            Band {
                low_w: 168.0,
                high_w: 188.0
            }
        );
        // FTP change mid-step scales the band.
        s.set_ftp(250.0, &mut ride).unwrap();
        assert_eq!(
            ride.band,
            Band {
                low_w: 210.0,
                high_w: 235.0
            }
        );
        for bad in [49.0, 601.0, f32::NAN] {
            assert!(s.set_ftp(bad, &mut ride).is_err());
        }
        assert_eq!(s.ftp_w, 250.0);
        // Unload: free ride with the band from before.
        s.unload(&mut ride, &mut profile, &mut race);
        assert_eq!(
            (s.runner.is_none(), ride.band),
            (true, Band::new(180.0, 220.0).unwrap())
        );
        assert!(s.skip(&mut ride, &mut race).is_err());
    }

    #[test]
    fn the_workout_pauses_while_the_ride_is_idle() {
        let (mut s, mut ride) = session();
        let (mut profile, mut race) = road();
        s.load(mit(), &mut ride, &mut profile, &mut race);
        assert_eq!(ride.state, BandState::Idle);
        assert!(s.advance(&mut ride, 10.0).is_empty());
        assert_eq!(s.runner.as_ref().unwrap().elapsed_s, 0.0);
        crate::race::step(&mut ride, 90.0, 0.1);
        s.advance(&mut ride, 0.1);
        assert!((s.runner.as_ref().unwrap().elapsed_s - 0.1).abs() < 1e-6);
        // A step change sets the next band and a fresh grace.
        assert_eq!(
            s.advance(&mut ride, 180.0),
            vec![WorkoutEvent::StepStarted { index: 1 }]
        );
        assert_eq!(
            (ride.band, ride.grace),
            (
                Band {
                    low_w: 112.0,
                    high_w: 132.0
                },
                Some(Grace {
                    elapsed_s: 0.0,
                    entered_at_s: None
                })
            )
        );
    }

    #[test]
    fn finishing_stops_the_ride_and_restart_returns_to_free_ride() {
        let (mut s, mut ride) = session();
        let (mut profile, mut race) = road();
        s.load(mit(), &mut ride, &mut profile, &mut race);
        for _ in 0..18 {
            s.skip(&mut ride, &mut race).unwrap();
        }
        crate::race::step(&mut ride, 85.0, 1.0);
        let score = ride.score;
        assert!(score > 0.0);
        assert_eq!(s.advance(&mut ride, 500.0), vec![WorkoutEvent::Finished]);
        assert!(complete(&s) && ride.finished && ride.state == BandState::Finished);
        crate::race::step(&mut ride, 100.0, 10.0);
        assert_eq!(ride.score, score);
        assert!(s.skip(&mut ride, &mut race).is_err());
        // FTP can still change; the finished ride keeps its band.
        let band = ride.band;
        s.set_ftp(300.0, &mut ride).unwrap();
        assert_eq!(ride.band, band);
        s.unload(&mut ride, &mut profile, &mut race);
        assert!(!ride.finished && !complete(&s));
    }

    /// The whole sample ridden by the autopilot: in band at least 95 % of
    /// the time, no match burned, finished at 4200 s.
    #[test]
    fn autopilot_rides_the_whole_sample_in_band() {
        let (mut s, mut ride) = session();
        let (mut profile, mut race) = road();
        s.load(mit(), &mut ride, &mut profile, &mut race);
        let dt = 1.0 / 60.0;
        let mut t = 0.0_f64;
        while !complete(&s) && t < 5000.0 {
            let power = crate::input::autopilot_reading(t, &ride.band).power_w;
            crate::race::step(&mut ride, power, dt);
            s.advance(&mut ride, dt);
            t += f64::from(dt);
        }
        let r = s.runner.as_ref().unwrap();
        assert!(r.finished && (r.elapsed_s - 4200.0).abs() < 1e-6, "{}", r.elapsed_s);
        assert!(ride.in_band_pct() >= 95.0, "{}", ride.in_band_pct());
        assert_eq!((ride.matches, ride.burns), (5, 0));
    }

    /// One game frame as `main.rs` runs it: ride, workout, world.
    fn frame(s: &mut WorkoutSession, ride: &mut Ride, race: &mut RaceState, profile: &Profile, power_w: f32, dt: f32) {
        crate::race::step(ride, power_w, dt);
        s.advance(ride, dt);
        s.move_world(
            ride,
            race,
            RiderInput {
                power_w,
                balance_left_pct: 50.0,
            },
            profile,
            dt,
        );
    }

    #[test]
    fn loading_puts_the_rider_on_the_workout_course_and_unloading_takes_them_back() {
        let (mut s, mut ride) = session();
        let (mut profile, mut race) = road();
        race.distance_m = 1234.0;
        s.load(mit(), &mut ride, &mut profile, &mut race);
        assert_eq!(
            (profile.name(), profile.is_looping(), race.distance_m),
            ("workout", false, 0.0)
        );
        let length = s.course.as_ref().unwrap().length_m();
        assert!(length > 20_000.0 && profile.total_length_m() == length);
        assert_eq!(profile.gradient_at(100.0), 0.0);
        // Loading another keeps the profile from before the first.
        s.load(
            load_path("samples/lit-1h.json").unwrap(),
            &mut ride,
            &mut profile,
            &mut race,
        );
        assert_eq!(s.free_profile.as_ref().map(|p| p.name()), Some("demo"));
        s.unload(&mut ride, &mut profile, &mut race);
        assert_eq!(
            (profile.name(), race.distance_m, s.course.is_none()),
            ("demo", 0.0, true)
        );
        // Unloading with nothing loaded changes nothing.
        race.distance_m = 50.0;
        s.unload(&mut ride, &mut profile, &mut race);
        assert_eq!((profile.name(), race.distance_m), ("demo", 50.0));
    }

    #[test]
    fn skip_and_seek_move_the_rider_with_the_workout() {
        let (mut s, mut ride) = session();
        let (mut profile, mut race) = road();
        s.load(mit(), &mut ride, &mut profile, &mut race);
        let starts = s.course.as_ref().unwrap().step_start_m.clone();
        s.skip(&mut ride, &mut race).unwrap();
        assert_eq!(race.distance_m, starts[1]);
        // 15 s before the first sweet-spot rep.
        s.seek(1185.0, &mut ride, &mut race).unwrap();
        let r = s.runner.as_ref().unwrap();
        assert_eq!((r.index, r.elapsed_s, r.step_elapsed_s), (5, 1185.0, 315.0));
        assert_eq!(ride.band, step_band(&r.workout.steps[5], 200.0));
        assert!(ride.grace.is_some());
        assert!(
            race.distance_m > starts[5] && race.distance_m < starts[6],
            "{}",
            race.distance_m
        );
        assert!(s.drift_m(race.distance_m).unwrap().abs() < 1e-3);
        s.seek(1200.0, &mut ride, &mut race).unwrap();
        assert!((race.distance_m - starts[6]).abs() < 0.5);
        for bad in [-1.0, 4200.5, f32::NAN] {
            assert!(s.seek(bad, &mut ride, &mut race).is_err(), "{bad}");
        }
        // To the end: finished; back again: running.
        s.seek(4200.0, &mut ride, &mut race).unwrap();
        assert!(complete(&s) && ride.finished);
        assert!((race.distance_m - s.course.as_ref().unwrap().length_m()).abs() < 0.5);
        s.seek(0.0, &mut ride, &mut race).unwrap();
        assert!(!complete(&s) && !ride.finished && race.distance_m == 0.0);
        // Skipping the last step ends at the course's end.
        s.seek(4000.0, &mut ride, &mut race).unwrap();
        s.skip(&mut ride, &mut race).unwrap();
        assert!(complete(&s) && race.distance_m == s.course.as_ref().unwrap().length_m());
        s.unload(&mut ride, &mut profile, &mut race);
        assert!(s.seek(10.0, &mut ride, &mut race).is_err());
        assert_eq!(s.drift_m(0.0), None);
    }

    /// Time is the master: at a 50× time scale (0.83 s frames) the
    /// autopilot rides the whole sample with the distance locked to the
    /// workout's time; afterwards the world rolls on at flat speed.
    #[test]
    fn the_distance_stays_on_the_workout_time() {
        for (sample, min_in_band) in [("mit-sweetspot-6x5", 95.0), ("hit-30-30", 93.0)] {
            let (mut s, mut ride) = session();
            let (mut profile, mut race) = road();
            s.load(
                load_path(&format!("samples/{sample}.json")).unwrap(),
                &mut ride,
                &mut profile,
                &mut race,
            );
            let dt = 50.0 / 60.0;
            let (mut t, mut worst) = (0.0_f64, 0.0_f32);
            while !complete(&s) && t < 9000.0 {
                let power = crate::input::autopilot_reading(t, &ride.band).power_w;
                frame(&mut s, &mut ride, &mut race, &profile, power, dt);
                if !complete(&s) {
                    worst = worst.max(s.drift_m(race.distance_m).unwrap().abs());
                }
                t += f64::from(dt);
            }
            let r = s.runner.as_ref().unwrap();
            assert!(
                r.finished && (r.elapsed_s - f64::from(r.total_s())).abs() < 1e-3,
                "{sample}"
            );
            assert!(worst < 15.0, "{sample}: drift {worst} m");
            assert_eq!(ride.matches, 5, "{sample}");
            assert!(ride.in_band_pct() >= min_in_band, "{sample}: {}", ride.in_band_pct());
            eprintln!(
                "{sample} at 50x: worst drift {worst:.2} m, in band {:.2} %",
                ride.in_band_pct()
            );
            // Finished: rolling on along the flat end, at 10 m/s once past
            // the blend.
            assert!(race.distance_m >= s.course.as_ref().unwrap().length_m() - 15.0);
            frame(&mut s, &mut ride, &mut race, &profile, 100.0, 5.0);
            let at = race.distance_m;
            frame(&mut s, &mut ride, &mut race, &profile, 100.0, 1.0);
            frame(&mut s, &mut ride, &mut race, &profile, 100.0, 1.0);
            assert!(
                (race.distance_m - at - 20.0).abs() < 1e-3 && race.world_speed_mps == 10.0,
                "{}",
                race.distance_m - at
            );
        }
    }

    #[test]
    fn the_world_stands_still_while_the_ride_is_idle() {
        let (mut s, mut ride) = session();
        let (mut profile, mut race) = road();
        s.load(mit(), &mut ride, &mut profile, &mut race);
        let dt = 1.0 / 60.0;
        // Nothing moves before the first pedal stroke.
        frame(&mut s, &mut ride, &mut race, &profile, 0.0, 1.0);
        assert_eq!((race.distance_m, race.world_speed_mps), (0.0, 0.0));
        let mut t = 0.0;
        while t < 600.0 {
            let power = ride.band.centre_w();
            frame(&mut s, &mut ride, &mut race, &profile, power, dt);
            t += dt;
        }
        // 0 W: within 4 s both the distance and the workout stop.
        let mut stopped_at = None;
        let mut t = 0.0;
        while t < 10.0 {
            let (d, e) = (race.distance_m, s.runner.as_ref().unwrap().elapsed_s);
            frame(&mut s, &mut ride, &mut race, &profile, 0.0, dt);
            if race.distance_m == d && s.runner.as_ref().unwrap().elapsed_s == e && stopped_at.is_none() {
                stopped_at = Some(t);
            }
            t += dt;
        }
        assert!(stopped_at.is_some_and(|t| t <= 4.0), "{stopped_at:?}");
        assert_eq!(race.world_speed_mps, 0.0);
        // Pedalling again resumes both.
        let (d, e) = (race.distance_m, s.runner.as_ref().unwrap().elapsed_s);
        frame(&mut s, &mut ride, &mut race, &profile, 100.0, 0.5);
        assert!(race.distance_m > d && s.runner.as_ref().unwrap().elapsed_s > e);
    }

    /// The samples' file stems.
    const SAMPLES: [&str; 5] = [
        "hidits-4x12",
        "hit-30-30",
        "lactate-elimination",
        "lit-1h",
        "mit-sweetspot-6x5",
    ];

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Rider {
        /// Always at the band centre.
        Exact,
        /// Moves linearly to each new band centre over 4 s, then holds it
        /// ± 3 %.
        Human,
        /// Stays at the previous target for 12 s after every step change.
        Late,
    }

    /// Rides a whole sample at `ftp_w` in 0.1 s steps; the finished ride.
    fn simulate(sample: &str, ftp_w: f32, rider: Rider) -> Ride {
        let free = Band::new(180.0, 220.0).unwrap();
        let (mut s, mut ride) = (WorkoutSession::new(ftp_w, free), Ride::new(free));
        let (mut profile, mut race) = road();
        s.load(
            load_path(&format!("samples/{sample}.json")).unwrap(),
            &mut ride,
            &mut profile,
            &mut race,
        );
        let total = f64::from(s.runner.as_ref().unwrap().total_s());
        let dt = 0.1_f32;
        let (mut t, mut since) = (0.0_f64, 0.0_f32);
        let first = ride.band.centre_w();
        let (mut from, mut previous) = (first, first);
        while !complete(&s) && t < total + 60.0 {
            let target = ride.band.centre_w();
            let power = match rider {
                Rider::Exact => target,
                Rider::Human if since < 4.0 => from + (target - from) * since / 4.0,
                Rider::Human => target * (1.0 + 0.03 * (std::f64::consts::TAU * t / 10.0).sin() as f32),
                Rider::Late if since < 12.0 => previous,
                Rider::Late => target,
            };
            crate::race::step(&mut ride, power, dt);
            let events = s.advance(&mut ride, dt);
            t += f64::from(dt);
            since += dt;
            if events.iter().any(|e| matches!(e, WorkoutEvent::StepStarted { .. })) {
                (since, from, previous) = (0.0, power, target);
            }
        }
        let r = s.runner.as_ref().unwrap();
        assert!(
            r.finished && (r.elapsed_s - total).abs() < 1e-3,
            "{sample}: {}",
            r.elapsed_s
        );
        ride
    }

    /// Part 0 of task 010: a rider who holds every target exactly never
    /// burns a match, and stays in band nearly all the time.
    #[test]
    fn exact_rider_burns_nothing_on_any_sample() {
        for sample in SAMPLES {
            for ftp in [200.0, 300.0] {
                let ride = simulate(sample, ftp, Rider::Exact);
                eprintln!(
                    "exact {sample} FTP {ftp}: burns {} in band {:.2} %",
                    ride.burns,
                    ride.in_band_pct()
                );
                assert_eq!(ride.burns, 0, "{sample} FTP {ftp}");
                assert!(ride.in_band_pct() >= 93.0, "{sample} FTP {ftp}: {}", ride.in_band_pct());
            }
        }
    }

    #[test]
    fn human_like_rider_burns_nothing_on_any_sample() {
        for sample in SAMPLES {
            for ftp in [200.0, 300.0] {
                let ride = simulate(sample, ftp, Rider::Human);
                eprintln!(
                    "human {sample} FTP {ftp}: burns {} in band {:.2} %",
                    ride.burns,
                    ride.in_band_pct()
                );
                assert_eq!(ride.burns, 0, "{sample} FTP {ftp}");
            }
        }
    }

    /// The grace is not a loophole: staying on the old target for 12 s
    /// after every change burns matches on the 30/30s.
    #[test]
    fn late_rider_burns_matches_on_the_30_30s() {
        for ftp in [200.0, 300.0] {
            let ride = simulate("hit-30-30", ftp, Rider::Late);
            eprintln!(
                "late hit-30-30 FTP {ftp}: burns {} in band {:.2} %",
                ride.burns,
                ride.in_band_pct()
            );
            assert!(ride.burns > 0, "FTP {ftp}");
        }
    }

    #[test]
    fn relative_paths_resolve_in_order() {
        let dir = std::env::temp_dir().join(format!("breakaway-workouts-{}", std::process::id()));
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("x.json"), "{}").unwrap();
        let dirs = [a.clone(), b.clone()];
        assert_eq!(resolve("x.json", &dirs).unwrap(), b.join("x.json"));
        std::fs::write(a.join("x.json"), "{}").unwrap();
        assert_eq!(resolve("x.json", &dirs).unwrap(), a.join("x.json"));
        assert!(resolve("missing.json", &dirs).unwrap_err().contains("not found"));
        assert!(resolve("", &dirs).is_err());
        assert_eq!(resolve("/abs/w.json", &dirs).unwrap(), PathBuf::from("/abs/w.json"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(load_path("samples/lit-1h.json").is_ok());
        assert!(load_path("samples/nope.json").is_err());
    }

    #[test]
    fn display_name_falls_back_to_the_class() {
        let mut s = Workout::parse(ONE).unwrap().steps.remove(0);
        assert_eq!(s.display_name(), "Warm up");
        s.name = String::new();
        s.class = StepClass::Active;
        assert_eq!(s.display_name(), "Active");
    }
}
