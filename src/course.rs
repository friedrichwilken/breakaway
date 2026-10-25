//! The course generated from a workout (pure): an open profile whose
//! gradients follow each step's intensity and whose lengths are solved so
//! that riding it at the scripted speed (`speed_for_gradient`) takes each
//! step's time. Subdivision, bends and rolling terrain come from a small
//! PRNG seeded by the workout's name and total duration, so the same
//! workout always gives the same course.

use crate::terrain::{Profile, Segment, open_gradient_at, speed_for_gradient};
use crate::workout::{Step, StepClass, Workout};

/// Every step is cut into chunks this long (a shorter step is one chunk).
pub const MIN_CHUNK_M: f32 = 150.0;
pub const MAX_CHUNK_M: f32 = 500.0;
/// No profile segment is shorter; a step that would be is merged into the
/// previous chunk for geometry.
pub const MIN_SEGMENT_M: f32 = 60.0;
/// Bends: at most this much turn per chunk, never tighter than this radius.
pub const MAX_TURN_DEG: f32 = 75.0;
pub const MIN_RADIUS_M: f32 = 80.0;
/// Steps longer than this roll: chunk gradients vary by up to
/// `ROLLING_PCT` around the step's, with zero mean over the step.
pub const ROLLING_ABOVE_S: f32 = 300.0;
pub const ROLLING_PCT: f32 = 1.5;
/// No chunk is steeper or falls more than these.
pub const MAX_GRADIENT_PCT: f32 = 10.0;
pub const MIN_GRADIENT_PCT: f32 = -4.0;

/// Rounds of the length solve, at most.
const MAX_ROUNDS: usize = 10;
/// The solve stops early once every boundary is this close; well inside
/// the 0.5 s the spec allows, so riding the course in frame steps stays
/// inside that too.
const SOLVE_TARGET_S: f64 = 0.05;
/// Integration step (m) of the travel time.
const INTEGRATION_STEP_M: f64 = 1.0;
/// Rolling steps are cut before the solve, on the first length estimate,
/// into chunks within these bounds, so the solve's rescaling keeps them
/// within 150–500 m.
const PLAN_MIN_CHUNK_M: f32 = 190.0;
const PLAN_MAX_CHUNK_M: f32 = 400.0;
/// Share of straight chunks.
const STRAIGHT_SHARE: f32 = 1.0 / 3.0;
/// Chance that a bend turns the other way than the previous bend.
const ALTERNATE_CHANCE: f32 = 0.7;
/// Smallest bend that counts as one.
const MIN_TURN_DEG: f32 = 10.0;

/// Workout time (s) → distance (m): the distance at each whole second.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeTable {
    distance_m: Vec<f32>,
}

impl TimeTable {
    /// Distance at workout time `elapsed_s`, linearly between whole
    /// seconds; clamped to the table (before 0: 0, after the end: the end).
    pub fn at(&self, elapsed_s: f64) -> f32 {
        let last = self.distance_m.len() - 1;
        let x = elapsed_s.clamp(0.0, last as f64);
        let i = (x.floor() as usize).min(last.saturating_sub(1));
        if last == 0 {
            return self.distance_m[0];
        }
        let f = (x - i as f64) as f32;
        self.distance_m[i] + (self.distance_m[i + 1] - self.distance_m[i]) * f
    }

    /// The distance at each whole second, from 0.
    #[cfg(test)]
    pub fn seconds(&self) -> &[f32] {
        &self.distance_m
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Course {
    /// Open profile named `workout`.
    pub profile: Profile,
    /// Distance where each step starts.
    pub step_start_m: Vec<f32>,
    /// Workout time (s) → distance (m).
    pub expected_distance: TimeTable,
}

impl Course {
    pub fn length_m(&self) -> f32 {
        self.profile.total_length_m()
    }
}

/// The base gradient (percent) of a step, from the centre of its range:
/// warm-up and cool-down 0 %; active `(i − 55) / 6.5` clamped to 0..=10 and
/// rounded to 0.5 %; a rest shorter than 120 s 0 % (a ledge on the climb),
/// a longer one −3 %.
pub fn step_gradient(step: &Step) -> f32 {
    let centre = (step.low_pct + step.high_pct) / 2.0;
    match step.class {
        StepClass::WarmUp | StepClass::CoolDown => 0.0,
        StepClass::Active => (((centre - 55.0) / 6.5).clamp(0.0, MAX_GRADIENT_PCT) * 2.0).round() / 2.0,
        StepClass::Rest if step.duration_s < 120.0 => 0.0,
        StepClass::Rest => -3.0,
    }
}

/// splitmix64: small, deterministic, good enough for terrain.
struct Rng(u64);

impl Rng {
    /// Seeded from the workout's name and total duration.
    fn for_workout(workout: &Workout) -> Rng {
        // FNV-1a over the name, then the duration's bits.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in workout.name.bytes().chain(workout.total_s().to_bits().to_le_bytes()) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Rng(h)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in `[lo, hi)`.
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

/// Fractions (summing to 1) that cut `len_m` into chunks of `min..=max` m;
/// one chunk when the step is too short for two (or shorter than `min`).
fn split(len_m: f32, min: f32, max: f32, rng: &mut Rng) -> Vec<f32> {
    let k_min = ((len_m / max).ceil() as usize).max(1);
    let k_max = ((len_m / min).floor() as usize).max(k_min);
    let target = rng.range(1.3 * min, 0.8 * max);
    let k = ((len_m / target).round() as usize).clamp(k_min, k_max);
    if k == 1 {
        return vec![1.0];
    }
    let weights: Vec<f32> = (0..k).map(|_| rng.range(0.7, 1.3)).collect();
    let sum: f32 = weights.iter().sum();
    let fractions: Vec<f32> = weights.iter().map(|w| w / sum).collect();
    let fits = fractions.iter().all(|f| (min..=max).contains(&(f * len_m)));
    if fits { fractions } else { vec![1.0 / k as f32; k] }
}

/// Zero-mean (weighted by `fractions`) offsets of up to ± `ROLLING_PCT`
/// around `gradient`, scaled down where needed so no chunk leaves
/// `MIN_GRADIENT_PCT..=MAX_GRADIENT_PCT`.
fn rolling(gradient: f32, fractions: &[f32], rng: &mut Rng) -> Vec<f32> {
    let mut offsets: Vec<f32> = fractions.iter().map(|_| rng.range(-ROLLING_PCT, ROLLING_PCT)).collect();
    let mean: f32 = offsets.iter().zip(fractions).map(|(o, f)| o * f).sum();
    for o in &mut offsets {
        *o -= mean;
    }
    let lo = (-ROLLING_PCT).max(MIN_GRADIENT_PCT - gradient);
    let hi = ROLLING_PCT.min(MAX_GRADIENT_PCT - gradient);
    let mut scale: f32 = 1.0;
    for &o in &offsets {
        if o > hi {
            scale = scale.min((hi / o).max(0.0));
        } else if o < lo {
            scale = scale.min((lo / o).max(0.0));
        }
    }
    offsets.iter().map(|o| gradient + o * scale).collect()
}

/// A step's pieces for the solve: `(fraction of its length, gradient)`.
type Pieces = Vec<(f32, f32)>;

/// Segments (no bends) and their starts for the steps' pieces at `lengths`.
fn layout(pieces: &[Pieces], lengths: &[f64]) -> (Vec<Segment>, Vec<f32>) {
    let mut segments = Vec::new();
    let mut starts = Vec::new();
    let mut at = 0.0_f64;
    for (step, len) in pieces.iter().zip(lengths) {
        for &(fraction, gradient_pct) in step {
            starts.push(at as f32);
            let length_m = (f64::from(fraction) * len) as f32;
            segments.push(Segment {
                length_m,
                gradient_pct,
                curve_deg: 0.0,
            });
            at += f64::from(fraction) * len;
        }
    }
    (segments, starts)
}

/// Travel time over `[from, from + len)` at the scripted speed, integrated
/// in steps of at most 1 m (midpoint): `(end distance, time)` after each.
fn travel(from: f64, len: f64, speed_factor: f64, gradient_at: &impl Fn(f32) -> f32) -> Vec<(f64, f64)> {
    let n = ((len / INTEGRATION_STEP_M).ceil() as usize).max(1);
    let h = len / n as f64;
    let mut t = 0.0;
    (0..n)
        .map(|i| {
            let mid = from + (i as f64 + 0.5) * h;
            t += h / (f64::from(speed_for_gradient(gradient_at(mid as f32))) * speed_factor);
            (from + (i + 1) as f64 * h, t)
        })
        .collect()
}

/// Each step's travel time for `lengths` laid out end to end.
fn step_times(lengths: &[f64], speed_factor: f64, gradient_at: &impl Fn(f32) -> f32) -> Vec<f64> {
    let mut at = 0.0;
    lengths
        .iter()
        .map(|&len| {
            let t = travel(at, len, speed_factor, gradient_at).last().map_or(0.0, |p| p.1);
            at += len;
            t
        })
        .collect()
}

/// Largest gap between a step's end reached at `times` and its planned end.
fn largest_boundary_error(times: &[f64], durations: &[f64]) -> f64 {
    let (mut reached, mut planned, mut worst) = (0.0, 0.0, 0.0_f64);
    for (t, d) in times.iter().zip(durations) {
        reached += t;
        planned += d;
        worst = worst.max((reached - planned).abs());
    }
    worst
}

/// The course for `workout` (which must have steps; `Workout::validate`),
/// solved for the scripted speed times `speed_factor` (the tuning file's;
/// tests pass 1.0).
pub fn generate(workout: &Workout, speed_factor: f32) -> Course {
    let speed_factor = f64::from(speed_factor);
    let steps = &workout.steps;
    let n = steps.len();
    let mut rng = Rng::for_workout(workout);
    let durations: Vec<f64> = steps.iter().map(|s| f64::from(s.duration_s)).collect();
    let base: Vec<f32> = steps.iter().map(step_gradient).collect();
    let estimate = |k: usize, g: f32| f64::from(speed_for_gradient(g)) * speed_factor * durations[k];

    // Steps too short for a segment of their own take a neighbour's
    // gradient: the previous chunk's, or the next one's for a first step.
    let short: Vec<bool> = (0..n)
        .map(|k| estimate(k, base[k]) < f64::from(MIN_SEGMENT_M) && n > 1)
        .collect();
    let mut pieces: Vec<Pieces> = vec![Vec::new(); n];
    for k in 0..n {
        if short[k] {
            continue;
        }
        pieces[k] = if steps[k].duration_s > ROLLING_ABOVE_S {
            let fractions = split(
                estimate(k, base[k]) as f32,
                PLAN_MIN_CHUNK_M,
                PLAN_MAX_CHUNK_M,
                &mut rng,
            );
            let gradients = rolling(base[k], &fractions, &mut rng);
            fractions.into_iter().zip(gradients).collect()
        } else {
            vec![(1.0, base[k])]
        };
    }
    for k in 0..n {
        if !short[k] {
            continue;
        }
        let neighbour = if k > 0 {
            pieces[k - 1].last().map(|p| p.1)
        } else {
            pieces.iter().find_map(|p| p.first().map(|q| q.1))
        };
        pieces[k] = vec![(1.0, neighbour.unwrap_or(base[k]))];
    }

    // Lengths: start from speed × time, then rescale each step by planned
    // over measured time until every boundary is on time.
    let mut lengths: Vec<f64> = (0..n)
        .map(|k| estimate(k, pieces[k].iter().map(|p| p.0 * p.1).sum()))
        .collect();
    for round in 0..=MAX_ROUNDS {
        let (segments, starts) = layout(&pieces, &lengths);
        let times = step_times(&lengths, speed_factor, &|d| open_gradient_at(&segments, &starts, d));
        if largest_boundary_error(&times, &durations) <= SOLVE_TARGET_S || round == MAX_ROUNDS {
            break;
        }
        for k in 0..n {
            if times[k] > 0.0 {
                lengths[k] *= durations[k] / times[k];
            }
        }
    }

    // Geometry: rolling steps keep their pieces; the others are cut now
    // (their chunks share one gradient, so cutting changes no time); short
    // steps join a neighbouring chunk.
    let mut chunks: Vec<(f64, f32)> = Vec::new();
    let mut lead = 0.0;
    for k in 0..n {
        let len = lengths[k];
        if short[k] {
            match chunks.last_mut() {
                Some(last) => last.0 += len,
                None => lead += len,
            }
            continue;
        }
        let cut: Pieces = if steps[k].duration_s > ROLLING_ABOVE_S {
            pieces[k].clone()
        } else {
            split(len as f32, MIN_CHUNK_M, MAX_CHUNK_M, &mut rng)
                .into_iter()
                .map(|f| (f, pieces[k][0].1))
                .collect()
        };
        for (fraction, gradient) in cut {
            chunks.push((f64::from(fraction) * len, gradient));
        }
        if lead > 0.0 {
            chunks[0].0 += lead;
            lead = 0.0;
        }
    }
    if chunks.is_empty() {
        // Only possible for a single very short step.
        chunks.push((lead, pieces[0][0].1));
    }
    for c in &mut chunks {
        c.0 = c.0.max(f64::from(MIN_SEGMENT_M));
    }

    // Bends: about a third straight, the rest alternating more often than not.
    let mut last_turn = 0.0_f32;
    let segments: Vec<Segment> = chunks
        .iter()
        .map(|&(len, gradient_pct)| {
            let length_m = len as f32;
            let max_turn = MAX_TURN_DEG.min((length_m / MIN_RADIUS_M).to_degrees() * 0.999);
            let curve_deg = if rng.unit() < STRAIGHT_SHARE || max_turn < MIN_TURN_DEG {
                0.0
            } else {
                let size = rng.range(MIN_TURN_DEG, max_turn);
                let side = if last_turn == 0.0 {
                    if rng.unit() < 0.5 { 1.0 } else { -1.0 }
                } else if rng.unit() < ALTERNATE_CHANCE {
                    -last_turn.signum()
                } else {
                    last_turn.signum()
                };
                last_turn = side;
                side * size
            };
            Segment {
                length_m,
                gradient_pct,
                curve_deg,
            }
        })
        .collect();
    let profile = Profile::open(segments).renamed("workout");

    // Step starts, and the time table: within each step the travel time
    // over the final profile, scaled to the step's planned time so that
    // every step starts exactly at its workout time.
    let mut step_start_m = Vec::with_capacity(n);
    let total_s = workout.total_s() as f64;
    let seconds = total_s.ceil() as usize;
    let mut table = Vec::with_capacity(seconds + 1);
    let (mut at, mut t0) = (0.0_f64, 0.0_f64);
    let gradient_at = |d: f32| profile.gradient_at(d);
    for k in 0..n {
        step_start_m.push(at as f32);
        let path = travel(at, lengths[k], speed_factor, &gradient_at);
        let measured = path.last().map_or(0.0, |p| p.1);
        let scale = if measured > 0.0 { durations[k] / measured } else { 0.0 };
        let (mut d_prev, mut t_prev) = (at, t0);
        for (d, t) in path {
            let t = t0 + t * scale;
            while table.len() <= seconds && table.len() as f64 <= t {
                let s = table.len() as f64;
                let f = if t > t_prev { (s - t_prev) / (t - t_prev) } else { 1.0 };
                table.push((d_prev + (d - d_prev) * f.clamp(0.0, 1.0)) as f32);
            }
            (d_prev, t_prev) = (d, t);
        }
        at += lengths[k];
        t0 += durations[k];
    }
    table.resize(seconds + 1, at as f32);

    Course {
        profile,
        step_start_m,
        expected_distance: TimeTable { distance_m: table },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workout::{BlockKind, load_path};

    /// Every step boundary is reached within this of its workout time.
    const BOUNDARY_TOLERANCE_S: f64 = 0.5;

    const SAMPLES: [&str; 5] = [
        "hidits-4x12",
        "hit-30-30",
        "lactate-elimination",
        "lit-1h",
        "mit-sweetspot-6x5",
    ];

    fn sample(name: &str) -> Workout {
        load_path(&format!("samples/{name}.json")).unwrap()
    }

    fn step(class: StepClass, duration_s: f32, low_pct: f32, high_pct: f32) -> Step {
        Step {
            name: String::new(),
            duration_s,
            low_pct,
            high_pct,
            class,
            block: 0,
            block_kind: BlockKind::Single,
            rep: 1,
            reps: 1,
            cadence_low: None,
            cadence_high: None,
            fixed_rest: false,
        }
    }

    #[test]
    fn gradient_table() {
        let active = |pct: f32| step_gradient(&step(StepClass::Active, 60.0, pct, pct));
        for (pct, want) in [
            (60.0, 1.0),
            (70.0, 2.5),
            (90.0, 5.5),
            (105.0, 7.5),
            (120.0, 10.0),
            (150.0, 10.0),
            (50.0, 0.0),
            (55.0, 0.0),
        ] {
            assert_eq!(active(pct), want, "{pct} %");
        }
        // The centre of the range counts: 65-75 % → 70 % → 2.5 %.
        assert_eq!(step_gradient(&step(StepClass::Active, 300.0, 65.0, 75.0)), 2.5);
        for class in [StepClass::WarmUp, StepClass::CoolDown] {
            assert_eq!(step_gradient(&step(class, 600.0, 90.0, 100.0)), 0.0);
        }
        assert_eq!(step_gradient(&step(StepClass::Rest, 30.0, 50.0, 55.0)), 0.0);
        assert_eq!(step_gradient(&step(StepClass::Rest, 119.0, 50.0, 55.0)), 0.0);
        assert_eq!(step_gradient(&step(StepClass::Rest, 120.0, 50.0, 55.0)), -3.0);
    }

    /// Rides the course at the scripted speed in 1/60 s steps (as the game
    /// does at normal speed); the time each step start is passed.
    fn ride(course: &Course) -> (Vec<f64>, f64) {
        ride_at(course, 1.0)
    }

    /// Like `ride`, at the scripted speed times `factor`.
    fn ride_at(course: &Course, factor: f64) -> (Vec<f64>, f64) {
        let dt = 1.0 / 60.0;
        let (mut d, mut t) = (0.0_f64, 0.0_f64);
        let mut passed = vec![0.0];
        let end = f64::from(course.length_m());
        for &start in &course.step_start_m[1..] {
            let start = f64::from(start);
            while d < start {
                let v = f64::from(speed_for_gradient(course.profile.gradient_at(d as f32))) * factor;
                if d + v * dt >= start {
                    t += (start - d) / v;
                    d = start;
                } else {
                    d += v * dt;
                    t += dt;
                }
            }
            passed.push(t);
        }
        while d < end {
            let v = f64::from(speed_for_gradient(course.profile.gradient_at(d as f32))) * factor;
            let s = (v * dt).min(end - d);
            d += s;
            t += s / v;
        }
        (passed, t)
    }

    #[test]
    fn the_speed_factor_scales_the_course_and_keeps_every_step_on_time() {
        for name in SAMPLES {
            let w = sample(name);
            let base = generate(&w, 1.0).length_m();
            for factor in [0.8_f32, 1.3] {
                let course = generate(&w, factor);
                let (passed, _) = ride_at(&course, f64::from(factor));
                let mut planned = 0.0_f64;
                for (s, t) in w.steps.iter().zip(&passed) {
                    assert!(
                        (t - planned).abs() <= BOUNDARY_TOLERANCE_S,
                        "{name} x{factor}: {t} vs {planned}"
                    );
                    planned += f64::from(s.duration_s);
                }
                let ratio = f64::from(course.length_m()) / f64::from(base) / f64::from(factor);
                assert!((ratio - 1.0).abs() < 0.01, "{name} x{factor}: ratio {ratio}");
            }
        }
    }

    #[test]
    fn every_sample_generates_on_time() {
        for name in SAMPLES {
            let w = sample(name);
            let course = generate(&w, 1.0);
            assert_eq!((course.profile.name(), course.profile.is_looping()), ("workout", false));
            assert_eq!(course.step_start_m.len(), w.steps.len());
            let (passed, end) = ride(&course);
            let mut planned = 0.0_f64;
            let mut worst = 0.0_f64;
            for (s, t) in w.steps.iter().zip(&passed) {
                worst = worst.max((t - planned).abs());
                planned += f64::from(s.duration_s);
            }
            let total = (end - planned).abs();
            let grads: Vec<f32> = course.profile.segments().iter().map(|s| s.gradient_pct).collect();
            let radius = course
                .profile
                .segments()
                .iter()
                .filter(|s| s.curve_deg != 0.0)
                .map(|s| s.length_m / s.curve_deg.to_radians().abs())
                .fold(f32::INFINITY, f32::min);
            eprintln!(
                "{name}: {:.0} m, {} segments, worst boundary {worst:.3} s, total {total:.3} s, gradient {:.2}..{:.2} %, sharpest radius {radius:.0} m",
                course.length_m(),
                grads.len(),
                grads.iter().copied().fold(f32::INFINITY, f32::min),
                grads.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            );
            assert!(worst <= BOUNDARY_TOLERANCE_S, "{name}: boundary off by {worst} s");
            assert!(total <= 2.0, "{name}: total off by {total} s");
            // The time table agrees with the step starts.
            let mut t = 0.0;
            for (s, &start) in w.steps.iter().zip(&course.step_start_m) {
                assert!((course.expected_distance.at(t) - start).abs() < 0.5, "{name} at {t} s");
                t += f64::from(s.duration_s);
            }
            assert!((course.expected_distance.at(t) - course.length_m()).abs() < 0.5);
            assert!((course.expected_distance.at(t + 100.0) - course.length_m()).abs() < 0.5);
            assert!(
                course.expected_distance.seconds().windows(2).all(|p| p[0] < p[1]),
                "{name}: table not increasing"
            );
        }
    }

    #[test]
    fn same_workout_same_course() {
        for name in SAMPLES {
            assert_eq!(generate(&sample(name), 1.0), generate(&sample(name), 1.0), "{name}");
        }
        // Another name gives another course.
        let mut w = sample("lit-1h");
        let a = generate(&w, 1.0);
        w.name.push('!');
        assert_ne!(generate(&w, 1.0).profile, a.profile);
    }

    #[test]
    fn segments_keep_their_limits() {
        for name in SAMPLES {
            let w = sample(name);
            let course = generate(&w, 1.0);
            let segs = course.profile.segments();
            let (mut straight, mut turns, mut alternations, mut pairs) = (0, 0, 0, 0);
            let mut last = 0.0_f32;
            for s in segs {
                assert!(s.length_m >= MIN_SEGMENT_M - 1e-3, "{name}: {s:?}");
                assert!(
                    (MIN_GRADIENT_PCT..=MAX_GRADIENT_PCT).contains(&s.gradient_pct),
                    "{name}: {s:?}"
                );
                assert!(s.curve_deg.abs() <= MAX_TURN_DEG, "{name}: {s:?}");
                if s.curve_deg == 0.0 {
                    straight += 1;
                } else {
                    turns += 1;
                    assert!(
                        s.length_m / s.curve_deg.to_radians().abs() >= MIN_RADIUS_M,
                        "{name}: {s:?}"
                    );
                    if last != 0.0 {
                        pairs += 1;
                        if last.signum() != s.curve_deg.signum() {
                            alternations += 1;
                        }
                    }
                    last = s.curve_deg;
                }
            }
            let share = straight as f32 / segs.len() as f32;
            assert!((0.2..=0.47).contains(&share), "{name}: straight share {share}");
            assert!(
                turns > 0 && alternations * 2 > pairs,
                "{name}: {alternations} of {pairs}"
            );
            // Chunks per step: 150-500 m, or one chunk under 150 m.
            for (k, s) in w.steps.iter().enumerate() {
                let from = course.step_start_m[k];
                let to = course.step_start_m.get(k + 1).copied().unwrap_or(course.length_m());
                let inside: Vec<f32> = segments_between(&course, from, to).iter().map(|s| s.length_m).collect();
                assert!(!inside.is_empty(), "{name} step {k}");
                if inside.len() > 1 || to - from >= MIN_CHUNK_M {
                    for l in &inside {
                        assert!(
                            (MIN_CHUNK_M - 0.01..=MAX_CHUNK_M + 0.01).contains(l),
                            "{name} step {k} ({}): chunk {l} m",
                            s.duration_s
                        );
                    }
                }
            }
        }
    }

    /// The segments that start in `[from, to)`.
    fn segments_between(course: &Course, from: f32, to: f32) -> Vec<Segment> {
        let mut at = 0.0_f64;
        let mut out = Vec::new();
        for s in course.profile.segments() {
            // Segments are at least 60 m long, so 1 m of slack is unambiguous.
            if at >= f64::from(from) - 1.0 && at < f64::from(to) - 1.0 {
                out.push(*s);
            }
            at += f64::from(s.length_m);
        }
        out
    }

    #[test]
    fn thirty_thirties_climb_on_the_efforts_and_level_out_on_the_rests() {
        let w = sample("hit-30-30");
        let course = generate(&w, 1.0);
        let mut checked = 0;
        for (k, s) in w.steps.iter().enumerate() {
            let from = course.step_start_m[k];
            let to = course.step_start_m.get(k + 1).copied().unwrap_or(course.length_m());
            let grads: Vec<f32> = segments_between(&course, from, to)
                .iter()
                .map(|x| x.gradient_pct)
                .collect();
            if s.name == "Hard" {
                assert!(grads.iter().all(|&g| g == 10.0), "step {k}: {grads:?}");
                checked += 1;
            } else if s.name == "Easy" && s.duration_s == 30.0 {
                assert!(grads.iter().all(|&g| g == 0.0), "step {k}: {grads:?}");
                checked += 1;
            }
        }
        assert_eq!(checked, 2 * (3 + 20 + 20));
    }

    #[test]
    fn long_steps_roll_around_their_gradient_with_zero_mean() {
        let w = sample("lit-1h");
        let course = generate(&w, 1.0);
        assert_eq!(step_gradient(&w.steps[1]), 1.5);
        let long = segments_between(&course, course.step_start_m[1], course.length_m());
        assert!(long.len() > 10);
        assert!(long.iter().all(|s| (0.0..=3.0).contains(&s.gradient_pct)), "{long:?}");
        assert!(long.iter().any(|s| s.gradient_pct != 1.5));
        for name in SAMPLES {
            let w = sample(name);
            let course = generate(&w, 1.0);
            for (k, s) in w
                .steps
                .iter()
                .enumerate()
                .filter(|(_, s)| s.duration_s > ROLLING_ABOVE_S)
            {
                let to = course.step_start_m.get(k + 1).copied().unwrap_or(course.length_m());
                let chunks = segments_between(&course, course.step_start_m[k], to);
                let g = step_gradient(s);
                let len: f32 = chunks.iter().map(|c| c.length_m).sum();
                let mean = chunks.iter().map(|c| c.length_m * (c.gradient_pct - g)).sum::<f32>() / len;
                assert!(mean.abs() < 1e-3, "{name} step {k}: mean offset {mean}");
                assert!(
                    chunks.iter().all(|c| (c.gradient_pct - g).abs() <= ROLLING_PCT + 1e-4),
                    "{name} step {k}"
                );
            }
            // Steps of 300 s or less keep their gradient exactly.
            for (k, s) in w
                .steps
                .iter()
                .enumerate()
                .filter(|(_, s)| s.duration_s <= ROLLING_ABOVE_S)
            {
                let to = course.step_start_m.get(k + 1).copied().unwrap_or(course.length_m());
                let g = step_gradient(s);
                assert!(
                    segments_between(&course, course.step_start_m[k], to)
                        .iter()
                        .all(|c| c.gradient_pct == g),
                    "{name} step {k}"
                );
            }
        }
    }

    #[test]
    fn a_step_too_short_for_a_segment_joins_the_previous_chunk() {
        let w = Workout {
            name: "short".into(),
            date: None,
            source: None,
            steps: vec![
                step(StepClass::WarmUp, 60.0, 50.0, 60.0),
                step(StepClass::Active, 15.0, 130.0, 140.0),
                step(StepClass::Rest, 60.0, 50.0, 55.0),
            ],
        };
        let course = generate(&w, 1.0);
        // 15 s at 10 % would be 43 m: it rides at 0 % inside the warm-up's chunk.
        let segs = course.profile.segments();
        assert!(
            segs.iter()
                .all(|s| s.gradient_pct == 0.0 && s.length_m >= MIN_SEGMENT_M),
            "{segs:?}"
        );
        assert!(
            (course.step_start_m[2] - course.step_start_m[1] - 150.0).abs() < 1.0,
            "{:?}",
            course.step_start_m
        );
        let (passed, _) = ride(&course);
        assert!((passed[2] - 75.0).abs() < 0.5, "{passed:?}");
    }

    #[test]
    fn time_table_interpolates_and_clamps() {
        let table = TimeTable {
            distance_m: vec![0.0, 10.0, 30.0],
        };
        assert_eq!(
            (
                table.at(-1.0),
                table.at(0.0),
                table.at(0.5),
                table.at(1.5),
                table.at(2.0),
                table.at(9.0)
            ),
            (0.0, 0.0, 5.0, 20.0, 30.0, 30.0)
        );
    }
}
