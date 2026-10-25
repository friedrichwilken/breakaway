# 014 — Scenery (grass bands, trees) and two ride basics (restart key, log file)

**Repo:** `~/claude/breakaway/`, branch `scenery`, `--no-ff` merge to
`main`.

**Depends on:** 013 (merged). Read `CLAUDE.md` and all of `src/` first,
especially `render.rs`, `terrain.rs`, `course.rs`, `main.rs`,
`ride_ui.rs`.

Everything visual here is placeholder art built from generated meshes and
flat colours. **No image or model files.**

## Part A — scenery (`src/scenery.rs` + changes in `render.rs`)

Purpose: make speed, gradient and bends readable, and make long rides
less uniform.

1. **Grass bands.** The shoulder strip alternates between two greens
   every 6 m of course distance (band index = `floor(d / 6)`, absolute
   distance, so bands do not crawl). The base plane and far land use the
   darker of the two.
2. **Edge strips.** Outside each white edge line, a 0.5 m wide strip of
   alternating red and white blocks, 3 m each, same layering rules as the
   existing lines (shared lateral cuts so nothing pokes through on
   twisted quads; the existing unit test on shared cuts must cover them).
3. **Trees.** Upright camera-facing billboards (same billboard system as
   riders), three kinds built from simple shapes in two shades each:
   conifer (stacked triangles on a trunk, 6–9 m tall), round tree (disc
   on a trunk, 5–7 m), bush (low blob, 1–1.5 m).
   - Placement is a pure function of the course distance and a seed:
     `fn scenery_at(seed: u64, cell: i64) -> Vec<Prop>` for 10 m cells,
     each prop with side (left/right), lateral offset 6–28 m from the
     centre line, along-offset within the cell, kind and height.
   - **Density varies** along the course in stretches of 200–600 m:
     forest (3–5 props per cell per side), scattered (0–1), open (0).
     Roughly a third each. The stretch type comes from the same seed.
   - Seed: the workout's course seed when a workout is loaded, a fixed
     constant for the built-in profiles. Same course → same trees.
   - Props stand on the ground at `render_height` of their own distance
     (the shoulder is flat across, so no lateral height term).
   - Only props within the drawn road window exist; they are recycled as
     the window moves (pool of entities, no per-frame spawning).
4. **Grass tufts.** Small dark-green billboards (0.3–0.5 m), 2–4 per
   10 m cell per side, at 3.6–6 m from the centre line, from the same
   placement function.
5. **Distant tree line.** On the backdrop ring, a dark-green jagged band
   0–25 m high standing on the horizon in front of the mountains,
   generated once like the mountain outline.

Unit tests: placement is deterministic; different seeds differ; every
prop's lateral offset is within its allowed range and never on the road
(|offset| > 3.5 m); stretch types each cover between 20 % and 45 % of a
20 km course; band index and edge-block index functions; prop count in a
720 m window never exceeds the pool size.

Performance: with the densest forest stretch in view the game must still
run at the display's refresh rate on this machine; measure the frame time
through a new `GET /state` field `frame_ms` (smoothed) and report it for
open and forest stretches.

## Part B — restart key

- `R` in normal play (not in `--playtest` mode, where `R` keeps meaning
  replay) opens a small centred egui prompt: `Restart?  [Y] yes  [N] no`.
  `Y` restarts; `N` or `Esc` closes it. Ignored while an egui widget has
  keyboard focus.
- **Restart keeps the loaded workout**: it seeks to workout time 0 and
  resets matches, score, totals, scenario and radio. Free ride (no
  workout): resets the ride as today.
- The ride panel's "Restart ride" button and `POST /ride/restart` do the
  same. Unloading a workout stays a separate action ("Unload" /
  `/workout/unload`). Update the tests that assumed restart unloads.

## Part C — log file

- In addition to the console, write the log to
  `<config dir>/breakaway/logs/breakaway-<YYYY-MM-DD>T<HH-MM-SS>.log`
  (one file per start; keep the newest 20, delete older ones at start).
  Use Bevy's `LogPlugin` `custom_layer` (or the equivalent in 0.19) so
  one subscriber serves both; no second global subscriber.
- Level: `info` for everything, `debug` for the game's own crate and for
  `trainer_core`. `--log-level <level>` overrides the game-crate level.
- Log, at `info`: start (version, commit, profile/workout), adapter
  state changes, scan start/end with device count, connect / disconnect
  / failure per device with name, input origin changes
  (mock/device/override), workout load/finish, restart, every ride and
  scenario event, settings changes. At `debug`: one line per second with
  power, cadence, HR, balance, band state.
- Logging is on in `--playtest` mode too.
- The path of the log file is printed once at start and shown at the
  bottom of the ride panel.

## Part D — playtest scenarios (owner's checks; do not answer them)

| File | Stage | Question |
|---|---|---|
| `014-1` | profile `flat`, teleport 100 | Do the grass bands and edge strips make your speed easy to feel? |
| `014-2` | profile `hills`, teleport 300 | With bands and trees, does the steady climb now clearly read as uphill? |
| `014-3` | profile `demo`, teleport 1470 | Do trees and bands help you read the hairpin, without hiding the road? |
| `014-4` | load `lit-1h`, autopilot, FTP 200, seek 600, speed 6, loop 60 s | Is the scenery varied enough for a long easy ride (forest, open, scattered)? |
| `014-5` | profile `demo`, teleport 700 | Do the trees look acceptable as placeholders, and do any of them pop in or out distractingly? |

## Acceptance (numbers and behaviour only)

- `just check`, `just test`, `just clippy` clean.
- Scripted (game in the background, port 7880 free first):
  1. `frame_ms` reported for an open and a forest stretch (find them via
     the placement function) at speed 1.
  2. Restart via API with `mit-sweetspot-6x5` loaded and 300 s elapsed:
     workout still loaded, `elapsed_s` ≈ 0, matches 5, score 0.
  3. A log file exists after start, contains the start line and, after
     `POST /devices/connect-mock {"preset":"quarq"}`, a connect line and
     an input-origin line; with 21 old dummy log files present at start,
     only 20 files remain afterwards (use a temporary config directory
     if the code allows overriding it, otherwise clean up exactly what
     you created).
- You may take two screenshots solely to confirm that trees and bands
  render at all.
- Stop the game; `playtests/results.ndjson` untouched; config directory
  restored to its prior state.
- Update `CLAUDE.md` (scenery layer, restart semantics, log file, new
  flag and state field). Close issue #3 with what changed
  and how it was verified.

## Out of scope

Rider art or rigs, textures, buildings, spectators, weather, sound,
shadows, banking, changing course generation, recording the ride.
