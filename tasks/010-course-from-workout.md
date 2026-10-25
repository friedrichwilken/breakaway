# 010 — The course is generated from the workout; fix band transitions

**Repo:** `~/claude/breakaway/`, branch `workout-course`, `--no-ff` merge
to `main`.

**Depends on:** 009 (merged). Read `CLAUDE.md`, `DESIGN.md` ("Fiction",
"Scenarios"), and all of `src/` first, especially `terrain.rs`, `sim.rs`,
`race.rs`, `workout.rs`.

## Part 0 — band transitions must not punish exact compliance

Task 009 showed that a rider who holds every target exactly burns a match
on almost every easy rep of a 30/30 (3 s smoothing needs ~10 s to fall
into a narrow easy band; grace is 5 s). Fix in `race.rs` / `workout.rs`:

- `SMOOTHING_TAU_S` 3.0 → **1.5** (≈ a 3-second average).
- Minimum band width 10 W → **20 W** (widened symmetrically).
- **Grace** after a step starts lasts until the smoothed power first
  enters the new band, at least 3 s and at most `TRANSITION_GRACE_MAX_S =
  10.0`. During grace: gap does not open, over-timer does not run. Score
  only accrues when in the new band (unchanged).
- New tests, using a simulated rider stepped at 0.1 s:
  - **exact rider** (always at band centre): for every file in
    `workouts/samples/`, at FTP 200 and FTP 300, the whole workout burns
    **0** matches and ends with in-band ≥ 93 %.
  - **human-like rider** (moves linearly to each new band centre over
    4 s, then holds it ± 3 %): 0 burns on every sample.
  - **late rider** (stays at the previous target for 12 s after every
    step change): does burn matches on `hit-30-30` (the grace is not a
    loophole).
  - over-band power during grace still never scores (keep 009's test).
- Update other tests whose numbers depend on the old constants.

## Part 1 — open (non-looping) courses in `terrain.rs`

`Profile` gets a second mode: `Profile::open(segments)`.

- No closure requirement. Before distance 0 and after the last segment
  the road continues straight and flat at the start / end elevation and
  heading (no wrap, no lap composition). Blending applies between
  segments and into those flat extensions.
- `is_looping()`, and `total_length_m()` works for both.
- All existing looping behaviour and tests stay unchanged.

## Part 2 — course generator (`src/course.rs`, pure)

```rust
pub fn generate(workout: &Workout) -> Course;
pub struct Course {
    pub profile: Profile,                 // open
    pub step_start_m: Vec<f32>,           // distance where each step starts
    pub expected_distance: TimeTable,     // workout time (s) → distance (m)
}
```

**Gradient from intensity.** `i` = centre of the step's range in % FTP.

| Step | Gradient |
|---|---|
| `warm_up`, `cool_down` | 0 % |
| `active` | `clamp((i − 55) / 6.5, 0, 10)` rounded to 0.5 % (60 % → 1 %, 70 % → 2.5 %, 90 % → 5.5 %, 105 % → 7.5 %, ≥ 120 % → 10 %) |
| `rest`, shorter than 120 s | 0 % (a ledge on the climb) |
| `rest`, 120 s or longer | −3 % |

**Length from time.** The scripted speed is `speed_for_gradient`, so a
step of duration `T` on gradient `g` is about `speed_for_gradient(g) × T`
long. Because gradient blending changes speed near boundaries, solve
iteratively: start from that estimate, integrate the real travel time
over the blended profile (`dt = dd / speed_for_gradient(gradient_at(d))`,
1 m steps), rescale each step's length by `T / measured`, repeat until
every step boundary is within **0.5 s** of its workout time (at most 10
rounds).

**Subdivision and variety** (deterministic: seed a small inline PRNG from
the workout name and total duration; same workout → identical course):

- Every step is cut into chunks of 150–500 m (a step shorter than 150 m
  is one chunk). No chunk shorter than 60 m; if a step would be shorter
  than 60 m, it takes the previous chunk's gradient and is merged into it
  for geometry (its time still counts).
- Curves: each chunk gets a turn between −75° and +75°, about a third of
  chunks straight; radius never below 80 m; consecutive turns alternate
  direction more often than not so the course wanders rather than
  spirals.
- Rolling terrain: in steps longer than 300 s, chunk gradients vary by up
  to ± 1.5 % around the step's gradient with zero mean over the step, and
  never below −4 % or above 10 %. Steps of 300 s or less use the step's
  gradient exactly.

`TimeTable`: distance at each whole second of workout time.

Tests: every sample generates; per-step boundary time error ≤ 0.5 s and
total error ≤ 2 s (simulated at the scripted speed); determinism; segment
constraints (lengths, radius, gradient limits); the gradient table values
above; `hit-30-30`'s hard reps are climbs and its 30 s rests are 0 %;
`lit-1h`'s long step has base gradient 1.5 % (66 % of FTP), so all its
chunks lie within 0 % and 3 %; the zero-mean rolling holds per long
step.

## Part 3 — wiring

- Loading a workout generates its course and makes it the active profile
  (name `workout`); unloading or restarting returns to the profile that
  was active before. Distance starts at 0 with the workout.
- **Time is the master.** While a workout is loaded:
  - when the ride is `Idle` (auto-pause) the world does not move;
  - each frame the world speed is `speed_for_gradient(gradient)` scaled
    by `clamp(1 + (expected_distance(elapsed) − distance) / 20, 0.9, 1.1)`
    so distance stays locked to workout time without visible jumps;
  - `SkipStep` teleports to that step's `step_start_m`.
- After `Finished` the world rolls on at flat speed on the end extension.
- `GET /state`: `profile` reports `workout`; add `course_length_m` and
  `course_drift_m` (`expected − actual`).
- The progress strip from 009 stays as is.

## Part 4 — playtest scenarios (owner's checks; do not answer them)

All: FTP 200, autopilot on.

| File | Stage | Question |
|---|---|---|
| `010-1` | load `mit-sweetspot-6x5`, seek to 15 s before the first sweet-spot rep (workout time 1185 s), loop 60 s | Does the road start to climb when the effort begins, and ease off when it ends? |
| `010-2` | load `hit-30-30`, skip to the first rep of the main 20× block | Does each 30-second effort feel like a ramp and each recovery like a ledge on the climb? |
| `010-3` | load `lit-1h`, skip 1 step, speed 6 | Is the long easy ride varied enough (gentle bends and rolls) without being distracting? |
| `010-4` | load `hidits-4x12`, skip to the first hard block | Does the terrain change at the same moment the band changes? |
| `010-5` | load `mit-sweetspot-6x5`, skip to the last rest of the main set | Do the longer recoveries read as a descent? |

Add `ControlCommand::SeekWorkout { elapsed_s }` (also
`POST /workout/seek`): jumps the runner and the distance to a workout
time (400 outside `0..=total_s` or with no workout loaded). Use it
wherever a stage needs a precise moment.

## Acceptance (numbers and behaviour only)

- `just check`, `just test`, `just clippy` clean.
- Scripted (game in the background, port 7880 free first), FTP 200,
  autopilot on:
  1. Load `mit-sweetspot-6x5`: `profile` = `workout`,
     `course_length_m` > 20 000; step 0 gradient 0.
  2. Skip to "Sweet spot" (89 % centre): `gradient_pct` settles at 5.0
     (± 0.1) within 10 game-s; band 168–188 W.
  3. Run the whole workout at speed 50: `finished` true, `elapsed_s`
     4200 ± 1, `|course_drift_m|` < 15 at five sampled moments, matches
     5, in-band ≥ 95 %.
  4. Same for `hit-30-30` at speed 50: matches 5 at the end, in-band
     ≥ 93 %, drift < 15 m.
  5. Override 0 W for 10 game-s mid-workout: `distance_m` stops within
     4 game-s and `elapsed_s` with it; clearing resumes both.
  6. Unload: `profile` returns to `demo`.
- Stop the game; `playtests/results.ndjson` untouched; config directory
  state restored.
- Update `CLAUDE.md` (open profiles, the generator, time as master, new
  state fields, any new command). Move this task's line in `TODO.md` to
  `done-tasks.md`.

## Out of scope

Scenarios and the partner's behaviour, team radio, scenery or textures,
named climbs, trainer resistance, a course preview map, changing
`speed_for_gradient`.
