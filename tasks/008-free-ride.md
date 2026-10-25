# 008 — Free ride: a fixed band, holding the wheel, matches, score

**Repo:** `~/claude/breakaway/`, branch `free-ride`, `--no-ff` merge to
`main`.

**Depends on:** 007 (merged). Read `CLAUDE.md`, `DESIGN.md` sections
"Principle", "Fiction" and "HUD", and all of `src/` first.

## Goal

The first actual game loop. The rider has a target power band. In band they
hold their partner's wheel and score; under it the partner rides away;
over it they gain nothing and burn matches. No workouts yet: one fixed
band for the whole ride, set in a panel.

**The one rule applies:** nothing in this task may reward power above the
band.

## Part A — race logic (`src/race.rs`, pure, `dt` injected, no Bevy render/UI types)

```rust
pub struct Band { pub low_w: f32, pub high_w: f32 }          // low < high, both > 0
pub enum BandState { Idle, Under, In, Over }
#[derive(Resource)]
pub struct Ride {
    pub band: Band,
    pub smoothed_power_w: f32,
    pub state: BandState,
    pub gap_m: f32,            // 0 = on the wheel
    pub matches: u32,
    pub score: f32,
    pub ride_time_s: f32,      // time not Idle
    pub in_band_time_s: f32,
    pub over_timer_s: f32,     // time continuously Over since the last burn
    pub burns: u32,            // matches burned this ride (incl. at zero)
}
pub fn step(ride: &mut Ride, power_w: f32, dt: f32) -> Vec<RideEvent>;
pub enum RideEvent { MatchBurned { left: u32 }, OutOfMatches, BackOnWheel, Dropped }
```

Constants (named, at the top of the file):

| Name | Value | Meaning |
|---|---|---|
| `SMOOTHING_TAU_S` | 3.0 | exponential smoothing of power; all decisions use the smoothed value |
| `IDLE_BELOW_W` | 20.0 | smoothed power below this → `Idle`: nothing accrues, gap frozen |
| `START_MATCHES` | 5 | |
| `OVER_SUSTAIN_S` | 4.0 | continuously Over this long → burn one match, timer restarts, burns again every 4 s while still Over (same timing as the alert engine's default) |
| `GAP_CLOSE_MPS` | 0.5 | In band: gap shrinks at this rate, not below 0 |
| `GAP_OPEN_MPS_PER_10PCT` | 0.2 | Under: gap grows at this rate per 10 % that smoothed power is below `low_w`, capped at `GAP_OPEN_MAX_MPS` |
| `GAP_OPEN_MAX_MPS` | 2.0 | |
| `GAP_MAX_M` | 60.0 | gap never exceeds this ("dropped"; partner waits there) |
| `WHEEL_M` | 1.0 | gap at or below this counts as on the wheel |
| `SCORE_IN_BAND_PER_S` | 1.0 | |
| `SCORE_WHEEL_BONUS_PER_S` | 1.0 | extra while In band and on the wheel |
| `SCORE_OVER_PENALTY` | 50.0 | per burn once matches are 0; score never below 0 |

Rules:

- `In`: `low_w ≤ smoothed ≤ high_w`. `Under` / `Over` outside, `Idle`
  below `IDLE_BELOW_W`.
- Over: gap unchanged (no gain, no loss), no score. Leaving Over resets
  `over_timer_s` to 0 immediately.
- A burn with matches > 0 decrements matches and emits `MatchBurned`;
  reaching 0 also emits `OutOfMatches` once. A burn at 0 subtracts
  `SCORE_OVER_PENALTY`.
- `BackOnWheel` when the gap crosses from above `WHEEL_M` to at or below
  it; `Dropped` when it first reaches `GAP_MAX_M`.
- `in_band_pct()` = `in_band_time_s / ride_time_s * 100` (0 when no ride
  time).
- `restart(&mut self)` resets everything except the band.

Unit tests: each state's classification incl. the band edges; smoothing
reaches 95 % of a step in about 9 s; gap closes at 0.5 m/s and stops at
0; gap opens proportionally and is capped (rate and maximum); Over for
3.9 s burns nothing, 4.0 s burns one, 12 s burns three; leaving Over at
3 s and returning restarts the 4 s; five burns → `OutOfMatches` once;
sixth burn costs 50 score and score does not go negative; Idle freezes
everything; score rates; **a ride held at `high_w + 50` for 60 s ends
with score 0 and a ride held at band centre for 60 s scores more than
any ride with power above the band** (the rule, as a test); events fire
once per crossing.

## Part B — wiring

- `Ride` is stepped in `GameSet::Sim` with `InputSource::current().power_w`
  and the virtual-time `dt`.
- Partner position: `PARTNER_AHEAD_M + ride.gap_m` ahead of the player
  (render only reads it).
- Settings file `<config dir>/breakaway/settings.json` (same directory as
  the known-devices file): `{ "band_low_w": 120, "band_high_w": 160 }`,
  those being the defaults when the file is missing or invalid. Saved
  whenever the band changes through the panel.
- `ControlCommand` gains `SetBand { low_w, high_w }` (400 unless
  `0 < low < high ≤ 2000`), `RestartRide`, `SetGap { gap_m }` (0..=60),
  `SetMatches { matches }` (0..=5). `SetBand` via command does not write
  the settings file. The playtest runner's base reset also does
  `RestartRide` and restores the band from settings.
- Debug API: `POST /band`, `POST /ride/restart`, `POST /ride/gap`,
  `POST /ride/matches`; `GET /state` gains `band_low_w`, `band_high_w`,
  `band_state` (`idle|under|in|over`), `smoothed_power_w`, `gap_m`,
  `matches`, `score`, `in_band_pct`, `over_timer_s`. Update `CLAUDE.md`.

## Part C — UI (egui)

- **Band bar** (top centre): a horizontal bar whose scale runs from
  `low − 40 %` of band centre to `high + 40 %`; the band is a marked zone
  in the middle; a marker shows smoothed power; the numbers `low` and
  `high` at the zone's edges and the smoothed power at the marker. Zone
  and marker colour by state: in = green, under = blue, over = red,
  idle = grey. While Over, a line under the bar: `OVER  match burns in
  2.4 s` (or `OVER  no matches left` at 0).
- **Matches** (next to the bar): five slots, burned ones dimmed.
- **HUD lines** added: `GAP 3.2 m`, `SCORE 124`, `IN BAND 87 %`.
- **Ride panel** (`B` toggles): band low and high in 5 W steps (low
  always at least 10 W below high), "Restart ride" button, and the
  current totals (ride time, in-band %, burns).
- A short on-screen text for 2 s on each `RideEvent`
  (`Match burned`, `Out of matches`, `Back on the wheel`, `Dropped`).

## Part D — playtest scenarios (owner's checks; do not answer them)

All on profile `flat`, band 180–220 W via `set_band`, `loop_seconds = 30`.

| File | Stage | Question |
|---|---|---|
| `008-1` | input 200 W | Is it obvious at a glance that you are in the band and on the wheel? |
| `008-2` | input 150 W | Is it obvious that you are under the band, and do you see the red rider riding away? |
| `008-3` | input 260 W | Is it obvious that you are over the band, and do you notice each match burning? |
| `008-4` | gap 30 m, input 200 W | Do you see yourself closing the gap back to the wheel? |
| `008-5` | matches 0, input 260 W | Is it clear that you are out of matches and losing score? |
| `008-6` | no input override, panel hint | Press B, change the band, close the panel. Is the ride panel easy to use? |

## Acceptance (numbers and behaviour only)

- `just check`, `just test`, `just clippy` clean.
- Scripted (game in the background, port 7880 free first; use
  `/set-speed` to shorten waits, remembering all timings are in game
  time):
  1. `POST /band {"low_w":180,"high_w":220}`, restart, input 200 W for
     20 game-s → `band_state` `in`, `gap_m` 0, `score` ≈ 24–28 (smoothed power needs
     about 7 s from 0 to reach the band, then 2 points per second), `matches` 5.
  2. Input 150 W for 20 game-s → `under`, `gap_m` > 5, score unchanged
     since the switch (± 3).
  3. Input 200 W → `gap_m` shrinks back to 0.
  4. Input 260 W for 14 game-s → `over`, `matches` ≤ 3, `gap_m` unchanged
     during Over, score unchanged since the switch (± 3).
  5. `POST /ride/matches {"matches":0}` still at 260 W for 9 game-s →
     score dropped by ≥ 50 or is 0.
  6. Invalid band (`low ≥ high`) → 400.
  7. With no override and no device: the scripted mock (200 ± 15 W) in a
     180–220 band stays `in`.
- Settings file: changing the band through the debug API does not create
  it. Remove any settings file your run created.
- `--playtest 008` selects six scenarios (unit test). Stop the game; leave
  `playtests/results.ndjson` untouched.
- Move this task's line in `TODO.md` to `done-tasks.md`.

## Out of scope

Workouts and changing bands, scenarios other than "partner ahead", team
radio, sound, recording, result screen, calendar-aware or conservative-mode
match budgets, balance affecting score, terrain affecting the band.
