//! Scenario director: stages parts of a workout as a race situation. So far
//! one scenario: on/off interval sets become a two-rider breakaway (ON =
//! your pull on the front, OFF = drafting your partner); how well the rider
//! stays in band decides whether the break stays away from the bunch.
//! Every other step keeps the "hold the wheel" behaviour of `race.rs`. The
//! workout always runs to its end: being caught changes the story, never
//! stops the ride. Pure logic: `dt` comes in as a parameter, no rendering
//! or UI types.

use bevy::prelude::{Message, Resource};

use crate::race::{BandState, Ride};
use crate::workout::{BlockKind, Step, StepClass, Workout, WorkoutSession};

// ── Finding breakaway sets ────────────────────────────────────────────────────
/// Every step of a set lasts this long (s), both ends included.
pub const SET_STEP_MIN_S: f32 = 15.0;
pub const SET_STEP_MAX_S: f32 = 180.0;
/// A hard step's band centre is at least this times the following easy
/// step's.
pub const HARD_EASY_RATIO: f32 = 1.2;
/// A set has at least this many hard/easy pairs.
pub const SET_MIN_PAIRS: usize = 3;

// ── The break ─────────────────────────────────────────────────────────────────
/// Advantage (s) over the bunch at the start of each set.
pub const ADVANTAGE_START_S: f32 = 20.0;
/// While `In` band (pull or draft), after the transition grace.
pub const ADVANTAGE_GAIN_PER_S: f32 = 0.05;
/// While `Under`, after the transition grace.
pub const ADVANTAGE_LOSS_UNDER_PER_S: f32 = 0.25;
/// While `Idle` (grace or not: stopping is not a transition).
pub const ADVANTAGE_LOSS_IDLE_PER_S: f32 = 0.5;
pub const ADVANTAGE_MAX_S: f32 = 180.0;
/// Dropping below this fires `BunchClosing` (once per crossing).
pub const BUNCH_CLOSING_S: f32 = 10.0;

// ── Staging ───────────────────────────────────────────────────────────────────
/// On the wheel the partner rides this far ahead of the player, on the
/// centre line; the ride's gap adds to it.
pub const PARTNER_AHEAD_M: f32 = 2.0;
/// On a pull the partner sits behind and beside the player.
pub const PULL_PARTNER_AHEAD_M: f32 = -2.0;
pub const PULL_PARTNER_LATERAL_M: f32 = 1.2;
/// Under band on a pull: he moves up alongside, about to come round.
pub const PULL_UNDER_PARTNER_AHEAD_M: f32 = 0.0;
/// Over band on a pull: you ride him off your wheel.
pub const PULL_OVER_PARTNER_AHEAD_M: f32 = -6.0;
pub const CAMERA_BEHIND_M: f32 = 6.0;
/// On a pull the camera pulls back so both riders are in view.
pub const PULL_CAMERA_BEHIND_M: f32 = 9.0;

/// What a workout step is staged as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepRole {
    /// Hold the wheel (`race.rs` as it is).
    #[default]
    Default,
    /// A hard step of breakaway set `set`: your turn on the front.
    Pull { set: usize },
    /// An easy step of breakaway set `set`: sit on your partner's wheel.
    Draft { set: usize },
}

impl StepRole {
    /// The breakaway set, if the step is in one.
    pub fn set(self) -> Option<usize> {
        match self {
            StepRole::Default => None,
            StepRole::Pull { set } | StepRole::Draft { set } => Some(set),
        }
    }

    /// `default`, `pull` or `draft`.
    pub fn label(self) -> &'static str {
        match self {
            StepRole::Default => "default",
            StepRole::Pull { .. } => "pull",
            StepRole::Draft { .. } => "draft",
        }
    }

    fn phase(self) -> Option<Phase> {
        match self {
            StepRole::Default => None,
            StepRole::Pull { .. } => Some(Phase::Pull),
            StepRole::Draft { .. } => Some(Phase::Draft),
        }
    }
}

/// Band centre of a step, percent of FTP (the watts band's centre scales
/// with it; the 20 W minimum width widens symmetrically).
fn centre_pct(step: &Step) -> f32 {
    (step.low_pct + step.high_pct) / 2.0
}

/// A step that can be part of a set: 15–180 s long, and not a warm-up or
/// cool-down step (a run stops at those).
fn set_step(step: &Step) -> bool {
    let class_ok = !matches!(step.class, StepClass::WarmUp | StepClass::CoolDown);
    class_ok && (SET_STEP_MIN_S..=SET_STEP_MAX_S).contains(&step.duration_s)
}

/// Steps `i` (hard) and `i + 1` (easy) form a pair of a breakaway set.
fn is_pair(steps: &[Step], i: usize) -> bool {
    match (steps.get(i), steps.get(i + 1)) {
        (Some(hard), Some(easy)) => {
            set_step(hard) && set_step(easy) && centre_pct(hard) >= HARD_EASY_RATIO * centre_pct(easy)
        }
        _ => false,
    }
}

/// A run of `steps` may begin with a step of a `ramp` block only if the
/// whole run lies inside that one block (a pyramid qualifies, the end of a
/// warm-up ramp does not start a set).
fn ramp_start_ok(steps: &[Step]) -> bool {
    let first = &steps[0];
    first.block_kind != BlockKind::Ramp || steps.iter().all(|s| s.block == first.block)
}

/// The role of every step. A breakaway set is a maximal run of consecutive
/// hard/easy pairs, every step 15–180 s and neither a warm-up nor a
/// cool-down step, each hard step's band centre at least 1.2 × the following
/// easy step's, at least 3 pairs; a run that begins with a step of a `ramp`
/// block must lie inside that one block. Other classes and block kinds do
/// not matter. Sets are numbered from 0 in riding order; every other step
/// is `Default`.
pub fn plan(workout: &Workout) -> Vec<StepRole> {
    let steps = &workout.steps;
    let mut roles = vec![StepRole::Default; steps.len()];
    let mut set = 0;
    let mut i = 0;
    while i < steps.len() {
        let mut pairs = 0;
        while is_pair(steps, i + 2 * pairs) {
            pairs += 1;
        }
        if pairs >= SET_MIN_PAIRS && ramp_start_ok(&steps[i..i + 2 * pairs]) {
            for p in 0..pairs {
                roles[i + 2 * p] = StepRole::Pull { set };
                roles[i + 2 * p + 1] = StepRole::Draft { set };
            }
            set += 1;
            i += 2 * pairs;
        } else {
            // A shorter run may still start one step later (other phase).
            i += 1;
        }
    }
    roles
}

/// `(first, last)` step index of every set, in order.
pub fn set_ranges(plan: &[StepRole]) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (i, role) in plan.iter().enumerate() {
        let Some(set) = role.set() else { continue };
        if set == ranges.len() {
            ranges.push((i, i));
        } else {
            ranges[set].1 = i;
        }
    }
    ranges
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Pull,
    Draft,
}

/// The running breakaway.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Break {
    pub set: usize,
    /// Time gap to the bunch (s).
    pub advantage_s: f32,
    /// Reached 0: caught for the rest of the set, the advantage stays 0.
    pub caught: bool,
    pub phase: Phase,
}

impl Break {
    pub fn new(set: usize, phase: Phase) -> Break {
        Break {
            set,
            advantage_s: ADVANTAGE_START_S,
            caught: false,
            phase,
        }
    }
}

#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum ScenarioEvent {
    BreakStarted {
        set: usize,
    },
    PullStarted,
    DraftStarted,
    /// The advantage dropped below 10 s.
    BunchClosing,
    Caught,
    /// The set's last step ended and the break was not caught.
    BreakSurvived {
        advantage_s: f32,
    },
}

/// `+0:34`: whole seconds rounded up, so a break that is not caught never
/// shows `+0:00`.
pub fn advantage_text(advantage_s: f32) -> String {
    let s = advantage_s.max(0.0).ceil() as u32;
    format!("+{}:{:02}", s / 60, s % 60)
}

/// Advances a break by `dt` with the ride's state after its own step.
/// `In` gains, `Under` and `Idle` lose, `Over` changes nothing (matches
/// handle it); during the transition grace only `Idle` changes it. At 0 the
/// break is caught and stays caught.
pub fn advance(b: &mut Break, ride: &Ride, dt: f32) -> Vec<ScenarioEvent> {
    let mut events = Vec::new();
    if b.caught || ride.finished {
        return events;
    }
    // Already at 0 (set from outside): caught before anything can gain.
    if b.advantage_s <= 0.0 {
        b.advantage_s = 0.0;
        b.caught = true;
        events.push(ScenarioEvent::Caught);
        return events;
    }
    let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let rate = match ride.state {
        BandState::Idle => -ADVANTAGE_LOSS_IDLE_PER_S,
        _ if ride.grace.is_some() => 0.0,
        BandState::In => ADVANTAGE_GAIN_PER_S,
        BandState::Under => -ADVANTAGE_LOSS_UNDER_PER_S,
        BandState::Over | BandState::Finished => 0.0,
    };
    let before = b.advantage_s;
    b.advantage_s = (before + rate * dt).clamp(0.0, ADVANTAGE_MAX_S);
    if before >= BUNCH_CLOSING_S && b.advantage_s < BUNCH_CLOSING_S {
        events.push(ScenarioEvent::BunchClosing);
    }
    if b.advantage_s <= 0.0 {
        b.advantage_s = 0.0;
        b.caught = true;
        events.push(ScenarioEvent::Caught);
    }
    events
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BunchView {
    Hidden,
    /// Caught: the bunch rides around the player until the set ends.
    Around,
}

/// Where the partner and the camera should be (render eases toward it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Staging {
    /// Partner's distance ahead of the player (m, negative = behind).
    pub partner_ahead_m: f32,
    /// Partner's lateral offset from the player's line (m, + = right).
    pub partner_lateral_m: f32,
    pub camera_behind_m: f32,
    pub bunch: BunchView,
}

impl Default for Staging {
    fn default() -> Staging {
        Staging {
            partner_ahead_m: PARTNER_AHEAD_M,
            partner_lateral_m: 0.0,
            camera_behind_m: CAMERA_BEHIND_M,
            bunch: BunchView::Hidden,
        }
    }
}

/// `Default` and `Draft`: the partner on the centre line `2 m + gap` ahead,
/// camera 6 m behind. `Pull`: the partner 2 m behind and 1.2 m beside the
/// player, camera 9 m behind; after the transition grace he moves up
/// alongside while the rider is under the band and drops back to 6 m while
/// over it. The bunch rides around once the break is caught.
pub fn staging(role: StepRole, brk: Option<&Break>, ride: &Ride) -> Staging {
    let caught = role.set().is_some() && brk.is_some_and(|b| b.caught && Some(b.set) == role.set());
    let bunch = if caught { BunchView::Around } else { BunchView::Hidden };
    match role {
        StepRole::Pull { .. } => {
            let ahead = match ride.state {
                _ if ride.grace.is_some() => PULL_PARTNER_AHEAD_M,
                BandState::Under => PULL_UNDER_PARTNER_AHEAD_M,
                BandState::Over => PULL_OVER_PARTNER_AHEAD_M,
                _ => PULL_PARTNER_AHEAD_M,
            };
            Staging {
                partner_ahead_m: ahead,
                partner_lateral_m: PULL_PARTNER_LATERAL_M,
                camera_behind_m: PULL_CAMERA_BEHIND_M,
                bunch,
            }
        }
        StepRole::Default | StepRole::Draft { .. } => Staging {
            partner_ahead_m: PARTNER_AHEAD_M + ride.gap_m,
            partner_lateral_m: 0.0,
            camera_behind_m: CAMERA_BEHIND_M,
            bunch,
        },
    }
}

/// The scenario layer: the plan of the loaded workout, the current step's
/// role, the break (during a set) and the staging. Stepped after the ride
/// and the workout each frame (`step`); no workout → everything `Default`.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct Scenario {
    pub plan: Vec<StepRole>,
    pub role: StepRole,
    pub brk: Option<Break>,
    pub staging: Staging,
    /// `WorkoutSession::loads` the plan was made for.
    loads_seen: Option<u64>,
    /// Step index last seen while the workout ran (`None`: none, or
    /// finished).
    position: Option<usize>,
    /// Events since the last `take_events`.
    events: Vec<ScenarioEvent>,
}

impl Scenario {
    /// Follows the session: replans after a load or unload, handles step
    /// changes (a set starts, the phase swaps, a set ends) and holds the
    /// gap at 0 during a pull. Called every frame by `step`, and before a
    /// command that needs the current set.
    pub fn sync(&mut self, session: &WorkoutSession, ride: &mut Ride) {
        if self.loads_seen != Some(session.loads) {
            self.loads_seen = Some(session.loads);
            self.plan = session.runner.as_ref().map_or_else(Vec::new, |r| plan(&r.workout));
            self.role = StepRole::Default;
            self.brk = None;
            self.position = None;
            if session.runner.is_some() {
                ride.sets_total = set_ranges(&self.plan).len() as u32;
            }
        }
        let position = session.runner.as_ref().filter(|r| !r.finished).map(|r| r.index);
        if position != self.position {
            let finished = session.runner.as_ref().is_some_and(|r| r.finished);
            self.change_step(position, finished, ride);
            self.position = position;
        }
        ride.hold_gap = matches!(self.role, StepRole::Pull { .. });
        if ride.hold_gap {
            ride.gap_m = 0.0;
        }
    }

    /// One frame, after the ride and the workout have been stepped by `dt`.
    pub fn step(&mut self, session: &WorkoutSession, ride: &mut Ride, dt: f32) {
        self.sync(session, ride);
        if let Some(b) = self.brk.as_mut() {
            self.events.extend(advance(b, ride, dt));
        }
        self.staging = staging(self.role, self.brk.as_ref(), ride);
    }

    /// The events since the last call, in order.
    pub fn take_events(&mut self) -> Vec<ScenarioEvent> {
        std::mem::take(&mut self.events)
    }

    /// Sets the running break's advantage (0..=180 s; 0 is caught on the
    /// next step, more un-catches it). Refused outside a set.
    pub fn set_advantage(&mut self, seconds: f32, session: &WorkoutSession, ride: &mut Ride) -> Result<(), String> {
        if !(seconds.is_finite() && (0.0..=ADVANTAGE_MAX_S).contains(&seconds)) {
            return Err(format!("seconds must be 0..={ADVANTAGE_MAX_S}"));
        }
        self.sync(session, ride);
        let Some(b) = self.brk.as_mut() else {
            return Err("no breakaway set is running".into());
        };
        b.advantage_s = seconds;
        b.caught = false;
        Ok(())
    }

    /// The step changed from `self.position` to `position` (`None`: no
    /// workout or it finished).
    fn change_step(&mut self, position: Option<usize>, finished: bool, ride: &mut Ride) {
        let old = self.role;
        let new = position.and_then(|i| self.plan.get(i).copied()).unwrap_or_default();
        if let Some(set) = old.set().filter(|s| new.set() != Some(*s)) {
            // Left the set: it ended if its last step did; a jump out of
            // the middle (seek) drops the break without a result.
            let last = set_ranges(&self.plan).get(set).map(|r| r.1);
            let ended = self.position.is_some()
                && self.position == last
                && match position {
                    Some(i) => Some(i) > last,
                    None => finished,
                };
            if let Some(b) = self.brk.take().filter(|_| ended) {
                ride.set_advantages_s.push(b.advantage_s);
                if !b.caught {
                    ride.sets_survived += 1;
                    self.events.push(ScenarioEvent::BreakSurvived {
                        advantage_s: b.advantage_s,
                    });
                }
            }
        }
        self.role = new;
        let (Some(set), Some(phase)) = (new.set(), new.phase()) else {
            return;
        };
        let started = match self.brk.as_mut() {
            Some(b) if b.set == set => {
                let swapped = b.phase != phase;
                b.phase = phase;
                swapped
            }
            _ => {
                self.brk = Some(Break::new(set, phase));
                self.events.push(ScenarioEvent::BreakStarted { set });
                true
            }
        };
        if started {
            self.events.push(match phase {
                Phase::Pull => ScenarioEvent::PullStarted,
                Phase::Draft => ScenarioEvent::DraftStarted,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::{Band, Grace, START_MATCHES};
    use crate::sim::RaceState;
    use crate::terrain::Profile;
    use crate::workout::load_path;

    fn sample(name: &str) -> Workout {
        load_path(&format!("samples/{name}.json")).unwrap()
    }

    fn step(duration_s: f32, low_pct: f32, high_pct: f32) -> Step {
        Step {
            name: String::new(),
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

    fn workout(steps: Vec<Step>) -> Workout {
        Workout {
            name: "w".into(),
            date: None,
            source: None,
            steps,
        }
    }

    /// `pairs` × (hard 30 s at 130 %, easy 30 s at 50 %) between 300 s
    /// steps.
    fn on_off(pairs: usize) -> Workout {
        let mut steps = vec![step(300.0, 50.0, 60.0)];
        for _ in 0..pairs {
            steps.push(step(30.0, 125.0, 135.0));
            steps.push(step(30.0, 45.0, 55.0));
        }
        steps.push(step(300.0, 50.0, 60.0));
        workout(steps)
    }

    #[test]
    fn breakaway_sets_in_the_samples() {
        let found = |name: &str| set_ranges(&plan(&sample(name)));
        // The 3 × 30/30 opener and the two 20 × 30/30 blocks. The opener's
        // set starts at its first 30 s hard step: the warm-up ramp's last
        // step (45 s at 92-100 %) and the 90 s recovery would pair up by
        // the ratio, but a run may not begin in a ramp block it leaves.
        assert_eq!(found("hit-30-30"), [(6, 11), (13, 52), (54, 93)]);
        // The same kind of opener (its own warm-up, same step indices), then
        // each of the four 12-minute pyramids from its first hard step
        // (180 s is inside the limit): 180/60/150/40/90/20/60/15/45/15/30/15 s
        // (720 s).
        assert_eq!(found("hidits-4x12"), [(6, 11), (13, 24), (26, 37), (39, 50), (52, 63)]);
        for name in ["lit-1h", "mit-sweetspot-6x5", "lactate-elimination"] {
            assert!(found(name).is_empty(), "{name}");
        }
        // Pulls on the hard steps, drafts on the easy ones.
        let p = plan(&sample("hit-30-30"));
        assert_eq!(p[..14].iter().filter(|r| r.set().is_some()).count(), 7);
        assert_eq!(
            (p[4], p[5], p[6], p[7], p[12], p[13], p[52], p[53]),
            (
                StepRole::Default,
                StepRole::Default,
                StepRole::Pull { set: 0 },
                StepRole::Draft { set: 0 },
                StepRole::Default,
                StepRole::Pull { set: 1 },
                StepRole::Draft { set: 1 },
                StepRole::Default,
            )
        );
        let w = sample("hidits-4x12");
        let total: f32 = w.steps[13..=24].iter().map(|s| s.duration_s).sum();
        assert_eq!(total, 720.0);
        let p = plan(&w);
        for first in [13, 26, 39, 52] {
            assert_eq!(
                (w.steps[first].name.as_str(), w.steps[first].duration_s),
                ("Hard", 180.0)
            );
            assert!(
                matches!(p[first], StepRole::Pull { .. }) && p[first - 1] == StepRole::Default,
                "{first}"
            );
        }
    }

    #[test]
    fn set_detection_edges() {
        let sets = |w: &Workout| set_ranges(&plan(w));
        assert!(sets(&on_off(2)).is_empty());
        assert_eq!(sets(&on_off(3)), [(1, 6)]);
        // A 190 s step ends a run: 1 pair before it, 3 after.
        let mut w = on_off(5);
        w.steps[4].duration_s = 190.0;
        assert_eq!(sets(&w), [(5, 10)]);
        // 15 s and 180 s are inside, 14 s and 181 s are not.
        let mut w = on_off(3);
        w.steps[1].duration_s = 181.0;
        assert!(sets(&w).is_empty());
        w.steps[1].duration_s = 180.0;
        w.steps[2].duration_s = 15.0;
        assert_eq!(sets(&w), [(1, 6)]);
        w.steps[2].duration_s = 14.0;
        assert!(sets(&w).is_empty());
        // Ratio 1.19 → none; exactly 1.2 → a set.
        let ratio = |r: f32| {
            let mut w = on_off(3);
            for i in (1..=5).step_by(2) {
                w.steps[i] = step(30.0, 50.0 * r, 50.0 * r);
                w.steps[i + 1] = step(30.0, 50.0, 50.0);
            }
            sets(&w)
        };
        assert!(ratio(1.19).is_empty());
        assert_eq!(ratio(1.2), [(1, 6)]);
        // A warm-up or cool-down step ends a run, whatever its length.
        for class in [StepClass::WarmUp, StepClass::CoolDown] {
            let mut w = on_off(5);
            w.steps[4].class = class;
            assert_eq!(sets(&w), [(5, 10)], "{class:?}");
            let mut w = on_off(3);
            w.steps[1].class = class;
            assert!(sets(&w).is_empty(), "{class:?}");
        }
        // Other classes and block kinds do not matter; a run starts on the
        // phase that works (easy first here).
        let mut w = on_off(4);
        w.steps.insert(1, step(30.0, 45.0, 55.0));
        for s in &mut w.steps {
            s.class = StepClass::Rest;
            s.block_kind = BlockKind::Ramp;
        }
        assert_eq!(sets(&w), [(2, 9)]);
        // A run may begin in a ramp block only if it stays inside it.
        let mut w = on_off(4);
        for (i, s) in w.steps.iter_mut().enumerate() {
            s.block = if (1..=2).contains(&i) { 1 } else { 2 };
            s.block_kind = if (1..=2).contains(&i) {
                BlockKind::Ramp
            } else {
                BlockKind::Repetition
            };
        }
        // Steps 1–2 are a ramp block the run would leave: it starts at 3.
        assert_eq!(sets(&w), [(3, 8)]);
        for s in &mut w.steps[1..=8] {
            s.block = 1;
            s.block_kind = BlockKind::Ramp;
        }
        assert_eq!(sets(&w), [(1, 8)]);
        // A ramp block in the middle of a run does not matter.
        let mut w = on_off(4);
        w.steps[3].block = 7;
        w.steps[3].block_kind = BlockKind::Ramp;
        assert_eq!(sets(&w), [(1, 8)]);
        // Two runs, two sets numbered in order.
        let mut steps = on_off(3).steps;
        steps.extend(on_off(4).steps);
        let p = plan(&workout(steps));
        assert_eq!(set_ranges(&p), [(1, 6), (9, 16)]);
        assert_eq!(p[9], StepRole::Pull { set: 1 });
    }

    fn ride_at(state: BandState) -> Ride {
        Ride {
            state,
            ..Ride::new(Band::new(100.0, 120.0).unwrap())
        }
    }

    #[test]
    fn advantage_rates_and_grace() {
        let after = |state: BandState, grace: bool, secs: f32| {
            let mut ride = ride_at(state);
            if grace {
                ride.grace = Some(Grace {
                    elapsed_s: 0.0,
                    entered_at_s: None,
                });
            }
            let mut b = Break::new(0, Phase::Pull);
            advance(&mut b, &ride, secs);
            b.advantage_s
        };
        assert!((after(BandState::In, false, 10.0) - 20.5).abs() < 1e-4);
        assert!((after(BandState::Under, false, 10.0) - 17.5).abs() < 1e-4);
        assert!((after(BandState::Idle, false, 10.0) - 15.0).abs() < 1e-4);
        assert_eq!(after(BandState::Over, false, 10.0), 20.0);
        // Grace: no change, except that stopping still costs.
        for state in [BandState::In, BandState::Under, BandState::Over] {
            assert_eq!(after(state, true, 10.0), 20.0, "{state:?}");
        }
        assert!((after(BandState::Idle, true, 10.0) - 15.0).abs() < 1e-4);
        // Capped at 180 s.
        let mut b = Break {
            advantage_s: 179.9,
            ..Break::new(0, Phase::Draft)
        };
        advance(&mut b, &ride_at(BandState::In), 100.0);
        assert_eq!(b.advantage_s, ADVANTAGE_MAX_S);
        // A finished ride changes nothing.
        let mut ride = ride_at(BandState::Idle);
        ride.finish();
        let mut b = Break::new(0, Phase::Draft);
        advance(&mut b, &ride, 10.0);
        assert_eq!(b.advantage_s, 20.0);
    }

    /// The one rule: per second, over the band never gains more than in it.
    #[test]
    fn over_the_band_never_gains_faster_than_in_it() {
        for grace in [false, true] {
            let rate = |state| {
                let mut ride = ride_at(state);
                if grace {
                    ride.grace = Some(Grace {
                        elapsed_s: 0.0,
                        entered_at_s: None,
                    });
                }
                let mut b = Break {
                    advantage_s: 50.0,
                    ..Break::new(0, Phase::Pull)
                };
                advance(&mut b, &ride, 1.0);
                b.advantage_s - 50.0
            };
            assert!(rate(BandState::Over) <= rate(BandState::In), "grace {grace}");
            assert!(rate(BandState::Over) <= 0.0, "grace {grace}");
        }
    }

    #[test]
    fn caught_at_zero_once_and_closing_once_per_crossing() {
        let mut b = Break {
            advantage_s: 10.5,
            ..Break::new(0, Phase::Pull)
        };
        let mut events = Vec::new();
        for (state, secs) in [
            (BandState::Under, 4.0),
            (BandState::In, 60.0),
            (BandState::Under, 30.0),
            (BandState::Under, 30.0),
        ] {
            for _ in 0..(secs * 10.0) as usize {
                events.extend(advance(&mut b, &ride_at(state), 0.1));
            }
        }
        // 10.5 → 9.5 (crossing) → (in band) 12.5 → 5 (second crossing) → 0.
        assert_eq!(
            events,
            [
                ScenarioEvent::BunchClosing,
                ScenarioEvent::BunchClosing,
                ScenarioEvent::Caught
            ]
        );
        assert!(b.caught && b.advantage_s == 0.0);
        // Caught stays caught, whatever the rider does.
        for state in [BandState::In, BandState::Over, BandState::Idle] {
            assert!(advance(&mut b, &ride_at(state), 100.0).is_empty());
        }
        assert_eq!((b.caught, b.advantage_s), (true, 0.0));
        // Set to 0 from outside: caught on the next step, even in band.
        let mut b = Break {
            advantage_s: 0.0,
            ..Break::new(0, Phase::Pull)
        };
        assert_eq!(advance(&mut b, &ride_at(BandState::In), 0.1), [ScenarioEvent::Caught]);
        assert_eq!((b.caught, b.advantage_s), (true, 0.0));
        // Sitting below 10 s fires nothing more; a big drop fires both.
        let mut b = Break {
            advantage_s: 10.5,
            ..Break::new(0, Phase::Draft)
        };
        assert_eq!(
            advance(&mut b, &ride_at(BandState::Idle), 100.0),
            [ScenarioEvent::BunchClosing, ScenarioEvent::Caught]
        );
    }

    #[test]
    fn staging_per_role_and_band_state() {
        let mut ride = Ride {
            gap_m: 3.5,
            ..ride_at(BandState::In)
        };
        let draft = staging(StepRole::Draft { set: 0 }, None, &ride);
        assert_eq!(
            draft,
            Staging {
                partner_ahead_m: 5.5,
                partner_lateral_m: 0.0,
                camera_behind_m: 6.0,
                bunch: BunchView::Hidden
            }
        );
        assert_eq!(staging(StepRole::Default, None, &ride), draft);
        ride.gap_m = 0.0;
        let pull = |state: BandState, grace: bool| {
            let mut ride = ride_at(state);
            if grace {
                ride.begin_grace();
            }
            staging(StepRole::Pull { set: 0 }, None, &ride)
        };
        assert_eq!(
            pull(BandState::In, false),
            Staging {
                partner_ahead_m: -2.0,
                partner_lateral_m: 1.2,
                camera_behind_m: 9.0,
                bunch: BunchView::Hidden,
            }
        );
        assert_eq!(pull(BandState::Under, false).partner_ahead_m, 0.0);
        assert_eq!(pull(BandState::Over, false).partner_ahead_m, -6.0);
        assert_eq!(pull(BandState::Idle, false).partner_ahead_m, -2.0);
        // During the transition grace he stays where he is.
        assert_eq!(pull(BandState::Under, true).partner_ahead_m, -2.0);
        assert_eq!(pull(BandState::Over, true).partner_ahead_m, -2.0);
        // The bunch is around once caught, in that set only.
        let caught = Break {
            caught: true,
            advantage_s: 0.0,
            ..Break::new(1, Phase::Pull)
        };
        assert_eq!(
            staging(StepRole::Pull { set: 1 }, Some(&caught), &ride).bunch,
            BunchView::Around
        );
        assert_eq!(
            staging(StepRole::Draft { set: 1 }, Some(&caught), &ride).bunch,
            BunchView::Around
        );
        assert_eq!(
            staging(StepRole::Default, Some(&caught), &ride).bunch,
            BunchView::Hidden
        );
        assert_eq!(
            staging(StepRole::Pull { set: 1 }, Some(&Break::new(1, Phase::Pull)), &ride).bunch,
            BunchView::Hidden
        );
    }

    #[test]
    fn texts() {
        assert_eq!(advantage_text(34.0), "+0:34");
        assert_eq!(advantage_text(41.2), "+0:42");
        assert_eq!(advantage_text(0.3), "+0:01");
        assert_eq!(advantage_text(125.0), "+2:05");
    }

    /// One simulated ride through a workout.
    struct Run {
        ride: Ride,
        /// `(workout time, event)`.
        events: Vec<(f64, ScenarioEvent)>,
        /// Every frame inside a set: `(set, workout time, advantage)`.
        trace: Vec<(usize, f64, f32)>,
        /// Roles seen, one per frame.
        roles: Vec<StepRole>,
        /// Gap during pulls, largest seen.
        pull_gap_max: f32,
    }

    impl Run {
        fn count(&self, e: ScenarioEvent) -> usize {
            self.events.iter().filter(|(_, x)| *x == e).count()
        }

        /// Time from each set's start to its capture.
        fn capture_times(&self) -> Vec<f64> {
            let mut started = 0.0;
            let mut out = Vec::new();
            for (t, e) in &self.events {
                match e {
                    ScenarioEvent::BreakStarted { .. } => started = *t,
                    ScenarioEvent::Caught => out.push(t - started),
                    _ => {}
                }
            }
            out
        }
    }

    /// Rides `w` at FTP 200 in `dt` steps; `power(time, band centre, high
    /// edge, role, seconds into the step)`.
    fn ride_workout(w: Workout, dt: f32, mut power: impl FnMut(f64, f32, f32, StepRole, f32) -> f32) -> Run {
        let free = Band::new(180.0, 220.0).unwrap();
        let (mut session, mut ride) = (WorkoutSession::new(200.0, free), Ride::new(free));
        let (mut profile, mut race) = (Profile::demo(), RaceState::default());
        session.load(w, &mut ride, &mut profile, &mut race);
        let mut sc = Scenario::default();
        sc.step(&session, &mut ride, 0.0);
        let total = f64::from(session.runner.as_ref().unwrap().total_s());
        let mut run = Run {
            ride: Ride::new(free),
            events: Vec::new(),
            trace: Vec::new(),
            roles: Vec::new(),
            pull_gap_max: 0.0,
        };
        let mut t = 0.0_f64;
        while !session.runner.as_ref().unwrap().finished && t < total + 60.0 {
            let since = session.runner.as_ref().unwrap().step_elapsed_s as f32;
            let p = power(t, ride.band.centre_w(), ride.band.high_w, sc.role, since);
            crate::race::step(&mut ride, p, dt);
            session.advance(&mut ride, dt);
            sc.step(&session, &mut ride, dt);
            t += f64::from(dt);
            run.events.extend(sc.take_events().into_iter().map(|e| (t, e)));
            if let Some(b) = &sc.brk {
                run.trace.push((b.set, t, b.advantage_s));
            }
            if matches!(sc.role, StepRole::Pull { .. }) {
                run.pull_gap_max = run.pull_gap_max.max(ride.gap_m);
            }
            run.roles.push(sc.role);
        }
        assert!(session.runner.as_ref().unwrap().finished);
        run.ride = ride;
        run
    }

    fn exact(_: f64, centre: f32, _: f32, _: StepRole, _: f32) -> f32 {
        centre
    }

    #[test]
    fn exact_rider_survives_every_set_with_the_advantage_rising() {
        let run = ride_workout(sample("hit-30-30"), 0.1, exact);
        let r = &run.ride;
        eprintln!("exact hit-30-30: final advantages {:?}", r.set_advantages_s);
        assert_eq!((r.sets_total, r.sets_survived, r.set_advantages_s.len()), (3, 3, 3));
        assert_eq!(
            (run.count(ScenarioEvent::Caught), run.count(ScenarioEvent::BunchClosing)),
            (0, 0)
        );
        assert!(
            r.set_advantages_s[0] > ADVANTAGE_START_S + 5.0,
            "{:?}",
            r.set_advantages_s
        );
        assert!(
            r.set_advantages_s[1] > 60.0 && r.set_advantages_s[2] > 60.0,
            "{:?}",
            r.set_advantages_s
        );
        // Never falls inside a set.
        for w in run.trace.windows(2) {
            if w[0].0 == w[1].0 {
                assert!(w[1].2 >= w[0].2, "{w:?}");
            }
        }
        let survived: Vec<f32> = run
            .events
            .iter()
            .filter_map(|(_, e)| {
                if let ScenarioEvent::BreakSurvived { advantage_s } = e {
                    Some(*advantage_s)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(survived, r.set_advantages_s);
        assert_eq!(run.count(ScenarioEvent::BreakStarted { set: 1 }), 1);
        // One pull per hard step, one draft per easy step.
        assert_eq!(
            (
                run.count(ScenarioEvent::PullStarted),
                run.count(ScenarioEvent::DraftStarted)
            ),
            (43, 43)
        );
        assert_eq!((r.matches, run.pull_gap_max), (START_MATCHES, 0.0));
    }

    #[test]
    fn rider_a_quarter_under_is_caught_in_every_set() {
        let run = ride_workout(sample("hit-30-30"), 0.1, |_, c, _, _, _| 0.75 * c);
        let r = &run.ride;
        let times = run.capture_times();
        eprintln!(
            "25 % under hit-30-30: caught after {times:?} s; final {:?}",
            r.set_advantages_s
        );
        assert_eq!((r.sets_survived, r.set_advantages_s.clone()), (0, vec![0.0; 3]));
        assert_eq!(run.count(ScenarioEvent::Caught), 3);
        assert_eq!(run.count(ScenarioEvent::BunchClosing), 3);
        // Never in band on a pull: the 10 s grace runs out, then 0.25 s/s.
        // On a draft the falling power crosses the easy band on its way
        // down, which ends the grace after 3 s. Every set starts with 30/30
        // pairs: 5 + 6.75 s per pair, caught 16 s into the second draft =
        // 106 s.
        let want = [106.0, 106.0, 106.0];
        assert_eq!(times.len(), 3);
        for (t, w) in times.iter().zip(want) {
            assert!((t - w).abs() < 0.5, "{times:?}");
        }
        // Under on a pull costs advantage, not the wheel.
        assert_eq!(run.pull_gap_max, 0.0);
    }

    #[test]
    fn rider_a_quarter_over_is_never_caught_but_gains_nothing() {
        let run = ride_workout(sample("hit-30-30"), 0.1, |_, c, _, _, _| 1.25 * c);
        let r = &run.ride;
        eprintln!("25 % over hit-30-30: final {:?}, burns {}", r.set_advantages_s, r.burns);
        assert_eq!((r.sets_survived, run.count(ScenarioEvent::Caught)), (3, 0));
        for a in &r.set_advantages_s {
            assert!(*a <= ADVANTAGE_START_S + 1e-4, "{a}");
        }
        assert!(run.trace.iter().all(|x| x.2 <= ADVANTAGE_START_S + 1e-4));
        assert!(r.burns > 0 && r.matches == 0, "{}", r.burns);
    }

    /// The one rule as a test: no trajectory with power above the band ends
    /// a set with a larger advantage than the exact rider's.
    #[test]
    fn power_above_the_band_never_beats_the_exact_rider() {
        for name in ["hit-30-30", "hidits-4x12"] {
            let reference = ride_workout(sample(name), 0.1, exact).ride.set_advantages_s;
            let mut tried = 0;
            let mut check = |label: &str, run: Run| {
                let got = &run.ride.set_advantages_s;
                assert_eq!(got.len(), reference.len(), "{name} {label}");
                for (g, e) in got.iter().zip(&reference) {
                    assert!(g <= e, "{name} {label}: {got:?} vs exact {reference:?}");
                }
                tried += 1;
            };
            for over_w in [1.0, 10.0, 40.0, 150.0] {
                check(
                    &format!("{over_w} W over"),
                    ride_workout(sample(name), 0.1, |_, _, h, _, _| h + over_w),
                );
            }
            check(
                "over on pulls",
                ride_workout(sample(name), 0.1, |_, c, h, r, _| {
                    if matches!(r, StepRole::Pull { .. }) {
                        h + 30.0
                    } else {
                        c
                    }
                }),
            );
            check(
                "over on drafts",
                ride_workout(sample(name), 0.1, |_, c, h, r, _| {
                    if matches!(r, StepRole::Draft { .. }) {
                        h + 30.0
                    } else {
                        c
                    }
                }),
            );
            // A surge into every step, then the centre.
            check(
                "surge at each step start",
                ride_workout(sample(name), 0.1, |_, c, h, _, s| if s < 2.0 { 2.0 * h } else { c }),
            );
            // Over-band spells mixed into a centred ride.
            for (every, spell) in [(5.0, 1.0), (10.0, 3.0), (7.0, 5.0)] {
                check(
                    &format!("{spell} s spells every {every} s"),
                    ride_workout(
                        sample(name),
                        0.1,
                        move |t, c, h, _, _| {
                            if (t % every) < spell { h + 25.0 } else { c }
                        },
                    ),
                );
            }
            // Deterministic pseudo-random wandering above the centre.
            let mut seed = 12345_u32;
            check(
                "random over",
                ride_workout(sample(name), 0.1, move |_, c, h, _, _| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    let u = (seed >> 16) as f32 / 65536.0;
                    c + u * (h + 60.0 - c)
                }),
            );
            assert_eq!(tried, 11);
        }
    }

    /// Sets the step to `index` (skips) on a fresh session; `(session, ride,
    /// scenario)` after one scenario step.
    fn at_step(w: Workout, index: usize) -> (WorkoutSession, Ride, RaceState, Scenario) {
        let free = Band::new(180.0, 220.0).unwrap();
        let (mut session, mut ride) = (WorkoutSession::new(200.0, free), Ride::new(free));
        let (mut profile, mut race) = (Profile::demo(), RaceState::default());
        session.load(w, &mut ride, &mut profile, &mut race);
        for _ in 0..index {
            session.skip(&mut ride, &mut race).unwrap();
        }
        let mut sc = Scenario::default();
        sc.step(&session, &mut ride, 0.0);
        (session, ride, race, sc)
    }

    #[test]
    fn set_advantage_only_during_a_set() {
        let (session, mut ride, _, mut sc) = at_step(sample("hit-30-30"), 12);
        assert!(sc.set_advantage(30.0, &session, &mut ride).is_err());
        let (session, mut ride, _, mut sc) = at_step(sample("hit-30-30"), 14);
        assert_eq!(sc.role, StepRole::Draft { set: 1 });
        sc.set_advantage(4.0, &session, &mut ride).unwrap();
        assert_eq!(sc.brk.unwrap().advantage_s, 4.0);
        for bad in [-0.1, 180.1, f32::NAN] {
            assert!(sc.set_advantage(bad, &session, &mut ride).is_err(), "{bad}");
        }
        // 0: caught on the next step; more: no longer caught.
        sc.set_advantage(0.0, &session, &mut ride).unwrap();
        sc.step(&session, &mut ride, 0.1);
        assert!(sc.brk.unwrap().caught);
        assert_eq!(sc.staging.bunch, BunchView::Around);
        sc.set_advantage(45.0, &session, &mut ride).unwrap();
        assert!(!sc.brk.unwrap().caught);
        // No workout: refused.
        let mut sc = Scenario::default();
        let free = WorkoutSession::new(200.0, Band::new(180.0, 220.0).unwrap());
        assert!(sc.set_advantage(10.0, &free, &mut ride).is_err());
        sc.step(&free, &mut ride, 0.1);
        assert_eq!((sc.role, sc.plan.len()), (StepRole::Default, 0));
    }

    #[test]
    fn seek_reload_and_finish() {
        let (mut session, mut ride, mut race, mut sc) = at_step(sample("hit-30-30"), 13);
        assert_eq!(
            (sc.role, ride.sets_total, ride.hold_gap),
            (StepRole::Pull { set: 1 }, 3, true)
        );
        assert_eq!(
            sc.take_events(),
            [ScenarioEvent::BreakStarted { set: 1 }, ScenarioEvent::PullStarted]
        );
        // Seeking out of the middle of a set drops the break, no result.
        session.seek(3000.0, &mut ride, &mut race).unwrap();
        sc.step(&session, &mut ride, 0.1);
        assert_eq!(
            (sc.role, sc.brk, ride.set_advantages_s.len(), ride.hold_gap),
            (StepRole::Default, None, 0, false)
        );
        assert!(sc.take_events().is_empty());
        // Seeking into a set mid-way starts a fresh break there.
        session.seek(1830.0, &mut ride, &mut race).unwrap();
        sc.step(&session, &mut ride, 0.0);
        assert_eq!(
            (sc.role, sc.brk.map(|b| b.advantage_s)),
            (StepRole::Draft { set: 1 }, Some(ADVANTAGE_START_S))
        );
        sc.set_advantage(50.0, &session, &mut ride).unwrap();
        // A reload (same workout, same step) is a fresh break.
        let w = session.runner.as_ref().unwrap().workout.clone();
        session.load(w, &mut ride, &mut Profile::demo(), &mut race);
        session.seek(1830.0, &mut ride, &mut race).unwrap();
        sc.set_advantage(5.0, &session, &mut ride).unwrap();
        assert_eq!(sc.brk.map(|b| (b.set, b.advantage_s)), Some((1, 5.0)));
        sc.take_events();
        // A set that ends the workout survives on the finish.
        let mut w = on_off(3);
        w.steps.pop();
        let (mut session, mut ride, mut race, mut sc) = at_step(w, 6);
        assert_eq!(sc.role, StepRole::Draft { set: 0 });
        session.skip(&mut ride, &mut race).unwrap();
        sc.step(&session, &mut ride, 0.1);
        assert_eq!(
            (ride.sets_survived, ride.set_advantages_s.clone(), sc.role),
            (1, vec![ADVANTAGE_START_S], StepRole::Default)
        );
        assert!(sc.take_events().contains(&ScenarioEvent::BreakSurvived {
            advantage_s: ADVANTAGE_START_S
        }));
        // Unloading keeps the totals.
        session.unload(&mut ride, &mut Profile::demo(), &mut race);
        sc.step(&session, &mut ride, 0.1);
        assert_eq!((ride.sets_total, ride.sets_survived, sc.plan.len()), (1, 1, 0));
    }

    #[test]
    fn workouts_without_sets_stay_default() {
        for name in ["mit-sweetspot-6x5", "lit-1h"] {
            let run = ride_workout(sample(name), 0.5, exact);
            assert!(run.roles.iter().all(|r| *r == StepRole::Default), "{name}");
            assert!(run.events.is_empty(), "{name}");
            assert_eq!((run.ride.sets_total, run.ride.sets_survived), (0, 0), "{name}");
        }
    }

    #[test]
    fn hidits_sets_with_the_exact_rider() {
        let run = ride_workout(sample("hidits-4x12"), 0.1, exact);
        eprintln!("exact hidits-4x12: final advantages {:?}", run.ride.set_advantages_s);
        assert_eq!((run.ride.sets_total, run.ride.sets_survived), (5, 5));
    }
}
