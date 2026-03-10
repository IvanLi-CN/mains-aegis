#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime, timezone
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

SUSPICIOUS_VOLTAGE_MV = 5911
SUSPICIOUS_CURRENT_MA = 5911
SUSPICIOUS_STATUS = 0x1717
MAX_SAMPLE_STREAK_GAP_SEC = 15.0
SESSION_BOUNDARY_EVENT = "monitor_session_start"

LOG_LEVEL_PREFIX = r"(?:\[[A-Z ]+\]\s+)?"

SAMPLE_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"temp_c_x10=(?P<temp>-?\d+) voltage_mv=(?P<voltage>\d+) "
    rf"current_ma=(?P<current>-?\d+) soc_pct=(?P<soc>\d+) "
    rf"status=0x(?P<status>[0-9a-fA-F]+)"
)
POLL_ERR_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_diag: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"stage=poll_snapshot err=(?P<err>[a-zA-Z0-9_]+)"
)
POLL_RETRY_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_diag: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"stage=poll_snapshot_retry_(?P<result>ok|fail) first_err=(?P<first>[a-zA-Z0-9_]+)"
    rf"(?: retry_err=(?P<retry>[a-zA-Z0-9_]+))?"
)
ROM_DETECTED_RE = re.compile(
    r"stage=(?:rom_mode_detected(?:_after_enter|_post_flash)?|wake_window_rom_entered|wake_window_rom_signature)\b"
)
ROM_FLASH_BEGIN_RE = re.compile(r"stage=(probe_rom_flash_begin|rom_flash_start)")
ROM_FLASH_IMAGE_DONE_RE = re.compile(r"stage=rom_flash_done\b")
ROM_FLASH_DONE_RE = re.compile(r"stage=probe_rom_flash_done")
ADDR16_RE = re.compile(r"addr=0x16\b")


@dataclass
class Sample:
    addr: int
    temp: int
    voltage: int
    current: int
    soc: int
    status: int

    @property
    def valid(self) -> bool:
        if not (-400 <= self.temp <= 1250):
            return False
        if not (2500 <= self.voltage <= 20000):
            return False
        if not (0 <= self.soc <= 100):
            return False
        if (
            self.voltage == SUSPICIOUS_VOLTAGE_MV
            and self.current == SUSPICIOUS_CURRENT_MA
            and self.status == SUSPICIOUS_STATUS
        ):
            return False
        return True


def parse_entry_ts(entry: dict) -> Optional[float]:
    ts = entry.get("ts")
    if not isinstance(ts, str):
        return None
    normalized = ts[:-1] + "+00:00" if ts.endswith("Z") else ts
    try:
        return datetime.fromisoformat(normalized).astimezone(timezone.utc).timestamp()
    except ValueError:
        return None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["canonical", "dual-diag"], required=True)
    parser.add_argument("--duration-sec", type=int, required=True)
    parser.add_argument("--monitor-file", required=True)
    parser.add_argument("--report-out", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    monitor_file = Path(args.monitor_file).expanduser().resolve()
    report_out = Path(args.report_out).expanduser().resolve()
    report_out.mkdir(parents=True, exist_ok=True)

    poll_errors: Counter[str] = Counter()
    samples_total = 0
    valid_samples = 0
    current_streak = 0
    max_streak = 0
    rom_detected = False
    rom_flash_attempted = False
    rom_flash_image_done = False
    rom_flash_done = False
    canonical_touched_0x16 = False
    last_sample_ts: Optional[float] = None
    allowed_addrs = {0x0B} if args.mode == "canonical" else {0x0B, 0x16}

    if not monitor_file.is_file():
        print(f"monitor file not found: {monitor_file}", file=sys.stderr)
        return 3

    try:
        with monitor_file.open("r", encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    continue

                if entry.get("src") == "meta" and entry.get("event") == SESSION_BOUNDARY_EVENT:
                    # Any monitor boundary is a streak discontinuity (even if timestamps look
                    # contiguous in the merged log).
                    current_streak = 0
                    last_sample_ts = None
                    continue

                text = entry.get("text", "")
                if not isinstance(text, str):
                    continue

                if ADDR16_RE.search(text):
                    canonical_touched_0x16 = True

                if ROM_DETECTED_RE.search(text):
                    rom_detected = True
                if ROM_FLASH_BEGIN_RE.search(text):
                    rom_flash_attempted = True
                if ROM_FLASH_IMAGE_DONE_RE.search(text):
                    rom_flash_image_done = True
                if ROM_FLASH_DONE_RE.search(text):
                    rom_flash_done = True

                err_match = POLL_ERR_RE.search(text)
                if err_match:
                    poll_errors[err_match.group("err")] += 1
                    current_streak = 0

                retry_match = POLL_RETRY_RE.search(text)
                if retry_match:
                    # `retry_ok` has no terminal poll error line, so keep first_err for visibility.
                    # `retry_fail` is represented by a later `poll_snapshot err=...` line.
                    if retry_match.group("result") == "ok":
                        poll_errors[retry_match.group("first")] += 1
                    else:
                        current_streak = 0

                sample_match = SAMPLE_RE.search(text)
                if not sample_match:
                    continue

                sample = Sample(
                    addr=int(sample_match.group("addr"), 16),
                    temp=int(sample_match.group("temp")),
                    voltage=int(sample_match.group("voltage")),
                    current=int(sample_match.group("current")),
                    soc=int(sample_match.group("soc")),
                    status=int(sample_match.group("status"), 16),
                )

                if sample.addr not in allowed_addrs:
                    continue

                entry_ts = parse_entry_ts(entry)
                if (
                    entry_ts is not None
                    and last_sample_ts is not None
                    and entry_ts - last_sample_ts > MAX_SAMPLE_STREAK_GAP_SEC
                ):
                    current_streak = 0
                if entry_ts is not None:
                    last_sample_ts = entry_ts

                samples_total += 1
                if sample.valid:
                    valid_samples += 1
                    current_streak += 1
                    max_streak = max(max_streak, current_streak)
                else:
                    current_streak = 0
    except OSError as exc:
        print(f"failed to read monitor file: {exc}", file=sys.stderr)
        return 4

    reasons: list[str] = []
    if samples_total == 0:
        reasons.append("no_bms_samples")
    if max_streak < 10:
        reasons.append("max_valid_streak_lt_10")
    if args.mode == "canonical" and canonical_touched_0x16:
        reasons.append("canonical_mode_touched_0x16")

    verdict_pass = len(reasons) == 0
    verdict_reason = "ok" if verdict_pass else ";".join(reasons)

    summary = {
        "mode": args.mode,
        "duration_sec": args.duration_sec,
        "samples_total": samples_total,
        "valid_samples": valid_samples,
        "max_valid_streak": max_streak,
        "poll_errors": dict(sorted(poll_errors.items())),
        "rom_events": {
            "detected": rom_detected,
            "flash_attempted": rom_flash_attempted,
            "flash_image_done": rom_flash_image_done,
            "flash_done": rom_flash_done,
        },
        "verdict": {
            "pass": verdict_pass,
            "reason": verdict_reason,
        },
    }

    (report_out / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=True, indent=2) + "\n",
        encoding="utf-8",
    )

    md = [
        "# BQ40 Communication Summary",
        "",
        f"- mode: `{summary['mode']}`",
        f"- duration_sec: `{summary['duration_sec']}`",
        f"- samples_total: `{summary['samples_total']}`",
        f"- valid_samples: `{summary['valid_samples']}`",
        f"- max_valid_streak: `{summary['max_valid_streak']}`",
        f"- verdict: `{'PASS' if summary['verdict']['pass'] else 'FAIL'}` ({summary['verdict']['reason']})",
        "",
        "## Poll Errors",
        "",
    ]

    if summary["poll_errors"]:
        for key, value in summary["poll_errors"].items():
            md.append(f"- {key}: {value}")
    else:
        md.append("- none")

    md.extend(
        [
            "",
            "## ROM Events",
            "",
            f"- detected: `{summary['rom_events']['detected']}`",
            f"- flash_attempted: `{summary['rom_events']['flash_attempted']}`",
            f"- flash_image_done: `{summary['rom_events']['flash_image_done']}`",
            f"- flash_done: `{summary['rom_events']['flash_done']}`",
            "",
            f"source_log: `{monitor_file}`",
        ]
    )

    (report_out / "summary.md").write_text("\n".join(md) + "\n", encoding="utf-8")

    print(str((report_out / "summary.json")))
    print(str((report_out / "summary.md")))
    return 0 if verdict_pass else 20


if __name__ == "__main__":
    raise SystemExit(main())
