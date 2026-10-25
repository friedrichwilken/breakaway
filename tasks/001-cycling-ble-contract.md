# 001 — cycling-ble: API contract and contract tests

**Repo:** `~/claude/libs/cycling-ble/cycling-ble/` (the public
cycling-ble GitHub repo). Branch `api-contract`, GitHub PR, per that
repo's `CLAUDE.md`. This repo is public: no mention of Breakaway's rider
or session narration in code, docs or commit messages.

**Depends on:** nothing. **Blocks:** 002 (so consumers adapt once).

## Why

The crate will be published as 1.0 once its API has held up in two real
consumers. Until then we want the API written down and mechanically
guarded, so every change to it is deliberate.

Versioning rule being implemented: the crate version is the contract
version. Breaking the contract needs a breaking bump (minor while `0.x`,
major from `1.0`); additions are additive bumps; fixes are patches.

## Deliverables

### 1. `#[non_exhaustive]`

Add `#[non_exhaustive]` to every public result struct and public enum:
`CyclingPowerMeasurement`, `HeartRateMeasurement`, `CscMeasurement`,
`IndoorBikeData`, `PedalPowerBalanceReference`, `ClickButtonState`, and any
other public struct/enum found in `src/`. Not on `ParseError` (consumers
may construct it in tests; its two fields are final).

Make sure each of those types derives `Debug, Clone, PartialEq` (add what
is missing) so contract tests can compare whole values.

### 2. Contract tests — `tests/contract/`

Integration tests, so they can only use the public API.

- `tests/contract/main.rs` with one module per characteristic:
  `power.rs`, `heart_rate.rs`, `csc.rs`, `ftms.rs`, `helpers.rs`
  (`revolutions_per_minute` and the public constants), and
  `zwift_click.rs` behind `#[cfg(feature = "zwift-click")]`.
- Golden vectors: for every existing unit test in `src/` whose input is a
  real-device capture or a spec-derived payload, add a contract test with
  the same bytes asserting the **complete** parsed value (every field), not
  single fields. Because the structs are `non_exhaustive`, build the
  expected value by parsing is not allowed; assert field by field in one
  helper per type instead.
- Error contract: for each parser, a payload truncated at every length
  from 0 to full-1 returns `Err(ParseError { .. })` with `got` equal to the
  actual length, or `Ok` only where the shorter payload is itself valid.
- Never panics: `tests/contract/no_panic.rs` feeds each parser all payloads
  of length 0..=3 exhaustively over the first two bytes (flags), then
  100 000 pseudo-random payloads of length 0..=40 from a fixed-seed LCG
  written inline (no new dependency). The test passes if nothing panics.
- `tests/contract/README.md`: states the append-only rule — within one
  breaking version, existing contract tests are never edited or deleted;
  new ones may be added. The one exception: an expectation that
  contradicts the Bluetooth spec or real hardware is a bug and may be
  corrected in a patch release with a `CHANGELOG.md` "Fixed" entry.

Existing unit tests in `src/` stay as they are.

### 3. Public API snapshot

- Commit `public-api.txt` at the repo root: output of
  `cargo public-api --all-features` (tool: `cargo install cargo-public-api`;
  it needs a nightly toolchain installed alongside stable).
- `justfile`: `api` regenerates the file, `api-check` fails if the current
  API differs from the committed file.
- CI (`.github/workflows/ci.yml`): new job `public-api` that installs
  nightly + `cargo-public-api` and runs the same diff. Add
  `cargo test --all-targets` already covers `tests/contract`; confirm both
  feature configurations run it.

### 4. `STABILITY.md`

Short, factual, at the repo root; linked from `README.md`. Content:

- what is covered: everything in `public-api.txt` built with default
  features; feature-gated modules (`zwift-click`) are explicitly outside
  the contract until promoted;
- behavioural guarantees: parsers never panic on any input; a field absent
  from the payload is `None`, never a zero; units and scaling are those in
  each field's doc comment (check every public field has its unit in the
  doc comment and add the missing ones);
- no runtime dependencies with default features;
- MSRV 1.74; raising it is an additive (minor) change, never a patch;
- the versioning rule from "Why" above and the append-only test rule.

### 5. Version and changelog

Bump to `0.2.0` (`non_exhaustive` is breaking). `CHANGELOG.md` under
Unreleased: Added (contract tests, `STABILITY.md`, API snapshot), Changed
(`non_exhaustive`).

## Acceptance

- `just check`, `just test`, `just clippy`, `just api-check` all clean, with
  and without `--features zwift-click`.
- `cargo test --test contract` runs at least one golden test per
  characteristic plus the no-panic test.
- If the no-panic test finds a panic, fix the parser in this PR and add the
  offending bytes as a named regression test.
- `grep -rn "Breakaway" .` in the repo returns nothing.

## Out of scope

`cargo semver-checks` (comes with the publish task, it needs a published
baseline), new parsers, publishing, changing any parsed value or field
name, adapting consumers (trainer-alerts adapts in task 002).
