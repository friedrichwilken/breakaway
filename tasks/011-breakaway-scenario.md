# 011 — Scenario director: the two-rider breakaway for on/off reps

**Repo:** `~/claude/breakaway/`, branch `breakaway-scenario`, `--no-ff`
merge to `main`.

**Depends on:** 010 (merged). Read `CLAUDE.md`, `DESIGN.md` ("Principle",
"Fiction", "It is a play, not a simulation", "Scenarios"), and all of
`src/` first, especially `race.rs`, `workout.rs`, `course.rs`,
`render.rs`, `ride_ui.rs`.

## Goal

On/off interval sets are staged as a two-rider breakaway: on the ON step
the player pulls on the front, on the OFF step they draft behind their
partner. How well they stay in band decides whether the break stays away
from the bunch. Everything else in a workout keeps today's
"hold the wheel" behaviour. The workout always runs to its end; being
caught changes the story, never stops the ride.

**The one rule applies:** nothing may reward power above the band.

## Part 0 — small fix

`playtests/010-1.toml`: add `set_speed` 8 after the seek so one 60 s loop
shows the effort begin and end.

## Part 1 — director (`src/scenario.rs`, pure, `dt` injected)

### Finding breakaway sets in a workout

Scan the flat step list. A **breakaway set** is a maximal run of
consecutive steps that alternate hard, easy, hard, easy … where:

- every step in the run lasts 15–120 s;
- each hard step's band centre is at least 1.2 × the centre of the easy
  step that follows it;
- the run has at least 3 hard/easy pairs.

Step classes and block kinds do not matter (the HIDIT pyramids are
`ramp` blocks of `active` steps and must qualify). Each set gets an index.
All other steps are `Default`.

`pub fn plan(workout: &Workout) -> Vec<StepRole>` with
`StepRole::{ Default, Pull { set }, Draft { set } }`.

Tests against the samples: `hit-30-30` has three sets (the 3× opener and
the two 20× blocks); `hidits-4x12` has its four 12-minute blocks (state
the exact count and step ranges you find in the test); `lit-1h`,
`mit-sweetspot-6x5` have none. Edge cases: 2 pairs → none; a 130 s step
ends a run; ratio 1.19 → none.

### Break state

```rust
pub struct Break {
    pub set: usize,
    pub advantage_s: f32,      // time gap to the bunch
    pub caught: bool,
    pub phase: Phase,          // Pull | Draft
}
```

| Constant | Value | Meaning |
|---|---|---|
| `ADVANTAGE_START_S` | 20.0 | at the first step of each set |
| `ADVANTAGE_GAIN_PER_S` | 0.05 | while `In` band (pull or draft) |
| `ADVANTAGE_LOSS_UNDER_PER_S` | 0.25 | while `Under` |
| `ADVANTAGE_LOSS_IDLE_PER_S` | 0.5 | while `Idle` |
| `ADVANTAGE_MAX_S` | 180.0 | |

- `Over`: no gain, no loss (matches handle it, as today).
- During transition grace: no change.
- Reaching 0 → `caught = true` for the rest of that set; advantage stays
  0. The next set starts a fresh break.
- When a set's last step ends and it was not caught → the break
  **survived** with its advantage.
- Events: `PullStarted`, `DraftStarted`, `BunchClosing` (advantage drops
  below 10 s, once per crossing), `Caught`, `BreakSurvived { advantage_s }`,
  `BreakStarted { set }`.

Tests: exact rider through `hit-30-30` survives all three sets with the
advantage rising; a rider 25 % under band is caught in every set (give
the time to capture in the test); a rider 25 % over is never caught but
gains nothing and burns matches; grace neutrality; one event per
crossing; **no trajectory with power above the band ends with a larger
advantage than the exact rider's** (the rule, as a test).

### Partner and camera intent (still pure)

`pub fn staging(role, break_state, ride) -> Staging` with
`Staging { partner_ahead_m: f32, partner_lateral_m: f32, camera_behind_m: f32, bunch: BunchView }`:

- `Default` and `Draft`: partner ahead on the centre line at
  `PARTNER_AHEAD_M + ride.gap_m` (today's behaviour); camera 6 m behind.
- `Pull`: the partner sits **behind and beside** the player:
  `partner_ahead_m = −2.0`, `partner_lateral_m = +1.2` relative to the
  player; camera 9 m behind so both are in view. `ride.gap_m` is held at
  0 during a pull and is not changed by band state (under band on a pull
  costs advantage, not the wheel).
  - Under band on a pull: the partner moves up alongside
    (`partner_ahead_m` eases to 0.0): "he is about to come round you".
  - Over band on a pull: the partner drops back (eases to −6.0): "you are
    riding him off your wheel".
- `BunchView::{ Hidden, Around }`: `Around` once caught, until the set
  ends.

## Part 2 — wiring

- A `Scenario` resource holds the plan, the current `StepRole`, the
  `Break` (if any) and the `Staging`; stepped after the ride and workout
  each frame. No workout loaded → everything `Default`.
- `race.rs`: add the "gap frozen at 0" hook used during pulls.
- `render.rs`: partner position from `Staging` (smoothly eased, about
  1 s, including the swap at each pull/draft change); camera distance
  from `Staging` (eased); when `BunchView::Around`, a dozen extra rider
  billboards in mixed flat colours ride around and ahead of the player
  (fixed offsets, deterministic), and disappear when the set ends.
- `ControlCommand`: `SetAdvantage { seconds }` (0..=180, only during a
  set, else 400), `AutopilotBias { pct }` (−50..=50; autopilot rides at
  band centre × (1 + pct/100); reset to 0 by the playtest base reset).
  Debug API: `POST /scenario/advantage`, `POST /autopilot/bias`;
  `GET /state` gains `scenario`: `{role: "default"|"pull"|"draft", set,
  advantage_s, caught, sets_total, sets_survived}` (`set`/`advantage_s`
  null outside a set).
- Ride totals gain `sets_total`, `sets_survived`, and per set the final
  advantage (kept for the later result screen; show nothing new at the
  end yet beyond the lines below).

## Part 3 — UI (egui)

- **Race strip** (shown only during a set, below the band bar): a
  horizontal line with the bunch on the left, the break on the right,
  the distance between them proportional to the advantage (full width =
  60 s; clamp beyond), label `BREAK +0:34`. Colour: steady, or warning
  when below 10 s; `CAUGHT` replaces it once caught.
- **Role line** in large text for 3 s at each change, then small next to
  the band bar: `PULL: your turn on the front` / `DRAFT: sit on the
  wheel`.
- Event texts (same mechanism as today): `The bunch is closing`,
  `Caught by the bunch`, `The break stays away  +0:42`.
- Ride panel totals: `Breaks survived 2 / 3`.

## Part 4 — playtest scenarios (owner's checks; do not answer them)

All: FTP 200, autopilot on, load `samples/hit-30-30.json`, seek into the
first 20× set unless stated. Find the workout times you need from the
sample and state them in the files' comments.

| File | Stage | Question |
|---|---|---|
| `011-1` | seek to 10 s before the set's first hard step, loop 120 s | Is it clear when it is your turn to pull and when to sit on the wheel, without reading small text? |
| `011-2` | same start, loop 120 s | During a pull, can you see your partner behind you; during the draft, are you clearly on his wheel? Do the swaps look right? |
| `011-3` | seek 2 min into the set, autopilot bias −25, loop 60 s | Do you understand from the screen that the bunch is closing because you are under the band? |
| `011-4` | seek 2 min into the set, advantage 4 s, bias −25, loop 60 s | Is being caught clear, and does the ride carry on sensibly afterwards? |
| `011-5` | seek 2 min into the set, bias +25, loop 60 s | On your pull, does it read that you are riding your partner off your wheel and burning matches for nothing? |
| `011-6` | seek to 40 s before the set's last step ends, advantage 45 s, loop 90 s | Is the outcome of the break clear when the set ends? |

## Acceptance (numbers and behaviour only)

- `just check`, `just test`, `just clippy` clean.
- Scripted (game in the background, port 7880 free first), FTP 200,
  autopilot on, `hit-30-30`:
  1. Seek into the first hard step of the 20× set: `role` `pull`,
     `advantage_s` ≈ 20, `gap_m` 0; 30 game-s later `role` `draft`.
  2. Run that whole set at speed 20: `caught` false throughout,
     `advantage_s` at the end > 60, `sets_survived` incremented.
  3. Reload, seek to the set, bias −25: `caught` true within 100 game-s;
     `advantage_s` 0; the workout keeps advancing; at the next set
     `caught` is false again with `advantage_s` ≈ 20.
  4. Reload, seek to the set, bias +25, 60 game-s: `advantage_s` not
     above its start value + 0.5 (grace-time gain only, state the number),
     `matches` < 5.
  5. `POST /scenario/advantage` outside a set → 400.
  6. Whole workout at speed 50 with bias 0: finished, `sets_survived` ==
     `sets_total` == 3, matches 5.
  7. `mit-sweetspot-6x5` whole workout: `sets_total` 0, role `default`
     throughout, behaviour identical to before (in-band ≥ 95 %, matches
     5).
- Stop the game; `playtests/results.ndjson` untouched; config directory
  state restored.
- Update `CLAUDE.md` (scenario layer, new commands, state fields). Move
  this task's line in `TODO.md` to `done-tasks.md`.

## Out of scope

Other scenarios (chase, climb, group ride, roll-out), team radio and
portraits, sound, result screen, bonuses, rider art, the bunch as a
visible group behind the player, sticky bottle, changing course
generation.
