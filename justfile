# Breakaway — justfile
# Only errors reach stdout: no output after filtering (or "ok") means success.

set shell := ["/bin/zsh", "-l", "-c"]

export CARGO_TERM_COLOR := "never"

# Type-check only — fast, errors only.
check:
    @cargo check -q --message-format=short 2>&1 | grep "^error" || echo "ok"

# Build only — errors only.
build:
    @cargo build -q --message-format=short 2>&1 | grep "^error" || echo "ok"

# Run the test suite. Case-sensitive block filter, not a case-insensitive
# grep -vi: cargo test's own harness lines ("running N tests", "test
# result: ... finished in 0.0s") are lowercase and would collide with a
# case-insensitive match meant for cargo's capitalized build-status lines,
# silently swallowing the pass/fail summary. Warning blocks are dropped in
# full (start line through the next blank line). pipestatus[1] propagates
# cargo's real exit code past awk in the pipe.
# The TrainingPeaks export's self-test (no network), the voice clip check
# and the beep clip check run after cargo's.
test:
    @cargo test -q 2>&1 | awk '/^warning:/{skip=1} skip{if($0==""){skip=0}; next} /^ *(Compiling|Checking|Finished|Running) /{next} {print}'; rc=${pipestatus[1]}; \
     python3 tools/tp_export.py --self-test || rc=1; \
     python3 tools/gen_voices.py --check || rc=1; \
     python3 tools/gen_sfx.py --check || rc=1; exit $rc

# Radio voice clips from assets/radio/lines.toml + voices.toml, through the
# local Kokoro server (`docker start tts` first); only changed clips are
# regenerated, `just voices --force` regenerates all. Run after editing a line.
voices *args='':
    @python3 tools/gen_voices.py {{args}}

# Without the server: the committed clips and manifest match the lines and
# voices (part of `just test`).
voices-check:
    @python3 tools/gen_voices.py --check

# Countdown beeps (assets/sfx/, ffmpeg only). `just sfx --check` (part of
# `just test`) fails when a committed clip differs from a fresh render.
sfx *args='':
    @python3 tools/gen_sfx.py {{args}}

# Lint — clippy's whole point is its warnings, so those aren't filtered out,
# only the build-progress noise.
clippy:
    @cargo clippy --all-targets -q 2>&1 | grep -v "^ *Compiling\|^ *Checking\|^ *Finished" || echo "ok"

# Build (errors only), then run the game.
run *args='':
    @cargo build -q --message-format=short 2>&1 | grep "^error" && exit 1; \
     cargo run -q -- {{args}}

# Ride today's planned workout (or `just ride 2026-01-09`): fetches it from
# TrainingPeaks, starts the game with it, and copies everything the game
# prints into ~/breakaway-logs/<timestamp>.log. No structured workout that
# day (or TrainingPeaks unreachable) → free ride. Record the ride on your
# head unit; the game does not save it yet.
ride date='':
    #!/bin/zsh
    set -u
    mkdir -p "$HOME/breakaway-logs"
    log="$HOME/breakaway-logs/$(date +%Y-%m-%dT%H-%M-%S).log"
    dir="$HOME/Library/Application Support/breakaway/workouts"
    args=()
    if just fetch-workout {{date}}; then
      file=$(command ls -t "$dir"/*.json 2>/dev/null | head -1)
      [ -n "$file" ] && args=(--workout "$file")
    else
      echo "no workout fetched: starting a free ride"
    fi
    echo "log: $log"
    cargo build -q --message-format=short 2>&1 | grep "^error" && exit 1
    cargo run -q -- "${args[@]}" 2>&1 | tee "$log"

# Staged playtests: `just playtest 004` (a task), `004-1` (one step) or
# `all`. Answer with Y/N/S (R replays); verdicts go to
# playtests/results.ndjson. Exit 0 only if every verdict is a pass.
playtest sel:
    @cargo build -q --message-format=short 2>&1 | grep "^error" && exit 1; \
     cargo run -q -- --playtest {{sel}}

# Planned bike workouts of a day (default today) from TrainingPeaks →
# <config dir>/breakaway/workouts/<date>-<id>.json. Read-only towards TP;
# uses the TrainingPeaks MCP server's virtualenv and keychain login
# (override the interpreter with TP_PYTHON). Exit 1: nothing structured.
fetch-workout date='':
    @${TP_PYTHON:-$HOME/claude/athlete/trainingpeaks-mcp/.venv/bin/python} tools/tp_export.py fetch {{date}}

# One TrainingPeaks workout by id → <out.json>.
fetch-workout-id id out:
    @${TP_PYTHON:-$HOME/claude/athlete/trainingpeaks-mcp/.venv/bin/python} tools/tp_export.py fetch-id {{id}} {{out}}

# Apply rustfmt.
fmt:
    @cargo fmt

# Formatting and spelling (typos.toml), the pre-commit hook's check.
lint:
    @cargo fmt --check -q 2>&1 | grep '^Diff in' | sed 's/^Diff in /unformatted: /; s/ at line.*//' | sort -u; rc=${pipestatus[1]}; \
     typos || rc=1; exit $rc

# The merge gate, in place of CI (the pre-push hook): lint, check, clippy
# with every warning an error, tests. Stops at the first failing step.
gate:
    @just lint
    @cargo clippy --all-targets -q -- -D warnings 2>&1 | grep -v "^ *Compiling\|^ *Checking\|^ *Finished"; [ ${pipestatus[1]} -eq 0 ]
    @just test

# Once per clone: route git's hooks to .githooks (pre-commit: lint,
# pre-push: gate).
setup:
    @git config core.hooksPath .githooks && chmod +x .githooks/* && echo "hooks: .githooks"

# Wipe build artefacts.
clean:
    @cargo clean -q && echo "cleaned"
