//! Game logic in plain 2D: x = lateral (metres, + = right), y = distance
//! along the road (metres). No rendering types; time comes in as `dt`.

use bevy::prelude::Resource;

use crate::terrain::{Profile, speed_for_gradient};
use crate::tuning::Tuning;

/// Half the road width; the rider's lateral position is clamped to it.
pub const ROAD_HALF_WIDTH_M: f32 = 3.0;

/// Time constant (s) of the lateral easing toward the balance target.
const LATERAL_TIME_CONSTANT_S: f32 = 0.5;

/// World speed on the flat. The terrain scripts it; power never changes it.
pub const DEFAULT_WORLD_SPEED_MPS: f32 = 10.0;

/// While a workout runs, the world speed is scaled by `1 + drift / 20 m`
/// (drift = expected − actual distance), within ± 10 %.
pub const LOCK_DRIFT_M: f32 = 20.0;
pub const LOCK_MAX_SCALE: f32 = 0.1;
/// Locked movement is integrated in steps of at most this (s), so a frame
/// at a high time scale does not skip over gradient changes.
pub const LOCK_SUBSTEP_S: f32 = 0.05;

/// One reading from the rider: power and left-leg share of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiderInput {
    pub power_w: f32,
    pub balance_left_pct: f32,
}

impl Default for RiderInput {
    fn default() -> Self {
        Self {
            power_w: 0.0,
            balance_left_pct: 50.0,
        }
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct RaceState {
    pub distance_m: f32,
    pub lateral_m: f32,
    pub world_speed_mps: f32,
    /// Road height (m) at `distance_m`, from the terrain profile.
    pub elevation_m: f32,
    /// Road gradient (percent) the current world speed was taken from.
    pub gradient_pct: f32,
    /// Road heading (degrees clockwise from north, not wrapped) at
    /// `distance_m`, from the terrain profile; for the debug API.
    pub heading_deg: f32,
}

impl Default for RaceState {
    fn default() -> Self {
        Self {
            distance_m: 0.0,
            lateral_m: 0.0,
            world_speed_mps: DEFAULT_WORLD_SPEED_MPS,
            elevation_m: 0.0,
            gradient_pct: 0.0,
            heading_deg: 0.0,
        }
    }
}

/// Target lateral position for a balance reading. 50 % is the centre line.
/// `m_per_pct` metres per percentage point away from 50 % (the tuning
/// file's `lateral_m_per_pct`), clamped to the road. Left-heavy is negative
/// x (left on screen). NaN → centre.
pub fn lateral_from_balance(balance_left_pct: f32, m_per_pct: f32) -> f32 {
    if balance_left_pct.is_nan() {
        return 0.0;
    }
    ((balance_left_pct - 50.0) * m_per_pct).clamp(-ROAD_HALF_WIDTH_M, ROAD_HALF_WIDTH_M)
}

/// Advances the race by `dt` seconds. The terrain's gradient at the current
/// distance sets the world speed; distance (horizontal, not slope length)
/// moves at that speed, and power has no say in it. Lateral position eases
/// toward the balance target with a first-order lag, which cannot overshoot.
pub fn step(state: &mut RaceState, input: RiderInput, profile: &Profile, tuning: &Tuning, dt: f32) {
    state.gradient_pct = profile.gradient_at(state.distance_m);
    state.world_speed_mps = speed_for_gradient(state.gradient_pct) * tuning.speed_factor;
    state.distance_m += state.world_speed_mps * dt;
    follow(state, input, profile, tuning, dt);
}

/// Speed scale that keeps the distance on the workout's time:
/// `clamp(1 + (expected − distance) / 20, 0.9, 1.1)`.
pub fn lock_scale(expected_m: f32, distance_m: f32) -> f32 {
    (1.0 + (expected_m - distance_m) / LOCK_DRIFT_M).clamp(1.0 - LOCK_MAX_SCALE, 1.0 + LOCK_MAX_SCALE)
}

/// Like `step`, but time is the master: in sub-steps of at most
/// `LOCK_SUBSTEP_S`, the scripted speed is scaled by `lock_scale` toward
/// `expected_at(s)`, the distance the workout expects `s` seconds into this
/// frame.
pub fn step_locked(
    state: &mut RaceState,
    input: RiderInput,
    profile: &Profile,
    tuning: &Tuning,
    dt: f32,
    expected_at: impl Fn(f32) -> f32,
) {
    let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let n = ((dt / LOCK_SUBSTEP_S).ceil() as usize).max(1);
    let h = dt / n as f32;
    for i in 0..n {
        state.gradient_pct = profile.gradient_at(state.distance_m);
        let scale = lock_scale(expected_at((i + 1) as f32 * h), state.distance_m);
        state.world_speed_mps = speed_for_gradient(state.gradient_pct) * tuning.speed_factor * scale;
        state.distance_m += state.world_speed_mps * h;
    }
    follow(state, input, profile, tuning, dt);
}

/// Auto-pause: the world does not move (speed 0); lateral position and the
/// road values still follow.
pub fn step_paused(state: &mut RaceState, input: RiderInput, profile: &Profile, tuning: &Tuning, dt: f32) {
    state.gradient_pct = profile.gradient_at(state.distance_m);
    state.world_speed_mps = 0.0;
    follow(state, input, profile, tuning, dt);
}

/// Lateral easing toward the balance target, and the road values at the
/// new distance.
fn follow(state: &mut RaceState, input: RiderInput, profile: &Profile, tuning: &Tuning, dt: f32) {
    let target = lateral_from_balance(input.balance_left_pct, tuning.lateral_m_per_pct);
    let alpha = 1.0 - (-dt / LATERAL_TIME_CONSTANT_S).exp();
    state.lateral_m += (target - state.lateral_m) * alpha;
    state.elevation_m = profile.elevation_at(state.distance_m);
    state.heading_deg = profile.heading_at(state.distance_m).to_degrees();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn mapping() {
        let m = Tuning::default().lateral_m_per_pct;
        assert!(approx(lateral_from_balance(50.0, m), 0.0));
        assert!(approx(lateral_from_balance(45.0, m), -1.25));
        assert!(approx(lateral_from_balance(55.0, m), 1.25));
        assert!(approx(lateral_from_balance(40.0, m), -2.5));
        assert!(approx(lateral_from_balance(60.0, m), 2.5));
        assert!(approx(lateral_from_balance(30.0, m), -ROAD_HALF_WIDTH_M));
        assert!(approx(lateral_from_balance(0.0, m), -ROAD_HALF_WIDTH_M));
        assert!(approx(lateral_from_balance(100.0, m), ROAD_HALF_WIDTH_M));
        assert!(approx(lateral_from_balance(40.0, 0.1), -1.0));
        assert_eq!(lateral_from_balance(f32::NAN, m), 0.0);
    }

    #[test]
    fn step_advances_distance_by_speed_times_dt() {
        let profile = Profile::demo();
        let mut s = RaceState {
            distance_m: 100.0,
            ..default_state()
        };
        let input = RiderInput {
            power_w: 400.0,
            balance_left_pct: 50.0,
        };
        step(&mut s, input, &profile, &Tuning::default(), 0.25);
        assert_eq!(s.distance_m, 100.0 + s.world_speed_mps * 0.25);
        assert_eq!(s.world_speed_mps, DEFAULT_WORLD_SPEED_MPS);
        assert_eq!(s.gradient_pct, 0.0);
        assert_eq!(s.elevation_m, profile.elevation_at(s.distance_m));
        assert_eq!(s.heading_deg, profile.heading_at(s.distance_m).to_degrees());
    }

    #[test]
    fn speed_factor_scales_the_scripted_speed() {
        let profile = Profile::flat();
        let input = RiderInput::default();
        let tuning = Tuning {
            speed_factor: 1.5,
            ..Tuning::default()
        };
        let mut s = default_state();
        step(&mut s, input, &profile, &tuning, 1.0);
        assert!(approx(s.world_speed_mps, 15.0));
        assert!(approx(s.distance_m, 15.0));
        let mut s = default_state();
        step_locked(&mut s, input, &profile, &tuning, 0.1, |_| 0.0);
        assert!(s.world_speed_mps < 15.0 && s.world_speed_mps >= 13.5 - 1e-3);
    }

    #[test]
    fn terrain_sets_speed_on_climb_and_descent() {
        let profile = Profile::demo();
        let input = RiderInput {
            power_w: 200.0,
            balance_left_pct: 50.0,
        };
        let mut s = RaceState {
            distance_m: 400.0,
            ..default_state()
        };
        step(&mut s, input, &profile, &Tuning::default(), 0.1);
        assert_eq!(s.gradient_pct, 6.0);
        assert!((s.world_speed_mps - 4.0).abs() < 1e-5);
        assert!((s.distance_m - 400.4).abs() < 1e-3);
        let mut s = RaceState {
            distance_m: 1000.0,
            ..default_state()
        };
        step(&mut s, input, &profile, &Tuning::default(), 0.1);
        assert_eq!(s.gradient_pct, -4.0);
        assert!((s.world_speed_mps - 14.8).abs() < 1e-5);
    }

    #[test]
    fn power_never_changes_distance() {
        let profile = Profile::demo();
        let dt = 1.0 / 60.0;
        let run = |power_w| {
            let mut s = default_state();
            let input = RiderInput {
                power_w,
                balance_left_pct: 50.0,
            };
            for _ in 0..3600 {
                step(&mut s, input, &profile, &Tuning::default(), dt);
            }
            s.distance_m
        };
        let (low, high) = (run(100.0), run(400.0));
        assert!(low > 0.0);
        assert_eq!(low, high);
    }

    #[test]
    fn lateral_converges_within_3s_without_overshoot() {
        let profile = Profile::demo();
        for (pct, start) in [(40.0, 0.0), (60.0, 0.0), (60.0, -3.0), (52.0, 1.0)] {
            let mut s = RaceState {
                lateral_m: start,
                ..default_state()
            };
            let target = lateral_from_balance(pct, 0.25);
            let input = RiderInput {
                power_w: 200.0,
                balance_left_pct: pct,
            };
            let dt = 1.0 / 60.0;
            for _ in 0..180 {
                step(&mut s, input, &profile, &Tuning::default(), dt);
                // Never past the target, seen from the starting side.
                assert!((s.lateral_m - target) * (start - target) >= -1e-6);
            }
            assert!((s.lateral_m - target).abs() < 0.05, "pct {pct}: {}", s.lateral_m);
        }
    }

    #[test]
    fn lock_scale_pulls_toward_the_expected_distance_within_ten_pct() {
        assert_eq!(lock_scale(100.0, 100.0), 1.0);
        assert!((lock_scale(101.0, 100.0) - 1.05).abs() < 1e-6);
        assert!((lock_scale(99.0, 100.0) - 0.95).abs() < 1e-6);
        assert_eq!(lock_scale(500.0, 100.0), 1.1);
        assert_eq!(lock_scale(0.0, 100.0), 0.9);
    }

    #[test]
    fn locked_step_follows_the_expected_distance_and_paused_stands_still() {
        let profile = Profile::flat();
        let input = RiderInput {
            power_w: 200.0,
            balance_left_pct: 50.0,
        };
        // Expected runs at 10.5 m/s; flat speed is 10 m/s: the lock catches up.
        let mut s = default_state();
        let mut t = 0.0_f32;
        for _ in 0..600 {
            let t0 = t;
            step_locked(&mut s, input, &profile, &Tuning::default(), 0.1, |x| 10.5 * (t0 + x));
            t += 0.1;
        }
        assert!(
            (s.distance_m - 10.5 * t).abs() < 2.0,
            "{} vs {}",
            s.distance_m,
            10.5 * t
        );
        // A large frame is integrated in sub-steps: same result as small ones.
        let (mut a, mut b) = (default_state(), default_state());
        step_locked(&mut a, input, &Profile::demo(), &Tuning::default(), 2.0, |x| 9.0 * x);
        for i in 0..40 {
            let t0 = i as f32 * 0.05;
            step_locked(&mut b, input, &Profile::demo(), &Tuning::default(), 0.05, |x| {
                9.0 * (t0 + x)
            });
        }
        assert!(
            (a.distance_m - b.distance_m).abs() < 1e-3,
            "{} {}",
            a.distance_m,
            b.distance_m
        );
        let before = s.distance_m;
        step_paused(&mut s, input, &profile, &Tuning::default(), 1.0);
        assert_eq!((s.distance_m, s.world_speed_mps), (before, 0.0));
    }

    fn default_state() -> RaceState {
        RaceState::default()
    }
}
