# 006 — Staged playtests: the machine stages, the human judges

**Repo:** `~/claude/breakaway/`, branch `staged-playtests`, `--no-ff` merge
to `main`.

**Depends on:** 005 (merged). Read `CLAUDE.md` and all of `src/` first.
Reference for the pattern (read only): `~/claude/dwartrain/iron-vale/docs/staged-playtests.md`
and `~/claude/dwartrain/iron-vale/tools/playtests/README.md`. Breakaway's
version is much smaller; do not port iron-vale's code.

## Why

Whether something *looks* right (does a climb read as a climb?) is the
project owner's call, not an agent's. Agents verify numbers through tests
and the debug API. For everything perceptual, a task ships scenario files:
the game stages the situation, shows one question, and records the owner's
yes/no. Agents stop judging screenshots.

## 1. One command layer — `src/control.rs`

Staging and the debug API must share one implementation.

```rust
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlCommand {
    Teleport { distance_m: f32 },
    Input { power_w: Option<f32>, balance_left_pct: Option<f32> }, // both → override, else clear
    SetSpeed { multiplier: Option<f32> },                          // None → 1.0, clamp 0..=50
    SetProfile { name: String },                                   // flat | hills | demo
}
```

- A pure-as-possible `apply(cmd, &mut RaceState, &mut InputSource, &mut Profile,
  &mut speed_multiplier) -> Result<(), String>` with the validation the
  debug API has today (same error cases). The Bevy system wrapper copies
  the multiplier into `Time<Virtual>`.
- `SetProfile` replaces the `Profile` resource at runtime and resets
  `distance_m` to 0. `render.rs` must react to a changed profile (rebuild
  the strips, recompute the base-plane height). `Profile::by_name(&str) ->
  Option<Profile>` replaces the match in `main.rs`.
- `debug_api.rs`: `/teleport`, `/input`, `/set-speed` and a new
  `POST /set-profile {"name": str}` all build a `ControlCommand` and go
  through `apply`. Responses stay as they are. `/state` gains
  `"profile": "<name>"`. `/screenshot` stays.

## 2. Scenario files — `playtests/<task>-<step>.toml`

```toml
task  = 4
step  = 1
title = "Steady climb"
loop_seconds = 20          # optional, default 20: re-stage this often while waiting

[[stage]]
cmd = "set_profile"
name = "hills"

[[stage]]
cmd = "teleport"
distance_m = 300

[ask]
text = "Does this read as riding uphill?"
```

- `[[stage]]` tables are `ControlCommand`s, applied in file order.
- The file stem is the scenario name (`004-1`), and must equal
  `{task:03}-{step}`; the loader rejects a mismatch.
- `src/playtest/scenario.rs`: types + `load(path)` + `load_all(dir)`
  (sorted by task, then step). `toml` crate as a dependency.
- `playtests/README.md`: the format, in under a page.

## 3. Runner — `src/playtest/runner.rs` (pure state machine, `dt` injected)

States per scenario: `Staging` → `Asking` → `Answered(Verdict)`; then the
next scenario; after the last one `Summary`.

- `Staging`: apply all stage commands in one go; a failing command makes
  the verdict `StageError(msg)` and moves on.
- `Asking`: the game runs at whatever speed the stage set (the owner needs
  motion to judge). Every `loop_seconds` of real time the stage commands
  are applied again, so the situation repeats until answered.
- Input while `Asking`: `Y` = pass, `N` = fail, `S` = skip, `R` = re-stage
  now. In `Summary`: any of `Enter`/`Esc`/`Q` quits.
- Before each scenario the runner resets to a known base: profile `demo`,
  distance 0, input override cleared, speed 1.0. Scenarios then only state
  what differs.

The state machine itself has no Bevy input or UI types; a thin system
feeds it key presses and `dt`. Unit tests: order of scenarios; Y/N/S
produce the right verdicts; re-stage after `loop_seconds`; stage error is
recorded and the run continues; summary counts.

## 4. Overlay — `src/playtest/overlay.rs`

Bevy UI, bottom of the screen, semi-transparent dark panel, large text:

```
004-1  Steady climb                               (2 / 11)
Does this read as riding uphill?
[Y] yes   [N] no   [S] skip   [R] replay
```

`Summary` shows one line per scenario with its verdict and the totals.
The normal HUD stays visible.

## 5. Running it

- CLI: `--playtest <sel>` where `<sel>` is a scenario name (`004-1`), a
  task number (`004` or `4`: all its steps), or `all`. Unknown selection
  or an empty match: print an error, exit code 2. Without `--playtest`
  the game behaves exactly as today.
- `just playtest <sel>`.
- Results: append one JSON line per answered scenario to
  `playtests/results.ndjson`: `{"scenario","title","question","verdict",
  "detail","at","commit"}` — `verdict` is `pass|fail|skip|stage_error`,
  `detail` holds the stage error text or is empty, `at` is UTC ISO 8601,
  `commit` is the short git hash baked in at build time (`build.rs`;
  `"unknown"` if git is unavailable). Add `playtests/results.ndjson` to
  `.gitignore`.
- On quit print a short table to stdout; exit code 0 if every verdict is
  `pass`, otherwise 1.

## 6. Scenarios to ship

Write these eleven files. Tune `distance_m` so the feature is in view for
most of the loop; keep the questions as written (they are the owner's
checks). Use balance overrides only where stated.

| File | Stage | Question |
|---|---|---|
| `003-1` | profile `flat`, teleport 100 | Does it feel like riding forward on a straight road at a steady speed? |
| `003-2` | profile `flat`, teleport 100, input 200 W / left share 40 % | Is the blue rider at the left edge of the road, clearly left of the red rider? |
| `003-3` | profile `flat`, teleport 100, input 200 W / left share 60 % | Is the blue rider at the right edge of the road, clearly right of the red rider? |
| `004-1` | profile `hills`, teleport 300 | Does this read as riding uphill? |
| `004-2` | profile `hills`, teleport 100 | Do you see the climb coming before you reach it? |
| `004-3` | profile `hills`, teleport 540 | Does the road disappear over the crest and come back into view as you ride over it? |
| `004-4` | profile `hills`, teleport 800 | Does this read as riding downhill, and clearly faster than the flat? |
| `005-1` | profile `demo`, teleport 400 | Does the road bend to the right while climbing, with no flicker, holes or popping? |
| `005-2` | profile `demo`, teleport 690 | Does the road bend left, then right, on the descent, with no flicker, holes or popping? |
| `005-3` | profile `demo`, teleport 1470 | Does the hairpin on the steep climb look right, including the inside edge of the bend? |
| `005-4` | profile `demo`, teleport 1940, loop 15 s | Do you ride across this stretch without any visible jump? (This is where the lap restarts.) |

## 7. Tests that never wait for a person

- Every file in `playtests/` parses and passes the name check
  (`every_checked_in_scenario_parses`).
- Every scenario's stage commands apply without error to a fresh
  `RaceState` / `InputSource` / `Profile` (no window).
- `cargo test` must never open a window or wait for a key.

## 8. `CLAUDE.md`

Add a section "Validation" with this content, and remove the phrase about
`POST /screenshot` making work verifiable "without anyone on a bike" from
the Debug API section (keep the endpoint in the table):

> Logic is verified by unit tests and through the debug API. Anything
> perceptual (does it look, read or feel right) is the owner's call and is
> never judged by an agent from a screenshot. Every task with something
> visible ships `playtests/<task>-<step>.toml` scenarios (format:
> `playtests/README.md`) with one concrete yes/no question each. A check
> that can be expressed as a number is a test, not a playtest question.
> The owner runs `just playtest <task>`; verdicts land in
> `playtests/results.ndjson`. A failed verdict becomes a new task; it does
> not reopen the old one.

Also document `--playtest`, `just playtest`, `/set-profile` and the
`profile` field there.

## Acceptance

- `just check`, `just test`, `just clippy` clean.
- By script (game in the background, debug API): `POST /set-profile
  {"name":"flat"}` then `GET /state` shows `profile: "flat"`,
  `distance_m` near 0, `gradient_pct` 0; `{"name":"nope"}` returns 400.
  Existing endpoints behave as before (teleport, input, set-speed incl.
  their error cases).
- `cargo run -- --playtest nope` exits with code 2 without opening a
  window. `--playtest 004` selects exactly the four `004-*` scenarios
  (assert in a unit test on the selection function).
- Start `just playtest 003` once to confirm it opens, shows the overlay
  for `003-1`, and that the process is alive; then stop it. Do **not**
  answer the questions and do not take screenshots to judge anything: the
  verdicts are the owner's. (Key handling is covered by the state-machine
  unit tests.)
- Move this task's line in `TODO.md` to `done-tasks.md`.

## Out of scope

Typed notes on a failed step, headless playtest mode, wait predicates
(scenarios here need none), new game features, changing any visual, the
balance-direction question in `TODO.md`.
