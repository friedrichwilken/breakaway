# 015 — Playtest comments: a note with each verdict

**Repo:** `~/claude/breakaway/`, branch `playtest-comments`, `--no-ff`
merge to `main`.

**Depends on:** 006 (merged). Independent of 014. Read `CLAUDE.md` and
`src/playtest/` first.

## Why

A playtest verdict is only `pass`, `fail` or `skip`. A failed verdict
becomes a new task, and a bare `fail` does not say what was wrong. The
owner wants to type a short comment while judging.

## Behaviour

1. **`N` opens a comment field** in the overlay instead of recording the
   fail at once. `Enter` records the fail with the comment (an empty
   comment is allowed); `Esc` closes the field, drops the text and returns
   to the question without a verdict.
2. **`C` opens the same field** without a verdict attached, for a remark
   on a pass or a skip. `Enter` keeps the note and closes the field; `Esc`
   drops the note and closes it. A kept note is shown under the question;
   `C` reopens it for editing. `Y`, `N` or `S` then record the verdict
   with that note (`N` opens the field with the note already in it).
3. The comment is written to the existing `detail` field of the result
   line (trimmed; one line, no line breaks). `stage_error` keeps its error
   text there. No new field in `results.ndjson`.
4. The summary (overlay and stdout table) shows the comment after the
   title, in parentheses, as it already does for stage errors.
5. A note belongs to one scenario: the next scenario starts with none.
6. While the field is open the scene keeps running and keeps re-staging
   every `loop_seconds`; `R` typed into the field is text, not a replay.
7. While the field is open no typed letter may act as a game key: not the
   verdict keys, not `B` (ride panel), not `D` (devices panel).
8. The key line shows `[C] comment` while asking, and `[Enter] record
   fail   [Esc] back` / `[Enter] keep comment   [Esc] discard` while the
   field is open.

## Architecture

- The runner stays pure (no Bevy or egui types): the note text, the open
  field and what `Enter` does live in `Runner`; the overlay only edits the
  text and reports `Enter` / `Esc`. The verdict is still recorded inside
  `Runner::update`, and the result line is still written in one place
  (`playtest::drive`).
- The `Enter` or `Esc` that closes the field must never also count as the
  summary's quit key, whatever the order of the systems in that frame.

## Tests (unit, no window)

- `N` → text → `Enter`: fail recorded, `detail` is the trimmed text;
  empty text: `detail` empty.
- `N` → `Esc`: no verdict, still asking, note empty.
- `C` → text → `Enter` → `Y`: pass with the note; same for `S`.
- `C` → text → `Esc`: note dropped.
- The note does not carry over to the next scenario.
- Keys passed to `update` while the field is open are ignored; the loop
  re-stage still happens.
- Closing the field on the last scenario with a quit key in the same
  `update` call ends in `Summary`, not `Finished`.
- Result line: comment in `detail`, line breaks replaced by spaces.
- Overlay text: key lines for asking and for both field modes; the kept
  note under the question; the comment in the summary.

## Docs

`playtests/README.md` (keys, `detail`), `CLAUDE.md` Validation section.

## Out of scope

Editing a verdict after it is recorded; comments on the summary screen; a
debug-API endpoint for comments.

## Owner check (no scenario file: the runner cannot stage itself)

Run `just playtest 003`: fail one scenario with a comment containing the
letters `b`, `d`, `y`, `s`, `r` and `q`; pass one after a `C` note. Do the
panels stay closed while typing, and do both comments appear in the
summary and in `playtests/results.ndjson`?
