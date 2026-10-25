# 004 — Terrain: climbs and descents

**Repo:** `~/claude/breakaway/`, branch `terrain-hills`, `--no-ff` merge to
`main`.

**Depends on:** 003 (merged). Read `CLAUDE.md`, then `src/sim.rs` and
`src/render.rs` in full before starting.

## Goal

The road goes up and down. A looping demo elevation profile drives road
geometry, rider and camera height, and the scripted world speed (slower
uphill, faster downhill). Power still never changes speed. Curves are not
part of this task.

## New module `src/terrain.rs` (pure, no rendering types)

```rust
pub struct Segment { pub length_m: f32, pub gradient_pct: f32 }

#[derive(Resource)]
pub struct Profile { /* segments + precomputed elevation table */ }

impl Profile {
    pub fn new(segments: Vec<Segment>) -> Self;
    pub fn demo() -> Self;
    pub fn total_length_m(&self) -> f32;
    pub fn gradient_at(&self, distance_m: f32) -> f32;   // percent
    pub fn elevation_at(&self, distance_m: f32) -> f32;  // metres
}
pub fn speed_for_gradient(gradient_pct: f32) -> f32;     // m/s
```

- The profile loops: any distance (including negative) is taken modulo
  `total_length_m()`.
- **Smooth gradient changes:** at each segment boundary (including the
  wrap from last to first) the gradient blends from the old to the new
  value over a 40 m zone centred on the boundary, using a smoothstep.
  Because the blend is symmetric, total climb is unchanged. `new` panics
  if a segment is shorter than 40 m.
- **Elevation:** integrate `gradient_at / 100` at 1 m steps once in `new`
  into a table; `elevation_at` interpolates linearly in it. Elevation at
  distance 0 is 0.
- `new` panics if the net elevation over one loop is more than 0.05 m off
  zero (a looping profile must close).
- **`demo()`** (2000 m, net zero):

  | length m | gradient % |
  |---|---|
  | 200 | 0 |
  | 400 | +6 |
  | 100 | 0 |
  | 600 | −4 |
  | 200 | 0 |
  | 150 | +10 |
  | 50 | 0 |
  | 300 | −5 |

- **`speed_for_gradient`:** flat = `DEFAULT_WORLD_SPEED_MPS` (10.0).
  Uphill (`g > 0`): `10.0 / (1.0 + 0.25 * g)`. Downhill (`g < 0`):
  `min(10.0 + 1.2 * |g|, 18.0)`. Continuous at 0. NaN → 10.0.

Unit tests: elevation 0 at 0 and at 2000; elevation at 620 m (top of the
first climb, past the blend) is 24.0 ± 0.1; gradient at 400 is 6.0, at
1000 is −4.0, at 100 is 0.0; `|elevation_at(d + 1) − elevation_at(d)|`
≤ 0.101 for every whole metre of the loop; looping (`elevation_at(d)` ==
`elevation_at(d + 2000)` and for a negative `d`); `new` panics on a
non-closing profile and on a 30 m segment; `speed_for_gradient` is 10.0
at 0, 4.0 at +6, about 2.857 at +10, 14.8 at −4, capped at 18.0, and
strictly decreasing from −6 to +12.

## `sim.rs`

- `RaceState` gains `elevation_m: f32` and `gradient_pct: f32`.
- `step(state, input, profile: &Profile, dt)`: first set
  `gradient_pct = profile.gradient_at(distance_m)` and
  `world_speed_mps = speed_for_gradient(gradient_pct)`, then advance
  distance and lateral as today, then set
  `elevation_m = profile.elevation_at(distance_m)`.
- `distance_m` is horizontal distance; do not correct for slope length.
- Update existing tests; add: input power 100 W vs 400 W over the same
  simulated 60 s gives identical `distance_m`.

`main.rs`: insert `Profile::demo()` as a resource and pass it to `step`.

## `render.rs`

All heights come from the `Profile` resource.

- **Road tiles** (keep 12 m, raise `TILE_COUNT` to 60): for a tile
  spanning distances `a..b`, with `ea = elevation_at(a)`,
  `eb = elevation_at(b)`: translation at the midpoint with height
  `(ea + eb) / 2`; pitch about the X axis by `atan2(eb − ea, TILE_LEN_M)`
  so the far end is higher on a climb; scale the tile's length by
  `hypot(TILE_LEN_M, eb − ea) / TILE_LEN_M`. Adjacent tiles then share
  their edge heights exactly, so no gaps. The road, edge-line and dash
  children ride along unchanged.
- **Ground:** delete the single `Ground` plane and `follow_ground`. Give
  every road tile one more child: a green quad 600 m wide and
  `TILE_LEN_M` long at local height −0.02 (below the road surface), so
  the land pitches with the road.
- **Posts:** each pair sits at `elevation_at` of its own distance and
  stays upright.
- **Riders:** height = `elevation_at(their distance) + RIDER_H_M / 2`.
  The partner uses its own distance (2 m ahead), not the player's.
  Billboards stay upright (rotation about Y only, as now).
- **Camera:** position at distance `d − CAM_BEHIND_M`, height
  `elevation_at(d − CAM_BEHIND_M) + CAM_HEIGHT_M`; looks at the centre
  line at distance `d + CAM_LOOK_AHEAD_M`, height
  `elevation_at(d + CAM_LOOK_AHEAD_M) + 1.0`. No extra smoothing.

## `hud.rs`

Two more lines: `GRAD +6.0 %` (always signed, one decimal) and
`ELEV 24 m` (whole metres).

## `debug_api.rs`

- `GET /state` adds `elevation_m` and `gradient_pct`.
- New `POST /teleport` `{"distance_m": f32}`: sets `distance_m` (any
  finite value ≥ 0; otherwise 400). Lateral position is kept.
- Update the table in `CLAUDE.md`.

## Acceptance

- `just check`, `just test`, `just clippy` clean.
- `just run` in the background, then by script. Use
  `POST /set-speed {"multiplier": 0.01}` before each teleport so the scene
  barely moves while the screenshot is taken, and set it back to 1 after.
  1. Teleport to 100: `gradient_pct` ≈ 0, `world_speed_mps` ≈ 10.0.
     Screenshot `/tmp/breakaway-flat.png`.
  2. Teleport to 400: `gradient_pct` ≈ 6.0, `world_speed_mps` ≈ 4.0,
     `elevation_m` ≈ 12 (± 1). Screenshot `/tmp/breakaway-climb.png`.
  3. Teleport to 585: screenshot `/tmp/breakaway-crest.png`.
  4. Teleport to 1000: `gradient_pct` ≈ −4.0, `world_speed_mps` ≈ 14.8.
     Screenshot `/tmp/breakaway-descent.png`.
  5. At multiplier 1, teleport to 300 and set
     `POST /input {"power_w":400,"balance_left_pct":50}`; after 3 s
     `world_speed_mps` is still ≈ 4.0.
  6. Read all four screenshots and confirm by eye: flat looks like before;
     on the climb the road rises ahead and the horizon sits higher in the
     frame than on the flat; at the crest the road ends at a visible edge
     against the sky; on the descent the road falls away and more of it is
     visible. No gaps between tiles, no ground showing through the road,
     no sky visible between road and ground, riders standing on the road
     surface (not floating or sunk). HUD shows `GRAD` and `ELEV`.
     Describe each screenshot in the merge commit message.
- If any visual check fails, fix it before merging; if it cannot be fixed
  within the approach above, do not merge, and report what is wrong with
  the screenshot paths.
- Move this task's line in `TODO.md` to `done-tasks.md` with the
  verification notes.

## Out of scope

Curves, textures, mountain backdrop, profile strip in the HUD, terrain
chosen by a workout, trainer resistance, the balance-direction question in
`TODO.md` (leave the sign as it is), any change to scoring.

---

## Revision 1 (2026-10-01) — make the slope readable

First build: numbers correct, but a steady climb looked flat. Cause: the
camera tilted fully with the slope, which cancels it on screen, and there
was nothing level in the scene to compare against. This revision replaces
the **Camera** bullet above and adds three things. Everything else stands.

### R1. Vertical exaggeration (render only)

`const VERTICAL_EXAGGERATION: f32 = 2.0;` in `render.rs`. Every height
taken from the profile for drawing goes through one helper,
`render_height(profile, d) = VERTICAL_EXAGGERATION * profile.elevation_at(d)`:
tiles (and therefore their pitch and length scale), posts, riders, camera.
`sim.rs`, `terrain.rs`, the HUD and the debug API keep true values.

### R2. Camera follows the slope only half

`const CAM_SLOPE_FOLLOW: f32 = 0.5;`

- Position unchanged: distance `d − CAM_BEHIND_M`, height
  `render_height(d − CAM_BEHIND_M) + CAM_HEIGHT_M`.
- Look target at distance `d + CAM_LOOK_AHEAD_M`, height
  `h_cam_ground + 1.0 + CAM_SLOPE_FOLLOW * (render_height(d + CAM_LOOK_AHEAD_M) − h_cam_ground)`
  where `h_cam_ground = render_height(d − CAM_BEHIND_M)`.

On the flat this is the old camera. On a steady climb the road now rises
on screen; on a descent it falls away.

### R3. Level backdrop (the reference for "level")

Follows the camera's position every frame, never its rotation. Placed
3000 m ahead of the camera (−Z), 12 000 m wide, unlit:

- **Far land:** a vertical green quad (same green as the ground strips)
  from camera eye height down to 600 m below it. Its top edge is the true
  horizon.
- **Mountains:** a darker blue-grey jagged silhouette standing on that
  horizon line, peaks between 60 m and 200 m above eye height, built once
  from a fixed list of points (deterministic, no RNG), spanning the full
  width.

Set the camera's far clip plane to at least 6000 m. The backdrop must draw
behind everything else (it is the farthest geometry; no depth tricks
needed).

### R4. Flat reference course

`Profile::flat()` (one 2000 m segment at 0 %) and a CLI flag
`--profile flat|demo` (default `demo`), so there is a clean flat
reference.

### Acceptance for the revision (replaces step 6; steps 1–5 as before)

Take five screenshots at time multiplier 0.01, 2000×1125 view coordinates,
smaller y = higher in frame:

- `--profile flat`, teleport 400 → `/tmp/breakaway-ref-flat.png`. Note
  `y_flat`, the y of the horizon line (top of the green).
- default profile, teleport 100 / 400 / 585 / 1000 → the four files named
  in the original acceptance.

Criteria:

1. Reference: road runs to the horizon; mountains stand on the horizon;
   no seam or colour step where the ground strips meet the far land.
2. Climb (400): the top of the visible road/hill is at least 40 px above
   `y_flat`, and the hill covers the lower part of the mountains or all of
   them. The road visibly rises.
3. Crest (585): unchanged criterion — road ends at a visible edge.
4. Descent (1000): the far end of the descending road is at least 30 px
   below `y_flat`, and far land plus mountains are visible above it.
5. In all five: no gaps between tiles, no ground through the road, no sky
   visible below the horizon line, riders and posts standing on the road
   surface, HUD shows the true gradient (+6.0 %, −4.0 %), not the
   exaggerated one.

Report `y_flat` and the measured y values. If criterion 2 or 4 misses by a
small margin, you may raise `VERTICAL_EXAGGERATION` up to 3.0 or change
`CAM_SLOPE_FOLLOW` within 0.3–0.6 to meet it; report the final values. If
it still fails, do not merge and report.

Still out of scope: curves, textures, sky gradient, profile strip.
