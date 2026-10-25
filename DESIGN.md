# Breakaway — design draft

Status: concept draft, 2026-10-01. Its own project, separate from
trainer-alerts; it shares only library crates with it. No task specs
written yet — this doc is the source for them.

## Principle

**Score comes from staying in your band, never from watts.** Over the band
is penalised exactly like under it. Every mechanic below has to pass that
test (the risk this game guards against is overdoing it, not
undertraining).

## Fiction: you are in a race, compliance is your race craft

Single player. The world's speed is scripted by the workout, not by your
watts. What your watts control is your **position relative to where you
should be** (the wheel you are following, the gap you are closing):

- in band → you hold the wheel / the gap closes at the scripted rate
- under band → a gap opens (you drift back)
- over band → you burn a match (limited stock per ride) and gain nothing:
  on a pull you ride your partner off your wheel, in the draft you sit in
  the wind instead of recovering

Out-of-band uses the existing `AlertEngine` timing (sustained 4 s, re-fire
while still out), so the game and the alarm tones always agree.

L/R balance = lateral position on the road (50/50 = centre line / the
sheltered spot on the wheel). Balance limits = how far you can drift before
you lose the draft.

## It is a play, not a simulation

The whole race is staged storytelling to motivate; nothing is simulated.
The workout always continues to its last step whatever happens. Messing up
only lowers the score and changes the story (dropped from the break → you
are now chasing); there is no game over and no restart.

## Team radio: talking heads

80s-anime-style portrait + text box (optionally a short voice blip), your
team mates and DS talking to you. Driven by `WorkoutRunner` and alert
events, so it doubles as the functional interval cueing:

- step about to change (~10 s before): "The other team is launching an
  attack — quick, we need to chase them!"
- step start / your turn on the front / swing off
- out of band (under): "You're losing the wheel!"; (over): "Easy — save
  your matches!"
- streaks, last rep, set done, final result

Lines live in a data file keyed by (scenario, event), several variants per
key, picked at random. Portraits are placeholder art first.

### Lines to use in later scenarios

- Chase scenario, Marco: "Close the gap, we can still catch them!"
  (owner, 2026-10-01). In the chase the gap closes while in band, so the
  call means "get back to your watts", never "go over".

### Look and sound (decided 2026-10-01)

- **JRPG dialogue feel**: portrait tile, name plate, typewriter text.
  Lean into JRPG conventions for other UI too (result screen, stamina
  bar, campaign).
- **DS portrait, to stylise later**: the Tour de France in-car shot. The
  DS drives; three-quarter view from the passenger side, steering wheel
  cutting into a corner, the wired handheld mic on its coiled cable in
  his raised hand, team polo, sunglasses pushed up, windscreen glare.
- **Partner**: Marco. Helmet, glasses, seen from the side at riding
  height.
- **Voices are pre-generated** per line with the local Kokoro server
  (Docker container `tts`), from its stock voices, never imitating a
  real person. A radio effect (filter, static, squelch clicks) is an
  ornament for later, not part of the first version.
  Numbers (watts, gaps) stay on screen; the spoken version of such lines
  is number-free.

## HUD

- **Matches = the health bar.** One resource, shown as a row of matches.
  Sustained over-band burns one. At zero nothing ends; the story degrades
  and further over-band time costs score directly.
- Band indicator: where you are inside the current target band.
- Step timer + next step preview, gap / position, score.
- **Step countdown** (owner, 2026-10-02): seconds in the middle of the
  screen with a beep each before a step change. 10–6 white; 5–1 red,
  growing, beeps louder. Neutral cueing: when, never how hard.

## Physiology bars (live estimates)

Display only; none of these feed the score. All labelled as modelled.

- **W′bal (stamina bar)** — Skiba differential model, power only, needs
  CP and W′. The real "anaerobic battery": drains above CP, refills below.
  Garbage if CP is wrong; needs current values, not a stale FTP.
- **Lactate bar (modelled)** — toy kinetic model from power: production
  rises steeply above CP, clearance proportional to the current level and
  fastest at moderate intensity. Tells the clearance-session story (fill
  with the hard effort, drain at sweet spot / fatmax). It is not a
  measurement and cannot show whether the rider's clearance is good; only
  finger-prick samples could calibrate it. HR cannot estimate lactate.
- **Cardiac drift / decoupling** — live W/HR vs the first steady block.
  Measured, not modelled. The rider's durability signal.
- **HR recovery per rep** — bpm dropped in the first 30/60 s of each OFF;
  a shrinking drop across reps is real measured fatigue.
- **DFA α1** — from RR intervals (`cycling-ble` already parses them; the
  app does not pass them on yet). Live marker of the aerobic threshold
  (~0.75), i.e. "am I really at fatmax". Needs a good chest strap and
  ~2 min windows; artefact-sensitive. Experimental.
- **Cadence sag** — cadence vs the session's own baseline at the same
  power; early neuromuscular-fatigue / cramp warning.
- **Fuel gauge** — kJ → estimated carbs burned vs carbs taken; team radio
  prompts "eat now".

## Further features (candidates, 2026-10-01)

Must-have (the game is unusable as the main ride app without them):

- **Recording + upload**: the game records the ride itself (crash-safe,
  incremental flush — the open bugfix in `TODO.md` becomes blocking) and
  uploads to intervals.icu.
- **One-step start**: launch → today's planned workout is already loaded,
  devices auto-reconnect, pedalling starts it. No menus to decide in.
- **Auto-pause**: stop pedalling → story and step timer freeze.
- **Time-accelerated mock mode**: replay a whole workout in seconds with a
  scripted mock rider, for development and tests.

Worth having:

- **Season / campaign**: each workout is a stage; the coach's TP plan is
  the race calendar; standings and a recurring rival carry the story
  between sessions.
- **Balance arc**: L/R balance trend across sessions as a persistent stat.
- **LIT content**: easy rides are where motivation is lowest and overdoing
  it is likeliest; the DS keeps you sitting in ("save it for tomorrow").
- **Result poster**: shareable race-result image (extends the "Save
  summary image" backlog item).
- **Adaptive music**: intensity follows the workout steps.

Deliberately not: multiplayer, shops / inventory / XP grind, avatar
customisation, anything that creates play time off the bike. The game ends
when the workout ends.

## Calendar-aware match budget

The band never moves; the number of matches does. The game reads the next
days from the TP calendar:

- rest day tomorrow → extra matches ("empty the tank, tomorrow we rest")
- key session or race tomorrow → fewer matches ("save it for tomorrow")
- **conservative mode** (on until the owner switches it off): calendar
  bonus disabled, budget stays minimal.

## Adaptive training (simple, coach stays in charge)

The coach owns the plan. The game never generates or rewrites it.

All advice comes from one character: the **DS** (directeur sportif) on the
radio from the team car.

- **In-session auto-regulation, escalating, downward only**:
  1. First rep(s) under band → encouragement only: "You were under —
     next one a touch harder, if you can." (A rider often needs a rep or
     two to find the groove; the first rep of a set never triggers an offer.)
  2. Still under on later reps, or HR recovery collapsing → the DS offers
     "drop the target 3–5 %" or "skip the last rep". One button to accept
     or decline. Targets are never raised automatically.
- **Sticky bottle**: ask the team car for a bottle = +30 s on the current
  between-set rest. Free (quality sets matter more than riding to
  failure), limited to a few per workout, logged. Only on between-set
  recoveries; rests inside a rep group (the OFF of a 30/30) and rests the
  workout file marks as `fixed` are part of the session and cannot be
  extended. Input: one button (Zwift Click / keyboard).
- **No readiness check before the ride** (a bad number beforehand
  becomes an excuse and a self-fulfilling prophecy). Sleep /
  HRV from intervals.icu appear only in the **post-ride debrief**, to
  explain a hard day after the fact.
- **Everything adapted is logged** and appears in the ride summary, so the
  coach sees what was actually done versus prescribed.
- **Progression rule for own templates only** (not coach workouts): all
  reps in band with no fade → the DS suggests a small bump next time;
  missed or faded → repeat. Same rule as the "Workout mode" backlog item.
- **No planned workout today** → default to an easy group-ride scenario.

Plan generator (TrainerRoad-style): not needed for a coached rider; parked
as a prerequisite for other users, far later.

## Result poster: progress versus last time

Workouts are matched by structure (same step pattern; targets may differ).
When a same or similar workout exists in the history, the poster shows
deltas: average rep power, fade first→last rep, HR at the same power, HR
recovery, L/R balance, compliance, sticky bottles used.

## Bonuses and achievements

- **No fade**: average power of the last rep (and last set) within a few
  % of the first.
- **Negative split**: power rises across reps or sets **while every rep
  stays inside its band**. Climbing from the bottom of the band to the top
  pays; going above the band never does (it burns a match instead). This
  is the only form in which more watts earn anything.
- Clean set (no alert fired), full workout without burning a match,
  balance held within limits for a whole block.

## Scenarios (staged from the workout structure)

| Workout pattern | Staged as |
|---|---|
| Warm-up | Neutral roll-out behind the lead car |
| Short on/off reps, ~1:1 (30/30, 40/20) | Two-rider breakaway: ON = your pull on the front, OFF = drafting your partner |
| Long steady block (sweet spot / threshold, 8–20 min) | Chase group bridging to the break: gap shrinks while in band |
| VO2 reps (3–5 min) | Closing a gap / holding the group over a climb |
| Over-unders | Surges on a climb: hold the wheel through each attack |
| Recovery between sets | Sitting in the bunch |
| LIT / endurance ride | Group ride: hold your place in the line |
| Cool-down | Rolling in after the line |
| No workout loaded | Free ride: the active alert profile's limits are the band |

Result screen: race outcome derived from compliance (break stays away by
N seconds / caught with X km to go, position in the group), plus matches
burned. No sprint finish — nothing may reward exceeding the band.

## Workout source

The game does not talk to TrainingPeaks. It loads a small local workout
file (flat list of steps: duration, target low/high in watts, intensity
class, rep-group id). A script in `tp_helper` exports today's planned TP
workout into that format, resolving %FTP to watts. `.zwo` import can come
later through the same format.

A `WorkoutRunner` state machine (same shape as `AlertEngine`: `now` is
injected, fully unit-testable) walks the steps and swaps the active power
band per step. (Replaces the "Workout mode" item in trainer-alerts'
`TODO.md`.)

No ERG / resistance control: the rider hits the target themselves. That is
what makes it a game, and FTMS control is not built yet anyway.

## Presentation

Game logic is plain 2D (x = lateral, y = distance along the road).
Rendering is a separate layer in the after-zelda `pseudo3d.rs` style:
`Camera3d` chase cam, textured ground plane, riders as camera-facing
billboards. That gives the Mode 7 look without any Mode 7 maths. A
top-down debug view of the same logic stays available for tests.

## Architecture

Separate project (`~/claude/breakaway/`, Bevy 0.19), not a mode of the
trainer-alerts app. Shared code lives in `~/claude/libs/`:

- `cycling-ble` (exists) — BLE GATT parsing.
- `trainer-core` (to extract from `trainer-alerts/app/src-tauri/src`) —
  `ble`, `alerts`, `mock`, `recording`. Used by both projects.

Game-only code (`WorkoutRunner`, scenario director, radio, HUD, history)
stays in this repo. BLE runs on a tokio thread, readings reach Bevy
through a channel. The game has its own device and band setup so it runs
without trainer-alerts installed; alert profiles are superseded by the
workout's own bands.

## cycling-ble: second consumer, then crates.io

Breakaway is the second real consumer of `cycling-ble` (after
trainer-alerts) and serves as its API evaluation before a crates.io
release. While building:

- Depend on it by path; every awkward call site, missing accessor or
  workaround gets logged as an issue against `cycling-ble`, not patched
  around in the game.
- Parts Breakaway exercises that trainer-alerts does not: RR intervals
  (DFA α1), Zwift Click input (sticky bottle), later FTMS control.
- Nothing game- or app-specific goes into `cycling-ble`; that belongs in
  `trainer-core` or here. The crate stays a pure parser.
- Release gate: API unchanged across a few weeks of real rides in both
  consumers → publish, then switch both to the crates.io version.

## Slices

1. Extract `trainer-core`; Tauri app unchanged in behaviour.
2. Bevy skeleton: mock device drives a rider on a pseudo-3D road.
3. Free ride: band = alert profile, hold-the-wheel mechanic, matches.
4. `WorkoutRunner` + workout file format + TP export script.
5. Scenario director: breakaway (30/30) first, then chase, then the rest.
6. Team radio (talking heads + line file) and HUD (matches, band, timer).
7. Result screen with bonuses/achievements; persistence per workout.
8. DS interactions: escalating auto-regulation, sticky bottle (needs one
   button: keyboard first, Zwift Click when the app can discover it).
9. Later: FTMS gradient, ghost of a previous ride.

## Decided

- All UI is egui (`bevy_egui`), no Bevy UI nodes (2026-10-01).
- Balance → lateral direction stays as built (2026-10-01): a left share
  below 50 % moves the rider left, above 50 % right. Not to be flipped.
- Scenario picking is automatic from the step pattern, manual override
  possible.
- A large failure mid-workout degrades the scenario; the workout never
  ends early (2026-10-01).

## Open

- Thresholds for the no-fade / negative-split bonuses.
- Dialogue language (English assumed) and portrait art source.
