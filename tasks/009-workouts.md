# 009 — Workouts: file format, TrainingPeaks export, runner, bands per step

**Repo:** `~/claude/breakaway/`, branch `workouts`, `--no-ff` merge to
`main`.

**Depends on:** 008 (merged). Read `CLAUDE.md`, `DESIGN.md` sections
"Workout source" and "Adaptive training", and all of `src/` first.

## Goal

A planned workout drives the ride: each step's target range becomes the
band for its duration. Today's workout is fetched from
TrainingPeaks with one command, then loaded and ridden. Course and story
do not react to the workout yet (later tasks); the demo course keeps
looping.

## Part 0 — two fixes from task 008

- **No false start:** when raw input power is ≥ `IDLE_BELOW_W` and the
  smoothed value is below it (ride start, restart, return from a pause),
  the smoothed value snaps to the raw value instead of climbing from 0.
  Update the affected unit tests; add one: after `restart`, 200 W in a
  180–220 band is `In` on the first step and the gap stays 0.
- `playtests/008-4.toml`: gap 10 m instead of 30 m.

## Part 1 — workout file format (JSON)

```json
{
  "name": "Sweet spot - 6x5 min",
  "date": "2026-01-09",
  "source": "trainingpeaks:100000005",
  "steps": [
    { "name": "Warm up", "duration_s": 120, "low_pct": 40, "high_pct": 50,
      "class": "warm_up", "block": 0, "block_kind": "single",
      "rep": 1, "reps": 1, "cadence_low": null, "cadence_high": null,
      "fixed_rest": false }
  ]
}
```

- Steps are flat and in riding order; repetition blocks are already
  expanded. `block` numbers the source blocks from 0; `block_kind` is
  `single | repetition | ramp`; `rep` / `reps` are the 1-based repetition
  and the block's repetition count (1 / 1 outside repetitions).
- `low_pct` / `high_pct` are percent of FTP, `0 < low ≤ high ≤ 300`.
- `class`: `warm_up | active | rest | cool_down`.
- `fixed_rest` defaults to false when absent (used by a later task).
- Loader rejects: no steps, a non-positive duration, bad percentages,
  unknown class or block kind, unknown top-level or step keys.

## Part 2 — TrainingPeaks export (`tools/tp_export.py`)

Runs with the TrainingPeaks MCP server's virtualenv and its authenticated
client (cookie in the macOS keychain; read
`~/claude/athlete/trainingpeaks-mcp/src/tp_mcp/client/` and
`tools/workouts.py` there for how a workout with its structure is
fetched). No credentials, tokens or cookies are written anywhere by this
script, and nothing is ever written back to TrainingPeaks.

- `just fetch-workout [YYYY-MM-DD]` (default: today) → for each planned
  bike workout that day that has a structure, write
  `<config dir>/breakaway/workouts/<date>-<id>.json` and print the path,
  name, step count and total duration. No structured workout that day →
  message, exit code 1.
- `just fetch-workout-id <id> <out.json>` for a specific workout.
- A pure function `convert(tp_workout: dict) -> dict` does the mapping:
  - block `type` `step` → `single`; `repetition` → `repetition`
    (`length.value` reps, steps repeated in order); `rampUp` / `rampDown`
    → `ramp`.
  - step duration: `length.unit` must be `second`; anything else → error
    naming the step.
  - steps with `openDuration: true` (lap-button markers) are dropped.
  - the target without a `unit` is the intensity in percent of FTP
    (`minValue` / `maxValue`); a single value `v` becomes `v − 5 %` to
    `v + 5 %` of itself. A target with unit `roundOrStridePerMinute` is
    the cadence range. `primaryIntensityMetric` other than `percentOfFtp`
    → error.
  - `intensityClass` `warmUp | active | rest | coolDown` →
    `warm_up | active | rest | cool_down`.
- `python tools/tp_export.py --self-test` converts the raw fixtures in
  `tools/fixtures/*.tp.json` and compares with the checked-in expected
  output; wired into `just test`.

**Samples to fetch and commit** (raw as `tools/fixtures/<slug>.tp.json`,
converted as `workouts/samples/<slug>.json`; strip `polyline`, comments
and file lists from the raw fixtures). The committed fixtures have since
been replaced by synthetic workouts in the same TrainingPeaks export shape
(same slugs, ids and headline formats, invented compositions):

| slug | id | title |
|---|---|---|
| `mit-sweetspot-6x5` | 100000005 | Sweet spot - 6x5 min |
| `lit-1h` | 100000004 | Endurance - 1 h easy |
| `hit-30-30` | 100000002 | HIT - 2x20x30/30 + 2x12 min tempo |
| `lactate-elimination` | 100000003 | Lactate clearance - surges into tempo |
| `hidits-4x12` | 100000001 | HIT - 4x12 min pyramids |

If the keychain auth fails, stop and report (do not ask for or handle
credentials). Known facts to check your conversion against:
`mit-sweetspot-6x5` has 19 steps totalling 4200 s (2 warm-up, 3 ramp,
1 recovery, 6 × (300 s at 84–94 % + 120 s at 45–55 %), 1 cool-down);
`lit-1h` has 2 steps totalling 3600 s, the second at 62–70 % with cadence
88–98.

## Part 3 — runner (`src/workout.rs`, pure, `dt` injected)

- `Workout`, `Step` (serde, as Part 1), `Workout::load(path)`.
- `WorkoutRunner { workout, index, step_elapsed_s, elapsed_s, finished }`
  with `advance(dt) -> Vec<WorkoutEvent>` (`StepStarted { index }`,
  `Finished`), `current()`, `next()`, `step_remaining_s()`, `skip()`.
  A large `dt` can cross several steps; each emits its event in order.
- Band for a step: `low_w = low_pct / 100 × ftp_w`, same for high; if the
  band is narrower than 10 W it is widened symmetrically to 10 W.

## Part 4 — wiring into the ride

- Settings gain `ftp_w` (default 200, range 50–600, 5 W steps in the ride
  panel). Changing FTP re-resolves the current band.
- A loaded workout owns the band: each `StepStarted` sets `ride.band`.
  The band controls in the ride panel are disabled while a workout is
  loaded.
- **Auto-pause:** the runner only advances while the ride is not `Idle`.
- **Transition grace:** for `TRANSITION_GRACE_S = 5.0` after a step
  starts, the gap does not open and the over-timer does not run; score
  still only accrues when in the new band. Implement inside `race.rs`
  (`Ride::begin_grace()`), unit-tested.
- Loading a workout restarts the ride (matches 5, score 0) and starts at
  step 0. On `Finished` the ride stops accruing (state shown as
  `finished`), totals stay on screen, and a message `Workout complete`
  is shown until a new workout is loaded or the ride is restarted (which
  returns to free ride with the settings band).
- **Autopilot** for tests and playtests: `ControlCommand::Autopilot
  { on }`; while on (and no override, no device), the scripted mock rides
  at the centre of the current band ± 3 % slow wave.
- `ControlCommand`: `LoadWorkout { path }` (relative paths resolve
  against the repo's `workouts/` directory baked in at build time, then
  the config `workouts/` directory), `UnloadWorkout`, `SkipStep`,
  `SetFtp { ftp_w }`, `Autopilot { on }`. Debug API: `POST /workout/load`,
  `/workout/unload`, `/workout/skip`, `/ftp`, `/autopilot`; `GET /state`
  gains `ftp_w`, `autopilot`, and `workout`: null or `{name, step_index,
  step_count, step_name, step_class, step_remaining_s, block_kind, rep,
  reps, elapsed_s, total_s, finished}`. CLI `--workout <path>`.
  Playtest base reset also unloads the workout and turns autopilot off.

## Part 5 — UI (egui)

- Under the band bar while a workout is loaded:
  `STEP 7/19  Sweet spot  rep 2/6      3:12`
  `NEXT  Easy  90–110 W  2:00` (dimmed; in the last 10 s of a step this
  line is highlighted and shows `in 8 s`).
- A thin progress strip across the top: one segment per step, width
  proportional to duration, height proportional to the band centre,
  current position marked.
- Ride panel gets a "Workout" section: FTP control; the list of files in
  the config `workouts/` directory and in `workouts/samples/` with a
  "Load" button each; "Unload"; the name and total duration of the loaded
  one.

## Part 6 — playtest scenarios (owner's checks; do not answer them)

All: profile `flat`, FTP 200, autopilot on.

| File | Stage | Question |
|---|---|---|
| `009-1` | load `samples/mit-sweetspot-6x5.json` | Is it clear at a glance which step you are in, how long it lasts and what comes next? |
| `009-2` | load the same, skip 6 steps (first sweet-spot rep), speed 4 | Watch a few step changes. Is each change announced early enough and clear when it happens? |
| `009-3` | load `samples/hit-30-30.json`, skip to the first 30/30 block | Can you follow 30-second on/off changes from the display alone? |
| `009-4` | load `samples/lit-1h.json`, skip 1 step | Does the display stay calm and readable for a long single step? |
| `009-5` | load `samples/mit-sweetspot-6x5.json`, skip to the last step, speed 20 | Is it clear when the workout is complete and what your totals are? |

`loop_seconds` 60 for 009-2, 009-3, 009-5; default for the others.

## Acceptance (numbers and behaviour only)

- `just check`, `just test` (incl. the export self-test), `just clippy`
  clean. Unit tests: loader (every sample parses; each rejection case);
  runner (step order, multi-step `dt`, skip, finish, remaining time);
  band resolution incl. the 10 W minimum and FTP change; grace; snap;
  autopilot centre; `--playtest 009` selects five.
- Scripted (game in background, port 7880 free first):
  1. `POST /ftp {"ftp_w":200}`, load `samples/mit-sweetspot-6x5.json`,
     autopilot on → `step_count` 19, `total_s` 4200, band 90–110 W
     (step 0), `matches` 5.
  2. `/workout/skip` × 6 → `step_name` "Sweet spot", `rep` 1, `reps` 6,
     band 168–188 W.
  3. Speed 50, run to the end → `finished` true, `elapsed_s` ≈ 4200,
     `in_band_pct` ≥ 95, `matches` 5, `burns` 0.
  4. Reload, override input 0 W for 10 game-s → `elapsed_s` does not
     advance; clear override → it advances.
  5. `POST /ftp {"ftp_w":250}` mid-step → band scales by 1.25.
  6. `/workout/load` with a missing file → 400, ride unchanged.
  7. `just fetch-workout 2026-01-09` writes a file equal in content to
     the committed `mit-sweetspot-6x5` sample (apart from nothing: it is
     the same workout; historical: the committed sample is synthetic
     now). Remove the fetched file and any config directory
     your run created (leave them if they existed before).
- Stop the game; `playtests/results.ndjson` untouched.
- Document in `CLAUDE.md`: the format (short), `just fetch-workout`,
  the new endpoints and CLI flag. Move this task's line in `TODO.md` to
  `done-tasks.md`.

## Out of scope

Course or terrain from the workout, scenarios, team radio, sound,
recording, result screen, cadence or heart-rate targets (stored, not
used), sticky bottle, auto-regulation, writing anything to TrainingPeaks,
other workout sources (`.zwo`, intervals.icu).
