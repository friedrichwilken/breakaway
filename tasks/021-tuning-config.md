# 021 — Tuning file: speed factor, lateral range, taller workout strip

**Repo:** `~/claude/breakaway/`, branch `tuning-config`, `--no-ff` merge
to `main`.

**Depends on:** 019 (merged). Read `CLAUDE.md`, `src/sim.rs`,
`src/terrain.rs` (`speed_for_gradient`), `src/course.rs`,
`src/workout.rs` (`pace` / `move_world`), `src/ride_ui.rs`
(`draw_progress_strip`).

## Why

Owner playtest 2026-10-02:

- 003-1: "a little to slow. I think we need to externalize a speed factor
  in a config.toml".
- 003-2 / 003-3: at 40 % / 60 % balance the rider is "a little too far"
  left / right (today 0.3 m per percentage point: the road edge at 40 %).
- 009-2: the workout display "could be displayed higher, more vertical
  pixels".

## Part A — `config.toml`

- A hand-edited tuning file at the repo root, committed with the
  defaults and a comment per key. Path baked in at build time like
  `assets/radio/lines.toml`; read once at start. A missing file means
  the defaults; an unreadable file, an unknown key or a value out of
  range prints the reason and exits 2 before the window opens.
- It is not `settings.json`: the game never writes it and the ride panel
  does not show it.
- `src/tuning.rs` (pure parsing + a `Tuning` resource).

| Key | Default | Range | Meaning |
|---|---|---|---|
| `speed_factor` | `1.0` | `0.5..=2.0` | multiplies the scripted world speed |
| `lateral_m_per_pct` | `0.25` | `0.05..=0.5` | metres of lateral offset per percentage point of balance away from 50 % |
| `workout_strip_px` | `48` | `16..=120` | height of the workout strip across the top |

## Part B — speed factor

- Every scripted speed is `speed_for_gradient(gradient) × speed_factor`:
  free ride, the workout pace, and course generation. Because the course
  is solved with the same speed, a workout's steps still take exactly
  their time; the course just gets longer or shorter. Time stays the
  master.
- `speed_for_gradient` stays a pure function of the gradient; the factor
  is passed in by its callers (course generation takes it as a
  parameter). No global state in `terrain.rs` / `course.rs`.
- The default stays 1.0: the owner tunes the number by hand. Note in
  the file's comment: task 014's scenery may change how fast the
  same speed feels, so retune after it.

## Part C — lateral range

- `lateral_from_balance` uses `lateral_m_per_pct` from the tuning; the
  clamp to the road half width stays. Default 0.25 (2.5 m at 40 % / 60 %,
  half a metre inside the edge line).
- The balance direction is not changed (DESIGN.md "Decided").

## Part D — workout strip

- `draw_progress_strip` uses `workout_strip_px`; everything placed below
  it (`TOP_OFFSET` / `top_offset`) follows the configured height.

## Tests (unit)

- Parsing: defaults for a missing file and for an empty file; each key
  alone; unknown key, wrong type, each value below and above its range
  → error naming the key.
- The committed `config.toml` parses to the defaults.
- Course: for every sample workout at `speed_factor` 0.8 and 1.3, every
  step boundary is still within 0.5 s of its time when ridden at the
  scripted speed, and the course length scales with the factor within
  1 %.
- Lateral: 40 % → −2.5 m at the default; 30 % stays clamped at −3.0 m.
- The existing tests keep passing at the defaults (speed 10 m/s on the
  flat at factor 1.0).

## Playtest scenarios (owner's checks; do not answer them)

They show the defaults; the owner edits `config.toml` and reruns to tune.

| File | Stage | Question |
|---|---|---|
| `021-1` | profile `flat`, teleport 100, input 200 W / 50 % | With your `speed_factor`, does the flat road feel fast enough? |
| `021-2` | profile `flat`, teleport 100, input 200 W / 40 % | Is the blue rider clearly left, but not too far out? |
| `021-3` | profile `flat`, teleport 100, input 200 W / 60 % | Is the blue rider clearly right, but not too far out? |
| `021-4` | FTP 200, autopilot, load `hit-30-30`, seek 1740 | Is the workout strip across the top tall enough to read the steps? |

## Acceptance

- `just check`, `just test`, `just clippy` clean.
- Scripted: with `speed_factor = 1.5` in a temporary copy of the file
  (restore it afterwards), `world_speed_mps` on `flat` is 15.0, and a
  loaded `mit-sweetspot-6x5` has a `course_length_m` 1.5 × the default's
  within 1 %; a `config.toml` with `speed_factor = 9` exits 2.
- Update `CLAUDE.md` (tuning file, keys, the speed rule in "Workout
  course"). Move this task's line in `TODO.md` to `done-tasks.md`.

## Out of scope

Reloading the file while the game runs, a UI for it, any other constant.
