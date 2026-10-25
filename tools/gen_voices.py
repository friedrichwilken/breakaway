#!/usr/bin/env python3
"""Generate the team radio's voice clips from assets/radio/lines.toml.

Speech comes from a local Kokoro text-to-speech server (Docker
container `tts`, Kokoro-FastAPI, OpenAI-compatible) at 127.0.0.1:8880; this
script talks to no other host and never starts, stops or changes the
container. Voices per speaker: assets/radio/voices.toml. Standard library
plus ffmpeg / ffprobe (FFMPEG / FFPROBE override the paths).

    gen_voices.py            generate the clips that changed or are missing,
                             delete clips that no line needs any more
    gen_voices.py --force    regenerate every clip
    gen_voices.py --check    no server: the manifest covers exactly the clips
                             the lines and voices require, with matching
                             hashes, and every file exists (exit 1 if not)

One clip per key, variant (1-based) and placeholder value:
assets/radio/audio/<key>-<variant>[-s<secs>][-m<matches>].ogg, from the
variant's `spoken` text (else its `text`); `{secs}` is rendered as 10 and 5,
`{matches}` as 1-4. Each clip: leading and trailing silence trimmed to about
50 ms, peak-normalised to PEAK_DB, Ogg Vorbis at 24 kHz. No other
processing. assets/radio/audio/manifest.json maps each file to a hash of
(spoken text, voice, lang_code, speed) and its duration in seconds; the game
reads the durations from it.
"""

import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
LINES = REPO / "assets" / "radio" / "lines.toml"
VOICES = REPO / "assets" / "radio" / "voices.toml"
AUDIO = REPO / "assets" / "radio" / "audio"
MANIFEST = AUDIO / "manifest.json"

SERVER = "http://127.0.0.1:8880"
FFMPEG = os.environ.get("FFMPEG", "/opt/homebrew/bin/ffmpeg")
FFPROBE = os.environ.get("FFPROBE", "/opt/homebrew/bin/ffprobe")

SECS = [10, 5]
MATCHES = [1, 2, 3, 4]
SAMPLE_RATE = 24000
# Peak level every clip is normalised to (dBFS).
PEAK_DB = -1.0
# Silence kept at each end (s) and what counts as silence.
KEEP_SILENCE_S = 0.05
SILENCE_DB = -45
# A clip shorter than this (after trimming) is retried once, then reported.
MIN_CLIP_S = 0.3

VOICE_KEYS = {"voice", "speed", "lang_code"}

# Only the local server, never through a proxy.
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


class Fail(Exception):
    pass


def placeholders(text):
    return re.findall(r"\{(\w+)\}", text)


def load_voices():
    with open(VOICES, "rb") as f:
        cfg = tomllib.load(f)
    out = {}
    for speaker, v in cfg.items():
        if not isinstance(v, dict) or "voice" not in v:
            raise Fail(f"{VOICES}: [{speaker}] needs a `voice`")
        unknown = set(v) - VOICE_KEYS
        if unknown:
            raise Fail(f"{VOICES}: [{speaker}] unknown keys {sorted(unknown)}")
        out[speaker] = {"voice": v["voice"], "speed": float(v.get("speed", 1.0)), "lang_code": v.get("lang_code")}
    return out


def required_clips():
    """{file name: (spoken text, voice config)} in the lines file's order."""
    with open(LINES, "rb") as f:
        lines = tomllib.load(f)["line"]
    voices = load_voices()
    clips = {}
    for line in lines:
        key, speaker, text = line["key"], line["speaker"], line["text"]
        spoken = line.get("spoken", text)
        if len(spoken) != len(text):
            raise Fail(f"{key}: spoken has {len(spoken)} variants, text {len(text)}")
        if speaker not in voices:
            raise Fail(f"{key}: no voice for speaker {speaker!r} in {VOICES}")
        for v, template in enumerate(spoken, start=1):
            names = placeholders(template)
            bad = [n for n in names if n not in ("secs", "matches")]
            if bad:
                raise Fail(f"{key} variant {v}: spoken text may only use {{secs}} and {{matches}}, not {bad}")
            for s in SECS if "secs" in names else [None]:
                for m in MATCHES if "matches" in names else [None]:
                    name = f"{key}-{v}" + (f"-s{s}" if s is not None else "") + (f"-m{m}" if m is not None else "")
                    rendered = template.replace("{secs}", str(s)).replace("{matches}", str(m))
                    clips[name + ".ogg"] = (rendered, voices[speaker])
    return clips


def clip_hash(text, voice):
    data = json.dumps([text, voice["voice"], voice["lang_code"] or "", voice["speed"]], ensure_ascii=False)
    return hashlib.sha256(data.encode()).hexdigest()[:16]


def read_manifest():
    try:
        return json.loads(MANIFEST.read_text())["clips"]
    except FileNotFoundError:
        return {}


def write_manifest(entries):
    body = {"clips": {k: entries[k] for k in sorted(entries)}}
    MANIFEST.write_text(json.dumps(body, indent=2, ensure_ascii=False) + "\n")


def check():
    clips = required_clips()
    manifest = read_manifest()
    problems = []
    for name in clips.keys() - manifest.keys():
        problems.append(f"missing from the manifest: {name}")
    for name in manifest.keys() - clips.keys():
        problems.append(f"in the manifest but no line needs it: {name}")
    for name, (text, voice) in clips.items():
        entry = manifest.get(name)
        if entry is None:
            continue
        if entry.get("hash") != clip_hash(text, voice):
            problems.append(f"stale (line or voice changed): {name}")
        if not (AUDIO / name).is_file():
            problems.append(f"file missing: {name}")
        if not isinstance(entry.get("duration_s"), (int, float)) or entry["duration_s"] <= 0:
            problems.append(f"no duration: {name}")
    if AUDIO.is_dir():
        for f in AUDIO.glob("*.ogg"):
            if f.name not in clips:
                problems.append(f"file no line needs: {f.name}")
    if problems:
        for p in sorted(problems):
            print(f"voices-check: {p}")
        print("voices-check: run `just voices` (the TTS server must be running)")
        return 1
    return 0


def get_voices():
    try:
        with OPENER.open(f"{SERVER}/v1/audio/voices", timeout=10) as r:
            data = json.loads(r.read())
    except (urllib.error.URLError, OSError, ValueError) as e:
        print(f"TTS server at {SERVER} does not answer ({e}); start it with: docker start tts")
        sys.exit(1)
    return sorted(v["id"] if isinstance(v, dict) else v for v in data.get("voices", []))


def speak(text, voice):
    body = {"model": "kokoro", "input": text, "voice": voice["voice"], "response_format": "wav", "speed": voice["speed"]}
    if voice["lang_code"]:
        body["lang_code"] = voice["lang_code"]
    req = urllib.request.Request(f"{SERVER}/v1/audio/speech", json.dumps(body).encode(), {"Content-Type": "application/json"})
    try:
        with OPENER.open(req, timeout=300) as r:
            return r.read()
    except (urllib.error.URLError, OSError) as e:
        print(f"TTS server at {SERVER} stopped answering ({e}); start it with: docker start tts")
        sys.exit(1)


def run(args):
    p = subprocess.run(args, capture_output=True, text=True)
    if p.returncode != 0:
        raise Fail(f"{Path(args[0]).name} failed: {p.stderr.strip()[-500:]}")
    return p


def duration_s(path):
    out = run([FFPROBE, "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)]).stdout
    return float(out.strip())


def peak_db(path):
    err = run([FFMPEG, "-hide_banner", "-nostats", "-i", str(path), "-af", "volumedetect", "-f", "null", "-"]).stderr
    m = re.search(r"max_volume: (-?[\d.]+|-inf) dB", err)
    if not m or m.group(1) == "-inf":
        raise Fail(f"{path.name}: silent")
    return float(m.group(1))


def has_libvorbis():
    return " libvorbis " in run([FFMPEG, "-hide_banner", "-encoders"]).stdout


def trim(wav, out):
    keep = f"start_periods=1:start_threshold={SILENCE_DB}dB:start_silence={KEEP_SILENCE_S}"
    chain = f"silenceremove={keep},areverse,silenceremove={keep},areverse"
    run([FFMPEG, "-v", "error", "-y", "-i", str(wav), "-af", chain, "-ac", "1", "-ar", str(SAMPLE_RATE), str(out)])


def encode(wav, out, gain_db, libvorbis):
    # Mono with libvorbis; ffmpeg's own Vorbis encoder only writes two
    # channels, so without libvorbis the mono voice goes into both.
    # The channel is copied as it is (`-ac 2` would mix it in at -3 dB).
    if libvorbis:
        chain, codec = f"volume={gain_db:.2f}dB", ["-c:a", "libvorbis", "-q:a", "5", "-ac", "1"]
    else:
        chain, codec = f"volume={gain_db:.2f}dB,pan=stereo|c0=c0|c1=c0", ["-c:a", "vorbis", "-strict", "-2"]
    # bitexact: a fixed Ogg stream serial and no encoder tag, so the same
    # input gives the same file (gen_sfx.py checks its clips byte for byte).
    run([FFMPEG, "-v", "error", "-y", "-i", str(wav), "-af", chain, "-ar", str(SAMPLE_RATE), *codec, "-fflags", "+bitexact", "-flags", "+bitexact", str(out)])


def make_clip(name, text, voice, tmp, libvorbis):
    """Generates one clip; returns its duration, or None when the server
    gave an empty or too short clip twice."""
    raw, trimmed = tmp / "raw.wav", tmp / "trimmed.wav"
    for attempt in (1, 2):
        data = speak(text, voice)
        if len(data) > 44:
            raw.write_bytes(data)
            trim(raw, trimmed)
            if duration_s(trimmed) >= MIN_CLIP_S:
                break
        print(f"  {name}: empty or under {MIN_CLIP_S} s (attempt {attempt})")
    else:
        return None
    encode(trimmed, AUDIO / name, PEAK_DB - peak_db(trimmed), libvorbis)
    return round(duration_s(AUDIO / name), 3)


def generate(force):
    clips = required_clips()
    ids = get_voices()
    used = sorted({v["voice"] for _, v in clips.values()})
    unknown = [v for v in used if v not in ids]
    if unknown:
        print(f"unknown voice id(s) {unknown} in {VOICES}; available: {', '.join(ids)}")
        return 1
    AUDIO.mkdir(parents=True, exist_ok=True)
    manifest = read_manifest()
    deleted = 0
    for name in list(manifest):
        if name not in clips:
            del manifest[name]
    for f in AUDIO.glob("*.ogg"):
        if f.name not in clips:
            f.unlink()
            deleted += 1
    todo = [
        (name, text, voice)
        for name, (text, voice) in clips.items()
        if force or manifest.get(name, {}).get("hash") != clip_hash(text, voice) or not (AUDIO / name).is_file()
    ]
    libvorbis = has_libvorbis()
    if todo and not libvorbis:
        print("note: this ffmpeg has no libvorbis; clips are written with its own Vorbis encoder (two identical channels)")
    failed = []
    with tempfile.TemporaryDirectory() as tmp:
        for i, (name, text, voice) in enumerate(todo, start=1):
            print(f"[{i}/{len(todo)}] {name} ({voice['voice']}): {text}")
            d = make_clip(name, text, voice, Path(tmp), libvorbis)
            if d is None:
                failed.append(name)
                manifest.pop(name, None)
                (AUDIO / name).unlink(missing_ok=True)
                continue
            manifest[name] = {"hash": clip_hash(text, voice), "duration_s": d}
            write_manifest(manifest)
    write_manifest(manifest)
    durations = sorted((e["duration_s"], n) for n, e in manifest.items())
    size = sum((AUDIO / n).stat().st_size for n in manifest)
    print(f"voices: {', '.join(used)}; generated {len(todo) - len(failed)}, kept {len(clips) - len(todo)}, deleted {deleted}")
    if durations:
        print(
            f"{len(manifest)} clips, {size / 1024:.0f} KiB; shortest {durations[0][1]} {durations[0][0]} s, "
            f"longest {durations[-1][1]} {durations[-1][0]} s"
        )
    if failed:
        print(f"no usable clip after a retry: {', '.join(failed)}")
        return 1
    return 0


def main():
    args = sys.argv[1:]
    if args not in ([], ["--force"], ["--check"]):
        print(__doc__)
        return 2
    try:
        if args == ["--check"]:
            return check()
        return generate(force=args == ["--force"])
    except Fail as e:
        print(f"gen_voices: {e}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
