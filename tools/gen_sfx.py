#!/usr/bin/env python3
"""Generates the countdown beeps (`just sfx`): assets/sfx/tick.ogg (880 Hz,
120 ms) and assets/sfx/go.ogg (1320 Hz, 400 ms), sine tones with 5 ms
fades at -1 dBFS, encoded with the same Vorbis settings as the voice clips
(gen_voices.encode). Needs ffmpeg only, no server. `--check` exits 1 when a
clip is missing or differs from a fresh render."""

import math
import struct
import sys
import tempfile
import wave
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_voices  # noqa: E402

SFX = gen_voices.REPO / "assets" / "sfx"
SAMPLE_RATE = gen_voices.SAMPLE_RATE
PEAK = 10 ** (gen_voices.PEAK_DB / 20)
FADE_S = 0.005

# name -> (frequency Hz, length s)
CLIPS = {"tick": (880.0, 0.12), "go": (1320.0, 0.4)}


def tone(path, hz, length_s):
    n = int(SAMPLE_RATE * length_s)
    fade = int(SAMPLE_RATE * FADE_S)
    frames = bytearray()
    for i in range(n):
        env = min(1.0, (i + 1) / fade, (n - i) / fade)
        v = PEAK * env * math.sin(2 * math.pi * hz * i / SAMPLE_RATE)
        frames += struct.pack("<h", int(round(v * 32767)))
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        w.writeframes(bytes(frames))


def render(out_dir):
    libvorbis = gen_voices.has_libvorbis()
    with tempfile.TemporaryDirectory() as tmp:
        for name, (hz, length_s) in CLIPS.items():
            wav = Path(tmp) / f"{name}.wav"
            tone(wav, hz, length_s)
            gen_voices.encode(wav, Path(out_dir) / f"{name}.ogg", 0.0, libvorbis)


def check():
    with tempfile.TemporaryDirectory() as tmp:
        render(tmp)
        bad = []
        for name in CLIPS:
            committed = SFX / f"{name}.ogg"
            if not committed.exists():
                bad.append(f"{committed.relative_to(gen_voices.REPO)}: missing")
            elif committed.read_bytes() != (Path(tmp) / f"{name}.ogg").read_bytes():
                bad.append(f"{committed.relative_to(gen_voices.REPO)}: differs from a fresh render")
    for line in bad:
        print(line)
    return 1 if bad else 0


def main():
    if "--check" in sys.argv[1:]:
        return check()
    SFX.mkdir(parents=True, exist_ok=True)
    render(SFX)
    return 0


if __name__ == "__main__":
    sys.exit(main())
