#!/usr/bin/env python3
"""Export planned TrainingPeaks workouts into Breakaway's workout format.

Read-only towards TrainingPeaks: GET requests only. Authentication is the
TrainingPeaks MCP server's (cookie in the macOS keychain); this script never
prints, logs or writes a cookie or token. Run it with that server's
virtualenv python (`just fetch-workout`), except `--self-test`, which needs
only the standard library.

    tp_export.py fetch [YYYY-MM-DD]     every structured bike workout that day
                                        → <config dir>/breakaway/workouts/<date>-<id>.json
    tp_export.py fetch-id ID OUT.json   one workout by id
    tp_export.py fetch-raw ID OUT.json  the raw workout (polyline, comments,
                                        file lists and share keys stripped),
                                        for tools/fixtures/
    tp_export.py --self-test            converts tools/fixtures/*.tp.json and
                                        compares with workouts/samples/
"""

import json
import os
import sys
from datetime import date
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FIXTURES = REPO / "tools" / "fixtures"
SAMPLES = REPO / "workouts" / "samples"

# workoutTypeValueId: 2 = Bike, 8 = MtnBike.
BIKE_TYPES = {2, 8}

BLOCK_KINDS = {"step": "single", "repetition": "repetition", "rampUp": "ramp", "rampDown": "ramp"}
CLASSES = {"warmUp": "warm_up", "active": "active", "rest": "rest", "coolDown": "cool_down"}
CADENCE_UNIT = "roundOrStridePerMinute"

# Raw-fixture keys dropped: comments, file lists and share keys.
STRIP_KEYS = {
    "coachComments",
    "workoutComments",
    "newComment",
    "unreadCommentCount",
    "workoutDeviceFileInfos",
    "attachmentFileInfos",
    "sharedWorkoutInformationKey",
    "sharedWorkoutInformationExpireKey",
}


class ConvertError(Exception):
    pass


def _structure(tp_workout):
    s = tp_workout.get("structure")
    if isinstance(s, str):
        s = json.loads(s)
    if not isinstance(s, dict) or not s.get("structure"):
        raise ConvertError("workout has no structure")
    return s


def _range(target, what):
    lo, hi = target.get("minValue"), target.get("maxValue")
    if lo is None and hi is None:
        raise ConvertError(f"{what}: target without a value")
    return lo, hi


def _step(raw, block, kind, rep, reps):
    name = raw.get("name") or ""
    what = f"step {name!r} in block {block}"
    length = raw.get("length") or {}
    if length.get("unit") != "second":
        raise ConvertError(f"{what}: length unit {length.get('unit')!r}, only 'second' is supported")
    duration = length.get("value")
    if not isinstance(duration, (int, float)) or duration <= 0:
        raise ConvertError(f"{what}: duration {duration!r}")
    intensity = [t for t in raw.get("targets") or [] if "unit" not in t]
    cadence = [t for t in raw.get("targets") or [] if t.get("unit") == CADENCE_UNIT]
    if not intensity:
        raise ConvertError(f"{what}: no intensity target")
    lo, hi = _range(intensity[0], what)
    if lo is None or hi is None:
        v = lo if hi is None else hi
        lo, hi = v * 0.95, v * 1.05
    cad_lo = cad_hi = None
    if cadence:
        cad_lo, cad_hi = _range(cadence[0], what)
        cad_lo = cad_hi if cad_lo is None else cad_lo
        cad_hi = cad_lo if cad_hi is None else cad_hi
    cls = CLASSES.get(raw.get("intensityClass"))
    if cls is None:
        raise ConvertError(f"{what}: intensityClass {raw.get('intensityClass')!r}")
    return {
        "name": name,
        "duration_s": duration,
        "low_pct": lo,
        "high_pct": hi,
        "class": cls,
        "block": block,
        "block_kind": kind,
        "rep": rep,
        "reps": reps,
        "cadence_low": cad_lo,
        "cadence_high": cad_hi,
        "fixed_rest": False,
    }


def convert(tp_workout: dict) -> dict:
    """A TrainingPeaks workout (as `GET .../workouts/{id}` returns it) → a
    Breakaway workout. Lap-button markers (`openDuration`) are dropped; a
    block left without steps gets no block number."""
    s = _structure(tp_workout)
    metric = s.get("primaryIntensityMetric")
    if metric != "percentOfFtp":
        raise ConvertError(f"primaryIntensityMetric {metric!r}, only 'percentOfFtp' is supported")
    steps = []
    block = 0
    for raw_block in s["structure"]:
        kind = BLOCK_KINDS.get(raw_block.get("type"))
        if kind is None:
            raise ConvertError(f"block type {raw_block.get('type')!r}")
        inner = [st for st in raw_block.get("steps") or [] if not st.get("openDuration")]
        if not inner:
            continue
        reps = 1
        if kind == "repetition":
            length = raw_block.get("length") or {}
            reps = length.get("value")
            if length.get("unit") != "repetition" or not isinstance(reps, int) or reps < 1:
                raise ConvertError(f"block {block}: repetition length {length!r}")
        for rep in range(1, reps + 1):
            steps.extend(_step(st, block, kind, rep, reps) for st in inner)
        block += 1
    if not steps:
        raise ConvertError("structure has no steps")
    return {
        "name": tp_workout.get("title") or "",
        "date": str(tp_workout.get("workoutDay") or "")[:10],
        "source": f"trainingpeaks:{tp_workout.get('workoutId')}",
        "steps": steps,
    }


def dumps(workout: dict) -> str:
    """Stable text: one step per line."""
    head = {k: v for k, v in workout.items() if k != "steps"}
    lines = ["{"]
    lines += [f"  {json.dumps(k)}: {json.dumps(v, ensure_ascii=False)}," for k, v in head.items()]
    lines.append('  "steps": [')
    steps = [f"    {json.dumps(st, ensure_ascii=False)}" for st in workout["steps"]]
    lines.append(",\n".join(steps))
    lines += ["  ]", "}"]
    return "\n".join(lines) + "\n"


def strip_raw(tp_workout: dict) -> dict:
    raw = {k: v for k, v in tp_workout.items() if k not in STRIP_KEYS}
    s = raw.get("structure")
    if isinstance(s, str):
        s = json.loads(s)
    if isinstance(s, dict):
        raw["structure"] = {k: v for k, v in s.items() if k != "polyline"}
    return raw


def summary(workout: dict) -> str:
    total = int(sum(st["duration_s"] for st in workout["steps"]))
    return f"{workout['name']}  {len(workout['steps'])} steps  {total // 3600}:{total % 3600 // 60:02}:{total % 60:02}"


def config_dir() -> Path:
    """Same place as the game's `dirs::config_dir()`."""
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support"
    if os.name == "nt":
        return Path(os.environ["APPDATA"])
    return Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")


# ── Network (GET only) ────────────────────────────────────────────────────────


class FetchError(Exception):
    pass


async def _get(client, endpoint):
    r = await client.get(endpoint)
    if not r.success:
        code = r.error_code.value if r.error_code else "API_ERROR"
        raise FetchError(f"{code}: {r.message}")
    return r.data


async def _athlete(client):
    athlete_id = await client.ensure_athlete_id()
    if not athlete_id:
        raise FetchError("TrainingPeaks authentication failed (no athlete id); re-authenticate the MCP server")
    return athlete_id


async def _fetch_one(client, athlete_id, workout_id):
    data = await _get(client, f"/fitness/v6/athletes/{athlete_id}/workouts/{workout_id}")
    if not isinstance(data, dict):
        raise FetchError(f"workout {workout_id} not found")
    return data


async def _fetch_day(day: str):
    from tp_mcp.client import TPClient

    async with TPClient() as client:
        athlete_id = await _athlete(client)
        listed = await _get(client, f"/fitness/v6/athletes/{athlete_id}/workouts/{day}/{day}") or []
        out = []
        for w in listed:
            if w.get("workoutTypeValueId") in BIKE_TYPES:
                out.append(await _fetch_one(client, athlete_id, w["workoutId"]))
        return out


async def _fetch_id(workout_id: str):
    from tp_mcp.client import TPClient

    async with TPClient() as client:
        return await _fetch_one(client, await _athlete(client), workout_id)


def _write(path: Path, workout: dict):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(dumps(workout), encoding="utf-8")
    print(f"{path}  {summary(workout)}")


def cmd_fetch(day: str) -> int:
    import asyncio

    date.fromisoformat(day)
    written = 0
    for raw in asyncio.run(_fetch_day(day)):
        try:
            workout = convert(raw)
        except ConvertError as e:
            if str(e) != "workout has no structure":
                print(f"skipped {raw.get('workoutId')} ({raw.get('title')}): {e}", file=sys.stderr)
            continue
        _write(config_dir() / "breakaway" / "workouts" / f"{day}-{raw.get('workoutId')}.json", workout)
        written += 1
    if not written:
        print(f"no structured bike workout on {day}", file=sys.stderr)
        return 1
    return 0


def cmd_fetch_id(workout_id: str, out: str, raw_only: bool) -> int:
    import asyncio

    raw = asyncio.run(_fetch_id(workout_id))
    path = Path(out)
    if raw_only:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(strip_raw(raw), indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(path)
    else:
        _write(path, convert(raw))
    return 0


# ── Self-test (no network) ────────────────────────────────────────────────────


def _expect_error(tp_workout, needle):
    try:
        convert(tp_workout)
    except ConvertError as e:
        if needle not in str(e):
            return [f"error {str(e)!r} does not mention {needle!r}"]
        return []
    return [f"no error for a workout that should fail with {needle!r}"]


def _synthetic(steps, metric="percentOfFtp", block_type="step"):
    block = {"type": block_type, "length": {"value": 1, "unit": "repetition"}, "steps": steps}
    return {"title": "t", "workoutDay": "2026-01-05T00:00:00", "workoutId": 1,
            "structure": {"structure": [block], "primaryIntensityMetric": metric}}


def _step_raw(**kw):
    st = {"name": "s", "length": {"value": 60, "unit": "second"},
          "targets": [{"minValue": 50, "maxValue": 60}], "intensityClass": "active", "openDuration": False}
    st.update(kw)
    return st


def self_test() -> int:
    errors = []
    fixtures = sorted(FIXTURES.glob("*.tp.json"))
    if not fixtures:
        errors.append(f"no fixtures in {FIXTURES}")
    for fx in fixtures:
        slug = fx.name[: -len(".tp.json")]
        expected_path = SAMPLES / f"{slug}.json"
        try:
            got = convert(json.loads(fx.read_text(encoding="utf-8")))
        except ConvertError as e:
            errors.append(f"{fx.name}: {e}")
            continue
        if not expected_path.exists():
            errors.append(f"{fx.name}: missing {expected_path.relative_to(REPO)}")
            continue
        expected_text = expected_path.read_text(encoding="utf-8")
        if got != json.loads(expected_text):
            errors.append(f"{fx.name}: conversion differs from {expected_path.relative_to(REPO)}")
        elif dumps(got) != expected_text:
            errors.append(f"{fx.name}: {expected_path.relative_to(REPO)} is not in dumps() layout")

    # Known facts about two samples.
    def facts(slug, check):
        path = SAMPLES / f"{slug}.json"
        if path.exists():
            w = json.loads(path.read_text(encoding="utf-8"))
            errors.extend(f"{slug}: {e}" for e in check(w["steps"]))

    def mit(steps):
        out = []
        if (len(steps), sum(s["duration_s"] for s in steps)) != (19, 4200):
            out.append(f"{len(steps)} steps / {sum(s['duration_s'] for s in steps)} s, expected 19 / 4200")
        elif [s["class"] for s in steps[:2]] != ["warm_up"] * 2 or [s["block_kind"] for s in steps[2:5]] != ["ramp"] * 3:
            out.append("expected 2 warm-up then 3 ramp steps")
        else:
            reps = steps[6:18]
            want = [(300, 84, 94), (120, 45, 55)] * 6
            if [(s["duration_s"], s["low_pct"], s["high_pct"]) for s in reps] != want or steps[18]["class"] != "cool_down":
                out.append("expected 6 x (300 s 84-94 % + 120 s 45-55 %) then a cool-down")
        return out

    def lit(steps):
        if len(steps) != 2 or sum(s["duration_s"] for s in steps) != 3600:
            return ["expected 2 steps / 3600 s"]
        s = steps[1]
        if (s["low_pct"], s["high_pct"], s["cadence_low"], s["cadence_high"]) != (62, 70, 88, 98):
            return ["expected step 2 at 62-70 % with cadence 88-98"]
        return []

    facts("mit-sweetspot-6x5", mit)
    facts("lit-1h", lit)

    # Conversion rules on synthetic input.
    w = convert(_synthetic([_step_raw(targets=[{"minValue": 100}])]))
    if (w["steps"][0]["low_pct"], w["steps"][0]["high_pct"]) != (95.0, 105.0):
        errors.append(f"single value 100 → {w['steps'][0]['low_pct']}-{w['steps'][0]['high_pct']}, expected 95-105")
    w = convert(_synthetic([_step_raw(openDuration=True), _step_raw(name="a"), _step_raw(name="b")], block_type="repetition"))
    if [s["name"] for s in w["steps"]] != ["a", "b"]:
        errors.append("lap-button step was not dropped")
    rep = _synthetic([_step_raw(name="on"), _step_raw(name="off", intensityClass="rest")], block_type="repetition")
    rep["structure"]["structure"][0]["length"]["value"] = 3
    w = convert(rep)
    got = [(s["name"], s["rep"], s["reps"], s["class"]) for s in w["steps"]]
    want = [(n, r, 3, c) for r in (1, 2, 3) for n, c in (("on", "active"), ("off", "rest"))]
    if got != want:
        errors.append(f"repetition expanded as {got}")
    ramp = convert(_synthetic([_step_raw()], block_type="rampDown"))
    if ramp["steps"][0]["block_kind"] != "ramp":
        errors.append("rampDown is not a ramp")
    errors += _expect_error(_synthetic([_step_raw(name="far", length={"value": 2, "unit": "kilometer"})]), "'far'")
    errors += _expect_error(_synthetic([_step_raw()], metric="percentOfThresholdHr"), "primaryIntensityMetric")
    errors += _expect_error(_synthetic([_step_raw(intensityClass="sprint")]), "intensityClass")
    errors += _expect_error(_synthetic([_step_raw()], block_type="pyramid"), "block type")

    for e in errors:
        print(f"tp_export self-test: {e}")
    return 1 if errors else 0


def main(argv) -> int:
    try:
        if argv[:1] == ["--self-test"]:
            return self_test()
        if argv[:1] == ["fetch"] and len(argv) <= 2:
            return cmd_fetch(argv[1] if len(argv) == 2 and argv[1] else date.today().isoformat())
        if argv[:1] in (["fetch-id"], ["fetch-raw"]) and len(argv) == 3:
            return cmd_fetch_id(argv[1], argv[2], raw_only=argv[0] == "fetch-raw")
    except (ConvertError, FetchError, ValueError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
