# 003 — Bevy skeleton: a rider on a pseudo-3D road, driven by a scripted mock

**Repo:** `~/claude/breakaway/`, branch `skeleton`, `--no-ff` merge to
`main`.

**Depends on:** nothing (deliberately independent of 001/002: no BLE, no
`trainer-core` yet). **Blocks:** 004+.

Read first: `CLAUDE.md`, `DESIGN.md` sections "Presentation" and
"Fiction", and `~/claude/after-zelda/src/systems/pseudo3d.rs` +
`camera.rs` for the rendering pattern (read only, no dependency).

## Goal

`just run` opens a window showing a road scrolling toward the camera in
perspective, the player's rider seen from behind, and one partner rider
ahead. A scripted mock produces power and L/R balance once a second. The
rider's lateral position follows balance. Nothing is scored yet.

## Project setup

- `cargo init --name breakaway`. `Cargo.toml`: `bevy = "0.19"` (default
  features), `serde`, `serde_json`, `tiny_http = "0.12"` optional behind
  feature `debug-api` (default on). Copy the two `[profile.dev]` blocks
  from `~/claude/after-zelda/Cargo.toml`.
- `justfile` with `check`, `build`, `test`, `clippy`, `run *args`, using
  the errors-only filters from `~/claude/libs/cycling-ble/cycling-ble/justfile`
  (mind its comment on the test filter).
- `.gitignore`: `/target`.
- `TODO.md` + `done-tasks.md` already exist; do not restructure them.

## Module layout (`src/`)

| Module | Contents | Bevy rendering types allowed? |
|---|---|---|
| `sim.rs` | `RiderInput { power_w: f32, balance_left_pct: f32 }`, `RaceState { distance_m, lateral_m, world_speed_mps }`, pure fns below | no |
| `input.rs` | `InputSource` resource holding the latest `RiderInput`; `ScriptedMock` system | no |
| `render.rs` | camera, ground, road, billboards; mirrors `RaceState` into transforms | yes |
| `hud.rs` | text overlay: power, balance, speed, distance | yes |
| `debug_api.rs` | HTTP endpoints | — |
| `main.rs` | app wiring, CLI flags | — |

### `sim.rs` (pure, unit-tested)

- `const ROAD_HALF_WIDTH_M: f32 = 3.0;`
- `fn lateral_from_balance(balance_left_pct: f32) -> f32` — 50 % → 0.0;
  each percentage point off 50 moves 0.3 m (left-heavy → negative x,
  i.e. left on screen); clamped to ±`ROAD_HALF_WIDTH_M`. NaN input → 0.0.
- `fn step(state: &mut RaceState, input: RiderInput, dt: f32)` —
  `distance_m += world_speed_mps * dt`; `lateral_m` eases toward
  `lateral_from_balance(..)` with a 0.5 s time constant.
  `world_speed_mps` is a constant 10.0 set at startup: **power does not
  change speed** (design rule; the workout will script speed later).
- Tests: the mapping at 50/45/55/0/100/NaN; `step` advances distance by
  exactly `speed*dt`; lateral converges within 3 s of simulated time and
  never overshoots.

### `input.rs`

`ScriptedMock`: once per simulated second writes a new `RiderInput`: power
= 200 W ± a slow sine of 15 W; balance = 50 % ± a slow sine of 4 points
with a different period. Deterministic (no RNG). When a debug-API override
is set, the mock is ignored until the override is cleared.

### `render.rs`

- Logical → render mapping as in after-zelda: logical `(x, y)` →
  `Vec3(x, height, -y)`.
- `Camera3d` chase camera: 6 m behind and 3 m above the player, looking at
  a point 10 m ahead of the player on the centre line. Follows distance
  only; does **not** follow lateral movement, so drifting is visible.
- Ground: a wide flat green plane. Road: grey strip
  `2 * ROAD_HALF_WIDTH_M` wide with a dashed white centre line and solid
  edge lines, built so it appears endless (recycle segments as the player
  passes; no texture assets). Roadside posts every 20 m on both sides to
  give a sense of speed.
- Riders: upright camera-facing quads, 0.6 m × 1.8 m, flat colours
  (player blue, partner red). Partner sits at a fixed 2 m ahead on the
  centre line. No art assets, no asset pipeline.
- Clear colour: sky blue. One directional light or unlit materials,
  whichever is simpler.

### `hud.rs`

Top-left text, updated each frame: `POWER 203 W`, `BAL 48 / 52`,
`SPEED 36.0 km/h`, `DIST 1.24 km`.

### `debug_api.rs` — `127.0.0.1:7880`, feature `debug-api`

Follow the structure of `~/claude/after-zelda/src/systems/api.rs`.

| Method | Path | Behaviour |
|---|---|---|
| GET | `/state` | `{power_w, balance_left_pct, lateral_m, distance_m, world_speed_mps, input_override}` |
| POST | `/input` | `{"power_w": f32, "balance_left_pct": f32}` sets an override; `{}` or `null` fields clear it |
| POST | `/set-speed` | `{"multiplier": f32}` time scale for the whole sim (1.0 default, max 50) |
| POST | `/screenshot` | `{"path": str}` saves a PNG of the window |

Document the table in `CLAUDE.md` under "Debug API".

## Acceptance

- `just check`, `just test`, `just clippy` clean.
- `just run` in the background, then by script:
  1. `GET /state` twice 2 s apart: `distance_m` grew by about 20 m.
  2. `POST /input {"power_w":200,"balance_left_pct":40}`, wait 3 s:
     `lateral_m` ≈ -3.0. Screenshot to
     `/tmp/breakaway-left.png`. Then `balance_left_pct` 60 → ≈ +3.0,
     screenshot `/tmp/breakaway-right.png`.
  3. `POST /input {"power_w":400,"balance_left_pct":50}`, wait 3 s:
     `world_speed_mps` is still 10.0.
  4. Open both screenshots and confirm by eye: road in perspective with
     centre line, player rider visibly left / right of the partner, HUD
     text readable. Describe what is seen in the merge commit message.
- Move this task's line from `TODO.md` to `done-tasks.md` with the
  verification notes.

## Out of scope

BLE, `trainer-core`, workouts, bands, matches, score, team radio, audio,
menus, real art, gamepad or Zwift Click input, persistence, config files.
