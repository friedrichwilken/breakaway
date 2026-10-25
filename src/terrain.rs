//! Course profile: segments of constant gradient and curvature, blended at
//! the joints, integrated once into elevation, heading and position tables.
//! Pure logic, no rendering types. Distances are horizontal metres along the
//! road.
//!
//! Two modes. A looping profile (`Profile::new`) closes in elevation and
//! repeats; it need not close in space or heading: lap n starts where lap
//! n - 1 ended. An open profile (`Profile::open`) has no closure
//! requirement: before distance 0 and after its last segment the road runs
//! on straight and flat at the start / end elevation and heading.
//!
//! Logical 2D plane: x = east, y = north; heading clockwise from north, so
//! forward = (sin h, cos h) and right = (cos h, -sin h).

use bevy::math::Vec2;
use bevy::prelude::Resource;

use crate::sim::DEFAULT_WORLD_SPEED_MPS;

/// Width (m) of the smoothstep gradient blend, centred on each boundary.
const BLEND_ZONE_M: f32 = 40.0;
/// Maximum allowed net elevation (m) over one loop.
const CLOSURE_TOLERANCE_M: f32 = 0.05;
/// Downhill world speed cap (m/s).
const MAX_DOWNHILL_SPEED_MPS: f32 = 18.0;
/// Tightest allowed bend radius (m).
const MIN_RADIUS_M: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub length_m: f32,
    pub gradient_pct: f32,
    /// Total heading change over the segment, degrees; positive = right.
    pub curve_deg: f32,
}

/// Position and heading of the centre line; the start of a lap.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    pos: Vec2,
    heading: f32,
}

/// Turns a vector given in a frame of heading 0 into the frame of heading `h`
/// (clockwise rotation by `h`).
fn rotate(v: Vec2, h: f32) -> Vec2 {
    let (s, c) = h.sin_cos();
    Vec2::new(v.x * c + v.y * s, -v.x * s + v.y * c)
}

impl Pose {
    const START: Pose = Pose {
        pos: Vec2::ZERO,
        heading: 0.0,
    };

    /// `self` followed by `local` expressed relative to `self`.
    fn then(self, local: Pose) -> Pose {
        Pose {
            pos: self.pos + rotate(local.pos, self.heading),
            heading: self.heading + local.heading,
        }
    }

    /// The pose `p` with `p.then(self) == START`.
    fn inverse(self) -> Pose {
        Pose {
            pos: -rotate(self.pos, -self.heading),
            heading: -self.heading,
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Profile {
    /// Course name (`flat`, `hills`, `demo`, `workout`); `custom` for
    /// `Profile::new` / `Profile::open`.
    name: &'static str,
    /// Looping (`new`) or open (`open`).
    looping: bool,
    segments: Vec<Segment>,
    /// Start distance of each segment.
    starts: Vec<f32>,
    total_m: f32,
    /// Distance of table sample 0: 0 for a loop, `-BLEND_ZONE_M / 2` for an
    /// open profile (its tables also cover the blends into the flat
    /// extensions).
    origin_m: f32,
    /// Spacing (m) of `elevation` samples; 1.0 for whole-metre loops and
    /// for open profiles.
    step_m: f32,
    /// Elevation at `origin_m + i * step_m`; a loop's from 0 to `total_m`
    /// inclusive.
    elevation: Vec<f32>,
    /// Heading (rad) at `origin_m + i * step_m` relative to distance 0.
    heading: Vec<f32>,
    /// Centre-line position at `origin_m + i * step_m` relative to distance 0.
    position: Vec<Vec2>,
    /// Pose at the end of the lap, relative to its start (loops only).
    lap_end: Pose,
}

/// Stands for the road before and after an open profile: flat, straight.
const FLAT_STRAIGHT: Segment = Segment {
    length_m: 1.0,
    gradient_pct: 0.0,
    curve_deg: 0.0,
};

/// A per-segment value at a distance, smoothstep-blended over 40 m around
/// every segment boundary. Looping: `distance_m` is folded into the loop and
/// the last segment blends into the first. Open: before 0 and after the end
/// the value is that of a flat straight road, blended like a segment.
fn blend_segments(
    segments: &[Segment],
    starts: &[f32],
    total_m: f32,
    looping: bool,
    distance_m: f32,
    value: impl Fn(&Segment) -> f32,
) -> f32 {
    let d = if looping {
        let d = distance_m.rem_euclid(total_m);
        if d >= total_m { 0.0 } else { d }
    } else {
        distance_m
    };
    let i = starts.partition_point(|&s| s <= d).saturating_sub(1);
    let n = segments.len();
    let half = BLEND_ZONE_M / 2.0;
    let start = starts[i];
    let end = start + segments[i].length_m;
    let g = value(&segments[i]);
    if d - start < half {
        let prev = if looping || i > 0 {
            &segments[(i + n - 1) % n]
        } else {
            &FLAT_STRAIGHT
        };
        let prev = value(prev);
        prev + (g - prev) * smoothstep((d - start + half) / BLEND_ZONE_M)
    } else if end - d < half {
        let next = if looping || i + 1 < n {
            &segments[(i + 1) % n]
        } else {
            &FLAT_STRAIGHT
        };
        let next = value(next);
        g + (next - g) * smoothstep((d - (end - half)) / BLEND_ZONE_M)
    } else {
        g
    }
}

/// Gradient (percent) of an open profile made of `segments` (starting at
/// the distances `starts`) at a distance, exactly as `Profile::open` would
/// give it, without building its tables.
pub fn open_gradient_at(segments: &[Segment], starts: &[f32], distance_m: f32) -> f32 {
    let total_m = starts
        .last()
        .zip(segments.last())
        .map_or(0.0, |(s, seg)| s + seg.length_m);
    blend_segments(segments, starts, total_m, false, distance_m, |s| s.gradient_pct)
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Profile {
    /// A looping profile. Panics on an empty profile, a segment shorter
    /// than the 40 m blend zone, a bend tighter than 40 m radius, or a loop
    /// whose net elevation is more than 0.05 m off zero.
    pub fn new(segments: Vec<Segment>) -> Self {
        let profile = Self::build(segments, true);
        let net = profile.elevation[profile.elevation.len() - 1];
        assert!(
            net.abs() <= CLOSURE_TOLERANCE_M,
            "profile does not close: net elevation {net} m"
        );
        profile
    }

    /// An open (non-looping) profile: no closure requirement; before
    /// distance 0 and after the last segment the road continues straight and
    /// flat. Panics on an empty profile, a segment shorter than the 40 m
    /// blend zone or a bend tighter than 40 m radius.
    pub fn open(segments: Vec<Segment>) -> Self {
        Self::build(segments, false)
    }

    fn build(segments: Vec<Segment>, looping: bool) -> Self {
        assert!(!segments.is_empty(), "profile needs at least one segment");
        for s in &segments {
            assert!(
                s.length_m >= BLEND_ZONE_M,
                "segment of {} m is shorter than the {BLEND_ZONE_M} m blend zone",
                s.length_m
            );
            let radius = s.length_m / s.curve_deg.to_radians().abs();
            assert!(
                radius >= MIN_RADIUS_M,
                "segment of {} m turning {} deg has radius {radius} m, below {MIN_RADIUS_M} m",
                s.length_m,
                s.curve_deg
            );
        }
        let mut starts = Vec::with_capacity(segments.len());
        let mut total_m = 0.0;
        for s in &segments {
            starts.push(total_m);
            total_m += s.length_m;
        }
        // A loop: samples from 0 to the end, spaced to hit the end exactly.
        // Open: whole metres from half a blend zone before 0 to at least
        // half a blend zone past the end, so the tables cover every blend.
        let half = BLEND_ZONE_M / 2.0;
        let (origin_m, steps, step_m) = if looping {
            let steps = total_m.ceil() as usize;
            (0.0, steps, total_m / steps as f32)
        } else {
            (-half, (total_m + 2.0 * half).ceil() as usize, 1.0)
        };
        // Index of distance 0 in the tables.
        let zero = (-origin_m / step_m).round() as usize;

        let mut profile = Self {
            name: "custom",
            looping,
            segments,
            starts,
            total_m,
            origin_m,
            step_m,
            elevation: Vec::new(),
            heading: Vec::new(),
            position: Vec::new(),
            lap_end: Pose::START,
        };
        let at = |i: usize| origin_m + i as f32 * step_m;
        // Trapezoid integration of gradient / 100 at 1 m steps.
        let mut elevation = Vec::with_capacity(steps + 1);
        elevation.push(0.0f32);
        let mut prev_g = profile.gradient_at(at(0));
        for i in 1..=steps {
            let g = profile.gradient_at(at(i));
            let last = elevation[i - 1];
            elevation.push(last + (prev_g + g) / 200.0 * step_m);
            prev_g = g;
        }
        // Elevation 0 at distance 0.
        let base = elevation[zero];
        if base != 0.0 {
            for e in &mut elevation {
                *e -= base;
            }
        }
        profile.elevation = elevation;

        // Heading: trapezoid integration of curvature; position: trapezoid
        // integration of the forward vector. In f64 to keep the lap-end pose
        // (composed once per lap) accurate.
        let step = f64::from(step_m);
        let mut heading = Vec::with_capacity(steps + 1);
        let mut position = Vec::with_capacity(steps + 1);
        let (mut h, mut x, mut y) = (0.0f64, 0.0f64, 0.0f64);
        heading.push(0.0f64);
        position.push((0.0f64, 0.0f64));
        let mut prev_k = f64::from(profile.curvature_at(at(0)));
        for i in 1..=steps {
            let k = f64::from(profile.curvature_at(at(i)));
            let h_next = h + (prev_k + k) / 2.0 * step;
            x += (h.sin() + h_next.sin()) / 2.0 * step;
            y += (h.cos() + h_next.cos()) / 2.0 * step;
            h = h_next;
            prev_k = k;
            heading.push(h);
            position.push((x, y));
        }
        // Heading 0 and position (0, 0) at distance 0.
        let (h0, (x0, y0)) = (heading[zero], position[zero]);
        let (s0, c0) = h0.sin_cos();
        let rebase = |(px, py): (f64, f64)| {
            let (dx, dy) = (px - x0, py - y0);
            // Clockwise rotation by -h0.
            Vec2::new((dx * c0 - dy * s0) as f32, (dx * s0 + dy * c0) as f32)
        };
        let end = *position.last().unwrap();
        profile.lap_end = Pose {
            pos: rebase(end),
            heading: (h - h0) as f32,
        };
        profile.heading = heading.iter().map(|hh| (hh - h0) as f32).collect();
        profile.position = position.into_iter().map(rebase).collect();
        profile
    }

    /// The 2000 m demo loop: the `hills` gradients with bends, a hairpin on
    /// the steep kicker. Net heading change per lap is +90 degrees.
    pub fn demo() -> Self {
        let seg = |length_m, gradient_pct, curve_deg| Segment {
            length_m,
            gradient_pct,
            curve_deg,
        };
        Self::named(
            "demo",
            vec![
                seg(200.0, 0.0, 0.0),
                seg(200.0, 6.0, 0.0),
                seg(200.0, 6.0, 90.0),
                seg(100.0, 0.0, 0.0),
                seg(150.0, -4.0, -120.0),
                seg(150.0, -4.0, 0.0),
                seg(150.0, -4.0, 120.0),
                seg(150.0, -4.0, 0.0),
                seg(200.0, 0.0, -90.0),
                seg(150.0, 10.0, 180.0),
                seg(50.0, 0.0, 0.0),
                seg(300.0, -5.0, -90.0),
            ],
        )
    }

    /// The straight 2000 m net-zero loop of task 004: climb, descent, short
    /// steep kicker.
    pub fn hills() -> Self {
        let seg = |length_m, gradient_pct| Segment {
            length_m,
            gradient_pct,
            curve_deg: 0.0,
        };
        Self::named(
            "hills",
            vec![
                seg(200.0, 0.0),
                seg(400.0, 6.0),
                seg(100.0, 0.0),
                seg(600.0, -4.0),
                seg(200.0, 0.0),
                seg(150.0, 10.0),
                seg(50.0, 0.0),
                seg(300.0, -5.0),
            ],
        )
    }

    /// A 2000 m loop at 0 %: the clean flat reference course.
    pub fn flat() -> Self {
        Self::named(
            "flat",
            vec![Segment {
                length_m: 2000.0,
                gradient_pct: 0.0,
                curve_deg: 0.0,
            }],
        )
    }

    /// A built-in course by name (`flat`, `hills`, `demo`).
    pub fn by_name(name: &str) -> Option<Profile> {
        match name {
            "flat" => Some(Self::flat()),
            "hills" => Some(Self::hills()),
            "demo" => Some(Self::demo()),
            _ => None,
        }
    }

    fn named(name: &'static str, segments: Vec<Segment>) -> Self {
        Self {
            name,
            ..Self::new(segments)
        }
    }

    /// The same course under another name.
    pub fn renamed(self, name: &'static str) -> Self {
        Self { name, ..self }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    /// One lap of a looping profile, or the whole of an open one (without
    /// the flat extensions).
    pub fn total_length_m(&self) -> f32 {
        self.total_m
    }

    // Profile API; so far only tests ask.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_looping(&self) -> bool {
        self.looping
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// `distance_m` folded into `[0, total)`; any finite value loops.
    fn wrap(&self, distance_m: f32) -> f32 {
        let d = distance_m.rem_euclid(self.total_m);
        if d >= self.total_m { 0.0 } else { d }
    }

    /// A per-segment value at a distance, smoothstep-blended over 40 m
    /// around every segment boundary, including the wrap from last to first
    /// (looping) or into the flat extensions (open).
    fn blended(&self, distance_m: f32, value: impl Fn(&Segment) -> f32) -> f32 {
        blend_segments(
            &self.segments,
            &self.starts,
            self.total_m,
            self.looping,
            distance_m,
            value,
        )
    }

    /// Gradient (percent) at a distance, smoothstep-blended over 40 m around
    /// every segment boundary, including the wrap from last to first (or
    /// into the flat extensions of an open profile).
    pub fn gradient_at(&self, distance_m: f32) -> f32 {
        self.blended(distance_m, |s| s.gradient_pct)
    }

    /// Curvature (rad per metre, positive = right) at a distance: constant
    /// within a segment, blended like the gradient. The blend is symmetric,
    /// so each segment keeps its total heading change.
    pub fn curvature_at(&self, distance_m: f32) -> f32 {
        self.blended(distance_m, |s| s.curve_deg.to_radians() / s.length_m)
    }

    /// Lap index of a distance and the distance within that lap, in
    /// `[0, total)`; lap 0 starts at distance 0.
    fn lap_of(&self, distance_m: f32) -> (i64, f32) {
        let (d, total) = (f64::from(distance_m), f64::from(self.total_m));
        let mut lap = (d / total).floor() as i64;
        let mut local = d - lap as f64 * total;
        if local < 0.0 {
            lap -= 1;
            local += total;
        } else if local >= total {
            lap += 1;
            local -= total;
        }
        (lap, (local as f32).min(self.total_m))
    }

    /// Start pose of lap `lap`: the end-of-lap pose composed `lap` times
    /// (its inverse for negative laps).
    fn lap_start(&self, lap: i64) -> Pose {
        let step = if lap >= 0 { self.lap_end } else { self.lap_end.inverse() };
        let mut pose = Pose::START;
        for _ in 0..lap.unsigned_abs() {
            pose = pose.then(step);
        }
        pose
    }

    /// Table index and fraction for an in-lap distance (looping) or any
    /// distance (open; clamped to the tables, past which the road is flat
    /// and straight).
    fn table_pos(&self, local_m: f32) -> (usize, f32) {
        let x = ((local_m - self.origin_m) / self.step_m).max(0.0);
        let i = (x.floor() as usize).min(self.elevation.len() - 2);
        (i, (x - i as f32).min(1.0))
    }

    /// Distances of the first and last table samples.
    fn table_range(&self) -> (f32, f32) {
        (
            self.origin_m,
            self.origin_m + (self.elevation.len() - 1) as f32 * self.step_m,
        )
    }

    /// Heading (rad, clockwise from north) at a distance; not wrapped to a
    /// range, so it keeps counting over laps. 0 at distance 0.
    pub fn heading_at(&self, distance_m: f32) -> f32 {
        if !self.looping {
            let (i, f) = self.table_pos(distance_m);
            return self.heading[i] + (self.heading[i + 1] - self.heading[i]) * f;
        }
        let (lap, local) = self.lap_of(distance_m);
        let (i, f) = self.table_pos(local);
        let h = self.heading[i] + (self.heading[i + 1] - self.heading[i]) * f;
        self.lap_start(lap).heading + h
    }

    /// Logical position of the centre line at a distance; (0, 0) at 0.
    pub fn centre_at(&self, distance_m: f32) -> Vec2 {
        if !self.looping {
            // Past the tables the road runs straight on.
            let (first, last) = self.table_range();
            let along = |p: Vec2, h: f32, by: f32| p + Vec2::new(h.sin(), h.cos()) * by;
            return if distance_m < first {
                along(self.position[0], self.heading[0], distance_m - first)
            } else if distance_m > last {
                let n = self.position.len() - 1;
                along(self.position[n], self.heading[n], distance_m - last)
            } else {
                let (i, f) = self.table_pos(distance_m);
                self.position[i].lerp(self.position[i + 1], f)
            };
        }
        let (lap, local) = self.lap_of(distance_m);
        let (i, f) = self.table_pos(local);
        let p = self.position[i].lerp(self.position[i + 1], f);
        self.lap_start(lap).then(Pose { pos: p, heading: 0.0 }).pos
    }

    /// Unit vector to the right of the direction of travel at a distance.
    pub fn right_at(&self, distance_m: f32) -> Vec2 {
        let (s, c) = self.heading_at(distance_m).sin_cos();
        Vec2::new(c, -s)
    }

    /// Elevation (m) at a distance, linearly interpolated in the table.
    /// Elevation at distance 0 is 0.
    pub fn elevation_at(&self, distance_m: f32) -> f32 {
        let d = if self.looping {
            self.wrap(distance_m)
        } else {
            distance_m
        };
        let (i, f) = self.table_pos(d);
        self.elevation[i] + (self.elevation[i + 1] - self.elevation[i]) * f
    }
}

/// Scripted world speed (m/s) for a gradient: 10 m/s on the flat, slower
/// uphill, faster downhill up to 18 m/s. Continuous at 0; NaN → flat.
pub fn speed_for_gradient(gradient_pct: f32) -> f32 {
    if gradient_pct.is_nan() {
        DEFAULT_WORLD_SPEED_MPS
    } else if gradient_pct > 0.0 {
        DEFAULT_WORLD_SPEED_MPS / (1.0 + 0.25 * gradient_pct)
    } else {
        (DEFAULT_WORLD_SPEED_MPS + 1.2 * gradient_pct.abs()).min(MAX_DOWNHILL_SPEED_MPS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    /// `demo` and `hills` share the gradient sequence, so every elevation
    /// and gradient test runs on both.
    fn graded() -> [Profile; 2] {
        [Profile::demo(), Profile::hills()]
    }

    #[test]
    fn demo_elevation_closes_and_tops_out() {
        for p in graded() {
            assert_eq!(p.total_length_m(), 2000.0);
            assert_eq!(p.elevation_at(0.0), 0.0);
            assert!(close(p.elevation_at(2000.0), 0.0, 1e-6));
            // Task 004 expected 24.0 ± 0.1, but its own definitions give less:
            // the wrap blend (-5 % → 0 %) is centred on distance 0, so its second
            // half (0..20 m) still descends 0.05 * 40 * 3/32 = 0.1875 m, and
            // elevation is pinned to 0 at distance 0. Hence 24 - 0.1875.
            let top = p.elevation_at(620.0);
            assert!(close(top, 23.8125, 0.01), "elevation at 620: {top}");
        }
    }

    #[test]
    fn by_name_knows_the_three_courses() {
        for name in ["flat", "hills", "demo"] {
            assert_eq!(
                Profile::by_name(name).map(|p| p.name().to_owned()),
                Some(name.to_owned())
            );
        }
        assert!(Profile::by_name("nope").is_none());
        assert_eq!(Profile::new(vec![seg(100.0, 0.0, 0.0)]).name(), "custom");
    }

    #[test]
    fn flat_profile_is_level() {
        let p = Profile::flat();
        assert_eq!(p.total_length_m(), 2000.0);
        for d in [0.0, 10.0, 999.0, 1990.0, 2500.0] {
            assert_eq!(p.gradient_at(d), 0.0);
            assert_eq!(p.elevation_at(d), 0.0);
        }
    }

    #[test]
    fn demo_gradients() {
        for p in graded() {
            assert_eq!(p.gradient_at(400.0), 6.0);
            assert_eq!(p.gradient_at(1000.0), -4.0);
            assert_eq!(p.gradient_at(100.0), 0.0);
        }
    }

    #[test]
    fn elevation_changes_at_most_max_gradient_per_metre() {
        for p in graded() {
            for d in 0..2000 {
                let d = d as f32;
                let delta = (p.elevation_at(d + 1.0) - p.elevation_at(d)).abs();
                assert!(delta <= 0.101, "at {d}: {delta}");
            }
        }
    }

    #[test]
    fn profile_loops() {
        for p in graded() {
            for d in [0.0, 37.0, 450.0, 1234.0, 1999.0] {
                assert!(close(p.elevation_at(d), p.elevation_at(d + 2000.0), 1e-4), "d {d}");
                assert!(close(p.gradient_at(d), p.gradient_at(d + 2000.0), 1e-4), "d {d}");
            }
            assert!(close(p.elevation_at(-250.0), p.elevation_at(1750.0), 1e-4));
            assert!(close(p.gradient_at(-250.0), p.gradient_at(1750.0), 1e-4));
        }
    }

    #[test]
    #[should_panic(expected = "does not close")]
    fn new_panics_on_non_closing_profile() {
        Profile::new(vec![
            Segment {
                length_m: 100.0,
                gradient_pct: 5.0,
                curve_deg: 0.0,
            },
            Segment {
                length_m: 100.0,
                gradient_pct: 0.0,
                curve_deg: 0.0,
            },
        ]);
    }

    #[test]
    #[should_panic(expected = "shorter than")]
    fn new_panics_on_short_segment() {
        Profile::new(vec![
            Segment {
                length_m: 100.0,
                gradient_pct: 0.0,
                curve_deg: 0.0,
            },
            Segment {
                length_m: 30.0,
                gradient_pct: 0.0,
                curve_deg: 0.0,
            },
        ]);
    }

    #[test]
    fn speed_curve() {
        assert_eq!(speed_for_gradient(0.0), 10.0);
        assert!(close(speed_for_gradient(6.0), 4.0, 1e-5));
        assert!(close(speed_for_gradient(10.0), 2.857, 1e-3));
        assert!(close(speed_for_gradient(-4.0), 14.8, 1e-5));
        assert_eq!(speed_for_gradient(-20.0), 18.0);
        assert_eq!(speed_for_gradient(f32::NAN), 10.0);
        let mut prev = speed_for_gradient(-6.0);
        let mut g = -6.0 + 0.1;
        while g <= 12.0 {
            let s = speed_for_gradient(g);
            assert!(s < prev, "not decreasing at {g}");
            prev = s;
            g += 0.1;
        }
    }

    // ── Bends ────────────────────────────────────────────────────────────

    fn seg(length_m: f32, gradient_pct: f32, curve_deg: f32) -> Segment {
        Segment {
            length_m,
            gradient_pct,
            curve_deg,
        }
    }

    fn heading_deg(p: &Profile, d: f32) -> f32 {
        p.heading_at(d).to_degrees()
    }

    #[test]
    fn straight_profile_runs_north() {
        for p in [Profile::flat(), Profile::hills()] {
            for d in (0..=2000).map(|d| d as f32).chain([5000.0]) {
                let c = p.centre_at(d);
                assert!((c - Vec2::new(0.0, d)).length() <= 1e-3, "d {d}: {c}");
                assert_eq!(p.heading_at(d), 0.0);
            }
        }
    }

    /// Residual heading (deg) at the lap start from the wrap blend: the
    /// last segment's curvature (-90 deg over 300 m) still acts over 0..20 m,
    /// where the smoothstep leaves 3/32 of the 40 m zone, and heading is
    /// pinned to 0 at distance 0 (same effect as the 0.1875 m elevation
    /// offset above). The task's expected values ignore it.
    const WRAP_RESIDUAL_DEG: f32 = -90.0 / 300.0 * 40.0 * 3.0 / 32.0;

    #[test]
    fn demo_headings() {
        let p = Profile::demo();
        assert!(close(WRAP_RESIDUAL_DEG, -1.125, 1e-6));
        for (d, want) in [
            (650.0, 90.0),
            (925.0, -30.0),
            (1225.0, 90.0),
            (1675.0, 180.0),
            (2100.0, 90.0),
        ] {
            let want = want + WRAP_RESIDUAL_DEG;
            let got = heading_deg(&p, d);
            assert!(close(got, want, 0.5), "heading at {d}: {got}, want {want}");
        }
        for d in [100.0, 650.0, 1675.0] {
            let turn = heading_deg(&p, d + 2000.0) - heading_deg(&p, d);
            assert!(close(turn, 90.0, 0.5), "lap turn at {d}: {turn}");
        }
    }

    #[test]
    fn centre_moves_at_unit_speed() {
        let p = Profile::demo();
        for d in 0..4000 {
            let d = d as f32;
            let step = (p.centre_at(d + 1.0) - p.centre_at(d)).length();
            assert!(close(step, 1.0, 0.002), "at {d}: {step}");
        }
    }

    #[test]
    fn right_turn_goes_east() {
        let p = Profile::new(vec![seg(100.0, 0.0, 0.0), seg(200.0, 0.0, 90.0), seg(100.0, 0.0, 0.0)]);
        let end = p.total_length_m();
        assert!(p.centre_at(end).x > 100.0, "end {}", p.centre_at(end));
        assert!(close(heading_deg(&p, end), 90.0, 0.5));
    }

    #[test]
    fn right_is_unit_and_perpendicular() {
        let p = Profile::demo();
        for d in (-500..4500).step_by(7) {
            let d = d as f32;
            let r = p.right_at(d);
            let h = p.heading_at(d);
            let fwd = Vec2::new(h.sin(), h.cos());
            assert!(close(r.length(), 1.0, 1e-5), "d {d}");
            assert!(r.dot(fwd).abs() < 1e-5, "d {d}");
        }
    }

    #[test]
    #[should_panic(expected = "radius")]
    fn new_panics_on_tight_bend() {
        Profile::new(vec![seg(100.0, 0.0, 0.0), seg(100.0, 0.0, 180.0)]);
    }

    #[test]
    fn lap_seam_is_continuous() {
        let p = Profile::demo();
        let gap = (p.centre_at(2000.5) - p.centre_at(1999.5)).length();
        assert!(close(gap, 1.0, 0.01), "gap {gap}");
        // Negative distances run the course backwards from the start.
        let back = (p.centre_at(0.5) - p.centre_at(-0.5)).length();
        assert!(close(back, 1.0, 0.01), "back {back}");
        assert!(close(
            p.heading_at(-1000.0) + p.lap_end.heading,
            p.heading_at(1000.0),
            1e-4
        ));
    }
    // ── Open profiles ────────────────────────────────────────────────────

    fn climb_and_turn() -> Profile {
        Profile::open(vec![seg(200.0, 5.0, 0.0), seg(300.0, 0.0, 90.0), seg(100.0, -3.0, 0.0)])
    }

    #[test]
    fn open_profile_needs_no_closure_and_does_not_wrap() {
        let p = Profile::open(vec![seg(200.0, 5.0, 0.0)]);
        assert!(!p.is_looping() && Profile::demo().is_looping());
        assert_eq!((p.total_length_m(), p.name()), (200.0, "custom"));
        assert_eq!(p.elevation_at(0.0), 0.0);
        // The blend into the flat lead-in (3/32 of the 40 m zone at 5 %)
        // lies before 0, the blend out after the end.
        let lead = 0.05 * 40.0 * 3.0 / 32.0;
        assert!(close(p.elevation_at(-100.0), -lead, 1e-3), "{}", p.elevation_at(-100.0));
        assert!(
            close(p.elevation_at(200.0), 10.0 - 2.0 * lead, 1e-3),
            "{}",
            p.elevation_at(200.0)
        );
        assert!(
            close(p.elevation_at(5000.0), 10.0 - lead, 1e-3),
            "{}",
            p.elevation_at(5000.0)
        );
        assert!(close(p.gradient_at(100.0), 5.0, 1e-6));
        assert!(close(p.gradient_at(0.0), 2.5, 1e-4) && close(p.gradient_at(200.0), 2.5, 1e-4));
        for d in [-1000.0, -20.0, 220.0, 300.0, 1e5] {
            assert_eq!(p.gradient_at(d), 0.0, "d {d}");
        }
        assert!(p.gradient_at(-10.0) > 0.0 && p.gradient_at(210.0) > 0.0);
    }

    #[test]
    fn open_profile_runs_straight_and_flat_past_its_ends() {
        let p = climb_and_turn();
        let end = p.total_length_m();
        assert_eq!(end, 600.0);
        assert!(close(p.heading_at(0.0), 0.0, 1e-6) && p.centre_at(0.0).length() < 1e-4);
        assert!(
            close(heading_deg(&p, end + 50.0), 90.0, 0.01),
            "{}",
            heading_deg(&p, end + 50.0)
        );
        for (a, b) in [(end + 100.0, end + 1100.0), (-1100.0, -100.0)] {
            assert_eq!(p.heading_at(a), p.heading_at(b));
            assert_eq!(p.elevation_at(a), p.elevation_at(b));
            let h = p.heading_at(a);
            let moved = p.centre_at(b) - p.centre_at(a);
            assert!(
                (moved - Vec2::new(h.sin(), h.cos()) * 1000.0).length() < 0.01,
                "{a}: {moved}"
            );
        }
        // Before the start it runs north (heading 0), after the end east.
        assert!(close(p.centre_at(-100.0).x, 0.0, 1e-3) && close(p.centre_at(-100.0).y, -100.0, 1e-3));
        for d in -200..800 {
            let d = d as f32;
            let step = (p.centre_at(d + 1.0) - p.centre_at(d)).length();
            assert!(close(step, 1.0, 0.002), "at {d}: {step}");
        }
    }

    #[test]
    fn open_profile_blends_between_segments_like_a_loop() {
        let p = climb_and_turn();
        // Boundary 200 m: 5 % → 0 %, halfway at the joint.
        assert!(close(p.gradient_at(200.0), 2.5, 1e-4));
        assert!(close(p.gradient_at(180.0), 5.0, 1e-4) && close(p.gradient_at(220.0), 0.0, 1e-4));
        // The same course as a loop's interior (away from the wrap).
        let looped = Profile::new(vec![seg(200.0, 5.0, 0.0), seg(300.0, 0.0, 90.0), seg(200.0, -5.0, 0.0)]);
        for d in [150.0, 190.0, 200.0, 210.0, 400.0] {
            assert!(close(p.gradient_at(d), looped.gradient_at(d), 1e-6), "d {d}");
            assert!(close(p.curvature_at(d), looped.curvature_at(d), 1e-9), "d {d}");
        }
        // The elevation changes by at most the gradient per metre.
        for d in -50..700 {
            let d = d as f32;
            assert!((p.elevation_at(d + 1.0) - p.elevation_at(d)).abs() <= 0.0501, "d {d}");
        }
    }
}
