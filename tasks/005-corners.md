# 005 — Corners (combined with climbs and descents)

**Repo:** `~/claude/breakaway/`, branch `corners`, `--no-ff` merge to
`main`.

**Depends on:** 004 (merged). Read `CLAUDE.md`, then `src/terrain.rs`,
`src/sim.rs`, `src/render.rs`, `src/debug_api.rs` in full before starting.

## Goal

The road bends left and right, on the flat, on climbs and on descents. The
game logic stays in track coordinates (distance along the road, lateral
offset from the centre line); only the course description and the
rendering learn about bends.

## Coordinate convention (logical 2D, unchanged axes)

x = east, y = north. Heading `h` in radians, clockwise from north.
Forward = `(sin h, cos h)`, right = `(cos h, −sin h)`. At distance 0 the
centre line is at `(0, 0)` with heading 0. A positive curve turns right.
On a straight course `centre_at(d) == (0, d)`, exactly today's layout.
Render mapping stays `to_render((x, y), height) = (x, height, −y)`.

## `terrain.rs`

`Segment` gains `curve_deg: f32`: total heading change over the segment
(positive = right). Constant curvature within the segment
(`curve_deg / length_m`), blended at every boundary (including the lap
wrap) over the same 40 m smoothstep zone as the gradient, so total heading
change is preserved.

New on `Profile`:

```rust
pub fn curvature_at(&self, distance_m: f32) -> f32;  // rad per metre
pub fn heading_at(&self, distance_m: f32) -> f32;    // rad, not wrapped to a range
pub fn centre_at(&self, distance_m: f32) -> Vec2;    // logical position of the centre line
pub fn right_at(&self, distance_m: f32) -> Vec2;     // unit vector
```

- Precompute, for one lap at 1 m steps, heading and position relative to
  the lap start (trapezoidal integration, like elevation), plus the pose
  at the end of the lap.
- **The course does not have to close in space or heading.** Lap `n`
  starts where lap `n − 1` ended: for a distance in lap `n`, compose the
  end-of-lap pose `n` times (a plain loop is fine), then apply the in-lap
  table. Negative distances: treat as lap −1 by composing the inverse
  pose. Elevation keeps looping as today.
- `new` panics if any segment's radius (`length_m / |curve in rad|`) is
  below 40 m.
- `bevy::math::Vec2` may be used here; no rendering types.

Profiles and CLI `--profile`:

- `flat`: as today.
- `hills`: today's `demo` (all `curve_deg` 0), kept for regression.
- `demo` (default), 2000 m, same gradient sequence as `hills`:

  | # | length m | gradient % | curve ° |
  |---|---|---|---|
  | 1 | 200 | 0 | 0 |
  | 2 | 200 | +6 | 0 |
  | 3 | 200 | +6 | +90 |
  | 4 | 100 | 0 | 0 |
  | 5 | 150 | −4 | −120 |
  | 6 | 150 | −4 | 0 |
  | 7 | 150 | −4 | +120 |
  | 8 | 150 | −4 | 0 |
  | 9 | 200 | 0 | −90 |
  | 10 | 150 | +10 | +180 |
  | 11 | 50 | 0 | 0 |
  | 12 | 300 | −5 | −90 |

Unit tests (all existing elevation/gradient tests must still pass
unchanged, on `demo` and on `hills`):

- straight profile: `centre_at(d) == (0, d)` within 1e-3 for d in
  0..=2000 and for d = 5000; heading 0.
- `demo` heading in degrees, ± 0.5: 90 at 650; −30 at 925; 90 at 1225;
  180 at 1675; 90 at 2100 (second lap); and
  `heading_at(d + 2000) − heading_at(d)` is 90° for d = 100, 650, 1675.
- unit speed: `|centre_at(d + 1) − centre_at(d)|` within 1 ± 0.002 for
  every whole metre of 0..4000 (covers the lap seam).
- right turn goes east: a profile `[100 m straight, 200 m with +90°,
  100 m straight]` (zero gradient) ends with `centre.x > 100` and
  heading 90°.
- `right_at` is unit length and perpendicular to forward.
- `new` panics for a 100 m segment with 180° (radius 31.8 m).
- lap seam continuity: `centre_at(1999.5)` and `centre_at(2000.5)` are
  1 m apart ± 0.01.

## `sim.rs`

`RaceState` keeps `distance_m` and `lateral_m` as the truth and gains
`heading_deg: f32` (from `profile.heading_at`, for the debug API). Speed
is unchanged (gradient only; no corner slowing). No other change.

## `render.rs`

### Road, lines and shoulder: generated strips (replaces the recycled tiles)

Rectangular tiles leave wedge gaps in bends, so the road becomes generated
mesh strips. Remove `RoadTile`, `tile_transform`, `recycle_road` and the
per-tile ground quad.

- Cross-sections every 3 m of distance over a window from
  `window_start` (snap to 3 m, one step behind the camera) to 720 m ahead.
  At each cross-section: `c = centre_at(d)`, `r = right_at(d)`,
  `y = render_height(d)`; a point at lateral offset `o` is
  `to_render(c + r * o, y + layer)`.
- Strips (triangle lists between consecutive cross-sections):
  - shoulder: offsets −30..+30, layer `GROUND_Y`, grass colour
  - road: −3..+3, layer `ROAD_Y`
  - two edge lines, same positions and width as today, layer `LINE_Y`
  - centre dashes: 0.15 m wide, present where `d mod 12 < 6` (absolute
    distance, so they do not crawl), layer `LINE_Y`
- One mesh per colour or one mesh with vertex colours: implementer's
  choice. Rebuild only when the 3 m window start changes, by updating the
  existing mesh asset(s). Mark the strip entities `NoFrustumCulling`
  (their bounds change every rebuild).
- A pure function builds the cross-section list
  (`fn cross_sections(profile, start_d) -> Vec<CrossSection>`) so it is
  unit-testable.

### Ground beyond the shoulder

- **Base plane:** one flat grass quad 8000 m × 8000 m following the
  camera's x/z, at height `min over the lap of render_height − 5`
  (compute once at startup).
- **Backdrop becomes a ring** so it is present in every direction and
  pans when the road turns: far land = a vertical cylinder wall of radius
  3000 m from eye height down 600 m (grass); mountains = the existing
  outline repeated around a ring of radius 3001 m (48 peaks: the existing
  24 heights twice). Both follow the camera's position, never its
  rotation; both visible from inside (no culling).

### Posts, riders, camera

- Posts: pair at distance `d` stands at `centre_at(d) ± right_at(d) *
  POST_OFFSET_M`, upright, at `render_height(d)`.
- Riders: player at `centre_at(d) + right_at(d) * lateral_m`; partner on
  the centre line 2 m ahead (its own distance). Billboards as today.
- Camera: eye above the centre line at distance `d − CAM_BEHIND_M`
  (still ignores lateral), looking at the centre line at
  `d + CAM_LOOK_AHEAD_M`; heights exactly as in 004 revision 1.

Render unit tests: every cross-section's road edge points are 6.0 m apart
(± 1e-3) and centred on `centre_at`; consecutive cross-sections' centres
are 3 m apart (± 0.01) in the horizontal plane; dash presence follows
`d mod 12 < 6`.

## `debug_api.rs`

`GET /state` adds `heading_deg`, `x_m`, `y_m` (centre-line position at
the rider's distance). Update the table in `CLAUDE.md`, and document the
three `--profile` values there.

## Acceptance

- `just check`, `just test`, `just clippy` clean.
- Run in the background; screenshots at time multiplier 0.01, judged by
  eye after reading each file:
  1. `--profile flat`, teleport 400 → `/tmp/breakaway-c-flat.png`: looks
     like 004's flat reference (straight road to the horizon, mountains on
     it).
  2. `--profile hills`, teleport 400 and 1000 → `/tmp/breakaway-c-hills-climb.png`,
     `-descent.png`: look like 004's climb and descent.
  3. default profile:
     - teleport 60 → `/tmp/breakaway-c-start.png`: straight flat road, the
       climb ahead.
     - teleport 480 → `/tmp/breakaway-c-climb-right.png`: road rises and
       bends to the right.
     - teleport 740 → `/tmp/breakaway-c-descent-left.png`: road falls
       away and bends left.
     - teleport 1540 → `/tmp/breakaway-c-hairpin.png`: tight right-hand
       bend on a steep climb.
     - teleport 1675 → `/tmp/breakaway-c-after.png`: `heading_deg` ≈ 180;
       mountains are on the horizon in this direction too.
  4. At 480 with `POST /input {"power_w":200,"balance_left_pct":40}`,
     after 3 s at multiplier 1 then back to 0.01: the player is on the
     same side of the partner as it is with the same input on the flat
     profile (lateral offset is relative to the road, not the screen).
     Screenshot `/tmp/breakaway-c-lateral.png`.
  5. Numbers from `GET /state`: `heading_deg` 90 ± 0.5 at 650 and
     180 ± 0.5 at 1675; `world_speed_mps` 4.0 at 480 and 14.8 at 775.
- In every screenshot: no gaps or slivers in road or shoulder in bends;
  edge lines and dashes follow the bend smoothly; posts on both sides
  follow the bend; riders stand on the road; no sky colour anywhere below
  the horizon line; HUD unchanged.
- Let it run at multiplier 20 for one full lap and into the second
  (distance past 2200): no panic, no visible jump at the lap seam
  (screenshot at 2010 looks like a straight flat road).
- Describe each screenshot in the merge commit message. If a visual
  criterion fails and cannot be fixed within this approach, do not merge;
  report with the screenshot paths.
- Move this task's line in `TODO.md` to `done-tasks.md` with verification
  notes.

## Out of scope

Slowing down for corners, banking, textures, sky gradient, a course map
or profile strip in the HUD, courses chosen by a workout, the
balance-direction question in `TODO.md` (leave the sign as it is), any
scoring.
