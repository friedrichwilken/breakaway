# Breakaway — Claude instructions

Single-player indoor-trainer game in Rust + Bevy 0.19. The planned workout
is staged as a bike race; staying in the target band is what scores. Full
concept: [DESIGN.md](DESIGN.md). Read it before any design decision.

## The one rule

**Score comes from staying in band, never from watts.** Over the band is
penalised like under it. The race is a staged play: workout steps drive the
world, the rider's watts only decide how well they hold their place. Any
mechanic that rewards exceeding the band is wrong, however fun. **It
applies to words too:** no radio line may tell the rider to exceed the
band; under-band lines call them back up to the band, over-band lines back
down.

## Planning vs implementing

- Planning sessions produce `DESIGN.md` changes and task specs in
  `tasks/NNN-name.md`. They do not write game code.
- Implementation sessions take exactly one task file and build it. The task
  body is the contract: if it is ambiguous, stop and report instead of
  guessing; do not widen scope.
- Tasks are GitHub issues (`gh issue list`);
  the pinned issue `Backlog (ordered)` (#20) is the order. Labels: `task`
  (has a spec in `tasks/`, ready), `needs-spec`, `proposal`, `playtest`,
  `admin` (owner), `trainer-core`, `later`. A planning session files the
  issue and adds it to the backlog list; a finished task closes its issue
  with what changed and how it was verified (`done-tasks.md` is history
  up to task 021 and gets no new entries).

## Repos touched

| Path | What | Workflow |
|---|---|---|
| `~/claude/breakaway/` | this game, public on GitHub | local feature branches, `--no-ff` merge to `main`, push `main`; `main` has a ruleset: direct pushes only by the owner (admin bypass), everyone else via PR, no force pushes or deletion; no CI: `just setup` once per clone installs the git hooks (pre-commit `just lint`, pre-push `just gate`) |
| `~/claude/libs/cycling-ble/cycling-ble/` | BLE GATT parser crate, public on GitHub | branch + GitHub PR; no personal context in that repo, ever |
| `~/claude/libs/trainer-core/` | shared BLE manager / alerts / mock / recording (created by task 002) | local branches until a remote exists |
| `~/claude/athlete/trainer-alerts/app/` | Tauri app, the other consumer | branch + GitHub PR, see its own `CLAUDE.md` |

Never commit feature work directly to `main` in any of them. One commit per
meaningful change.

## cycling-ble is under evaluation

Breakaway is the second consumer of `cycling-ble` and its API test before a
crates.io 1.0. If the crate's API is awkward or missing something, add an
entry to `~/claude/libs/cycling-ble/TODO.md` and fix it in the crate. Do not
work around it in game code. Nothing game-specific goes into the crate.

## Architecture rules

- Game logic is plain 2D (x = lateral, y = distance along the road) and has
  no rendering types in it, so it is unit-testable without a window.
- Rendering is a separate pseudo-3D layer (`Camera3d` chase cam, ground
  plane, camera-facing billboards), after `~/claude/after-zelda/src/systems/pseudo3d.rs`.
  Read that file for the pattern; do not depend on that repo.
- Time-dependent logic takes `now` / `dt` as a parameter (same shape as
  `AlertEngine::ingest`), never reads the clock itself.
- All UI is egui (`bevy_egui`); no Bevy UI nodes.

## Devices

- The device layer is `src/devices.rs` (`DevicesPlugin`) on top of
  `trainer-core`; the panel is `src/devices_panel.rs` (`D` toggles it).
  The plugin owns a tokio runtime (2 workers), the `Services` and the
  `BleManager`. Mock presets (`kickr`, `quarq`, `hr`) run through
  `trainer_core::mock::spawn`, so they work without a Bluetooth adapter.
- **Never block a Bevy system on async work.** An action spawns a task on
  the runtime; its result comes back over a `std::sync::mpsc` channel that
  a system drains every frame. Readings arrive the same way.
- Input priority (`input::select_input`): debug-API override, else any
  connected device (power and pedal balance only if younger than 3 s, else
  0 W / 50 %), else the scripted mock. `GET /state` shows `input_origin`.
- Real devices that connected once are saved to
  `<config dir>/breakaway/known_devices.json` (macOS:
  `~/Library/Application Support/breakaway/`); a normal start (not
  `--playtest`) scans 5 s and reconnects them. Mocks are never saved.
- Real hardware can only be verified by the owner (playtests `007-*`);
  agents verify with mock devices and the debug API.

## Free ride

- `src/race.rs` is the pure ride logic (`Ride`, `step(ride, power_w, dt)`,
  constants at the top): one fixed band; smoothed power (time constant
  1.5 s, about a 3-second average) decides
  `Idle`/`Under`/`In`/`Over`. In band the gap to the partner closes and
  score accrues (double on the wheel); under it the gap opens; over it
  nothing is gained and a match burns every 4 s, at zero matches each
  burn costs 50 score. Its tests include the one rule.
- Stepped in `GameSet::Sim`; the partner rides `PARTNER_AHEAD_M + gap_m`
  ahead. UI in `src/ride_ui.rs`: band bar + matches (top centre), ride
  panel (`B` toggles: band in 5 W steps, restart, totals, `Team radio`
  and `Voice` switches, `Voice volume` slider, `Countdown beeps` switch, workout section: FTP, files, load, unload); with a workout loaded
  also the step / next-step lines and a progress strip across the top.
  Ride and scenario events have no on-screen texts: the team radio tells
  them.
- The band is saved in `<config dir>/breakaway/settings.json`
  (`band_low_w`, `band_high_w`; default 120–160 W when missing or
  invalid; also `ftp_w`, `radio_enabled` (default true), `voice_enabled`
  (default true), `voice_volume` (0–1, default 0.8) and `countdown_beeps`
  (default true)), written only when one of them changes through the
  ride panel.
  The playtest base restores it and restarts the ride.

## Workouts

- Format (`src/workout.rs`, JSON): `{name, date, source, steps: [...]}`;
  steps flat in riding order (repetitions expanded), each `{name,
  duration_s, low_pct, high_pct, class, block, block_kind, rep, reps,
  cadence_low, cadence_high, fixed_rest}`; targets in percent of FTP
  (`0 < low <= high <= 300`), `class` `warm_up|active|rest|cool_down`,
  `block_kind` `single|repetition|ramp`; unknown keys are rejected.
  Samples: `workouts/samples/` (synthetic workouts in TrainingPeaks
  export shape; see `tools/fixtures/`).
- `just fetch-workout [YYYY-MM-DD]` (default today) writes every planned
  bike workout with a structure that day to `<config
  dir>/breakaway/workouts/<date>-<id>.json` (exit 1 if none);
  `just fetch-workout-id <id> <out.json>` one workout. `tools/tp_export.py`
  runs with the TrainingPeaks MCP server's virtualenv and keychain login,
  GET only, never stores credentials. `just test` runs its `--self-test`
  (raw fixtures `tools/fixtures/*.tp.json` → samples).
- `WorkoutSession` (resource): a loaded workout owns the band (step band =
  `pct × FTP / 100`, at least 20 W wide) and the course; the runner only
  advances while the ride is not `Idle`. After each step start the gap does
  not open and the over-timer does not run until the smoothed power first
  enters the new band, at least 3 s and at most 10 s
  (`Ride::begin_grace`, `TRANSITION_GRACE_MIN_S` / `_MAX_S`); score still
  only accrues in band. The workout tests ride every sample with simulated
  riders (exact, human-like: 0 burns; late by 12 s: burns on the 30/30s).
  At the end the ride state is `finished` and nothing accrues. Restart
  returns to free ride. FTP lives in `settings.json` (`ftp_w`, default
  200, panel only writes it). Run with a workout: `just run --workout
  <path>` (relative: the repo's `workouts/`, then the config `workouts/`).

## Step countdown

- `src/countdown.rs` (pure) + `src/countdown_ui.rs` (egui, audio):
  before every step end (the last one too) the remaining seconds show in
  the middle of the screen, `ceil(remaining)` from 10 (from 5 for steps
  under 20 s): 10–6 white 72 px, 5–1 red growing 96 → 160 px. A beep per
  number (`assets/sfx/tick.ogg`), 10–6 at 0.35, 5–1 rising 0.5 → 1.0,
  and `go.ogg` at 1.0 as the step ends, all × `voice_volume`. Fed the
  events of the workout's own advance (`WorkoutEvent` message), so a
  skip, seek, load or unload never beeps `Go`; a number that rises or
  disappears is silent; the countdown freezes with the workout while
  `Idle`. Setting `countdown_beeps` (ride panel, `settings.json`);
  `Countdown` resource (`beeps_enabled`). `just sfx` (`tools/gen_sfx.py`,
  ffmpeg only) renders the clips through the voice encoder (bitexact);
  `just sfx --check` (part of `just test`) fails when a committed clip
  differs from a fresh render.

## Workout course

- `src/terrain.rs` has two profile modes: `Profile::new` loops (closes in
  elevation, laps compose); `Profile::open` does not: before 0 and after
  the last segment the road runs straight and flat at the start / end
  elevation and heading, with the usual 40 m blends into those extensions.
  `is_looping()`, `total_length_m()` (one lap / the open course).
- `src/course.rs` (pure): `generate(&Workout) -> Course {profile,
  step_start_m, expected_distance}`. Gradient per step from the centre of
  its %FTP range: warm-up / cool-down 0 %, active `(i − 55) / 6.5` clamped
  to 0..10 and rounded to 0.5 %, rest under 120 s 0 % (a ledge), longer
  rest −3 %. Lengths are solved so that riding at `speed_for_gradient` takes
  each step's time (1 m integration, rescale per step, ≤ 10 rounds; every
  boundary within 0.5 s). Steps are cut into 150–500 m chunks (a step under
  60 m joins the previous chunk); each chunk turns −75..75° (about a third
  straight, radius ≥ 80 m, alternating more often than not); steps over
  300 s roll ± 1.5 % around their gradient with zero mean, within −4..10 %.
  Seeded by the workout's name and total time: same workout, same course.
  `TimeTable` = distance at each whole second of workout time (scaled per
  step so every step starts exactly at its time).
- Loading a workout makes its course the active profile (`workout`) at
  distance 0; unload / restart restore the previous profile (distance 0).
  `set_profile` is refused while a workout is loaded.
- **Time is the master** (`WorkoutSession::pace` / `move_world`, called
  after the ride and the workout each frame): while the ride is `Idle` the
  world stands still; otherwise the speed is `speed_for_gradient` × `clamp(1
  + (expected − distance) / 20, 0.9, 1.1)`, integrated in ≤ 0.05 s
  sub-steps; `skip_step` jumps to the next step's `step_start_m`,
  `seek_workout` to a workout time. After the finish the world rolls on at
  the scripted speed along the flat end.
- **Speed rule:** every scripted speed is `speed_for_gradient(g)` ×
  the tuning file's `speed_factor` (default 1.0): free ride
  (`sim::step`), the workout pace (`step_locked`) and `course::generate(
  workout, speed_factor)`, so a workout's steps still take exactly their
  time and only the course length scales. `speed_for_gradient` itself
  stays a pure function of the gradient. `WorkoutSession::tuning` carries
  the values to those three places.

## Tuning file

- `config.toml` at the repo root (path baked in at build time like the
  radio lines; `src/tuning.rs`, `Tuning` resource), hand-edited and read
  once at start: restart the game after a change. The game never writes
  it and the ride panel does not show it. Missing file: defaults; an
  unreadable file, an unknown key or a value out of range prints the
  reason and exits 2 before the window.
- Keys: `speed_factor` (default 1.0, 0.5..=2.0: multiplies the scripted
  world speed; retune after task 014's scenery), `lateral_m_per_pct`
  (0.25, 0.05..=0.5: metres of lateral offset per percentage point of
  balance away from 50 %, still clamped to the 3.0 m road half width),
  `workout_strip_px` (48, 16..=120: height of the workout strip across
  the top; `top_offset` and everything below it follow).

## Scenario layer

- `src/scenario.rs` (pure) stages parts of a workout as a race situation.
  `plan(&Workout) -> Vec<StepRole>` (`Default`, `Pull { set }`,
  `Draft { set }`): a **breakaway set** is a maximal run of hard/easy pairs,
  every step 15–180 s and not a `warm_up` / `cool_down` step (a run stops
  at them), each hard step's band centre (percent of FTP) ≥ 1.2 × the
  following easy step's, at least 3 pairs; a run may not begin with a step
  of a `ramp` block unless the whole run lies inside that one block; other
  classes and block kinds do not matter. Sets are numbered from 0.
  Samples: `hit-30-30` steps 6–11 (the 3 × 30/30 opener from its first
  hard step, not the warm-up ramp), 13–52, 54–93; `hidits-4x12` 6–11 and
  the four pyramids (each one `ramp` block) from their first hard step
  (13–24, 26–37, 39–50, 52–63, 720 s each); none in the others.
- `Break {set, advantage_s, caught, phase}`: 20 s at the start of each set
  (also when entering it mid-way by a seek); +0.05 s/s while `In` band,
  −0.25 s/s `Under`, −0.5 s/s `Idle`, nothing while `Over` (matches handle
  it) or during the transition grace (except `Idle`); 180 s at most. At 0
  caught for the rest of the set. A set whose last step ends uncaught
  survived. A jump out of the middle of a set (seek) drops the break
  without a result. Events (`ScenarioEvent`): `BreakStarted`,
  `PullStarted`, `DraftStarted`, `BunchClosing` (below 10 s, once per
  crossing), `Caught`, `BreakSurvived`.
- `staging(role, break, ride)`: `Default` / `Draft` partner on the centre
  line `2 m + gap` ahead, camera 6 m behind; `Pull` partner 2 m behind and
  1.2 m right of the player, camera 9 m behind; after the grace, under band
  he eases up alongside (0 m), over band he drops back (−6 m). The gap is
  held at 0 during a pull (`Ride::hold_gap`). Once caught a bunch of 12
  billboards rides around the player until the set ends.
- The `Scenario` resource is stepped after the ride and the workout each
  frame; it replans when `WorkoutSession::loads` changes (every load and
  unload). `Ride` keeps `sets_total`, `sets_survived` and
  `set_advantages_s` (0 when caught); restart clears them, unload keeps
  them. Render eases the partner's place and the camera (τ 0.33 s; a swap
  goes round the player's side). UI: race strip under the band bar during
  a set (`BREAK +0:34`, full width = 60 s, warning colour below 10 s,
  `CAUGHT`), role line large for 3 s at each change then small next to the
  bar, `Breaks survived 2 / 3` in the ride panel.

## Team radio

- `src/radio.rs` (pure, `dt` injected). Lines: `assets/radio/lines.toml`
  (path baked in at build time; a bad file exits 2 before the window):
  one `[[line]]` per key with `speaker` (`ds` / `partner`) and at least two
  `text` variants; placeholders `{secs}`, `{low}`, `{high}` (W, rounded),
  `{rep}`, `{reps}`, `{advantage}` (`m:ss`, whole seconds rounded down),
  `{matches}`. Optional `spoken` per line: what the voice says, one per
  `text` variant (absent: `text` is spoken); every spoken text may only use
  `{secs}` (10, 5) and `{matches}` (1–4); watts, reps and gaps stay on
  screen only. The texts are fixed: do not reword, add or drop lines
  without being asked.
- `Radio`: one line at a time, typewriter at 40 chars/s, on screen
  `max(reveal + max(3, chars / 15), clip length)` s. `Cue` (step warnings, `pull_start`,
  `draft_start`) > `Alert` (under / over band, matches, bunch closing,
  caught) > `Story` (the rest). A pushed line starts at once when nothing
  is on screen; a `Cue` replaces a `Story` line at once and cuts an `Alert`
  once its current sentence has been visible 1.5 s (the reveal stops at
  the sentence end). Waiting lines below `Cue` are dropped after 8 s; at
  most 3 wait (the lowest priority, newest first, goes). Lines due in the
  same frame are pushed highest priority first. Variants: splitmix64
  seeded from the workout name (reseeded on load / unload / restart,
  which also clear the radio); never the same variant of a key twice in a
  row. Cooldowns from acceptance: `under_band`, `over_band` 30 s,
  `in_band_streak` 300 s. `radio_enabled` off: silent, nothing queues.
  `match_burned` is not said for the last match (`out_of_matches` alone).
- `Triggers` (stepped after the scenario each frame, then `Radio::advance`
  by the virtual frame time): `workout_start` (first non-Idle after a
  load); step warnings when the step's remaining time crosses 10 s (5 s,
  `{secs}` 5, for steps under 20 s): `pull` / `draft` by the next step's
  role, else `harder` / `easier` by band centres, none within 5 % or on the
  last step; `last_rep` (first step of the last rep, repetition blocks of
  ≥ 3 reps); `block_done` (last step of a repetition block of more than
  one rep ends); scenario and ride events one line each; `under_band`
  (Under 5 s continuously outside grace), `over_band` (Over 2 s, outside
  grace), `in_band_streak` (120 s In), each once per continuous spell;
  `halfway` (elapsed crosses half the total); `workout_done`.
- UI `src/radio_ui.rs`: dialogue box bottom left (above the playtest
  overlay, whose drawn height `OverlayHeight` records): 96 px portrait
  drawn with egui shapes (DS: dark blue tile, cap; partner `Marco`: red
  tile, helmet; mouth moves while revealing), name, revealed text.
- **Voice clips** (generated files, committed): `assets/radio/audio/<key>-<variant>[-s<secs>][-m<matches>].ogg`
  (variant from 1), one per variant and placeholder value, plus
  `manifest.json` (per file: hash of spoken text, voice, lang_code, speed;
  `duration_s`). `just voices` (`tools/gen_voices.py`) makes them with a
  local Kokoro server (Docker container `tts`, `127.0.0.1:8880`;
  not answering → `start it with: docker start tts`, exit 1; the script
  never starts or changes the container): WAV → silence trimmed to ~50 ms
  at each end, peak-normalised to −1 dBFS, Ogg Vorbis 24 kHz (this
  machine's ffmpeg has no libvorbis, so its own encoder writes the mono
  voice into two identical channels). No radio effect. Incremental by hash
  (`just voices --force`: all), clips no line needs are deleted. Voices per
  speaker: `assets/radio/voices.toml` (`voice`, `speed`, optional
  `lang_code`; an unknown id lists the available ones). **After editing a
  line or a voice run `just voices` and commit the clips and the manifest**;
  `just voices-check` (part of `just test`, no server) fails while the
  manifest does not match the lines and voices exactly or a file is
  missing.
- Playback `src/radio_audio.rs` (Bevy audio, Vorbis is a default feature;
  asset root baked in as the repo's `assets/`): the line's clip
  (`radio::clip_name`, chosen with the variant) plays when it goes on
  screen and stops when it ends, is cut or dropped; no clip → text only.
  Settings `voice_enabled` (default on) and `voice_volume` (0–1, default
  0.8): ride panel and `settings.json`. Voice off: text only.
- Simulated-rider tests: exact-rider key sequences for the first 10 min
  of the main part (from the first non-warm-up step) of `hit-30-30` and
  `mit-sweetspot-6x5`; the exact rider never hears `under_band`,
  `over_band`, `match_burned` or `caught` on any sample; every key fires
  with every placeholder supplied.

## Command output — errors only

Use the `justfile` targets (`just check`, `just test`, `just clippy`,
`just run`). No output after filtering means success. Add a target rather
than hand-rolling a filter pipe. `just lint` (rustfmt `--check`, `typos`;
`rustfmt.toml`, `typos.toml`) runs before every commit, `just gate`
(lint, clippy with every warning an error, `just test`) before every
push; both must be green before a task is reported done.

## Debug API

Local JSON API on `127.0.0.1:7880`, Cargo feature `debug-api` (default on).
(7878 = iron-vale, 7879 = echoveil and trainer-alerts.) Every rider-visible
feature needs an endpoint that lets a script drive and inspect it. The
driving endpoints build a `ControlCommand` and go through `control::apply`
(`src/control.rs`), the same path playtest staging uses.

| Method | Path | Behaviour |
|---|---|---|
| GET | `/state` | `{power_w, balance_left_pct, lateral_m, distance_m, world_speed_mps, elevation_m, gradient_pct, heading_deg, x_m, y_m, input_override, profile, input_origin, cadence_rpm, hr_bpm, band_low_w, band_high_w, band_state, smoothed_power_w, gap_m, matches, score, in_band_pct, over_timer_s}` (`profile`: `flat`, `hills`, `demo` or `workout`; `heading_deg`: road heading, degrees clockwise from north, not wrapped; `x_m`/`y_m`: centre-line position at the rider's distance, x = east, y = north; `input_override` is `null` or `{power_w, balance_left_pct}`; `input_origin`: `override`, `device` or `mock`; `cadence_rpm`/`hr_bpm`: `null` when absent or older than 3 s); free ride: `band_low_w`, `band_high_w`, `band_state` (`idle`, `under`, `in`, `over`, `finished`), `smoothed_power_w`, `gap_m`, `matches`, `score`, `in_band_pct`, `over_timer_s`; `ftp_w`, `autopilot`, `workout`: `null` or `{name, step_index, step_count, step_name, step_class, step_remaining_s, block_kind, rep, reps, elapsed_s, total_s, finished}`; `course_length_m`, `course_drift_m` (expected − actual distance): `null` without a workout; `scenario`: `{role: "default"\|"pull"\|"draft", set, advantage_s, caught, sets_total, sets_survived}` (`set` from 0; `set` / `advantage_s` `null` outside a set); `radio`: `{speaker: "ds"\|"partner", key, text, queue_len, clip, voice_enabled}` (the line on screen, full text; `clip`: its voice clip's name without `.ogg`, `null` when the voice is off or there is none) or `null`; `countdown`: the number on screen or `null` |
| POST | `/input` | `{"power_w": f32, "balance_left_pct": f32}` sets an override (mock ignored); `{}` or any `null`/missing field clears it |
| POST | `/set-profile` | `{"name": str}` swaps the course at runtime (`flat`, `hills`, `demo`, else 400; 400 while a workout is loaded) and resets `distance_m` to 0 |
| POST | `/set-speed` | `{"multiplier": f32}` time scale for the whole sim (1.0 default, clamped to max 50, `null` resets to 1.0) |
| POST | `/screenshot` | `{"path": str}` saves a PNG of the window (written about a frame later) |
| POST | `/teleport` | `{"distance_m": f32}` jumps along the road (finite and ≥ 0, else 400); lateral kept, gradient/speed/elevation/heading follow on the next sim step |
| POST | `/band` | `{"low_w": f32, "high_w": f32}` sets the free-ride band (400 unless `0 < low < high <= 2000`, or while a workout is loaded); never writes the settings file |
| POST | `/ride/restart` | fresh ride (gap 0, 5 matches, score and totals 0, smoothed power 0); unloads a workout (free ride with the band and profile from before it, distance 0), else the band stays |
| POST | `/workout/load` | `{"path": str}` loads a workout (relative: repo `workouts/`, then config `workouts/`), restarts the ride at step 0 on its generated course (profile `workout`, distance 0); 400 on a missing/invalid file, ride unchanged |
| POST | `/workout/unload` | back to free ride (band and profile from before the workout, distance 0), totals kept |
| POST | `/workout/skip` | ends the current step and jumps to the next step's start on the course (400 without a workout or when finished) |
| POST | `/workout/seek` | `{"elapsed_s": f32}` jumps the workout and the distance to that workout time; the step there starts afresh (band, grace), totals kept (400 outside `0..=total_s` or without a workout) |
| POST | `/ftp` | `{"ftp_w": f32}` 50..=600 (else 400); the current step's band follows; never writes the settings file |
| POST | `/autopilot` | `{"on": bool}`: with no override and no device the mock rides the band centre ± 3 % |
| POST | `/autopilot/bias` | `{"pct": f32}` −50..=50 (else 400): the autopilot rides band centre × (1 + pct / 100), ± 3 %; the playtest base resets it to 0 |
| POST | `/scenario/advantage` | `{"seconds": f32}` the running breakaway's advantage, 0..=180 (400 outside a breakaway set or out of range); 0 is caught on the next frame, more un-catches |
| POST | `/radio/say` | `{"key": str}` puts that line on the radio (`RadioSay`; current band, rep, advantage or 0, matches, `{secs}` 10); 400 for a missing or unknown key; ignored while off or cooling down |
| POST | `/radio/enabled` | `{"on": bool}` radio on / off (`RadioEnabled`); never writes the settings file |
| POST | `/radio/voice` | `{"on": bool}` the radio's voice on / off (`VoiceEnabled`; off stops the clip on air, the text stays); never writes the settings file |
| POST | `/countdown/beeps` | `{"on": bool}` the step countdown's beeps on / off (`CountdownBeeps`); never writes the settings file |
| POST | `/ride/gap` | `{"gap_m": f32}` gap to the partner's wheel, `0..=60` (else 400) |
| POST | `/ride/matches` | `{"matches": int}` matches left, `0..=5` (else 400) |
| GET | `/devices` | `{adapter, scanning, discovered:[{address,name,rssi}], connected:[{id,name,is_mock,metrics}], known:[{address,name}], metrics:{power:{value,age_s,device_id}, …}, error}` (`adapter`: `starting`, `ready` or `unavailable: <reason>`; `metrics`: latest per metric, newest device wins, stale ones included) |
| POST | `/devices/scan` | scans 5 s (retries an unavailable adapter first); `202 {"accepted": true}`, result in `GET /devices` |
| POST | `/devices/connect` | `{"address": str, "name": str}` connects a scanned device; `202`, outcome in `GET /devices` |
| POST | `/devices/connect-mock` | `{"preset": "kickr"\|"quarq"\|"hr"}` (else 400); `202` |
| POST | `/devices/disconnect` | `{"device_id": str}`; `202` |

Course: `--profile flat|hills|demo` (e.g. `just run --profile flat`).
Workout: `--workout <path>` (resolved like `/workout/load`; a bad file
exits 2 before the window opens).

- `flat`: 2000 m straight at 0 %, the clean reference.
- `hills`: task 004's straight 2000 m climb/descent loop, kept for regression.
- `demo` (default): the `hills` gradients with bends (right on the climb,
  S-bends on the descent, a 180° hairpin on the 10 % kicker); 2000 m per
  lap, +90° net heading change per lap, so laps do not close in space.

## Validation

Logic is verified by unit tests and through the debug API. Anything
perceptual (does it look, read or feel right) is the owner's call and is
never judged by an agent from a screenshot. Every task with something
visible ships `playtests/<task>-<step>.toml` scenarios (format:
`playtests/README.md`) with one concrete yes/no question each. A check
that can be expressed as a number is a test, not a playtest question.
The owner runs `just playtest <task>`; verdicts land in
`playtests/results.ndjson`. A failed verdict becomes a new task; it does
not reopen the old one.

- `just playtest <sel>` = `cargo run -- --playtest <sel>`; `<sel>` is a
  scenario (`004-1`), a task (`004` or `4`: all its steps) or `all`. An
  unknown selection or empty match prints an error and exits 2 before a
  window opens. Without `--playtest` the game runs as normal.
- Before each scenario the runner resets to no workout, autopilot off (bias 0),
  profile `demo`, distance 0, no input override, speed 1.0, the saved band
  and a fresh ride, then applies the `[[stage]]` commands, and
  repeats both (base, then stage) every `loop_seconds` (default 20) of
  real time and on `R`. Keys:
  `Y` pass, `N` fail (opens a comment field: `Enter` records the fail with
  the comment, `Esc` back), `C` comment for a pass or skip (`Enter` keeps
  it, `Esc` drops it), `S` skip, `R` replay; on the summary
  `Enter`/`Esc`/`Q` quits. The comment lands in `detail`. While the field
  is open `UiTextEntry` is set and no letter is a game key (`B`, `D`
  included). The debug API stays live during a run.
- Each answer appends `{scenario, title, question, verdict, detail, at,
  commit}` to `playtests/results.ndjson` (git-ignored; `commit` from
  `build.rs`). On quit a table goes to stdout; exit 0 only if every
  verdict is `pass`.
- `cargo test` parses every scenario and applies its stages to a fresh
  world; it never opens a window or waits for a key.

## Design constraints

- Nothing may nudge toward more load. The design's conservative mode (see
  DESIGN.md) stays on until the owner switches it off.
- Never require or reward low cadence at high power.
- No readiness numbers (sleep, HRV) before a ride; post-ride debrief only.
