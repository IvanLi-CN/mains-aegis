#!/usr/bin/env python3
"""Generate status/warning/error preview assets (score + MIDI + WAV) for speaker cues."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from tempfile import TemporaryDirectory

WARNING_INTERVAL_MS_DEFAULT = 2000
TEMPO_BPM = 240
BAR_BEATS = 4.0


@dataclass(frozen=True)
class CueDef:
    cue_id: str
    title_zh: str
    category: str
    trigger_condition_zh: str
    loop_mode: str
    loop_interval_ms: int
    events: list[dict[str, object]]


def note(name: str, beats: float, velocity: int | None = None) -> dict[str, object]:
    event: dict[str, object] = {"note": name, "beats": beats}
    if velocity is not None:
        event["velocity"] = velocity
    return event


def rest(beats: float) -> dict[str, object]:
    return {"rest_beats": beats}


def cues() -> list[CueDef]:
    return [
        CueDef(
            cue_id="boot_startup",
            title_zh="开机音",
            category="status",
            trigger_condition_zh="系统上电启动成功后触发",
            loop_mode="one_shot",
            loop_interval_ms=0,
            events=[note("C5", 0.5), note("E5", 0.5), note("G5", 0.5), note("C6", 0.5), rest(2.0)],
        ),
        CueDef(
            cue_id="mains_present_dc",
            title_zh="市电出现音（仅DC桶）",
            category="status",
            trigger_condition_zh="检测到 DC 桶输入恢复时触发",
            loop_mode="one_shot",
            loop_interval_ms=0,
            events=[note("G4", 0.5), note("D5", 0.5), note("G5", 1.0), rest(2.0)],
        ),
        CueDef(
            cue_id="charge_started",
            title_zh="充电开始音",
            category="status",
            trigger_condition_zh="充电状态从未充电切换为充电中时触发",
            loop_mode="one_shot",
            loop_interval_ms=0,
            events=[note("C5", 0.5), note("E5", 0.5), note("G5", 0.5), note("A5", 0.5), rest(2.0)],
        ),
        CueDef(
            cue_id="charge_completed",
            title_zh="充电完成音",
            category="status",
            trigger_condition_zh="充电状态进入完成态时触发",
            loop_mode="one_shot",
            loop_interval_ms=0,
            events=[note("C5", 0.5), note("E5", 0.5), note("G5", 0.5), note("C6", 1.0), rest(1.5)],
        ),
        CueDef(
            cue_id="shutdown_mode_entered",
            title_zh="进入关闭模式音",
            category="status",
            trigger_condition_zh="系统进入关闭模式流程时触发",
            loop_mode="one_shot",
            loop_interval_ms=0,
            events=[note("E5", 0.5), note("C5", 0.5), note("G4", 1.0), rest(2.0)],
        ),
        CueDef(
            cue_id="mains_absent_dc",
            title_zh="市电不存在告警（仅DC桶）",
            category="warning",
            trigger_condition_zh="DC 桶输入丢失时触发间隔循环",
            loop_mode="interval_loop",
            loop_interval_ms=WARNING_INTERVAL_MS_DEFAULT,
            events=[
                note("F4", 0.25, 126),
                rest(0.25),
                note("F4", 0.25, 122),
                rest(0.25),
                note("D4", 0.5, 124),
                rest(0.5),
                note("F4", 0.25, 120),
                rest(0.25),
                note("D4", 0.5, 122),
                rest(1.0),
            ],
        ),
        CueDef(
            cue_id="high_stress",
            title_zh="压力大告警",
            category="warning",
            trigger_condition_zh="任一模块温度/负载不佳但未触发保护时触发间隔循环",
            loop_mode="interval_loop",
            loop_interval_ms=WARNING_INTERVAL_MS_DEFAULT,
            events=[
                note("C5", 0.25, 122),
                rest(0.25),
                note("D5", 0.25, 120),
                rest(0.25),
                note("E5", 0.25, 118),
                rest(0.25),
                note("D5", 0.25, 116),
                rest(0.25),
                note("C5", 0.5, 114),
                rest(1.5),
            ],
        ),
        CueDef(
            cue_id="battery_low_no_mains",
            title_zh="电池电量低告警（无市电）",
            category="warning",
            trigger_condition_zh="电池低电且市电不存在时触发间隔循环",
            loop_mode="interval_loop",
            loop_interval_ms=WARNING_INTERVAL_MS_DEFAULT,
            events=[
                note("E4", 0.5, 126),
                rest(0.25),
                note("C#4", 0.5, 120),
                rest(0.25),
                note("A3", 0.75, 126),
                rest(0.25),
                note("A3", 0.75, 122),
                rest(0.75),
            ],
        ),
        CueDef(
            cue_id="battery_low_with_mains",
            title_zh="电池电量低告警（有市电）",
            category="warning",
            trigger_condition_zh="电池低电且检测到市电时触发间隔循环",
            loop_mode="interval_loop",
            loop_interval_ms=WARNING_INTERVAL_MS_DEFAULT,
            events=[
                note("G4", 0.5, 122),
                rest(0.25),
                note("D4", 0.5, 116),
                rest(0.25),
                note("C4", 0.5, 118),
                rest(0.25),
                note("D4", 0.5, 114),
                rest(1.25),
            ],
        ),
        CueDef(
            cue_id="shutdown_protection",
            title_zh="停机保护错误",
            category="error",
            trigger_condition_zh="任一模块触发保护动作导致停机时连续循环",
            loop_mode="continuous_loop",
            loop_interval_ms=0,
            events=[
                note("D3", 0.75, 126),
                rest(0.25),
                note("D3", 0.5, 122),
                rest(0.5),
                note("A2", 0.75, 124),
                rest(1.25),
            ],
        ),
        CueDef(
            cue_id="io_over_voltage",
            title_zh="输入输出过压错误",
            category="error",
            trigger_condition_zh="输入或输出检测到过压时连续循环",
            loop_mode="continuous_loop",
            loop_interval_ms=0,
            events=[
                rest(0.25),
                note("A4", 0.25, 126),
                rest(0.25),
                note("C5", 0.25, 122),
                rest(0.25),
                note("E5", 0.25, 124),
                rest(0.25),
                note("A5", 0.25, 120),
                rest(2.0),
            ],
        ),
        CueDef(
            cue_id="io_over_current",
            title_zh="输入输出过流错误",
            category="error",
            trigger_condition_zh="输入或输出检测到过流时连续循环",
            loop_mode="continuous_loop",
            loop_interval_ms=0,
            events=[
                note("G4", 0.5, 122),
                rest(0.5),
                note("D4", 0.5, 118),
                rest(0.5),
                note("G4", 0.5, 120),
                rest(0.25),
                note("D4", 0.25, 114),
                rest(1.0),
            ],
        ),
        CueDef(
            cue_id="io_over_power",
            title_zh="输入输出过功率错误",
            category="error",
            trigger_condition_zh="输入或输出检测到过功率时连续循环",
            loop_mode="continuous_loop",
            loop_interval_ms=0,
            events=[
                note("F4", 0.5, 120),
                rest(0.25),
                note("C4", 0.5, 116),
                rest(0.25),
                note("F3", 0.75, 122),
                rest(0.25),
                note("C4", 0.5, 114),
                rest(1.0),
            ],
        ),
        CueDef(
            cue_id="module_fault",
            title_zh="模块故障错误",
            category="error",
            trigger_condition_zh="部分硬件通信失败期间连续循环",
            loop_mode="continuous_loop",
            loop_interval_ms=0,
            events=[
                note("C4", 0.25, 118),
                rest(0.25),
                note("G3", 0.25, 112),
                rest(0.25),
                note("C4", 0.25, 116),
                rest(0.25),
                note("G3", 0.25, 110),
                rest(0.25),
                note("C4", 0.25, 118),
                rest(0.25),
                note("G3", 0.25, 110),
                rest(0.25),
                rest(1.0),
            ],
        ),
        CueDef(
            cue_id="battery_protection",
            title_zh="电池保护错误",
            category="error",
            trigger_condition_zh="BMS 触发保护时连续循环",
            loop_mode="continuous_loop",
            loop_interval_ms=0,
            events=[
                note("A3", 0.5, 124),
                rest(0.25),
                note("Bb3", 0.25, 120),
                rest(0.25),
                note("A3", 0.5, 122),
                rest(0.5),
                note("E3", 0.75, 116),
                rest(0.25),
                note("D3", 0.5, 114),
                rest(0.25),
            ],
        ),
    ]


def category_audio_profile(category: str) -> dict[str, object]:
    volume = {"status": 0.60, "warning": 0.82, "error": 0.76}[category]
    fade_ms = {"status": 6, "warning": 5, "error": 4}[category]
    harmonics = {
        "status": [1.0, 0.16, 0.04],
        "warning": [1.0, 0.22, 0.08],
        "error": [1.0, 0.28, 0.10],
    }[category]
    return {
        "waveform": "sine",
        "sample_rate_hz": 44_100,
        "volume": volume,
        "fade_ms": fade_ms,
        "harmonics": harmonics,
    }


def score_for(cue: CueDef) -> dict[str, object]:
    return {
        "tempo_bpm": TEMPO_BPM,
        "audio": category_audio_profile(cue.category),
        "midi": {
            "channel": 0,
            "program": 80,
            "velocity": 98,
        },
        "events": cue.events,
    }


def duration_ms(score: dict[str, object]) -> int:
    tempo_bpm = float(score.get("tempo_bpm", 120.0))
    total_s = 0.0
    for event in score["events"]:  # type: ignore[index]
        event_map: dict[str, object] = event  # type: ignore[assignment]
        if "ms" in event_map:
            total_s += float(event_map["ms"]) / 1000.0
            continue
        if "rest_ms" in event_map:
            total_s += float(event_map["rest_ms"]) / 1000.0
            continue
        if "beats" in event_map:
            total_s += (60.0 / tempo_bpm) * float(event_map["beats"])
            continue
        if "rest_beats" in event_map:
            total_s += (60.0 / tempo_bpm) * float(event_map["rest_beats"])
            continue
        raise ValueError(f"Unsupported event duration shape: {event_map}")
    return int(round(total_s * 1000.0))


def duration_beats(score: dict[str, object]) -> float:
    tempo_bpm = float(score.get("tempo_bpm", TEMPO_BPM))
    total_beats = 0.0
    for event in score["events"]:  # type: ignore[index]
        event_map: dict[str, object] = event  # type: ignore[assignment]
        if "beats" in event_map:
            total_beats += float(event_map["beats"])
            continue
        if "rest_beats" in event_map:
            total_beats += float(event_map["rest_beats"])
            continue
        if "ms" in event_map:
            total_beats += (float(event_map["ms"]) / 1000.0) * tempo_bpm / 60.0
            continue
        if "rest_ms" in event_map:
            total_beats += (float(event_map["rest_ms"]) / 1000.0) * tempo_bpm / 60.0
            continue
        raise ValueError(f"Unsupported event duration shape: {event_map}")
    return total_beats


def write_json(path: Path, payload: dict[str, object]) -> None:
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def generated_at_iso_utc_date() -> str:
    # Keep regenerated manifests stable within the same UTC day.
    day = datetime.now(timezone.utc).date().isoformat()
    return f"{day}T00:00:00Z"


def run_buzzer_preview(tool_script: Path, score_path: Path, out_dir: Path, stem: str) -> None:
    cmd = [
        sys.executable,
        str(tool_script),
        "--in",
        str(score_path),
        "--out-dir",
        str(out_dir),
        "--stem",
        stem,
    ]
    # Preserve a machine-readable stdout channel for the final JSON report.
    subprocess.run(cmd, check=True, stdout=sys.stderr, stderr=sys.stderr)


def publish_outputs(staging_dir: Path, base_dir: Path) -> None:
    targets = (
        ("scores", True),
        ("audio", True),
        ("cues.manifest.json", False),
        ("generation-report.json", False),
    )
    for name, is_dir in targets:
        src = staging_dir / name
        dst = base_dir / name
        if not src.exists():
            raise FileNotFoundError(f"missing staged output: {src}")

        if dst.exists():
            if is_dir:
                shutil.rmtree(dst)
            else:
                dst.unlink()

        shutil.move(str(src), str(dst))


def parse_args() -> argparse.Namespace:
    local_tool = Path(__file__).resolve().with_name("buzzer_preview.py")
    parser = argparse.ArgumentParser(description="Generate status/warning/error preview assets.")
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="Repository root path (default: inferred from script location).",
    )
    parser.add_argument(
        "--buzzer-tool",
        type=Path,
        default=local_tool,
        help="Path to buzzer_preview.py (default: tools/audio/buzzer_preview.py)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    tool_script = args.buzzer_tool.resolve()

    if not tool_script.exists():
        raise FileNotFoundError(f"buzzer preview tool not found: {tool_script}")

    base_dir = repo_root / "docs/audio-cues-preview"
    cue_defs = cues()

    if len(cue_defs) != 15:
        raise ValueError(f"Expected 15 cues, got {len(cue_defs)}")

    seen_ids: set[str] = set()
    for cue in cue_defs:
        if cue.cue_id in seen_ids:
            raise ValueError(f"duplicate cue id: {cue.cue_id}")
        seen_ids.add(cue.cue_id)
    base_dir.mkdir(parents=True, exist_ok=True)

    with TemporaryDirectory(prefix=".audio-cues-stage-", dir=base_dir) as temp_dir:
        staging_dir = Path(temp_dir)
        score_dir = staging_dir / "scores"
        audio_dir = staging_dir / "audio"
        score_dir.mkdir(parents=True, exist_ok=True)
        audio_dir.mkdir(parents=True, exist_ok=True)

        manifest_items: list[dict[str, object]] = []
        for cue in cue_defs:
            score_payload = score_for(cue)
            cue_beats = duration_beats(score_payload)
            bars = cue_beats / BAR_BEATS
            if abs(cue_beats - BAR_BEATS) > 1e-6:
                raise ValueError(
                    f"cue {cue.cue_id} must use unified {BAR_BEATS:g} beats, got {cue_beats:g}"
                )
            if abs(bars - round(bars)) > 1e-6:
                raise ValueError(
                    f"cue {cue.cue_id} must be an integer number of bars, got {bars:.4f} bars"
                )
            score_path = score_dir / f"{cue.cue_id}.json"
            write_json(score_path, score_payload)

            run_buzzer_preview(tool_script=tool_script, score_path=score_path, out_dir=audio_dir, stem=cue.cue_id)

            cue_duration_ms = duration_ms(score_payload)
            item = {
                "id": cue.cue_id,
                "title_zh": cue.title_zh,
                "category": cue.category,
                "trigger_condition_zh": cue.trigger_condition_zh,
                "loop_mode": cue.loop_mode,
                "loop_interval_ms": cue.loop_interval_ms,
                "score_path": f"scores/{cue.cue_id}.json",
                "wav_path": f"audio/{cue.cue_id}.wav",
                "mid_path": f"audio/{cue.cue_id}.mid",
                "duration_ms": cue_duration_ms,
            }
            manifest_items.append(item)

        category_counts: dict[str, int] = {"status": 0, "warning": 0, "error": 0}
        for item in manifest_items:
            category_counts[str(item["category"])] += 1

        if category_counts != {"status": 5, "warning": 4, "error": 6}:
            raise ValueError(f"Unexpected category counts: {category_counts}")

        score_count = len(list(score_dir.glob("*.json")))
        wav_count = len(list(audio_dir.glob("*.wav")))
        mid_count = len(list(audio_dir.glob("*.mid")))
        expected_count = len(cue_defs)
        if not (score_count == wav_count == mid_count == expected_count):
            raise ValueError(
                "generated artifact counts mismatch: "
                f"score={score_count}, wav={wav_count}, mid={mid_count}, expected={expected_count}"
            )

        manifest_payload = {
            "version": 1,
            "profile": "speaker_chime_v1",
            "generated_at": generated_at_iso_utc_date(),
            "tempo_bpm": TEMPO_BPM,
            "bar_beats": BAR_BEATS,
            "warning_interval_ms_default": WARNING_INTERVAL_MS_DEFAULT,
            "items": manifest_items,
        }
        write_json(staging_dir / "cues.manifest.json", manifest_payload)

        report_payload = {
            "score_count": score_count,
            "wav_count": wav_count,
            "mid_count": mid_count,
            "expected_count": expected_count,
            "category_counts": category_counts,
            "total_duration_ms": sum(int(item["duration_ms"]) for item in manifest_items),
        }
        write_json(staging_dir / "generation-report.json", report_payload)

        publish_outputs(staging_dir=staging_dir, base_dir=base_dir)
        print(json.dumps(report_payload, ensure_ascii=False))
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
