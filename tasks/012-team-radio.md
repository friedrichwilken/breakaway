# 012 — Team radio: talking heads that cue and narrate the ride

**Repo:** `~/claude/breakaway/`, branch `team-radio`, `--no-ff` merge to
`main`.

**Depends on:** 011 (merged). Read `CLAUDE.md`, `DESIGN.md` ("Team radio:
talking heads", "It is a play, not a simulation", "Adaptive training"),
and all of `src/` first.

## Goal

Two characters talk to the rider in a dialogue box with a portrait: the
**DS** (directeur sportif, on the radio from the team car) and the
**partner** in the break. Their lines are triggered by workout, ride and
scenario events, so the radio is both the story and the interval cueing
(a warning before every step change). Text only; no sound in this task.

**The one rule applies to words too:** no line may tell the rider to
exceed the band. Under-band lines call them *back up to the band*;
over-band lines call them *back down*.

## Part 0 — breakaway-set detection fix (`scenario.rs`)

Task 011 found two flaws in the rule. Change it to:

- `warm_up` and `cool_down` steps never belong to a set (a run stops at
  them);
- step duration limit 15–**180** s (was 120).

Update the tests with the real set ranges this produces on `hit-30-30`
and `hidits-4x12` (report them); the other three samples still have none.
The opener of `hit-30-30` must no longer start inside the warm-up, and
each HIDIT pyramid must start at its first hard step.

## Part 1 — lines file (`assets/radio/lines.toml`)

```toml
[[line]]
key = "step_warning_pull"
speaker = "partner"
text = [
  "Your turn in {secs}. Come through smooth.",
  "{secs} seconds, then you're on the front.",
]
```

- One `[[line]]` per key; `speaker` is `ds` or `partner`; `text` has at
  least two variants. Placeholders: `{secs}`, `{low}`, `{high}` (next or
  current band in watts), `{rep}`, `{reps}`, `{advantage}` (`m:ss`),
  `{matches}`.
- Create the file with exactly the keys and texts in the appendix below.
- The path is baked in at build time (like `playtests/`).

## Part 2 — radio logic (`src/radio.rs`, pure, `dt` injected)

- `Radio::push(key, vars, priority)`, `Radio::advance(dt)`,
  `Radio::current() -> Option<&Spoken { speaker, text, shown_chars }>`.
- One line at a time. Typewriter reveal at 40 characters per second; the
  line stays for `max(3.0, chars / 15)` seconds after it is fully shown.
- Priorities: `Cue` (step warnings, pull/draft) > `Alert` (under/over,
  matches, bunch closing, caught) > `Story` (everything else). A `Cue`
  interrupts a `Story` line immediately and an `Alert` after its current
  sentence would have been visible for 1.5 s; a waiting line of lower
  priority is dropped if it has waited more than 8 s. Queue length at
  most 3.
- Variant choice: deterministic PRNG seeded from the workout name; never
  the same variant of a key twice in a row.
- Per-key cooldowns so it does not nag: `under_band` and `over_band`
  30 s; `in_band_streak` 300 s; all others none.
- A `radio_enabled` setting (default true) in `settings.json` and the
  ride panel.

### Triggers

| Key | When |
|---|---|
| `workout_start` | workout loaded and the rider first leaves Idle |
| `step_warning_pull` / `step_warning_draft` / `step_warning_harder` / `step_warning_easier` | 10 s before a step ends (never for steps shorter than 20 s: then 5 s before with `{secs}` = 5). Pull/draft when the next step has that role; otherwise harder/easier by comparing band centres; none if the next band is within 5 % |
| `pull_start`, `draft_start` | scenario events |
| `break_start` | `BreakStarted` |
| `last_rep` | first step of the last repetition of a block with at least 3 reps |
| `block_done` | last step of a repetition block ends |
| `bunch_closing`, `caught`, `break_survived` | scenario events |
| `under_band` | Under for 5 s continuously, outside grace |
| `over_band` | Over for 2 s continuously, outside grace |
| `match_burned`, `out_of_matches` | ride events |
| `back_on_wheel`, `dropped` | ride events |
| `in_band_streak` | 120 s continuously In band |
| `halfway` | workout elapsed passes half of the total |
| `workout_done` | `Finished` |

The existing on-screen event texts for the same events are removed (the
radio replaces them); the large PULL / DRAFT role cue stays.

## Part 3 — UI (egui)

A dialogue box at the bottom left: portrait (96 × 96), speaker name
(`DS` / the partner's name, constant `"Marco"`), the text with the
typewriter reveal. It sits above the playtest overlay when that is
shown. Portraits are placeholders drawn with egui shapes: a head and
shoulders silhouette on a coloured tile (DS: dark blue tile, cap;
partner: red tile, helmet), with a mouth that opens and closes while
characters are being revealed. No image assets.

## Part 4 — commands and API

`ControlCommand::RadioSay { key }` (400 for an unknown key),
`POST /radio/say`; `GET /state` gains `radio`: `{speaker, key, text,
queue_len}` or null.

## Part 5 — playtest scenarios (owner's checks; do not answer them)

FTP 200, autopilot on.

| File | Stage | Question |
|---|---|---|
| `012-1` | load `hit-30-30`, seek 15 s before the first hard step of the first 20× set, loop 120 s | Do the radio calls tell you early enough what is coming, and are they readable while riding? |
| `012-2` | same set, seek 2 min in, bias −25, loop 60 s | Do the under-band calls feel like a team mate helping, not nagging? |
| `012-3` | same set, seek 2 min in, bias +25, loop 60 s | Do the over-band calls clearly tell you to back off? |
| `012-4` | load `mit-sweetspot-6x5`, seek to 1185 s, speed 4, loop 90 s | Over a few minutes, is the amount of talking about right? |
| `012-5` | load `hit-30-30`, radio say `break_survived`, then `caught` after re-stage | Are the placeholder portraits and the dialogue box acceptable as a starting point? |

## Acceptance (numbers and behaviour only)

- `just check`, `just test`, `just clippy` clean. Unit tests: the lines
  file parses; every key used by the code exists with ≥ 2 variants and
  only known placeholders; every placeholder a line uses is supplied by
  its trigger; priorities, interruption, drop-after-8-s, queue limit;
  no immediate variant repeat; cooldowns; each trigger fires once per
  occasion (simulate `hit-30-30` and `mit-sweetspot-6x5` with the exact
  rider and assert the sequence of keys for the first 10 minutes of the
  main part); with the exact rider no `under_band`, `over_band`,
  `match_burned` or `caught` ever fires on any sample; the Part 0
  detection ranges.
- Scripted (game in the background, port 7880 free first):
  1. Load `hit-30-30`, autopilot, seek to 12 s before the first 20× set:
     within 3 s `radio.key` is `step_warning_pull`; at the step start it
     becomes `pull_start` or `break_start`.
  2. Bias −25 for 20 game-s inside the set: `under_band` appears; it does
     not appear again within 30 game-s.
  3. `POST /radio/say {"key":"nope"}` → 400.
  4. `radio_enabled` false → `radio` stays null through a step change.
- Stop the game; `playtests/results.ndjson` untouched; config directory
  state restored.
- Update `CLAUDE.md` (radio layer, the words rule, lines file, command,
  state field). Move this task's line in `TODO.md` to `done-tasks.md`.

## Out of scope

Sound and voice blips, real portrait art, more characters, other
languages, the DS's auto-regulation offers and sticky bottle, fuelling
prompts, the result screen.

## Appendix — `lines.toml` content

`step_warning_pull` (partner): "Your turn in {secs}. Come through
smooth." / "{secs} seconds, then you're on the front." / "Get ready,
your pull in {secs}."

`step_warning_draft` (partner): "{secs} more, then I come through." /
"Hold it {secs} seconds, then swing off." / "Nearly there. {secs}, then
get on my wheel."

`step_warning_harder` (ds): "Heads up: {low} to {high} watts in {secs}
seconds." / "It kicks up in {secs}. Target {low} to {high}." / "Effort
coming in {secs}. Settle at {low} to {high}, no more."

`step_warning_easier` (ds): "Ease off in {secs}. Down to {low} to
{high}." / "{secs} seconds, then recover at {low} to {high}." / "Almost
done with this one. {secs} to go, then {low} to {high}."

`workout_start` (ds): "Radio check. Nice and steady today, ride the
plan." / "Morning. Stay in your zones and we'll be fine." / "All right,
let's go. The plan is the plan."

`break_start` (ds): "That's the move! Two of you up the road. Work
together." / "You're away! Share the work, keep it smooth." / "Gap's
open. Twenty seconds. Take your turns."

`pull_start` (partner): "You're on. {low} to {high}, steady." / "Your
pull. Keep it at {low} to {high}." / "Take it. Smooth, don't surge."

`draft_start` (partner): "I've got it. Sit in." / "My turn. Get on the
wheel and breathe." / "Tuck in behind me."

`last_rep` (ds): "Last one of this set. Same as the first." / "Final
rep, {rep} of {reps}. Keep the shape." / "One more. Don't change
anything."

`block_done` (ds): "Set done. Good work, spin the legs." / "That's the
block. Easy now." / "Done. Recover properly, it counts."

`bunch_closing` (ds): "Gap is down to {advantage}. Back up to your
watts." / "They're closing, {advantage}. Get back in the zone." /
"Bunch at {advantage}. You're under, bring it back up to target."

`caught` (ds): "They've got you. Sit in the bunch, we go again next
set." / "Caught. No drama. Recover in the wheels and reset." / "That
one's over. Stay with them, the plan carries on."

`break_survived` (ds): "They're not seeing you again! {advantage} at the
line." / "The break stays away, {advantage}. Textbook." / "That's how
you do it. {advantage} in hand."

`under_band` (partner): "You're slipping. Back up to {low}." / "Bit
more, up to {low}. Stay with me." / "Don't let it drop. {low} and hold."

`over_band` (ds): "Easy! Back under {high}. Save your matches." / "Too
hot. Down to {high}, that's the plan." / "No heroics. {high} is the
ceiling."

`match_burned` (ds): "That's a match gone. {matches} left." / "Burned
one. {matches} left, bring it down." / "Match gone, {matches} left.
Back in the zone."

`out_of_matches` (ds): "No matches left. Every surge costs you now." /
"That was the last match. Ride the numbers." / "Box is empty. Stay
under the ceiling."

`back_on_wheel` (partner): "There you are. Good." / "Back on. Nicely
done." / "On the wheel. Stay there."

`dropped` (partner): "I'll wait up the road. Come back at your watts." /
"Lost you. Ride your zone, I'll ease." / "Gap's big. Steady at target
and you'll close it."

`in_band_streak` (ds): "Lovely riding. Just like that." / "Right in the
zone. Keep it boring." / "That's perfect. Don't touch a thing."

`halfway` (ds): "Halfway. Eat something, drink something." / "Half
done. Take a drink." / "That's half. Fuel up and keep going."

`workout_done` (ds): "That's the workout. Proud of that one." / "Done.
Great job today, spin it out." / "Finished. That's money in the bank."
