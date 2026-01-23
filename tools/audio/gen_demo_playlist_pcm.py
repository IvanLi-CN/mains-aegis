#!/usr/bin/env python3
"""
Generate PCM-only demo playlist WAV assets for Plan #0004.

Decision:
- We only accept WAV(PCM16LE) on the target chain (audible-quality-first).

Outputs:
- docs/plan/0004:firmware-audio-playback-demo/assets/demo-playlist/{02,03,06}_*.wav
- firmware/assets/audio/demo-playlist/{02,03,06}_*.wav
"""

from __future__ import annotations

import math
import struct
from pathlib import Path

SAMPLE_RATE = 8000
TARGET_PEAK = 14745  # ~= -6 dBFS


def clamp_i16(v: int) -> int:
    return max(-32768, min(32767, v))


def normalize_peak(samples: list[int], target_peak: int) -> list[int]:
    if not samples:
        return []
    peak = max(abs(int(s)) for s in samples)
    if peak <= 0:
        return samples[:]
    scale = target_peak / peak
    return [clamp_i16(int(round(int(s) * scale))) for s in samples]


def synth_chirp(num_samples: int, f0: float, f1: float, amp: float) -> list[int]:
    out: list[int] = []
    phase = 0.0
    for n in range(num_samples):
        t = n / SAMPLE_RATE
        f = f0 + (f1 - f0) * (t / (num_samples / SAMPLE_RATE))
        phase += 2.0 * math.pi * f / SAMPLE_RATE
        out.append(int(math.sin(phase) * amp))
    return out


def synth_melody(num_samples: int, amp: float, base_hz: float) -> list[int]:
    # Repeating note pattern with phase continuity and short attack/release.
    notes = [0, 2, 4, 7, 9, 7, 4, 2]
    note_len = 800  # 0.1s
    ramp = 80  # 10ms
    out: list[int] = []
    phase = 0.0
    for n in range(num_samples):
        semis = notes[(n // note_len) % len(notes)]
        freq = base_hz * (2.0 ** (semis / 12.0))
        local = n % note_len
        if local < ramp:
            w = local / ramp
        elif local >= note_len - ramp:
            w = (note_len - 1 - local) / ramp
        else:
            w = 1.0
        phase += 2.0 * math.pi * freq / SAMPLE_RATE
        out.append(int(math.sin(phase) * amp * w))
    return out


def wav_pcm16le(samples: list[int]) -> bytes:
    data = bytearray()
    for s in samples:
        data.extend(struct.pack("<h", clamp_i16(int(s))))

    n_channels = 1
    bits_per_sample = 16
    block_align = n_channels * (bits_per_sample // 8)
    avg_bytes_per_sec = SAMPLE_RATE * block_align

    fmt = struct.pack(
        "<HHIIHH",
        0x0001,  # WAVE_FORMAT_PCM
        n_channels,
        SAMPLE_RATE,
        avg_bytes_per_sec,
        block_align,
        bits_per_sample,
    )

    def chunk(tag: bytes, payload: bytes) -> bytes:
        pad = b"\x00" if (len(payload) & 1) else b""
        return tag + struct.pack("<I", len(payload)) + payload + pad

    riff_payload = b"WAVE" + chunk(b"fmt ", fmt) + chunk(b"data", bytes(data))
    return b"RIFF" + struct.pack("<I", len(riff_payload)) + riff_payload


def main() -> None:
    seg2 = synth_melody(84800, amp=16000.0, base_hz=660.0)
    seg3 = synth_chirp(74396, f0=120.0, f1=1800.0, amp=12000.0)
    seg6 = synth_melody(81600, amp=16000.0, base_hz=523.25)

    outputs = {
        "02_melody_pcm2.wav": wav_pcm16le(normalize_peak(seg2, TARGET_PEAK)),
        "03_sweep_pcm3.wav": wav_pcm16le(normalize_peak(seg3, TARGET_PEAK)),
        "06_melody_pcm3.wav": wav_pcm16le(normalize_peak(seg6, TARGET_PEAK)),
    }

    targets = [
        Path("docs/plan/0004:firmware-audio-playback-demo/assets/demo-playlist"),
        Path("firmware/assets/audio/demo-playlist"),
    ]

    for t in targets:
        t.mkdir(parents=True, exist_ok=True)
        for name, content in outputs.items():
            out = t / name
            out.write_bytes(content)
            print(f"Wrote {out} ({out.stat().st_size} bytes)")


if __name__ == "__main__":
    main()

