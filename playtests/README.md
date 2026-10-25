# Playtest scenarios

One file per perceptual check: `playtests/<task>-<step>.toml`. The game
stages the situation, shows one question at the bottom of the screen and
records the owner's yes/no. Run with `just playtest <sel>` (`004-1`, `004`
or `4` for all steps of a task, or `all`).

```toml
task  = 4                  # the task the check belongs to
step  = 1                  # its numbered step
title = "Steady climb"
loop_seconds = 20          # optional, default 20: re-stage this often while waiting

[[stage]]                  # zero or more; each is a ControlCommand (src/control.rs)
cmd = "set_profile"
name = "hills"

[[stage]]
cmd = "teleport"
distance_m = 300

[ask]
text = "Does this read as riding uphill?"
```

- The file stem must be `{task:03}-{step}` (`004-1.toml`); the loader
  rejects a mismatch. Unknown keys are errors.
- Before each scenario the runner resets to a base: no workout, autopilot
  off (bias 0), profile `demo`, distance 0, input override cleared, speed 1.0, the
  band from the settings file, a fresh ride. Stages only state what
  differs. Every `loop_seconds` of real time (and on `R`) the base and then the
  stages are applied again.
- `[[stage]]` commands (same as the debug API):
  - `teleport` — `distance_m` (finite, ≥ 0)
  - `input` — `power_w` and `balance_left_pct`; both set an override,
    leaving either out clears it
  - `set_speed` — `multiplier` (0..50; leave out for 1.0)
  - `set_profile` — `name`: `flat`, `hills` or `demo` (restarts at 0 m;
    refused while a workout is loaded: it brings its own course)
  - `set_band` — `low_w`, `high_w` (`0 < low < high <= 2000`); does not
    write the settings file
  - `restart_ride` — fresh ride, band kept
  - `set_gap` — `gap_m` (0..=60)
  - `set_matches` — `matches` (0..=5)
  - `load_workout` — `path` (relative: the repo's `workouts/`, then the
    config `workouts/`); restarts the ride at step 0 on the course
    generated from the workout
  - `unload_workout` — back to free ride
  - `skip_step` — ends the current workout step and jumps to the next
    step's start on the course (repeat it to skip more)
  - `seek_workout` — `elapsed_s` (`0..=total_s`): jumps the workout and the
    rider to that workout time
  - `set_ftp` — `ftp_w` (50..=600); does not write the settings file
  - `autopilot` — `on` (bool): the mock rides the band centre ± 3 %
  - `autopilot_bias` — `pct` (-50..=50): the autopilot rides band centre ×
    (1 + pct / 100); the base resets it to 0
  - `set_advantage` — `seconds` (0..=180): the running breakaway's
    advantage over the bunch (fails outside a breakaway set; 0 is caught
    on the next frame)
  - `radio_say` — `key`: a line of `assets/radio/lines.toml` on the team
    radio, filled with the current band, rep, advantage and matches (fails
    for an unknown key)
  - `radio_enabled` — `on` (bool): the team radio on or off (does not
    write the settings file)
  - `countdown_beeps` — `on` (bool): the step countdown's beeps on or off
    (does not write the settings file)
- Keys while asking: `Y` pass, `N` fail, `S` skip, `R` re-stage now, `C`
  comment. On the summary: `Enter`, `Esc` or `Q` quits.
- Comments: `N` opens a one-line comment field; `Enter` records the fail
  with the comment (it may be empty), `Esc` goes back to the question
  without a verdict. `C` opens the same field for a note on a pass or a
  skip: `Enter` keeps it (shown under the question), `Esc` drops it; the
  next `Y` / `N` / `S` records it. While the field is open every letter is
  text (no verdict keys, no `B` / `D` panels) and the scene keeps looping.
- A stage command that fails records `stage_error` and moves on.
- Verdicts are appended to `playtests/results.ndjson` (git-ignored), one
  JSON line per answered scenario: `scenario`, `title`, `question`,
  `verdict` (`pass|fail|skip|stage_error`), `detail` (stage error text, else the
  owner's comment, else empty), `at` (UTC ISO 8601), `commit` (short hash of the build).
- The process exits 0 when every verdict is `pass`, otherwise 1.
- Ask one concrete yes/no question. A check that can be expressed as a
  number is a unit test, not a playtest question.
- `cargo test` parses every file here and applies its stages to a fresh
  world (`src/playtest/scenario.rs`); it never opens a window.
