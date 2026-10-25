# 020 — Step countdown: big numbers and beeps before a step change

**Repo:** `~/claude/breakaway/`, branch `step-countdown`, `--no-ff` merge
to `main`.

**Depends on:** 019 (merged). Read `CLAUDE.md`, `src/workout.rs`,
`src/ride_ui.rs`, `src/radio_audio.rs`, `tools/gen_voices.py`.

## Why

Owner playtest 2026-10-02, 010-1: "how about a count in the middle of the
screen. counting second + beeps. counting down from 10 to 5 white letters.
5-0 gets bigger, beep louder, and letters get red."

The countdown is neutral cueing: it says *when* the step changes, never
how hard to ride. The one rule is not touched.

## Part A — logic (`src/countdown.rs`, pure)

- `number(remaining_s, step_duration_s) -> Option<u32>`: the number on
  screen. `ceil(remaining_s)` while `0 < remaining_s <= start`, else
  `None`. `start` is 10 for steps of 20 s or more, 5 for shorter steps
  (the same rule the radio's step warnings use).
- It runs before **every** step end, the last step included, and only
  while a workout is loaded and not finished.
- `Countdown::step(number) -> Option<Beep>`: a beep when the number
  changes to a lower one or first appears (`Beep::Tick { n }`), and
  `Beep::Go` when the step ends while a number was showing. No beep when
  the number disappears or rises because of a skip, seek, restart, load
  or unload. At most one beep per frame.
- The workout only advances while the ride is not `Idle`, so the
  countdown freezes with it.

## Part B — display (`src/ride_ui.rs`, egui)

- Centre of the screen, at the place of the role banner (the two never
  show together: the banner starts when the countdown ends).
- 10–6: white, 72 px. 5–1: red (the over-band red), growing linearly from
  96 px at 5 to 160 px at 1. Monospace, bold, with the dark backing tile
  the role banner uses. Nothing is shown at 0.

## Part C — beeps

- Two generated clips, committed: `assets/sfx/tick.ogg` (880 Hz sine,
  120 ms, 5 ms fades) and `assets/sfx/go.ogg` (1320 Hz, 400 ms). A
  `tools/gen_sfx.py` writes them with the Ogg Vorbis encoder that
  `tools/gen_voices.py` already uses (share the code, do not copy it);
  `just sfx` runs it. No server needed.
- Level (× `voice_volume`): ticks 10–6 at 0.35; 5–1 rising linearly from
  0.5 at 5 to 1.0 at 1; `go` at 1.0.
- New setting `countdown_beeps` (default true): switch `Countdown beeps`
  in the ride panel, saved in `settings.json` like the other switches.
  Off: numbers only.
- Beeps play over a radio line; the radio is not changed.

## Part D — debug API and commands

- `GET /state` gains `countdown`: the number on screen or `null`.
- `POST /countdown/beeps {"on": bool}` (`ControlCommand::CountdownBeeps`,
  never writes the settings file); stage command `countdown_beeps`.

## Tests (unit)

- `number`: 10.0 → 10, 9.01 → 10, 9.0 → 9, 0.2 → 1, 0.0 → none, 10.5 →
  none; a 15 s step starts at 5; a 20 s step at 10.
- Beeps over a simulated step end at 60 fps: ticks 10, 9, … 1, then one
  `Go`; nothing while `Idle`.
- A seek into the last 3 s of a step: one tick for 3, then 2, 1, `Go`.
  A seek away while a number shows: no `Go`.
- Sizes and levels at 10, 6, 5, 3, 1.
- Every sample workout ridden by the exact rider: one `Go` per step.

## Playtest scenarios (owner's checks; do not answer them)

| File | Stage | Question |
|---|---|---|
| `020-1` | FTP 200, autopilot, load `mit-sweetspot-6x5`, seek 12 s before the first sweet-spot rep, loop 30 s | Does the countdown make the moment of the step change unmistakable? |
| `020-2` | same, seek 12 s before the end of that rep | Are the red numbers readable without covering the road ahead too much? |
| `020-3` | FTP 200, autopilot, load `hit-30-30`, seek 1740 (start of the first 20× set), loop 90 s | Through a 30/30 set (a countdown every 30 s), do the numbers and beeps help rather than annoy? |
| `020-4` | as `020-1` | Are the beeps loud enough against your fan, and is the step up in loudness from 5 noticeable? |

## Acceptance

- `just check`, `just test`, `just clippy` clean; `just sfx` reproduces
  the committed clips byte for byte.
- Scripted through the debug API: load `mit-sweetspot-6x5` with
  autopilot, seek to 12 s before a step end, and read `countdown` going
  10 → 1 → `null` with the step index advancing at `null`.
- Update `CLAUDE.md` (countdown, setting, state field, command, `just
  sfx`), `playtests/README.md` (stage command), `DESIGN.md` is already
  updated. Move this task's line in `TODO.md` to `done-tasks.md`.

## Out of scope

Spoken numbers, a countdown in free ride, changing the radio's step
warnings, any other sound effect.
