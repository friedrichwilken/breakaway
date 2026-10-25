# 007 — Real devices: connect a trainer, pedals and HR strap; UI moves to egui

**Repo:** `~/claude/breakaway/`, branch `devices`, `--no-ff` merge to
`main`.

**Depends on:** 006 (merged), `~/claude/libs/trainer-core` (on `main`).
Read `CLAUDE.md` and all of `src/` first, then `trainer-core`'s
`src/lib.rs`, `ble.rs` (public items), `services.rs`, `mock.rs`.
egui reference in a Bevy 0.19 project (read only):
`~/claude/after-zelda/src/main.rs` and `src/systems/rig_editor_ui.rs`
(`bevy_egui = "0.40"`, `EguiPlugin::default()`, systems in
`EguiPrimaryContextPass`).

`trainer-core` builds against the `cycling-ble` working tree, which is
checked out on branch `api-contract`. Do not change either library or
switch their branches. If the `trainer-core` API is awkward or missing
something, do not work around it silently: use what exists, and list the
friction in your report.

## Goal

The owner can start the game, connect a smart trainer (power, cadence),
power-meter pedals (power, cadence, left/right balance) and HR strap over
Bluetooth from an in-game panel, and the rider on screen is driven by
those readings. Without devices the game behaves as today.

**Decision (2026-10-01): all UI is egui (`bevy_egui`).** This task adds
the devices panel in egui and moves the existing HUD and playtest overlay
from Bevy UI to egui.

## Part A — device layer (`src/devices.rs`, a Bevy plugin)

- Dependencies: `trainer-core = { path = "../libs/trainer-core" }`,
  `tokio` (rt-multi-thread), `bevy_egui = "0.40"`, `dirs`.
- A resource owns a tokio multi-thread runtime (2 workers) for the
  lifetime of the app, an `Arc<trainer_core::Services>` built with its
  handle, and the `BleManager` once created. `BleManager::new()` runs on
  the runtime at startup; if it fails (no adapter, no permission) the
  error is kept and shown in the panel with a "Retry" button, and the
  game keeps running.
- **Never block a Bevy system on async work.** UI actions spawn a task on
  the runtime; results come back over a `std::sync::mpsc` channel that a
  Bevy system drains each frame. One forwarding task subscribes to
  `Services::subscribe()` and pushes every `CoreEvent::Reading` into such
  a channel (`Lagged` → continue).
- `DeviceState` resource (what the panel and the debug API read):
  adapter status, `scanning: bool`, discovered devices (last scan),
  connected devices (`ConnectedDeviceInfo`), last error text.
- `LiveMetrics` resource: latest value per `trainer_core::Metric` with
  the `Instant` it arrived and the device id. If two devices report the
  same metric the newest reading wins (known limitation, fine for now).
- Actions: `Scan` (5 s), `Connect { address, name }`, `ConnectMock
  { preset }`, `Disconnect { device_id }`, `RetryAdapter`. After every
  connect/disconnect refresh the connected list from the manager.
- Mock presets (same ranges as the trainer-alerts app):
  `kickr` = "Mock KICKR CORE" power 100–320, cadence 70–95;
  `quarq` = "Mock Quarq PM" power 95–320, cadence 70–95, pedal balance
  45–55; `hr` = "Mock HR Strap" heart rate 120–155.
- **Known devices:** every successfully connected real device is saved
  (address + name) to `<config dir>/breakaway/known_devices.json`
  (`dirs::config_dir()`); "Forget" in the panel removes it. On a normal
  start (not `--playtest`), once the adapter is ready: scan 5 s, connect
  every known device found. Mock devices are never saved.

## Part B — rider input from devices (`src/input.rs`)

A pure function decides the input each frame, unit-tested:

```rust
pub enum InputOrigin { Override, Device, Mock }
pub fn select_input(
    override_input: Option<RiderInput>,
    device_connected: bool,
    power: Option<(f32, f32)>,        // (watts, age in seconds)
    balance_left: Option<(f32, f32)>, // (percent, age in seconds)
    mock: RiderInput,
) -> (RiderInput, InputOrigin)
```

- Override set → override.
- Else if any device is connected → `Device`: power = latest power if
  younger than 3 s, else 0; balance = latest `PedalBalance` (the value is
  the left leg's share in percent) if younger than 3 s, else 50.
- Else → the scripted mock, as today.

`InputSource` exposes the origin. The scripted mock keeps ticking so
falling back is seamless.

## Part C — UI in egui

Remove the Bevy UI code of the HUD and the playtest overlay; add
`EguiPlugin::default()`.

- **HUD** (top-left, no window frame, same monospace look, text with a
  dark outline or translucent backing so it reads on sky and grass): the
  existing six lines plus `CAD 88 rpm` and `HR 132 bpm` (`--` when no
  reading younger than 3 s) and a last line `INPUT device|mock|override`.
- **Devices panel** (`D` toggles it; opens automatically at start when
  nothing is connected and the game is not in `--playtest` mode): adapter
  status / error + Retry; "Scan" button (disabled and labelled
  "Scanning…" during a scan); discovered devices with name, signal
  strength and "Connect"; connected devices with their metrics, the live
  value of each, and "Disconnect"; known devices with "Forget"; the three
  mock presets with "Connect"; the last error in red. While a text field
  or the panel has keyboard focus, game keys other than `D` are ignored.
- **Playtest overlay:** same content and keys as today, as a compact
  bottom-anchored egui panel at most three text lines high plus padding,
  so the near rider stays fully visible above it. Summary as today.

## Part D — debug API

- `GET /devices` → `{adapter, scanning, discovered:[{address,name,rssi}],
  connected:[{id,name,is_mock,metrics}], known:[{address,name}],
  metrics:{power:{value,age_s,device_id}, …}, error}`.
- `POST /devices/scan`, `POST /devices/connect {"address","name"}`,
  `POST /devices/connect-mock {"preset":"kickr"|"quarq"|"hr"}`,
  `POST /devices/disconnect {"device_id"}`. All return immediately
  (`202`-style `{"accepted": true}`); the outcome shows up in
  `GET /devices`. Unknown preset → 400.
- `GET /state` gains `input_origin`, `cadence_rpm`, `hr_bpm` (null when
  absent or stale).
- Update the `CLAUDE.md` table.

## Part E — playtest scenarios (owner's checks, do not answer them)

All on profile `flat`, `loop_seconds = 600`, no input override:

| File | Question |
|---|---|
| `007-1` | Press D, scan and connect your trainer. Do POWER and CAD on screen follow your legs within about a second? |
| `007-2` | With the power-meter pedals connected: does the blue rider move sideways when you push harder with one leg? |
| `007-3` | With the HR strap connected: does HR on screen match your watch or head unit? |
| `007-4` | Are the HUD and this question panel easy to read, and is the blue rider fully visible above the panel? |
| `007-5` | Quit and start the game again (not in playtest mode): did your devices reconnect on their own? Answer here on the next run. |

In `--playtest` mode there is no auto-open of the panel and no
auto-reconnect; `D` and manual connecting work.

## `CLAUDE.md`

Add under "Architecture rules": "All UI is egui (`bevy_egui`); no Bevy UI
nodes." Add a short "Devices" section: where the layer lives, the
never-block rule, the input priority, the known-devices file, and that
real hardware can only be verified by the owner.

## Acceptance (numbers and behaviour only; no judging of visuals)

- `just check`, `just test`, `just clippy` clean. Unit tests cover
  `select_input` (all branches, the 3 s staleness edge), `LiveMetrics`
  newest-wins, known-devices load/save/forget round trip in a temp dir,
  mock preset lookup. `cargo test` must not touch Bluetooth.
- Scripted run with mock devices (game in the background, port 7880 free
  first):
  1. Fresh start: `GET /state` → `input_origin` = `"mock"`.
  2. `POST /devices/connect-mock {"preset":"quarq"}` → within 3 s
     `GET /devices` lists it; `GET /state` → `input_origin` = `"device"`,
     `power_w` within 95–320 and `balance_left_pct` within 45–55, both
     changing between two reads 2 s apart; `cadence_rpm` within 70–95;
     `lateral_m` non-zero after 3 s.
  3. Add `{"preset":"hr"}` → `hr_bpm` within 120–155.
  4. `POST /input {"power_w":400,"balance_left_pct":50}` →
     `input_origin` = `"override"`; clearing it returns to `"device"`.
  5. Disconnect both → `input_origin` back to `"mock"`, `hr_bpm` null
     within 4 s.
  6. `POST /devices/scan` → `scanning` true, then false within 8 s, no
     crash, whether or not Bluetooth is available. If macOS denies
     Bluetooth to the process, `adapter` reports the error and steps 1–5
     must still pass.
  7. Mock devices are not written to the known-devices file.
  8. `just playtest 006`-style check: `--playtest 003` still opens and
     stages (profile flat via `GET /state`); `--playtest 007` selects five
     scenarios (unit test).
- You may connect to real Bluetooth devices only if a scan happens to
  find them; do not treat their absence as a failure, and disconnect
  anything you connected. Remove any known-devices file your run created.
- Stop the game; leave no `playtests/results.ndjson` lines behind.
- Move this task's line in `TODO.md` to `done-tasks.md`.

## Out of scope

Recording, alert rules, bands, scoring, workouts, trainer resistance
control (FTMS write), Zwift Click, RR intervals, choosing a primary
device per metric, changing terrain or rendering.
