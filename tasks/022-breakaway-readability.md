# 022 — Breakaway readability: the gap, the cause, the outcome

**Repo:** `~/claude/breakaway/`, branch `breakaway-readability`, `--no-ff`
merge to `main`.

**Depends on:** 019 (merged). Read `CLAUDE.md` ("Scenario layer"),
`src/scenario.rs`, `src/ride_ui.rs` (race strip, role banner, over line),
`src/render.rs` (staging, bunch).

**Status: proposal.** The owner's verdicts say what did not read; the
remedies below are the planner's. Owner: strike or change any part
before this is built.

## Why

Owner playtest 2026-10-02, all four failed:

- 011-3 "Do you understand from the screen that the bunch is closing
  because you are under the band?": "nope".
- 011-4 "Is being caught clear, and does the ride carry on sensibly
  afterwards?": "nope".
- 011-5 "On your pull, does it read that you are riding your partner off
  your wheel and burning matches for nothing?": "looks more like they are
  riding next to us. but maybe that is the idea?"
- 011-6 "Is the outcome of the break clear when the set ends?": "the
  breakway bar does not read so clear."

No scoring or scenario rule changes here: the same numbers, shown so they
can be read. The one rule is untouched (over band still gains nothing).

## Part A — the gap readout replaces the race strip

Today: a 26 px line with a dot and `BREAK +0:34`. New, same place (under
the band bar), about 64 px high, the band bar's width:

1. **Text line, 28 px:** `GAP TO BUNCH  0:34` and a trend word:
   `growing` (green) while the advantage rises, `closing` (orange; red
   below 10 s) while it falls, `holding` (grey) while it does neither.
   The trend comes from the scenario's own rate for the current state,
   not from differencing frames.
2. **Bar, 14 px:** filled from the left in the trend colour, full width =
   60 s (as today). The left end is labelled `BUNCH`, the fill's end
   `YOU`.
3. **Cause line, 18 px, only when not growing:**
   - under band: `Under the band: the bunch is closing`
   - not pedalling: `Not pedalling: the bunch is closing fast`
   - over band: `Over the band: no gain, burning matches`
   - transition grace: nothing.
4. Caught: the text line is `CAUGHT · riding in the bunch` (red), the bar
   empty, no cause line, until the set ends.

Texts are pure functions with tests (`gap_readout(&Break, &Ride) ->
GapReadout {headline, trend, fraction, cause}`).

## Part B — outcome banners

The large centre banner used for the role line (44 px, 3 s) also shows,
for 5 s each:

- `ScenarioEvent::Caught`: `CAUGHT BY THE BUNCH` (red).
- `ScenarioEvent::BreakSurvived`: `BREAK SURVIVED  +0:45` (green, the
  final advantage).

A role banner that would start during an outcome banner waits until it
ends (queue of one; the newest role wins).

## Part C — the bunch arrives, and is counted

1. When the break is caught, the 12 bunch riders do not pop in: each
   rides in from 25 m behind its place to its place over 3 s (ease-out),
   so they come past the camera into view. `Break` gains the time since
   it was caught; the offset is a pure function of it.
2. Nobody has ever confirmed that the bunch renders. `GET /state`
   `scenario` gains `bunch_visible`: the number of bunch entities whose
   computed visibility is on this frame (read from the entities, not
   from the staging). Acceptance checks it is 12 a few seconds after a
   catch and 0 otherwise.

## Part D — the pull: over and under the band

Today on a pull the partner sits 2 m behind and 1.2 m right; over band he
drops to 6 m behind, which with the camera 9 m behind puts him large in
the right foreground ("riding next to us").

1. **Over band (after the grace):** the partner falls back past the
   camera and out of view: target 14 m behind the player, eased with a
   time constant of 1.5 s so the sliding back is seen. Back in band he
   returns to 2 m behind with the usual easing.
2. **Under band:** unchanged (he comes up alongside), but the cause line
   of Part A says why.
3. While he is out of view the cause line reads `Over the band: Marco is
   off your wheel, burning matches`.
4. The bunch offsets are checked against the new partner places (extend
   the existing clearance test).

## Tests (unit)

- `gap_readout` for: in band (growing), under (closing, cause), idle,
  over (holding, cause), grace (holding, no cause), below 10 s (red),
  caught.
- Banner: caught and survived texts and durations; a role change during
  an outcome banner shows afterwards; two role changes: the newest.
- Bunch arrival offsets at 0, 1.5 and 3 s after the catch; unchanged
  after 3 s.
- Pull staging over band: target 14 m behind; reaches within 1 m of it
  in 5 s; returns in band.
- The simulated-rider tests on all samples still pass unchanged (no
  rule moved).

## Playtest scenarios (owner's checks; do not answer them)

One thing per question. Stages as the 011 scenarios they replace
(`hit-30-30`, FTP 200, autopilot; workout times from their comments).

| File | Stage | Question |
|---|---|---|
| `022-1` | seek 1860, autopilot bias −25, loop 60 s | Can you tell from the screen that the bunch is closing? |
| `022-2` | same | Can you tell from the screen *why* it is closing? |
| `022-3` | seek 1860, advantage 4, bias −25, loop 60 s | Is the moment you are caught unmistakable? |
| `022-4` | same | After the catch, do you see a group of riders around you? |
| `022-5` | seek 1860, bias +25, loop 60 s | On your pull over the band, does Marco visibly fall off your wheel? |
| `022-6` | same | Is it clear that riding over the band gains you nothing? |
| `022-7` | seek 2900, advantage 45, loop 90 s | When the set ends, is it clear that the break survived? |
| `022-8` | seek 1800, loop 60 s (in band) | Is the gap readout easy to read at a glance while riding in band? |

Delete `playtests/011-3.toml` … `011-6.toml` (replaced by these) and
adjust the scenario count in the tests.

## Acceptance

- `just check`, `just test`, `just clippy` clean.
- Scripted through the debug API on `hit-30-30`: after a forced catch
  (`/scenario/advantage` 0) `bunch_visible` is 12 within 5 s and 0 after
  the set ends or an unload.
- Update `CLAUDE.md` ("Scenario layer": readout, banners, bunch arrival,
  pull staging, state field). Close issue #4 with what changed
  and how it was verified.

## Out of scope

Radio lines and voices, rider art, a rear-view of the bunch before the
catch, any change to advantage rates, matches or score.
