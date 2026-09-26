#!/usr/bin/env python3
"""Generate the original, deterministic 52 s launch-film sound bed."""
from __future__ import annotations
import argparse, math, struct, wave
from pathlib import Path

RATE = 48_000
DURATION = 52.0
SCENE_MARKS = (0.0, 6.0, 13.0, 22.0, 31.0, 38.0, 46.0)


def envelope(t: float, start: float, length: float) -> float:
    x = t - start
    if x < 0 or x >= length:
        return 0.0
    attack = min(1.0, x / 0.025)
    release = min(1.0, (length - x) / 0.12)
    return attack * release


def sample(t: float) -> float:
    # A quiet two-note bed leaves space for future narration without requiring it.
    bed = 0.020 * math.sin(2 * math.pi * 110.0 * t) + 0.010 * math.sin(2 * math.pi * 165.0 * t)
    pulse = 0.0
    for i, mark in enumerate(SCENE_MARKS):
        e = envelope(t, mark + 0.12, 0.34)
        pulse += e * (0.09 * math.sin(2 * math.pi * (440.0 + i * 22.0) * t))
    # Small proof/close accents; deterministic and intentionally restrained.
    for mark, freq in ((39.1, 660.0), (40.4, 740.0), (41.7, 820.0), (43.0, 900.0), (46.3, 520.0)):
        pulse += envelope(t, mark, 0.22) * 0.055 * math.sin(2 * math.pi * freq * t)
    fade = min(1.0, t / 0.8, (DURATION - t) / 1.2)
    return max(-0.28, min(0.28, (bed + pulse) * max(0.0, fade)))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("output", type=Path)
    args = ap.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(args.output), "wb") as out:
        out.setnchannels(2)
        out.setsampwidth(2)
        out.setframerate(RATE)
        chunk = bytearray()
        total = int(DURATION * RATE)
        for i in range(total):
            value = int(round(sample(i / RATE) * 32767.0))
            chunk += struct.pack("<hh", value, value)
            if len(chunk) >= 64 * 1024:
                out.writeframesraw(chunk)
                chunk.clear()
        if chunk:
            out.writeframesraw(chunk)
        out.writeframes(b"")


if __name__ == "__main__":
    main()
