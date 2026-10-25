# 002 — Extract `trainer-core` from the trainer-alerts app

**Repos:**
- new: `~/claude/libs/trainer-core/` (`git init`, branch `main`, no remote)
- changed: `~/claude/athlete/trainer-alerts/app/` (branch
  `use-trainer-core`, GitHub PR per `trainer-alerts/CLAUDE.md`)

**Depends on:** 001 merged (cycling-ble 0.2.0). **Blocks:** 004.

## Why

Breakaway needs the same BLE connection handling, alert engine, mock
devices and recorder as the trainer-alerts app. Those live in
`app/src-tauri/src/` and are tied to Tauri in three small ways. Move them
into a Tauri-free library crate both projects depend on. **Pure move: no
behaviour change, no API redesign.**

## Step 0 — land the pending changes in `app`

`app` has two uncommitted edits (`src-tauri/Cargo.toml`: cycling-ble path
now `~/claude/libs/cycling-ble/cycling-ble`;
`src-tauri/src/ble.rs`: `CRANK_REVOLUTIONS_WRAP_AT` argument). Commit them
first, as their own commit on the feature branch, message
"Follow cycling-ble move to ~/claude/libs and wrap-at parameter". If
`git status` shows anything else uncommitted, stop and report.

## What moves

From `app/src-tauri/src/` into `trainer-core/src/`:

| From | To | Notes |
|---|---|---|
| `ble.rs` | `ble.rs` | `Metric`, `Reading`, `DiscoveredDevice`, `ConnectedDeviceInfo`, `BleManager`, `handle_reading`, helpers |
| `alerts.rs` | `alerts.rs` | unchanged |
| `mock.rs` | `mock.rs` | |
| `recording.rs` | `recording.rs` | |
| `services.rs` | `services.rs` | rename `AppServices` → `Services`; `LiveState` unchanged |

Unit tests move with their modules. Stay in the app: `commands.rs`,
`debug_api.rs`, `logging.rs`, `lib.rs`, `main.rs`, and all JSON persistence
(`alert_rules.json`, `alert_profiles.json`, `known_devices.json`).

`trainer-core/Cargo.toml`: edition 2021, deps `cycling-ble` (path
`../cycling-ble/cycling-ble`), `btleplug`, `tokio`, `futures`, `serde`,
`uuid`, `rand`, `tracing`, `chrono` only if `recording.rs` needs it — same
versions as the app's `Cargo.toml`. **No `tauri` dependency.** Add a
`justfile` copied from `cycling-ble`'s (`check`, `build`, `test`, `clippy`,
`fmt`) and a short `README.md` + `CLAUDE.md` (what the crate is, the
"no Tauri, no Bevy, no app-specific logic" rule, branch workflow).

## The three Tauri seams and their replacements

1. **Events.** `handle_reading` calls `app.emit("reading", ..)` and
   `app.emit("alert", ..)`. Replace with a broadcast channel owned by
   `Services`:

   ```rust
   #[derive(Debug, Clone)]
   pub enum CoreEvent { Reading(Reading), Alert(AlertEvent) }

   impl Services {
       pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<CoreEvent>;
   }
   ```

   Channel capacity 1024. `handle_reading(services, reading)` sends on it;
   a send error because there are no receivers is ignored silently. Every
   `app: AppHandle` parameter disappears from `connect`, `connect_mock`,
   `mock::spawn`, `handle_reading`.

2. **Task spawning.** `tauri::async_runtime::spawn` / `JoinHandle` appear
   in `ble.rs`, `mock.rs`, `recording.rs`. Replace with
   `tokio::runtime::Handle::spawn` and `tokio::task::JoinHandle`. `Services::new`
   takes a `tokio::runtime::Handle` and stores it; all spawns go through
   that handle (never bare `tokio::spawn`, because the debug API calls in
   from a non-runtime thread).

3. **Doc comments** mentioning Tauri, the frontend or `AppState`: reword to
   "the consumer". No rider context in this crate.

Visibility: items the app uses become `pub`; everything else stays
`pub(crate)`. Re-export from `lib.rs`: `Metric`, `Reading`, `BleManager`,
`Services`, `CoreEvent`, `AlertRule`, `AlertProfile`, `AlertEngine`,
`AlertEvent`, `MockRange`, `Recorder`, `DiscoveredDevice`,
`ConnectedDeviceInfo`.

## App side

- `Cargo.toml`: add `trainer-core = { path = "../../../../libs/trainer-core" }`;
  remove deps that only the moved modules used (keep `cycling-ble` only if
  still referenced).
- `lib.rs` `setup`: build `Services` with Tauri's tokio handle
  (`tauri::async_runtime::handle()`; it wraps a tokio `Handle` — check the
  Tauri 2 API for the accessor), then spawn one forwarding task:
  `subscribe()` → `app.emit("reading", &r)` / `app.emit("alert", &e)` with
  the same event names and payloads as today, so the Svelte frontend is
  untouched. On `RecvError::Lagged`, log a warning and continue.
- `commands.rs`, `debug_api.rs`: update imports and drop the `app`
  arguments that the moved functions no longer take.
- Adapt to cycling-ble 0.2.0 (`non_exhaustive`) if anything constructs or
  exhaustively matches its types.

## Acceptance

- `trainer-core`: `just check`, `just test`, `just clippy` clean;
  `grep -rn -i tauri src/` returns nothing.
- `app`: `just check`, `just test`, `just clippy`, `just frontend-check`
  clean. Total test count across both crates is not lower than the app's
  count before the move (record both numbers in the PR description).
- Scripted run against the real app (`just dev -- --mock` in the
  background, then `curl` on `127.0.0.1:7879`):
  1. `POST /connect-mock` with a power range → `GET /state` shows the
     device and a fresh power reading within 3 s.
  2. Set a rule via the app's existing mechanism or `GET /rules` default,
     `POST /inject-reading` out of bounds repeatedly for longer than
     `sustained_secs` → `GET /state` lists the metric under active alerts;
     inject an in-bounds value → it clears.
  3. `POST /recording/start`, wait 5 s, `POST /recording/stop` → returned
     path exists and contains `<Trackpoint>` elements.
  Paste the three results into the PR description.
- No file under `app/src/` (the Svelte frontend) is modified.
- Append to `trainer-alerts/ride-checklist.md`: "After trainer-core
  extraction: connect real trainer + HR strap, gauges update, an alert
  fires and clears, recording produces a TCX."
- `trainer-alerts/TODO.md`: move the "Extract `trainer-core`" item to
  `done-tasks.md`. Update `trainer-alerts/CLAUDE.md` "Repo layout" (it
  still describes `cycling-ble/` as a child directory).

## Out of scope

Crash-safe recording, any API redesign, FTMS control, RR-interval
pass-through, Zwift Click discovery, `WorkoutRunner`, anything in
Breakaway itself, creating remotes.
