//! Free ride: one fixed target band. In band the rider holds the partner's
//! wheel and scores; under it the partner rides away; over it nothing is
//! gained and matches burn. Pure logic: power and `dt` come in as
//! parameters, no clock, no rendering or UI types.

use bevy::prelude::{Message, Resource};

/// Exponential smoothing of power (s, about a 3-second average); every
/// decision uses the smoothed value.
pub const SMOOTHING_TAU_S: f32 = 1.5;
/// Smoothed power below this → `Idle`: nothing accrues, the gap is frozen.
pub const IDLE_BELOW_W: f32 = 20.0;
pub const START_MATCHES: u32 = 5;
/// Continuously Over this long → burn one match; again every 4 s while
/// still Over (the alert engine's default timing).
pub const OVER_SUSTAIN_S: f32 = 4.0;
/// In band: the gap shrinks at this rate, not below 0.
pub const GAP_CLOSE_MPS: f32 = 0.5;
/// Under: the gap grows at this rate per 10 % that smoothed power is below
/// `low_w`, capped at `GAP_OPEN_MAX_MPS`.
pub const GAP_OPEN_MPS_PER_10PCT: f32 = 0.2;
pub const GAP_OPEN_MAX_MPS: f32 = 2.0;
/// The gap never exceeds this ("dropped"; the partner waits there).
pub const GAP_MAX_M: f32 = 60.0;
/// A gap at or below this counts as on the wheel.
pub const WHEEL_M: f32 = 1.0;
pub const SCORE_IN_BAND_PER_S: f32 = 1.0;
/// Extra while In band and on the wheel.
pub const SCORE_WHEEL_BONUS_PER_S: f32 = 1.0;
/// Per burn once matches are 0; the score never goes below 0.
pub const SCORE_OVER_PENALTY: f32 = 50.0;

/// After a workout step starts, the gap does not open and the over-timer
/// does not run until the smoothed power first enters the new band, but for
/// at least `TRANSITION_GRACE_MIN_S` and at most `TRANSITION_GRACE_MAX_S`.
/// Score still only accrues in the new band.
pub const TRANSITION_GRACE_MIN_S: f32 = 3.0;
pub const TRANSITION_GRACE_MAX_S: f32 = 10.0;

/// Highest accepted band edge (W).
pub const MAX_BAND_W: f32 = 2000.0;

/// Slack for the 4 s burn timer and the grace's end, so float accumulation
/// of many small `dt`s does not delay either by a frame.
const TIMER_EPS_S: f32 = 1e-4;

/// Target power band; `0 < low_w < high_w <= MAX_BAND_W`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub low_w: f32,
    pub high_w: f32,
}

impl Band {
    /// A band, or an error unless `0 < low < high <= 2000`.
    pub fn new(low_w: f32, high_w: f32) -> Result<Band, String> {
        if low_w.is_finite() && high_w.is_finite() && 0.0 < low_w && low_w < high_w && high_w <= MAX_BAND_W {
            Ok(Band { low_w, high_w })
        } else {
            Err(format!("band must be 0 < low_w < high_w <= {MAX_BAND_W}"))
        }
    }

    pub fn centre_w(&self) -> f32 {
        (self.low_w + self.high_w) / 2.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandState {
    Idle,
    Under,
    In,
    Over,
    /// The workout is over; nothing accrues any more.
    Finished,
}

impl BandState {
    /// `idle`, `under`, `in` or `over`.
    pub fn label(self) -> &'static str {
        match self {
            BandState::Idle => "idle",
            BandState::Under => "under",
            BandState::In => "in",
            BandState::Over => "over",
            BandState::Finished => "finished",
        }
    }
}

#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum RideEvent {
    MatchBurned { left: u32 },
    OutOfMatches,
    BackOnWheel,
    Dropped,
}

/// A running transition grace (see `TRANSITION_GRACE_MIN_S`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grace {
    /// Ride time (not Idle) since the step started.
    pub elapsed_s: f32,
    /// When the smoothed power first entered the new band.
    pub entered_at_s: Option<f32>,
}

impl Grace {
    /// Elapsed time at which the grace ends.
    pub fn end_s(&self) -> f32 {
        self.entered_at_s.map_or(TRANSITION_GRACE_MAX_S, |t| {
            t.clamp(TRANSITION_GRACE_MIN_S, TRANSITION_GRACE_MAX_S)
        })
    }
}

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Ride {
    pub band: Band,
    pub smoothed_power_w: f32,
    pub state: BandState,
    /// 0 = on the wheel.
    pub gap_m: f32,
    pub matches: u32,
    pub score: f32,
    /// Time not Idle.
    pub ride_time_s: f32,
    pub in_band_time_s: f32,
    /// Time continuously Over since the last burn.
    pub over_timer_s: f32,
    /// Matches burned this ride, including burns at zero.
    pub burns: u32,
    /// The transition grace, while it runs.
    pub grace: Option<Grace>,
    /// The workout is over: `step` changes nothing, totals stay.
    pub finished: bool,
    /// The gap is held at 0 (a breakaway pull: the rider is on the front,
    /// so the band state costs advantage, not the wheel). Set by the
    /// scenario layer.
    pub hold_gap: bool,
    /// Breakaway sets in the loaded workout and how many of them stayed
    /// away; set by the scenario layer.
    pub sets_total: u32,
    pub sets_survived: u32,
    /// Final advantage (s) of each breakaway set ridden to its end, in
    /// order; 0 when it was caught.
    pub set_advantages_s: Vec<f32>,
}

impl Ride {
    pub fn new(band: Band) -> Ride {
        Ride {
            band,
            smoothed_power_w: 0.0,
            state: BandState::Idle,
            gap_m: 0.0,
            matches: START_MATCHES,
            score: 0.0,
            ride_time_s: 0.0,
            in_band_time_s: 0.0,
            over_timer_s: 0.0,
            burns: 0,
            grace: None,
            finished: false,
            hold_gap: false,
            sets_total: 0,
            sets_survived: 0,
            set_advantages_s: Vec::new(),
        }
    }

    /// Resets everything except the band.
    pub fn restart(&mut self) {
        *self = Ride::new(self.band);
    }

    /// A workout step started: until the smoothed power first enters the
    /// new band (3..=10 s) the gap does not open and the over-timer does not
    /// run (it starts again from 0).
    pub fn begin_grace(&mut self) {
        self.grace = Some(Grace {
            elapsed_s: 0.0,
            entered_at_s: None,
        });
        self.over_timer_s = 0.0;
    }

    /// The workout is over: state `Finished`, nothing accrues any more.
    pub fn finish(&mut self) {
        self.finished = true;
        self.state = BandState::Finished;
        self.over_timer_s = 0.0;
        self.grace = None;
    }

    /// Share of ride time spent in band, percent; 0 without ride time.
    pub fn in_band_pct(&self) -> f32 {
        if self.ride_time_s > 0.0 {
            self.in_band_time_s / self.ride_time_s * 100.0
        } else {
            0.0
        }
    }

    /// Seconds until the next burn while Over.
    pub fn next_burn_in_s(&self) -> f32 {
        (OVER_SUSTAIN_S - self.over_timer_s).max(0.0)
    }
}

/// `Idle` below `IDLE_BELOW_W`, else `In` for `low <= smoothed <= high`,
/// `Under` / `Over` outside.
pub fn classify(band: &Band, smoothed_w: f32) -> BandState {
    if smoothed_w.is_nan() || smoothed_w < IDLE_BELOW_W {
        BandState::Idle
    } else if smoothed_w < band.low_w {
        BandState::Under
    } else if smoothed_w > band.high_w {
        BandState::Over
    } else {
        BandState::In
    }
}

/// Under: 0.2 m/s per 10 % below `low_w`, at most 2 m/s.
pub fn gap_open_rate(band: &Band, smoothed_w: f32) -> f32 {
    let below_pct = (band.low_w - smoothed_w) / band.low_w * 100.0;
    (below_pct / 10.0 * GAP_OPEN_MPS_PER_10PCT).clamp(0.0, GAP_OPEN_MAX_MPS)
}

/// Advances the ride by `dt` seconds with the current power reading.
pub fn step(ride: &mut Ride, power_w: f32, dt: f32) -> Vec<RideEvent> {
    let mut events = Vec::new();
    if ride.finished {
        ride.state = BandState::Finished;
        return events;
    }
    let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let power = if power_w.is_finite() { power_w.max(0.0) } else { 0.0 };
    // No false start: riding off from idle (start, restart, after a pause)
    // begins at the raw value instead of climbing from 0.
    if power >= IDLE_BELOW_W && ride.smoothed_power_w < IDLE_BELOW_W {
        ride.smoothed_power_w = power;
    }
    let alpha = 1.0 - (-dt / SMOOTHING_TAU_S).exp();
    ride.smoothed_power_w += (power - ride.smoothed_power_w) * alpha;
    ride.state = classify(&ride.band, ride.smoothed_power_w);
    if ride.state != BandState::Over {
        ride.over_timer_s = 0.0;
    }
    if ride.state == BandState::Idle {
        return events;
    }
    ride.ride_time_s += dt;
    // The part of `dt` after the grace: only it opens the gap or runs the
    // over-timer.
    let live_dt = match ride.grace.as_mut() {
        None => dt,
        Some(g) => {
            let t = g.elapsed_s + dt;
            if ride.state == BandState::In && g.entered_at_s.is_none() {
                g.entered_at_s = Some(t);
            }
            g.elapsed_s = t;
            let end = g.end_s();
            // Same slack as the burn timer: many small `dt`s must not keep
            // the grace a frame too long.
            if t + TIMER_EPS_S >= end {
                ride.grace = None;
            }
            (t - end).clamp(0.0, dt)
        }
    };
    let gap_before = ride.gap_m;
    match ride.state {
        BandState::In => {
            ride.gap_m = (ride.gap_m - GAP_CLOSE_MPS * dt).max(0.0);
            ride.in_band_time_s += dt;
            let bonus = if ride.gap_m <= WHEEL_M {
                SCORE_WHEEL_BONUS_PER_S
            } else {
                0.0
            };
            ride.score += (SCORE_IN_BAND_PER_S + bonus) * dt;
        }
        BandState::Under => {
            let rate = gap_open_rate(&ride.band, ride.smoothed_power_w);
            ride.gap_m = (ride.gap_m + rate * live_dt).min(GAP_MAX_M);
        }
        BandState::Over => {
            ride.over_timer_s += live_dt;
            while ride.over_timer_s + TIMER_EPS_S >= OVER_SUSTAIN_S {
                ride.over_timer_s = (ride.over_timer_s - OVER_SUSTAIN_S).max(0.0);
                burn(ride, &mut events);
            }
        }
        BandState::Idle | BandState::Finished => {}
    }
    if ride.hold_gap {
        ride.gap_m = 0.0;
    }
    if gap_before > WHEEL_M && ride.gap_m <= WHEEL_M {
        events.push(RideEvent::BackOnWheel);
    }
    if gap_before < GAP_MAX_M && ride.gap_m >= GAP_MAX_M {
        events.push(RideEvent::Dropped);
    }
    events
}

fn burn(ride: &mut Ride, events: &mut Vec<RideEvent>) {
    ride.burns += 1;
    if ride.matches > 0 {
        ride.matches -= 1;
        events.push(RideEvent::MatchBurned { left: ride.matches });
        if ride.matches == 0 {
            events.push(RideEvent::OutOfMatches);
        }
    } else {
        ride.score = (ride.score - SCORE_OVER_PENALTY).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.1;

    fn band() -> Band {
        Band::new(120.0, 160.0).unwrap()
    }

    /// A ride whose smoothed power already sits at `power_w`.
    fn settled(power_w: f32) -> Ride {
        Ride {
            smoothed_power_w: power_w,
            ..Ride::new(band())
        }
    }

    /// Steps `secs` of constant power in `DT` steps; all events.
    fn ride_for(ride: &mut Ride, power_w: f32, secs: f32) -> Vec<RideEvent> {
        let n = (secs / DT).round() as usize;
        (0..n).flat_map(|_| step(ride, power_w, DT)).collect()
    }

    fn burned(events: &[RideEvent]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, RideEvent::MatchBurned { .. }))
            .count()
    }

    fn count(events: &[RideEvent], e: RideEvent) -> usize {
        events.iter().filter(|x| **x == e).count()
    }

    #[test]
    fn band_validation() {
        assert!(Band::new(180.0, 220.0).is_ok());
        assert!(Band::new(1.0, 2000.0).is_ok());
        for (l, h) in [
            (220.0, 180.0),
            (200.0, 200.0),
            (0.0, 100.0),
            (-10.0, 100.0),
            (100.0, 2001.0),
            (f32::NAN, 100.0),
        ] {
            assert!(Band::new(l, h).is_err(), "{l} {h}");
        }
    }

    #[test]
    fn classification_incl_band_edges() {
        let b = band();
        assert_eq!(classify(&b, 0.0), BandState::Idle);
        assert_eq!(classify(&b, 19.9), BandState::Idle);
        assert_eq!(classify(&b, f32::NAN), BandState::Idle);
        assert_eq!(classify(&b, 20.0), BandState::Under);
        assert_eq!(classify(&b, 119.9), BandState::Under);
        assert_eq!(classify(&b, 120.0), BandState::In);
        assert_eq!(classify(&b, 140.0), BandState::In);
        assert_eq!(classify(&b, 160.0), BandState::In);
        assert_eq!(classify(&b, 160.1), BandState::Over);
        // Each state through `step`.
        for (power, state) in [
            (0.0, BandState::Idle),
            (100.0, BandState::Under),
            (140.0, BandState::In),
            (200.0, BandState::Over),
        ] {
            let mut r = settled(power);
            step(&mut r, power, DT);
            assert_eq!(r.state, state, "{power}");
        }
    }

    #[test]
    fn smoothing_reaches_95_pct_in_about_4_5_s() {
        // From the idle threshold (no snap) up to 200 W.
        let from = Ride {
            smoothed_power_w: IDLE_BELOW_W,
            ..Ride::new(band())
        };
        let mut r = from.clone();
        ride_for(&mut r, 200.0, 4.2);
        assert!(r.smoothed_power_w < 190.0, "{}", r.smoothed_power_w);
        ride_for(&mut r, 200.0, 0.2);
        assert!(r.smoothed_power_w >= 190.0, "{}", r.smoothed_power_w);
        // A whole step at once matches many small ones.
        let mut once = from;
        step(&mut once, 200.0, 4.4);
        assert!((once.smoothed_power_w - r.smoothed_power_w).abs() < 0.01);
    }

    #[test]
    fn gap_closes_at_half_a_metre_per_second_and_stops_at_zero() {
        let mut r = Ride {
            gap_m: 10.0,
            ..settled(140.0)
        };
        ride_for(&mut r, 140.0, 4.0);
        assert!((r.gap_m - 8.0).abs() < 1e-3, "{}", r.gap_m);
        ride_for(&mut r, 140.0, 30.0);
        assert_eq!(r.gap_m, 0.0);
    }

    #[test]
    fn gap_opens_proportionally_and_is_capped() {
        // 10 % below low → 0.2 m/s; 20 % → 0.4 m/s.
        for (power, rate) in [(108.0, 0.2), (96.0, 0.4), (60.0, 1.0)] {
            let mut r = settled(power);
            ride_for(&mut r, power, 10.0);
            assert!((r.gap_m - rate * 10.0).abs() < 1e-3, "{power}: {}", r.gap_m);
        }
        // Rate cap: 100 % below would be 2.0 m/s; nothing goes past it.
        assert_eq!(gap_open_rate(&band(), 0.0), GAP_OPEN_MAX_MPS);
        assert_eq!(gap_open_rate(&band(), -500.0), GAP_OPEN_MAX_MPS);
        assert_eq!(gap_open_rate(&band(), 130.0), 0.0);
        // Maximum gap: stops at 60 m, `Dropped` once.
        let mut r = Ride {
            gap_m: 55.0,
            ..settled(30.0)
        };
        let events = ride_for(&mut r, 30.0, 30.0);
        assert_eq!(r.gap_m, GAP_MAX_M);
        assert_eq!(events, vec![RideEvent::Dropped]);
    }

    #[test]
    fn over_burns_every_four_seconds() {
        let over = 210.0;
        for (secs, burns) in [(3.9, 0), (4.0, 1), (12.0, 3)] {
            let mut r = settled(over);
            let events = ride_for(&mut r, over, secs);
            assert_eq!(burned(&events), burns, "{secs} s");
            assert_eq!((r.burns, r.matches), (burns as u32, START_MATCHES - burns as u32));
        }
    }

    #[test]
    fn over_leaves_gap_and_score_alone() {
        let mut r = Ride {
            gap_m: 7.0,
            score: 30.0,
            ..settled(210.0)
        };
        ride_for(&mut r, 210.0, 15.0);
        assert_eq!((r.gap_m, r.score, r.state), (7.0, 30.0, BandState::Over));
    }

    #[test]
    fn leaving_over_restarts_the_four_seconds() {
        let mut r = settled(210.0);
        ride_for(&mut r, 210.0, 3.0);
        assert!((r.over_timer_s - 3.0).abs() < 1e-3);
        r.smoothed_power_w = 140.0;
        step(&mut r, 140.0, DT);
        assert_eq!(r.over_timer_s, 0.0);
        r.smoothed_power_w = 210.0;
        assert_eq!(burned(&ride_for(&mut r, 210.0, 3.9)), 0);
        assert_eq!(burned(&ride_for(&mut r, 210.0, 0.1)), 1);
    }

    #[test]
    fn five_burns_then_out_of_matches_once_then_score_penalty() {
        let mut r = Ride {
            score: 70.0,
            ..settled(210.0)
        };
        let events = ride_for(&mut r, 210.0, 20.0);
        let lefts: Vec<u32> = events
            .iter()
            .filter_map(|e| {
                if let RideEvent::MatchBurned { left } = e {
                    Some(*left)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(lefts, [4, 3, 2, 1, 0]);
        assert_eq!(count(&events, RideEvent::OutOfMatches), 1);
        assert_eq!(*events.last().unwrap(), RideEvent::OutOfMatches);
        assert_eq!((r.matches, r.score), (0, 70.0));
        // Sixth burn: 50 off, no further events.
        let events = ride_for(&mut r, 210.0, 4.0);
        assert!(events.is_empty(), "{events:?}");
        assert_eq!((r.burns, r.score), (6, 20.0));
        // Seventh: floors at 0.
        ride_for(&mut r, 210.0, 4.0);
        assert_eq!((r.burns, r.score), (7, 0.0));
    }

    #[test]
    fn idle_freezes_everything() {
        let mut r = Ride {
            gap_m: 5.0,
            score: 10.0,
            matches: 3,
            ride_time_s: 8.0,
            in_band_time_s: 4.0,
            ..settled(10.0)
        };
        let before = r.clone();
        let events = ride_for(&mut r, 0.0, 30.0);
        assert!(events.is_empty());
        assert_eq!(r.state, BandState::Idle);
        assert_eq!(
            Ride {
                smoothed_power_w: before.smoothed_power_w,
                ..r
            },
            before
        );
        // Idle also ends an Over spell.
        let mut r = settled(210.0);
        ride_for(&mut r, 210.0, 3.0);
        r.smoothed_power_w = 0.0;
        step(&mut r, 0.0, DT);
        assert_eq!(r.over_timer_s, 0.0);
    }

    #[test]
    fn score_rates() {
        let mut on_wheel = settled(140.0);
        ride_for(&mut on_wheel, 140.0, 10.0);
        assert!((on_wheel.score - 20.0).abs() < 1e-3, "{}", on_wheel.score);
        let mut off_wheel = Ride {
            gap_m: 30.0,
            ..settled(140.0)
        };
        ride_for(&mut off_wheel, 140.0, 10.0);
        assert!((off_wheel.score - 10.0).abs() < 1e-3, "{}", off_wheel.score);
        for power in [60.0, 210.0] {
            let mut r = settled(power);
            ride_for(&mut r, power, 10.0);
            assert_eq!(r.score, 0.0, "{power}");
        }
        assert!((on_wheel.in_band_pct() - 100.0).abs() < 1e-3);
        assert_eq!(Ride::new(band()).in_band_pct(), 0.0);
    }

    /// The one rule: nothing rewards power above the band.
    #[test]
    fn power_above_the_band_never_pays() {
        let b = band();
        let mut over = Ride::new(b);
        ride_for(&mut over, b.high_w + 50.0, 60.0);
        assert_eq!(over.score, 0.0);

        let mut centre = Ride::new(b);
        ride_for(&mut centre, b.centre_w(), 60.0);
        assert!(centre.score > 100.0, "{}", centre.score);
        // Any constant power above the band, from cold or already settled.
        let mut p = b.high_w + 0.5;
        while p < 2000.0 {
            for mut r in [Ride::new(b), settled(p)] {
                ride_for(&mut r, p, 60.0);
                assert!(r.score < centre.score, "{p} W: {} >= {}", r.score, centre.score);
            }
            p += 7.5;
        }
        // Mixing over-band spells into a centred ride never beats staying centred.
        for spell_s in [1.0, 3.0, 5.0, 10.0] {
            let mut r = Ride::new(b);
            let mut t = 0.0;
            while t < 60.0 - 1e-3 {
                ride_for(&mut r, b.centre_w(), 5.0);
                ride_for(&mut r, b.high_w + 80.0, spell_s);
                t += 5.0 + spell_s;
            }
            assert!(
                r.score < centre.score,
                "spell {spell_s}: {} >= {}",
                r.score,
                centre.score
            );
        }
    }

    #[test]
    fn events_fire_once_per_crossing() {
        let mut r = Ride {
            gap_m: 3.0,
            ..settled(140.0)
        };
        let events = ride_for(&mut r, 140.0, 10.0);
        assert_eq!(events, vec![RideEvent::BackOnWheel]);
        // Drift back past the wheel, then close again: one more.
        r.smoothed_power_w = 60.0;
        assert!(ride_for(&mut r, 60.0, 3.0).is_empty());
        assert!(r.gap_m > WHEEL_M);
        r.smoothed_power_w = 140.0;
        assert_eq!(ride_for(&mut r, 140.0, 10.0), vec![RideEvent::BackOnWheel]);
        // Sitting at the maximum fires `Dropped` only on arrival.
        let mut r = Ride {
            gap_m: 59.0,
            ..settled(60.0)
        };
        assert_eq!(ride_for(&mut r, 60.0, 20.0), vec![RideEvent::Dropped]);
        r.smoothed_power_w = 140.0;
        ride_for(&mut r, 140.0, 2.0);
        r.smoothed_power_w = 60.0;
        assert_eq!(ride_for(&mut r, 60.0, 20.0), vec![RideEvent::Dropped]);
    }

    #[test]
    fn riding_off_from_idle_snaps_to_the_raw_power() {
        let b = Band::new(180.0, 220.0).unwrap();
        let mut r = Ride {
            score: 50.0,
            smoothed_power_w: 260.0,
            ..Ride::new(b)
        };
        r.restart();
        step(&mut r, 200.0, DT);
        assert_eq!((r.state, r.smoothed_power_w, r.gap_m), (BandState::In, 200.0, 0.0));
        ride_for(&mut r, 200.0, 10.0);
        assert_eq!((r.state, r.gap_m), (BandState::In, 0.0));
        // Back from a pause: idle, then straight into the band.
        ride_for(&mut r, 0.0, 30.0);
        assert_eq!(r.state, BandState::Idle);
        step(&mut r, 200.0, DT);
        assert_eq!(r.state, BandState::In);
        // Below the idle threshold nothing snaps.
        let mut r = Ride::new(b);
        step(&mut r, 15.0, DT);
        assert!(r.smoothed_power_w < 15.0);
    }

    #[test]
    fn grace_holds_gap_and_over_timer_but_not_score() {
        // Under in grace, never reaching the band: the gap stays for 10 s,
        // then opens.
        let mut r = Ride {
            gap_m: 3.0,
            ..settled(60.0)
        };
        r.begin_grace();
        ride_for(&mut r, 60.0, TRANSITION_GRACE_MAX_S);
        assert!((r.gap_m - 3.0).abs() < 1e-4, "{}", r.gap_m);
        assert_eq!(r.grace, None);
        ride_for(&mut r, 60.0, 1.0);
        assert!((r.gap_m - 4.0).abs() < 1e-3, "{}", r.gap_m);
        // Over in grace: no timer, no burn; it starts counting afterwards.
        let mut r = settled(210.0);
        ride_for(&mut r, 210.0, 3.0);
        r.begin_grace();
        assert_eq!(r.over_timer_s, 0.0);
        let events = ride_for(&mut r, 210.0, TRANSITION_GRACE_MAX_S);
        assert_eq!((burned(&events), r.score), (0, 0.0));
        assert!(r.over_timer_s < 1e-4, "{}", r.over_timer_s);
        assert_eq!(burned(&ride_for(&mut r, 210.0, 4.0)), 1);
        // Score still only in band; in band it accrues as usual.
        let mut r = settled(140.0);
        r.begin_grace();
        ride_for(&mut r, 140.0, 2.0);
        assert!((r.score - 4.0).abs() < 1e-3, "{}", r.score);
        // One large step: only the part after the grace counts.
        let mut r = settled(60.0);
        r.begin_grace();
        step(&mut r, 60.0, 15.0);
        assert!((r.gap_m - 5.0).abs() < 1e-3, "{}", r.gap_m);
        // Idle does not use up the grace.
        let mut r = settled(0.0);
        r.begin_grace();
        ride_for(&mut r, 0.0, 10.0);
        assert_eq!(
            r.grace,
            Some(Grace {
                elapsed_s: 0.0,
                entered_at_s: None
            })
        );
    }

    #[test]
    fn grace_ends_when_the_band_is_first_reached_but_not_before_three_seconds() {
        // In band from the start: the grace still lasts 3 s.
        let mut r = settled(140.0);
        r.begin_grace();
        ride_for(&mut r, 140.0, 2.9);
        assert!(r.grace.is_some());
        ride_for(&mut r, 140.0, 0.1);
        assert_eq!(r.grace, None);
        // Reached after 1 s, left again: the gap opens from 3 s on.
        let mut r = settled(60.0);
        r.begin_grace();
        ride_for(&mut r, 60.0, 1.0);
        r.smoothed_power_w = 140.0;
        step(&mut r, 140.0, DT);
        r.smoothed_power_w = 60.0;
        ride_for(&mut r, 60.0, 1.9);
        assert_eq!(r.gap_m, 0.0);
        ride_for(&mut r, 60.0, 2.0);
        assert!((r.gap_m - 2.0).abs() < 1e-3, "{}", r.gap_m);
        // Reached after 6 s: from then on Over counts again (no waiting for
        // the 10 s maximum).
        let mut r = settled(210.0);
        r.begin_grace();
        ride_for(&mut r, 210.0, 6.0);
        r.smoothed_power_w = 140.0;
        step(&mut r, 140.0, DT);
        assert_eq!(r.grace, None);
        r.smoothed_power_w = 210.0;
        assert_eq!(burned(&ride_for(&mut r, 210.0, 4.0)), 1);
    }

    /// The one rule holds through step changes: over-band spells in the
    /// grace window never beat staying centred.
    #[test]
    fn grace_never_pays_for_power_above_the_band() {
        let b = band();
        let run = |power_in_grace: f32| {
            let mut r = Ride::new(b);
            for _ in 0..12 {
                r.begin_grace();
                ride_for(&mut r, power_in_grace, 5.0);
                ride_for(&mut r, b.centre_w(), 5.0);
            }
            r
        };
        let centred = run(b.centre_w());
        for p in [b.high_w + 1.0, b.high_w + 80.0, 1500.0] {
            let r = run(p);
            assert!(r.score < centred.score, "{p}: {} >= {}", r.score, centred.score);
        }
    }

    #[test]
    fn finished_ride_changes_nothing() {
        let mut r = Ride {
            gap_m: 4.0,
            score: 33.0,
            ..settled(60.0)
        };
        r.finish();
        let before = r.clone();
        for p in [0.0, 60.0, 140.0, 400.0] {
            assert!(ride_for(&mut r, p, 10.0).is_empty());
        }
        assert_eq!(r, before);
        assert_eq!((r.state, r.state.label()), (BandState::Finished, "finished"));
        r.restart();
        assert!(!r.finished);
    }

    #[test]
    fn a_held_gap_stays_at_zero_whatever_the_power() {
        for power in [30.0, 60.0, 140.0, 210.0] {
            let mut r = Ride {
                hold_gap: true,
                ..settled(power)
            };
            let events = ride_for(&mut r, power, 20.0);
            assert_eq!(r.gap_m, 0.0, "{power}");
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, RideEvent::Dropped | RideEvent::BackOnWheel)),
                "{power}"
            );
        }
        // Over still burns matches; in band still scores.
        let mut r = Ride {
            hold_gap: true,
            ..settled(210.0)
        };
        assert_eq!(burned(&ride_for(&mut r, 210.0, 8.0)), 2);
        let mut r = Ride {
            hold_gap: true,
            ..settled(140.0)
        };
        ride_for(&mut r, 140.0, 10.0);
        assert!(r.score > 0.0);
    }

    #[test]
    fn restart_keeps_only_the_band() {
        let b = Band::new(180.0, 220.0).unwrap();
        let mut r = Ride {
            gap_m: 12.0,
            matches: 1,
            score: 99.0,
            burns: 4,
            ride_time_s: 50.0,
            ..Ride::new(b)
        };
        ride_for(&mut r, 260.0, 5.0);
        r.restart();
        assert_eq!(r, Ride::new(b));
    }
}
