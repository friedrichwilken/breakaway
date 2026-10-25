# 013 — Radio voices: pre-generated speech; line fixes

**Repo:** `~/claude/breakaway/`, branch `radio-voices`, `--no-ff` merge to
`main`.

**Depends on:** 012 (merged). Read `CLAUDE.md`, `DESIGN.md` ("Team radio"),
`assets/radio/lines.toml`, `src/radio.rs`, `src/radio_ui.rs`,
`src/scenario.rs` first.

## Goal

Every radio line is also heard: a pre-generated voice clip. Reading a
dialogue box at 110 % of FTP is not possible; hearing a call is. Clips
are generated offline by a script from the lines file, using the owner's
local Kokoro text-to-speech server. The game only plays files.

**No radio effect in this task** (owner's decision, 2026-10-01: static,
squelch and filtering are ornaments for later). Clips are the plain
voice.

## Part 0 — fixes from task 012

1. **Set detection** (`scenario.rs`): a run may not begin with a step
   from a `ramp` block unless the whole run lies inside that one block.
   Result to assert in tests: `hit-30-30`'s opener set starts at the
   first 30 s hard step (no longer in the warm-up ramp); the HIDIT
   pyramids (each one `ramp` block) still qualify from their first hard
   step.
2. **`match_burned` is not said for the last match**; `out_of_matches`
   alone is said then.
3. **`{advantage}` rounds down** to the second (a 9.6 s gap reads
   `0:09`).
4. Line text changes in `lines.toml`:
   - `workout_start` variant 2: "Morning. Stay in your zones and we'll be
     fine." → "Here we go. Stay in your zones and we'll be fine."
   - `under_band` variant 2: "Bit more, up to {low}. Stay with me." →
     "Bit more, up to {low}. Hold it there."

## Part 1 — spoken text (`lines.toml`)

Each `[[line]]` gets an optional `spoken` array, parallel to `text` (same
length). Where it is absent, the spoken text equals `text`. A spoken
template may use only `{secs}` (values 10 and 5) and `{matches}` (values
1–4); the generator renders every combination. Watts, rep counts and
time gaps stay on screen only.

Add exactly these `spoken` arrays (keys not listed have none):

- `step_warning_harder`: "Heads up. Effort in {secs} seconds." / "It
  kicks up in {secs}. Hit your target, no more." / "Effort coming in
  {secs}. Settle at target."
- `step_warning_easier`: "Ease off in {secs}." / "{secs} seconds, then
  recover." / "Almost done with this one. {secs} to go."
- `pull_start`: "You're on. Steady." / "Your pull. Keep it on target." /
  "Take it. Smooth, don't surge."
- `last_rep`: "Last one of this set. Same as the first." / "Final rep.
  Keep the shape." / "One more. Don't change anything."
- `bunch_closing`: "Gap is coming down. Back up to your watts." /
  "They're closing. Get back in the zone." / "The bunch is coming. You're
  under, bring it back up to target."
- `break_survived`: "They're not seeing you again!" / "The break stays
  away. Textbook." / "That's how you do it."
- `under_band`: "You're slipping. Back up to target." / "Bit more, up to
  target. Hold it there." / "Don't let it drop. Hold your target."
- `over_band`: "Easy! Back under the ceiling. Save your matches." / "Too
  hot. Bring it down, that's the plan." / "No heroics. Stay under the
  ceiling."

The loader validates: `spoken` length equals `text` length; only the two
allowed placeholders.

## Part 2 — generator (`tools/gen_voices.py`, run by `just voices`)

- Speech comes from the owner's local Docker container `tts`
  (Kokoro-FastAPI, OpenAI-compatible) at `http://127.0.0.1:8880`:
  `POST /v1/audio/speech` with JSON `{"model": "kokoro", "input": text,
  "voice": id, "response_format": "wav", "speed": s, "lang_code": c}`
  (`lang_code` optional). `GET /v1/audio/voices` lists the voice ids.
  A working example is `audition/gen.sh` in the repo root (untracked,
  leave it alone).
- If the server does not answer, the script prints
  `start it with: docker start tts` and exits 1. It does not start,
  stop or change the container, and it talks to no other host.
- Config `assets/radio/voices.toml`:

  ```toml
  [ds]
  voice = "bm_george"
  speed = 1.1
  [partner]
  voice = "im_nicola"
  lang_code = "a"      # English reading with the Italian voice
  speed = 1.1
  ```

  An unknown voice id → error listing the available ids.
- For each key, variant and placeholder combination: request WAV, then
  `ffmpeg` (at `/opt/homebrew/bin/ffmpeg`) → mono 24 kHz Ogg Vorbis at
  `assets/radio/audio/<key>-<variant>[-s<secs>][-m<matches>].ogg`, with
  leading and trailing silence trimmed to about 50 ms and all clips
  peak-normalised to the same level. No other processing.
- Incremental: a manifest `assets/radio/audio/manifest.json` maps each
  file to a hash of (spoken text, voice, lang_code, speed) and stores
  the clip's duration in seconds; only changed clips are regenerated;
  clips with no line are deleted. `just voices --force` regenerates
  everything.
- `just voices-check` (also part of `just test`): verifies, without the
  server, that the manifest covers exactly the clips the lines file and
  voices config require and that every file exists. It must pass on a
  fresh checkout, so the generated `.ogg` files and the manifest are
  committed.

## Part 3 — playback (`src/radio_audio.rs`)

- Bevy audio (enable the feature needed for Ogg Vorbis if it is not
  already on). When a line starts being shown, its clip plays; if the
  line is interrupted or dropped, the clip stops. Missing clip → text
  only, no error.
- Clip choice is a pure function `clip_name(key, variant, secs, matches)
  -> String`, unit-tested against the manifest for every line and
  combination.
- The dialogue stays on screen at least as long as its clip
  (`max(current rule, clip duration)`; durations come from the manifest,
  which stores each clip's length in seconds).
- Settings: `voice_enabled` (default true), `voice_volume` (0.0–1.0,
  default 0.8), both in the ride panel and `settings.json`. Text radio
  works with voice off.
- `GET /state` `radio` gains `clip` (file name or null) and
  `voice_enabled`; `ControlCommand::VoiceEnabled { on }`,
  `POST /radio/voice`.

## Part 4 — playtest scenarios (owner's checks; do not answer them)

| File | Stage | Question |
|---|---|---|
| `013-1` | radio say `workout_start`, then `break_start` | Does the DS's voice fit a sports director, and is every word understandable? |
| `013-2` | load `hit-30-30`, autopilot, seek 12 s before the first hard step of the first 20× set, loop 120 s | With your eyes off the text, do the voices alone tell you what to do and when? |
| `013-3` | radio say `draft_start`, then `back_on_wheel`, then `under_band` | Does Marco sound like a different person from the DS, and is he understandable? |
| `013-4` | same set as 013-2, seek 2 min in, bias +25, loop 60 s | Is the amount and loudness of talking comfortable against your own music or fan noise? |

## Acceptance (numbers and behaviour only)

- `just check`, `just test` (incl. `voices-check`), `just clippy` clean.
- `just voices` runs to the end (the container is running; if it is
  not, report that and stop: do not start it yourself); report the
  voices used, the number of clips, total size on disk, and shortest / longest clip.
  Running it a second time regenerates nothing.
- For three clips, check with `ffprobe` that they are mono, 24 kHz,
  Vorbis, and between 0.5 s and 8 s long. You cannot judge how they
  sound; do not try (the owner's playtest does that).
- Scripted (game in the background, port 7880 free first): `POST
  /radio/say {"key":"workout_start"}` → `radio.clip` names an existing
  file; with voice disabled → `radio.clip` null while `radio.text` is
  set; a key with `{secs}` after a seek that triggers the 10 s warning →
  clip name ends in `-s10`.
- Part 0: unit tests for the detection ranges, the last-match rule and
  the rounding.
- Stop the game; `playtests/results.ndjson` untouched; config directory
  state restored.
- Update `CLAUDE.md` (voice pipeline, `just voices`, that clips are
  generated files which must be regenerated after editing lines, state
  fields, command). Move this task's line in `TODO.md` to
  `done-tasks.md`.

## Out of scope

Radio effects (filter, static, squelch), online or paid voice services,
cloning or imitating any real person's voice, spoken numbers for watts
or gaps, music, other sound effects, real portrait art, other languages.
