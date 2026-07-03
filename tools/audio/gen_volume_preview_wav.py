#!/usr/bin/env python3
"""Generate the firmware volume preview tone as a WAV preview asset."""

from __future__ import annotations

import struct
import wave
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DOCS_AUDIO_DIR = ROOT / "docs" / "audio-cues-preview" / "interaction-feedback" / "audio"
DOCS_SITE_AUDIO_DIR = (
    ROOT / "docs-site" / "docs" / "public" / "audio-cues" / "interaction-feedback"
)

PLAYBACK_SAMPLE_RATE_HZ = 8_000
PREVIEW_HALF_PERIOD_SAMPLES = PLAYBACK_SAMPLE_RATE_HZ // 1_500
PREVIEW_PULSE_SAMPLES = (PLAYBACK_SAMPLE_RATE_HZ * 110) // 1_000
PREVIEW_EDGE_RAMP_SAMPLES = (PLAYBACK_SAMPLE_RATE_HZ * 6) // 1_000
PREVIEW_PEAK_AMPLITUDE = 10_500


def preview_sample(sample_index: int) -> int:
    edge_ramp = max(PREVIEW_EDGE_RAMP_SAMPLES, 1)
    attack = min(sample_index + 1, edge_ramp)
    release = min(PREVIEW_PULSE_SAMPLES - sample_index, edge_ramp)
    envelope = min(attack, release)
    amplitude = (PREVIEW_PEAK_AMPLITUDE * envelope) // edge_ramp
    polarity = 1 if (sample_index // max(PREVIEW_HALF_PERIOD_SAMPLES, 1)) % 2 == 0 else -1
    return amplitude * polarity


def write_wav(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(PLAYBACK_SAMPLE_RATE_HZ)
        for sample_index in range(PREVIEW_PULSE_SAMPLES):
            wav.writeframesraw(struct.pack("<h", preview_sample(sample_index)))


def main() -> None:
    for out_dir in (DOCS_AUDIO_DIR, DOCS_SITE_AUDIO_DIR):
        write_wav(out_dir / "volume_preview.wav")


if __name__ == "__main__":
    main()
