# Breakaway — done tasks

Narrated archive: what changed, why, how it was verified.

## 021 — Tuning file: speed factor, lateral range, taller workout strip (2026-10-03)

[Spec](tasks/021-tuning-config.md). Branch `tuning-config`, merged `--no-ff`.

- **What changed:** `config.toml` (repo root, committed at the defaults,
  a comment per key) parsed by `src/tuning.rs` (`Tuning`, `parse`, `load`,
  `config_path`; unknown keys and out-of-range values name the key; a bad
  file exits 2 before the window). `speed_factor` multiplies the scripted
  speed in `sim::step` / `step_locked` and in `course::generate`, which
  takes it as a parameter (tests pass 1.0); `WorkoutSession::tuning`
  carries it. `lateral_from_balance(pct, m_per_pct)` uses
  `lateral_m_per_pct` (now 0.25, was 0.3: 40 % is 2.5 m, 30 % still
  clamps at 3.0 m). The progress strip height is `workout_strip_px`
  (48, was 24); `top_offset` is that plus a 6 px gap. Playtest scenarios
  `021-1..4`.
- **Why:** owner playtest 2026-10-02 (003-1 too slow, 003-2 / 003-3 too
  far to the edge, 009-2 strip too low in pixels).
- **Verified:** unit tests per the spec (198 pass: parsing, committed
  file = defaults, course at factors 0.8 and 1.3 within 0.5 s per step
  and length within 1 % of the factor, lateral mapping, factor in
  `sim`), clippy clean. Debug API: with the default file `world_speed_mps`
  10.0 on `flat` and `course_length_m` 37825.3 for `mit-sweetspot-6x5`;
  with `speed_factor = 1.5` 15.0 and 56659.7 (ratio 1.498). With
  `speed_factor = 9` the game prints the reason and exits 2. `config.toml`
  restored; `playtests/results.ndjson` and the settings file untouched.

## 020 — Step countdown: big numbers and beeps before a step change (2026-10-03)

[Spec](tasks/020-step-countdown.md). Branch `step-countdown`, merged `--no-ff`.

- **What changed:** `src/countdown.rs` (pure: `number`, `look`, `Beep`
  levels, `Countdown::step` fed the workout's own advance events) and
  `src/countdown_ui.rs` (the number at the role banner's place, one
  `AudioPlayer` per beep at level × `voice_volume`). `WorkoutEvent` is a
  Bevy message written by `advance_ride`. Setting `countdown_beeps`
  (panel switch, `settings.json`), `ControlCommand::CountdownBeeps`
  (`POST /countdown/beeps`, stage `countdown_beeps`), `countdown` in
  `GET /state`. `tools/gen_sfx.py` / `just sfx` render `assets/sfx/
  tick.ogg` and `go.ogg` through `gen_voices.encode`, which now passes
  `-fflags +bitexact -flags +bitexact` (fixed Ogg serial, no encoder
  tag) so a render is reproducible; `just sfx --check` is part of
  `just test`. Four playtest scenarios `020-1..4`. `__pycache__/` is
  git-ignored (a cache file had slipped into the first commit).
- **Verified:** unit tests per the spec (192 pass), clippy clean, the
  clips reproduce byte for byte. Debug API on `mit-sweetspot-6x5` with
  autopilot, seek 1488: `countdown` 10 → 1 → `null` with `step_index`
  6 → 7 at `null`; a step end with beeps on logs no asset error.
  `playtests/results.ndjson` untouched; the settings file was not
  written by the agent.

## 019 — Playtest re-stage starts from the base (2026-10-02)

[Spec](tasks/019-restage-from-base.md). Branch `restage-from-base`, merged `--no-ff`.

- **What changed:** a re-stage (every `loop_seconds`, or `R`) applies the
  base commands and then the stage, like the first staging; before, it
  replayed only the stage, and a stage that sets a profile and then loads
  a workout was refused the second time (`stage_error` on 009-1 and 009-3
  in the owner playtest). An open comment field and its text survive.
- **Verified:** new test stages every checked-in scenario twice from the
  base on one world; runner tests updated (185 pass), clippy clean.
  `just playtest 009-1` left unanswered: workout time ran to 15 s, went
  back to 0 at the 20 s loop with the workout still loaded, no result
  line written.

## 015 — Playtest comments: a note with each verdict (2026-10-02)

[Spec](tasks/015-playtest-comments.md). Branch `playtest-comments`, merged `--no-ff`.

- **What changed:** `Runner` holds the current note, the open comment
  field (`NoteMode::Fail` from `N`, `Keep` from `C`) and the UI's
  `close_note(Confirm | Cancel)`, acted on by the next `update` (which
  ignores its key while the field is open and in the closing call, so the
  closing `Enter` is never the summary's quit). `Verdict::detail` became
  `Runner::detail(index)` (stage error, else the comment as one line);
  `result_line` takes it. Overlay: `[C] comment` in the key line, a
  one-line text field under the question, the kept note as `Comment: …`.
  New `UiTextEntry` resource: `B` and `D` do nothing while it is set (`B`
  had no focus guard at all before).
- **Why:** a bare `fail` does not say what to fix; failed verdicts become
  tasks.
- **Verified:** unit tests for every case in the spec (184 pass), clippy
  clean, `just playtest 003` starts and answers `GET /state` (stopped
  without a verdict; `results.ndjson` untouched). Typing into the field
  was **not** exercised by an agent (no key injection): owner check in
  TODO admin.

## 013 — Radio voices: pre-generated speech; line fixes (2026-10-01)

[Spec](tasks/013-radio-voices.md). Branch `radio-voices`, merged `--no-ff`.

- **What changed:** Part 0: a breakaway run may not begin in a `ramp`
  block unless it lies inside it; `match_burned` is not said for the last
  match; `{advantage}` rounds down; two line texts. `spoken` arrays in
  `lines.toml` (the spec's, verbatim), checked by the loader.
  `tools/gen_voices.py` (`just voices`, `just voices-check` inside `just
  test`), `assets/radio/voices.toml` (DS `bm_george`, Marco `im_nicola`
  with `lang_code` `a`, speed 1.1), 87 clips + `manifest.json` committed.
  `src/radio_audio.rs` plays the line's clip and stops it with the line;
  lines stay on screen at least as long as their clip. `voice_enabled` /
  `voice_volume` in `settings.json` and the ride panel; `VoiceEnabled`
  (`POST /radio/voice`); `clip` and `voice_enabled` in `GET /state`
  `radio`. Scenarios `013-1..4`.
- **Sets now found:** `hit-30-30` 6–11, 13–52, 54–93; `hidits-4x12`
  6–11, 13–24, 26–37, 39–50, 52–63. Pulls / drafts in `hit-30-30`: 43
  each (was 44); a 25 % under rider is caught after 106 s in every set.
- **Clips:** 87, 2396 KiB; shortest `back_on_wheel-1` 0.957 s, longest
  `bunch_closing-3` 3.448 s; peaks −1.2 to −0.8 dBFS, mean −12.8 to
  −16.9 dB. A second `just voices` regenerated nothing.
- **Verified:** `just check`, `just test` (178 tests + voices-check),
  `just clippy` clean. Game + API: `/radio/say workout_start` → `clip`
  `workout_start-1` (file exists); voice off → `clip` null, text set;
  `hit-30-30` seek 1727 s, autopilot → at 1730.3 s `step_warning_pull`
  with `clip` `step_warning_pull-1-s10`.
- **Deviations:** this machine's ffmpeg has no libvorbis and its own
  Vorbis encoder writes only two channels, so the clips are 24 kHz Vorbis
  with the mono voice in two identical channels (the generator switches to
  mono libvorbis by itself when available; then `just voices --force`).
  The loader also checks the fallback (`text` as spoken text) for
  `{secs}` / `{matches}` only. `voices-check` also fails on stale hashes.

## 012 — Team radio: talking heads that cue and narrate the ride (2026-10-01)

[Spec](tasks/012-team-radio.md). Branch `team-radio`, merged `--no-ff`.

- **What changed:** Part 0: breakaway sets skip `warm_up` / `cool_down`
  steps and allow steps up to 180 s. `assets/radio/lines.toml` (the
  appendix verbatim: 22 keys × 3 variants). `src/radio.rs`: lines loader,
  `Radio` (typewriter, priorities, interruption, drop after 8 s, queue of
  3, seeded variants without repeats, cooldowns), `Triggers` (all keys of
  the spec's table). `src/radio_ui.rs`: dialogue box with placeholder
  portraits. `radio_enabled` in `settings.json` and the ride panel.
  `RadioSay` (`POST /radio/say`), `RadioEnabled` (`POST /radio/enabled`),
  `radio` in `GET /state`. The ride / scenario event texts are gone.
  Scenarios `012-1..5`.
- **Sets now found:** `hit-30-30` unchanged, 4–11 (360 s), 13–52, 54–93
  (1200 s each): the opener still starts in the ramp after the warm-up,
  because those ramp steps are class `active`, not `warm_up`.
  `hidits-4x12`: 4–11, then each pyramid from its first hard step 13–24,
  26–37, 39–50, 52–63 (770 s each). Others: none.
- **Exact rider, first 10 min of the main part** (from the first
  non-warm-up step): `hit-30-30` (900–1500 s) 21 lines (2.1 / min);
  `mit-sweetspot-6x5` (600–1200 s) 5 lines (0.5 / min). The exact rider
  never hears `under_band`, `over_band`, `match_burned` or `caught` on any
  sample.
- **Verified:** `just check`, `just test` (174 tests), `just clippy` clean.
  Game + API (FTP 200, autopilot): seek to 1728 s on `hit-30-30` →
  `step_warning_pull` at 1730.0 (2 game-s), at 1740.0 `pull_start`, then
  `break_start`; bias −25 at 1746 s → `under_band` at 1751.5 s, not again
  in the following 60 game-s; `POST /radio/say {"key":"nope"}` → 400;
  radio off → `radio` null over the step change 15 → 16. One screenshot:
  the box and the partner portrait render.
- **Choices / deviations:** `POST /radio/enabled` added for scripts
  (acceptance step 4); `block_done` only for repetition blocks of more
  than one rep (the warm-up block is a 1-rep "repetition"); under / over /
  streak fire once per continuous spell (a spell that starts inside the
  cooldown gets no call); `{secs}` is the lead (10 or 5), warnings fire on
  the crossing only (not on a seek into the window); a waiting `Cue` freezes
  an `Alert`'s reveal at its sentence end; lines due in the same frame are
  pushed highest priority first; `RadioSay` fills `{advantage}` with 0
  outside a set; `012-5` says both lines on every re-stage
  (`break_survived` shows first, `caught` waits behind it).

## 011 — Scenario director: the two-rider breakaway for on/off reps (2026-10-01)

[Spec](tasks/011-breakaway-scenario.md). Branch `breakaway-scenario`, merged `--no-ff`.

- **What changed:** Part 0: `010-1` gets speed 8 after the seek.
  `src/scenario.rs`: `plan` (breakaway sets), `Break` and `advance`
  (constants as specified), `ScenarioEvent`, `staging`, the `Scenario`
  resource (stepped after ride and workout; replans on
  `WorkoutSession::loads`). `race.rs`: `Ride::hold_gap` (gap held at 0 on
  pulls), `sets_total` / `sets_survived` / `set_advantages_s`. Commands
  `SetAdvantage` and `AutopilotBias` (reset by the playtest base);
  `POST /scenario/advantage`, `POST /autopilot/bias`, `scenario` in
  `GET /state`. Render: partner and camera eased from the staging (swap
  round the player's side), 12 bunch billboards while caught. UI: race
  strip, role line, event texts, `Breaks survived` in the ride panel.
  Scenarios `011-1..6`.
- **Sets found (rule as written):** `hit-30-30` 3 sets: steps 4–11 (360 s:
  60/120 s warm-up end + recovery pair, then 3 × 30/30), 13–52 and 54–93
  (1200 s each). `hidits-4x12` **5** sets, not 4: the same 4–11 opener and
  each pyramid from its second step, 15–24, 28–37, 41–50, 54–63 (470 s
  each; the 180 s first hard step is over 120 s). `lit-1h`,
  `mit-sweetspot-6x5`, `lactate-elimination`: none.
- **Simulated riders (hit-30-30, FTP 200, 0.1 s):** exact: all three
  survive, final advantages 36.5 / 72.8 / 72.8 s (hidits: 36.5, then 42.0
  each), 0 burns. 25 % under: caught in every set after 93 / 106 / 106 s.
  25 % over: never caught, every set ends at exactly 20.0 s, matches gone.
  No over-band trajectory tried (constant +1..+150 W, over on pulls / on
  drafts, surges at each step start, spells, random) ends a set above the
  exact rider.
- **Choices / deviations:** `Idle` loses advantage even during the grace
  (stopping is not a transition, and the grace does not run down while
  idle); the partner's under/over reactions on a pull wait for the grace
  to end; entering a set mid-way (seek) starts a fresh break at 20 s,
  leaving one mid-way drops it without a result; `SetAdvantage` above 0
  un-catches; `Breaks survived` only shows for a workout with sets; the
  race strip's 60 s span is its line, the label sits to its right.
- **Verified:** `just check`, `just test` (156 tests), `just clippy`
  clean. Debug API (FTP 200, autopilot, `hit-30-30`): (1) seek 1740:
  `pull`, advantage 20.0, gap 0 (0.0 over the whole pull), 30 game-s
  later `draft`; (2) set at speed 20: never caught, 72.3 s 2 s before the
  end, `sets_survived` 0 → 1; (3) bias −25: caught after 106.4 game-s (the
  spec's "within 100" does not follow from its own constants; 106 s is
  what they give), advantage 0, workout advanced, next set (3420 s)
  `caught` false, 20.0 s; (4) bias +25, 60 game-s: advantage stayed 20.0
  (gain 0), matches 0; (5) advantage outside a set / without workout 400,
  out of range 400; (6) whole workout at speed 50: finished, 3 / 3
  survived, matches 5, in band 97.6 %; (7) `mit-sweetspot-6x5`:
  `sets_total` 0, `default` throughout, in band 99.8 %, matches 5. Two
  screenshots: race strip, role line and partner beside/behind render;
  the bunch was not caught on camera (a 0 s advantage was undone by an
  in-band frame; fixed, catch then confirmed over the API).

## 010 — Course generated from the workout; band-transition fix (2026-10-01)

[Spec](tasks/010-course-from-workout.md). Branch `workout-course`, merged `--no-ff`.

- **What changed:** Part 0: smoothing 3.0 → 1.5 s, minimum step band
  10 → 20 W, grace until the smoothed power first enters the new band
  (3..10 s, `Grace` in `Ride`); simulated-rider tests (exact, human-like,
  late) over every sample at FTP 200 and 300. `terrain.rs`: `Profile::open`
  (flat straight extensions, blends into them), `is_looping`.
  `src/course.rs`: the generator (gradient table, length solve, 150–500 m
  chunks, bends, rolling, `TimeTable`). Wiring: loading a workout makes its
  course the active profile; time is the master (locked speed, still while
  idle, free after the finish, ≤ 0.05 s sub-steps); skip jumps to the
  step start; `SeekWorkout` / `POST /workout/seek`; `/state` gains
  `course_length_m`, `course_drift_m`. Scenarios `010-1..5`.
- **Choices:** the time table is scaled per step so each step starts
  exactly at its workout time; the solve stops at 0.05 s (spec: 0.5 s);
  rolling also applies to warm-up / cool-down steps over 300 s; a short
  first step (none in the samples) would join the next chunk; unload /
  restart reset the distance to 0; `set_profile` is refused while a
  workout is loaded; seek restarts the step's band and grace and keeps
  totals; loop times 010-2 and 010-4 120 s, 010-3 and 010-5 60 s; 010-4
  skips to the first `Hard` step (the 3× opener, step 7).
- **Verified:** 134 unit tests + export self-test. Part 0: exact rider
  0 burns, in band 95.9–99.98 %; human-like 0 burns; late rider 46 burns on
  `hit-30-30`. Generator: every boundary within 0.04 s when ridden in
  1/60 s steps. Scripted (FTP 200, autopilot): `mit-sweetspot-6x5` course
  37 825 m, step 0 at 0 %; `low SwSp` at 2.5 % after ≈ 3 s, 130–150 W;
  speed 50 to the end: 4200 s, matches 5, in band 99.7 %, drift ≤ 0.7 m;
  `hit-30-30`: matches 5, in band 98.7 %, drift ≤ 1.1 m; 0 W override:
  distance and elapsed stop after ≈ 3.0 s, resume on clearing; unload →
  `demo`.

## 009 — Workouts: format, TrainingPeaks export, runner, bands per step (2026-10-01)

[Spec](tasks/009-workouts.md). Branch `workouts`, merged `--no-ff`.

- **What changed:** Part 0: smoothed power snaps to the raw value when
  riding off from idle; `008-4` gap 10 m. `src/workout.rs`: JSON format +
  loader, `WorkoutRunner` (f64 times), `step_band` (10 W minimum),
  `WorkoutSession` (band per step, auto-pause while `Idle`, finish).
  `race.rs`: `TRANSITION_GRACE_S` / `begin_grace`, `finish` (`finished`
  state). Settings `ftp_w`. Autopilot in `input.rs`. Commands
  `load_workout`, `unload_workout`, `skip_step`, `set_ftp`, `autopilot`;
  endpoints `/workout/load|unload|skip`, `/ftp`, `/autopilot`; `/state`
  gains `ftp_w`, `autopilot`, `workout`; CLI `--workout`. UI: step and
  next lines, progress strip, `Workout complete` + totals, panel Workout
  section. `tools/tp_export.py` (+ `fetch-raw` for fixtures), five raw
  fixtures and samples, `just fetch-workout[-id]`. Scenarios `009-1..5`.
- **Deviations / choices:** restart (panel or API) always unloads the
  workout (free ride, band from before it); `/band` is refused while a
  workout is loaded; blocks left empty after dropping lap markers get no
  block number; MtnBike counts as bike; completed workouts are not
  filtered out; share keys are stripped from raw fixtures too; `009-3`
  skips to the first 30/30 block (the 3× opener, step 7).
- **Verified:** 111 unit tests + export self-test. Scripted: load →
  19 steps / 4200 s, 80–100 W, 5 matches; 6 skips → `low SwSp` rep 1/6,
  130–150 W; speed 50 to the end → finished, 4200 s, in band 98.7 %,
  5 matches; 0 W override → elapsed kept running ≈ 4.5 s while the
  smoothed power fell below 20 W, then stopped, and resumed on clearing;
  FTP 250 → band × 1.25; missing file → 400, ride unchanged;
  `just fetch-workout 2026-01-09` → byte-identical to the sample (plus a
  second identical workout that day, id 100000006).

## 008 — Free ride: band, holding the wheel, matches, score (2026-10-01)

[Spec](tasks/008-free-ride.md). Branch `free-ride`, merged `--no-ff`.

- **What changed:** `src/race.rs`: pure `Ride` / `step(ride, power_w, dt)`
  with the spec's constants (3 s smoothing, Idle below 20 W, 5 matches,
  burn every 4 s Over, gap 0.5 m/s closing / 0.2 m/s per 10 % under, max
  60 m, score 1/s in band + 1/s on the wheel, 50 per burn at zero).
  Stepped in `GameSet::Sim`; partner at `PARTNER_AHEAD_M + gap_m`.
  `src/settings.rs`: band in `<config dir>/breakaway/settings.json`
  (default 120–160 W), written only by the ride panel. `ControlCommand`
  `SetBand`/`RestartRide`/`SetGap`/`SetMatches`; the playtest base restores
  the saved band and restarts the ride. Debug API `POST /band`,
  `/ride/restart`, `/ride/gap`, `/ride/matches`, ride fields in `/state`.
  `src/ride_ui.rs`: band bar + match row, `OVER` line, 2 s event texts,
  ride panel (`B`); HUD gains `GAP`, `SCORE`, `IN BAND`. Scenarios
  `008-1..6`.
- **Deviations:** acceptance step 1 expects score ≈ 24–28 after 20 s at
  200 W; the spec's own rules give ≈ 20.5 (measured 20.9): the ramp from
  0 W counts as Under from 20 W on and opens a ≈ 3.9 m gap, so the wheel
  bonus starts ≈ 5.8 s after the band is reached. The rules were kept.
  The panel refuses a step that would leave low less than 10 W below high
  (or low ≤ 0, high > 2000) instead of clamping. `SetBand` keeps the ride
  running. `SCORE` shows whole points earned (rounded down). `008-6`'s
  question says that the scenario sets 180–220 W again every 30 s.
- **Verified:** 86 unit tests (classification and edges, smoothing 95 % at
  ≈ 9 s, gap rates/cap/max, burn timing 3.9/4/12 s, restart of the 4 s,
  out of matches once, penalty and floor, Idle freeze, score rates, the
  one rule as a test, events once per crossing, commands, settings round
  trip, panel steps, bar scale, `OVER` line, toasts, `--playtest 008` → six
  scenarios). Scripted at 2× on `flat`, 180–220 W: 200 W 20 s → `in`, gap
  0, score 20.9, 5 matches; 150 W 20 s → `under`, gap 5.23, score +2.6;
  200 W → gap 5.6 → 0 at ≈ 0.5 m/s; 260 W 14 s → `over`, 2 matches, gap
  unchanged, score +2.3; matches 0 + 9 s → score 39.5 → 0; invalid band
  → 400; mock without override 60 s → `in` throughout. No settings file
  created; band bar and ride panel render (two screenshots).

## 007 — Real devices + UI in egui (2026-10-01)

[Spec](tasks/007-devices.md). Branch `devices`, merged `--no-ff`.

- **What changed:** `src/devices.rs` (`DevicesPlugin`): tokio runtime (2
  workers), `trainer_core::Services`, `BleManager` created at start (10 s
  timeout, Retry), results and readings over an mpsc channel drained each
  frame; `DeviceState`, `LiveMetrics` (newest reading per metric wins),
  mock presets `kickr`/`quarq`/`hr`, known devices in
  `<config dir>/breakaway/known_devices.json` with scan-and-reconnect on a
  normal start. `input::select_input` (override → device → mock, 3 s
  staleness). HUD, devices panel (`D`) and playtest overlay in egui
  (`bevy_egui` 0.40); no Bevy UI left. Debug API: `GET /devices`,
  `POST /devices/{scan,connect,connect-mock,disconnect}`, `input_origin`,
  `cadence_rpm`, `hr_bpm` in `/state`. Scenarios `007-1..5`.
- **Deviations:** mocks run through `trainer_core::mock::spawn` and are
  tracked in the game, because `BleManager::connect_mock` needs an adapter;
  the start-up scan only runs when there are known devices; the panel's
  auto-open is decided after that reconnect; the playtest overlay puts
  title, keys and progress on one line so a two-line question still fits
  in three lines.
- **Verified:** 62 unit tests (`select_input` branches and the 3 s edge,
  newest-wins, known-devices round trip in a temp dir, presets, HUD text,
  007 selection). Scripted run with mock devices: `mock` at start; quarq →
  `device`, power/balance/cadence in range and changing, lateral non-zero;
  hr → `hr_bpm` in range; override → `override` and back; disconnect →
  `mock`, `hr_bpm` null; scan `true` → `false` after 5 s with the adapter
  `ready` (no devices advertising); no known-devices file written;
  `--playtest 003` stages `flat`. Real hardware: owner playtests `007-*`.

## 006 — Staged playtests: the machine stages, the human judges (2026-10-01)

[Spec](tasks/006-staged-playtests.md). Branch `staged-playtests`, merged `--no-ff`.

- **What changed:** `src/control.rs`: `ControlCommand` (teleport, input,
  set_speed, set_profile) and `apply`, used by the debug API and the
  playtest runner alike; `POST /set-profile`, `profile` in `/state`;
  `Profile::by_name` and `Profile::name`; render rebuilds the road strips
  and base-plane height on a profile swap. `src/playtest/`: scenario
  loader (`toml`, name check, selection), pure runner state machine,
  bottom overlay, `results.ndjson` writer; `build.rs` bakes the short
  commit. `--playtest <sel>` / `just playtest <sel>`. Eleven scenarios for
  tasks 003–005 in `playtests/`; `CLAUDE.md` gains a Validation section.
- **Distances tuned** (from the spec's table, by simulating the scripted
  speeds over the loop): `004-2` 100 → 60 m (≈12 s of approach before the
  climb instead of 8 s); `004-3` 540 → 560 m (from 540 the 20 s loop ends
  at 650 m, 14.5 s to reach the crest; from 560 the crest comes at ≈10 s
  and the loop ends at the descent edge, 700 m). All others as specified.
- **Keys** are read as logical keys, so `Y` is the key labelled Y on a
  QWERTZ keyboard (physical `KeyCode::KeyY` would be the Z key there).
- **Verified:** 49 unit tests (scenario parsing and stage application,
  runner order/verdicts/re-stage/stage error/summary, selection `004` →
  four scenarios, result line, ISO time). Debug API by script:
  set-profile flat → `profile: "flat"`, distance ≈0.16, gradient 0; `nope`
  → 400; teleport/input/set-speed incl. error cases unchanged.
  `--playtest nope` exits 2 without a window. `just playtest 003` opened
  with the `003-1` overlay (one screenshot, overlay text only), stopped
  with Ctrl-C: table printed, exit 1, no results line. The playtest
  verdicts themselves are the owner's (see TODO admin).

## 005 — Corners (combined with climbs and descents) (2026-10-01)

[Spec](tasks/005-corners.md). Branch `corners`, merged `--no-ff`.

- **What changed:** `Segment` gains `curve_deg`; curvature is constant per
  segment and smoothstep-blended over the same 40 m zones as the gradient
  (heading change preserved). `Profile` precomputes heading and position
  per lap at 1 m (trapezoid, f64) plus the end-of-lap pose; lap n composes
  that pose n times (inverse for negative laps), so the course need not
  close. New `curvature_at`, `heading_at` (unwrapped), `centre_at`,
  `right_at`; `new` panics below 40 m radius. Profiles: `flat`, `hills`
  (004's straight demo), `demo` (default, with bends and a hairpin).
  `RaceState.heading_deg`; `/state` adds `heading_deg`, `x_m`, `y_m`.
  Render: tiles replaced by three strip meshes (shoulder ±30 m, road,
  edge lines + absolute-distance dashes) from cross-sections every 3 m over
  720 m, rebuilt in place only when the 3 m window moves,
  `NoFrustumCulling`; flat 8 km base plane at lowest drawn height − 5 m;
  backdrop is a ring (far-land wall r 3000 m, 48-peak mountain ring
  r 3001 m) following camera position only; posts, riders and camera on the
  bending centre line.
- **Spec correction:** expected headings are 1.125° lower than the task's
  numbers (650 → 88.875, 925 → −31.125, 1225 → 88.875, 1675 → 178.875,
  2100 → 88.875): the wrap blend of the last segment (−90° over 300 m)
  still turns 0..20 m, where 3/32 of its 40 m zone lies, and heading is
  pinned to 0 at distance 0 — the same effect as 004's 0.1875 m elevation
  offset. Per-lap turn is exactly 90°.
- **Bug found and fixed during verification:** with the shoulder cut only
  at ±30 m, the shoulder poked through the road as green bands on the
  climbing right-hander (a quad spanning a bend on a slope is not planar,
  so differently cut layers cross). All three strips now share one set of
  lateral cuts, so overlapping layers have identical plan triangulation;
  a unit test checks it.
- **Verified:** `just check` / `just test` (31 tests) / `just clippy`
  clean. `/state` at multiplier 0.01: heading 88.87 at 650, 178.87 at
  1675; speed 4.0 at 480, 14.8 at 775. Balance 40 % for 3 s: lateral
  −2.99 on both flat and demo at 480; player left of the partner in both
  (`/tmp/breakaway-c-lateral.png`, `-lateral-flat.png`). Screenshots
  `/tmp/breakaway-c-{flat,hills-climb,hills-descent,start,climb-right,
  descent-left,hairpin,after,seam}.png` plus motion triples
  `/tmp/breakaway-m-*-{1,2,3}.png` at multiplier 1 in every
  curve-plus-gradient section and across the seam: no gaps, no layer
  crossings, posts on both edges, riders on the road, no sky below the
  horizon (pixel scan), HUD unchanged. Multiplier 20 from 0 to 2310 m: no
  panic; 2010 m is a straight flat road heading 89°.

## 004 — Terrain: climbs and descents (2026-10-01)

[Spec](tasks/004-terrain-hills.md) incl. Revision 1. Branch `terrain-hills`,
merged `--no-ff`.

- **What changed:** new pure `terrain.rs` (`Profile` of constant-gradient
  segments, 40 m smoothstep blends at every joint incl. the wrap, 1 m
  elevation table, `demo()` 2000 m loop, `flat()`, `speed_for_gradient`).
  `sim::step` takes the profile: gradient sets world speed, power never
  does; `RaceState` gains `elevation_m`, `gradient_pct`. `--profile
  flat|demo` CLI flag. HUD lines `GRAD +6.0 %` / `ELEV 24 m` (true values).
  Debug API: `/state` adds both fields, new `POST /teleport`. Render: 60
  pitched 12 m tiles, each carrying a 600 m ground strip (single ground
  plane removed); posts, riders, camera at profile height; render-only 2x
  vertical exaggeration; camera pitch follows half the slope; level
  backdrop (far land + mountain ridge) 3000 m ahead, following camera
  position only; far clip 8000 m.
- **Spec correction:** expected elevation at 620 m is 23.8125, not 24.0 ±
  0.1 — the wrap blend straddles distance 0, so 0.1875 m of descent lies in
  0..20 m (accepted by the planner).
- **Why Revision 1:** the first build's fully slope-following camera made a
  steady climb look flat (horizon even lower than on the flat).
- **Verified:** `just check` / `just test` (19 tests) / `just clippy`
  clean. Debug API at multiplier 0.01: teleport 100 → grad 0.0, speed 10.0;
  400 → grad 6.0, speed 4.0, elev 11.81; 585 → grad 5.74, elev 22.91;
  1000 → grad -4.0, speed 14.8. Multiplier 1, teleport 300, 400 W: after
  3 s speed 4.0 (distance 312.4). Screenshots (y in 2000x1125 view):
  `/tmp/breakaway-ref-flat.png` (`--profile flat`) horizon `y_flat` = 393,
  mountains on it, no seam between strips and far land; climb hill top
  y ≈ 331 (62 px above), mountains fully hidden; crest road ends at an
  edge at y ≈ 471 against the far land; descent road bottoms out at
  y ≈ 460 (67 px below) with far land + mountains above; flat (demo, 100 m)
  shows the climb ahead rising to y ≈ 270. Pixel scan: zero sky pixels
  below the horizon in all five; riders and posts on the surface.

## 003 — Bevy skeleton: rider on a pseudo-3D road (2026-10-01)

[Spec](tasks/003-bevy-skeleton.md). Branch `skeleton`, merged `--no-ff`.

- **What changed:** Cargo project (`bevy 0.19`, `debug-api` feature with
  `tiny_http`), `justfile` (errors-only `check`/`build`/`test`/`clippy`/`run`).
  `sim.rs` (pure balance → lateral mapping + `step`, unit-tested),
  `input.rs` (`InputSource` + deterministic `ScriptedMock`: 200 W ± 15 W
  over 40 s, 50 % ± 4 over 25 s, one reading per simulated second),
  `render.rs` (fixed-yaw chase `Camera3d`, green ground, recycled 12 m road
  tiles with edge lines and dashed centre line, posts every 20 m, unlit
  camera-facing rider quads), `hud.rs`, `debug_api.rs` (`GET /state`,
  `POST /input`, `/set-speed` via `Time<Virtual>` relative speed,
  `/screenshot`). Debug API table added to `CLAUDE.md`.
- **Deviation:** the spec's prose says "left-heavy → negative x", but its
  acceptance check wants `balance_left_pct` 40 → `lateral_m` ≈ -3.0. The
  code follows the acceptance check: `lateral = (balance_left_pct - 50) *
  0.3`. One sign to flip in `sim.rs` if the prose was meant.
- **No CLI flags:** the spec lists "CLI flags" for `main.rs` but names
  none; none were added (`just run` passes args through for later).
- **Verified:** `just check`, `just test` (6 tests), `just clippy` clean.
  `just run` + curl: two `GET /state` 2 s apart: distance 1.9 → 24.7 m
  (+22.7 m incl. request latency; an earlier run measured +20.8 m).
  Override 200 W / 40 %: `lateral_m` -2.99 after 3 s; 60 %: +2.99. 400 W /
  50 %: `world_speed_mps` 10.0. `/set-speed` 5: +53 m in ~1 s. Screenshots
  `/tmp/breakaway-left.png` / `-right.png` show the perspective road with
  dashed centre line and edge lines, red partner on the centre line, blue
  player at the left / right road edge, HUD (`POWER 200 W`, `BAL 40 / 60`,
  `SPEED 36.0 km/h`, `DIST 0.06 km`) readable.

## Owner playtest 2026-10-02

`just playtest all` at `d648b5d` (first run with comments, task 015: the comment field worked): 47 scenarios, 38 pass, 7 fail, 2 stage errors. Verdicts and comments, quoted from `playtests/results.ndjson`:

- 003–005, 007 (real hardware: trainer, pedals, HR, reconnect), 008, 010: all pass. Remarks: 003-1 "a little to slow. I think we need to externalize a speed factor in a config.toml"; 003-2 / 003-3 the rider is a little too far out at the left / right; 010-1 "how about a count in the middle of the screen. counting second + beeps. counting down from 10 to 5 white letters. 5-0 gets bigger, beep louder, and letters get red."
- 009: 009-2, -4, -5 pass (009-2: "the workout could be displayed higher, more vertical pixesl"); 009-1 and 009-3 `stage_error` "a loaded workout owns the course; unload it first" (re-stage bug, not a verdict).
- 011: 011-1, -2 pass; 011-3 fail "nope"; 011-4 fail "nope"; 011-5 fail "looks more like they are riding next to us. but maybe that is the idea?"; 011-6 fail "the breakway bar does not read so clear."
- 012: all pass except 012-3 (over-band calls) fail: 'can be more pushy. "push harder!"'. Clarified by the owner afterwards: this was about dropping **below** the band; Marco is too soft-spoken to encourage. Whether the over-band calls themselves are fine is not answered.
- 013: 013-1, -4 pass; 013-2 fail "the intonation of marco is off. hard to tell when he finished a sentence and starts a new one."; 013-3 fail "intonnation and accent are terribe."

Follow-ups are listed in `TODO.md`.

## Owner playtest 2026-10-01

`just playtest all`: all eleven scenarios (003-1..3, 004-1..4, 005-1..4) answered yes, including the steady climb, the hairpin's inside edge and the lap seam. The weak spots agents had flagged (faceted hairpin edge, 30 m land ribbon, leaning sprites, steady climb) are accepted as they are for now.
