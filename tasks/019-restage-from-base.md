# 019 — Playtest re-stage starts from the base

**Repo:** `~/claude/breakaway/`, branch `restage-from-base`, `--no-ff`
merge to `main`.

**Depends on:** 015 (merged). Read `src/playtest/runner.rs`.

## Why

Owner playtest 2026-10-02: 009-1 and 009-3 ended as `stage_error` ("a
loaded workout owns the course; unload it first"). A re-stage (every
`loop_seconds`, or `R`) replays only the scenario's stage. A stage that
begins with `set_profile` and then loads a workout is refused the second
time, because the workout from the first staging is still loaded. Every
scenario of that shape dies when the owner takes longer than its loop
time to answer.

## Change

1. A re-stage applies the base commands (`base_commands`) and then the
   stage, exactly like the first staging. A failing base or stage command
   records `stage_error` as before.
2. An open comment field and the note survive a re-stage.
3. Test: every checked-in scenario is staged twice in a row from the base
   on the same world without an error (this would have caught the bug).
4. Runner tests and docs (`playtests/README.md`, `CLAUDE.md` Validation)
   say "base, then stage" for the loop.

## Out of scope

Changing any scenario file.
