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
MAX_SAMPLE_STREAK_GAP_SEC = 5.0
SESSION_BOUNDARY_EVENT = "monitor_session_start"
RECENT_EXISTING_STDOUT_EVENT = "recent_existing_stdout"
PREEXISTING_BEGIN_EVENT = "preexisting_segment_begin"
PREEXISTING_END_EVENT = "preexisting_segment_end"

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
LIVE_DF_APPLY_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=(?P<profile>[a-zA-Z0-9_]+) stage=(?P<stage>[a-zA-Z0-9_]+)"
    rf"(?: .*?(?:writes|fields)=(?P<count>\d+))?"
)
BMS_DF_CFG_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_cfg: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf".*?\bfet_options=0x(?P<fet_options>[0-9a-fA-F]+)\b"
    rf".*?\bprot_cfg=0x(?P<prot_cfg>[0-9a-fA-F]+)\b"
    rf".*?\bcuv_recovery=0x(?P<cuv_recovery>[0-9a-fA-F]+)\b"
)
ROM_DETECTED_RE = re.compile(
    r"stage=(?:rom_mode_detected(?:_after_enter|_post_flash)?|wake_window_rom_entered|wake_window_rom_signature)\b"
)
ROM_FLASH_BEGIN_RE = re.compile(r"stage=(probe_rom_flash_begin|rom_flash_start)")
ROM_FLASH_IMAGE_DONE_RE = re.compile(r"stage=rom_flash_done\b")
ROM_FLASH_DONE_RE = re.compile(r"stage=probe_rom_flash_done")
ROM_FW_SEEN_RE = re.compile(
    r"stage=(?:probe_rom_post_flash_fw_seen(?:_status_unconfirmed)?|rom_post_flash_resume_not_rom)\b"
)
ROM_FW_INVALID_RUNTIME_RE = re.compile(r"stage=probe_rom_post_flash_fw_invalid_runtime\b")
ROM_RUNTIME_STATUS_UNCONFIRMED_RE = re.compile(
    r"stage=(?:probe_rom_post_flash_runtime_status_unavailable|probe_rom_post_flash_fw_seen_status_unconfirmed|probe_rom_post_flash_fw_invalid_runtime_status_unconfirmed)\b"
)
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
    ts = entry.get("ts") or entry.get("timestamp")
    if not isinstance(ts, str):
        return None
    normalized = ts[:-1] + "+00:00" if ts.endswith("Z") else ts
    try:
        return datetime.fromisoformat(normalized).astimezone(timezone.utc).timestamp()
    except ValueError:
        return None


def dedupe_key(entry: dict, text: str) -> Optional[tuple[int, str]]:
    entry_ts = parse_entry_ts(entry)
    if entry_ts is None:
        return None
    return (round(entry_ts * 1000), text)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--action",
        choices=["diagnose", "apply-df", "recover", "verify"],
        required=True,
    )
    parser.add_argument("--mode", choices=["canonical", "dual-diag"], required=True)
    parser.add_argument("--duration-sec", type=int, required=True)
    parser.add_argument("--monitor-file", required=True)
    # Provenance-only knobs: supplied by `run.sh` so `summary.json` can be traced back to the
    # exact live run configuration (especially when swapping ROM images).
    parser.add_argument("--force-min-charge", choices=["true", "false"])
    parser.add_argument("--probe-mode", choices=["strict", "mac-only"])
    parser.add_argument("--rom-image", choices=["r2", "r3", "r5"])
    parser.add_argument(
        "--repair-profile",
        choices=["none", "afe-default", "live-df-mainboard", "asset-df-mainboard"],
        required=True,
    )
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
    rom_fw_seen = False
    rom_fw_invalid_runtime = False
    rom_runtime_status_unconfirmed = False
    live_df_apply_attempted = False
    live_df_apply_done = False
    live_df_apply_applied = False
    live_df_apply_writes = 0
    live_df_apply_errors: Counter[str] = Counter()
    live_df_readback_seen = False
    live_df_readback_fet_options: Optional[int] = None
    live_df_readback_prot_cfg: Optional[int] = None
    live_df_readback_cuv_recovery: Optional[int] = None
    canonical_touched_0x16 = False
    last_sample_ts: Optional[float] = None
    in_preexisting_segment = False
    parse_preexisting_segment = False
    allow_preexisting_parse = False
    seen_entries: set[tuple[int, str]] = set()
    allowed_addrs = {0x0B} if args.mode == "canonical" else {0x0B, 0x16}

    run_config = {
        "action": args.action,
        "force_min_charge": None
        if args.force_min_charge is None
        else (args.force_min_charge == "true"),
        "probe_mode": args.probe_mode,
        "rom_image": args.rom_image,
        "repair_profile": args.repair_profile,
    }

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

                if entry.get("src") == "meta":
                    event = entry.get("event")
                    if event == RECENT_EXISTING_STDOUT_EVENT:
                        # `monitor.sh` may splice a small "pre-attach stdout" segment for the
                        # current run (e.g. flash finished and the MCU printed early logs before
                        # we attached). Allow the immediately following preexisting segment to
                        # participate in ROM/sample parsing.
                        allow_preexisting_parse = True
                        continue
                    if event == PREEXISTING_BEGIN_EVENT:
                        in_preexisting_segment = True
                        parse_preexisting_segment = allow_preexisting_parse
                        allow_preexisting_parse = False
                        continue
                    if event == PREEXISTING_END_EVENT:
                        in_preexisting_segment = False
                        parse_preexisting_segment = False
                        continue
                    if event == SESSION_BOUNDARY_EVENT:
                        # Only treat reset attaches (or older logs without the flag) as streak
                        # discontinuities. Re-attaches without a reset can still build a valid
                        # streak across the combined log.
                        allow_preexisting_parse = False
                        reset_on_attach = entry.get("reset_on_attach")
                        if reset_on_attach is None or reset_on_attach:
                            current_streak = 0
                            last_sample_ts = None
                        continue

                if in_preexisting_segment and not parse_preexisting_segment:
                    continue

                text = entry.get("text", "")
                if not isinstance(text, str):
                    continue
                key = dedupe_key(entry, text)
                if key is not None:
                    if key in seen_entries:
                        continue
                    seen_entries.add(key)

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
                if ROM_FW_SEEN_RE.search(text):
                    rom_fw_seen = True
                if ROM_FW_INVALID_RUNTIME_RE.search(text):
                    rom_fw_invalid_runtime = True
                if ROM_RUNTIME_STATUS_UNCONFIRMED_RE.search(text):
                    rom_runtime_status_unconfirmed = True

                live_df_match = LIVE_DF_APPLY_RE.search(text)
                if live_df_match and live_df_match.group("profile") == "live_df_mainboard":
                    live_df_apply_attempted = True
                    stage = live_df_match.group("stage")
                    count = live_df_match.group("count")
                    if stage == "field_written":
                        live_df_apply_applied = True
                        live_df_apply_writes += 1
                    elif stage == "done":
                        live_df_apply_done = True
                        if count is not None:
                            live_df_apply_writes = max(live_df_apply_writes, int(count))
                        if live_df_apply_writes > 0:
                            live_df_apply_applied = True
                    elif stage in {
                        "read_err",
                        "write_err",
                        "verify_err",
                        "verify_mismatch",
                        "field_failed",
                        "reset_err",
                    }:
                        live_df_apply_errors[stage] += 1

                bms_df_cfg_match = BMS_DF_CFG_RE.search(text)
                if bms_df_cfg_match and int(bms_df_cfg_match.group("addr"), 16) in allowed_addrs:
                    live_df_readback_seen = True
                    live_df_readback_fet_options = int(
                        bms_df_cfg_match.group("fet_options"), 16
                    )
                    live_df_readback_prot_cfg = int(bms_df_cfg_match.group("prot_cfg"), 16)
                    live_df_readback_cuv_recovery = int(
                        bms_df_cfg_match.group("cuv_recovery"), 16
                    )

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

    is_live_df_mainboard_apply = (
        args.action == "apply-df" and args.repair_profile == "live-df-mainboard"
    )
    live_df_readback_matches = (
        live_df_readback_seen
        and live_df_readback_fet_options == 0x19
        and live_df_readback_prot_cfg == 0x00
        and live_df_readback_cuv_recovery == 0x09F6
    )

    reasons: list[str] = []
    if samples_total == 0 and not (is_live_df_mainboard_apply and live_df_readback_matches):
        reasons.append("no_bms_samples")
    if max_streak < 10 and not is_live_df_mainboard_apply:
        reasons.append("max_valid_streak_lt_10")
    if args.mode == "canonical" and canonical_touched_0x16:
        reasons.append("canonical_mode_touched_0x16")
    if is_live_df_mainboard_apply:
        if not live_df_apply_done:
            reasons.append("live_df_apply_not_done")
        if live_df_apply_errors:
            reasons.append("live_df_apply_errors")
        if not live_df_readback_seen:
            reasons.append("live_df_readback_missing")
        elif not live_df_readback_matches:
            reasons.append("live_df_readback_mismatch")

    verdict_pass = len(reasons) == 0
    verdict_reason = "ok" if verdict_pass else ";".join(reasons)

    summary = {
        "mode": args.mode,
        "duration_sec": args.duration_sec,
        "run_config": run_config,
        "samples_total": samples_total,
        "valid_samples": valid_samples,
        "max_valid_streak": max_streak,
        "poll_errors": dict(sorted(poll_errors.items())),
        "rom_events": {
            "detected": rom_detected,
            "flash_attempted": rom_flash_attempted,
            "flash_image_done": rom_flash_image_done,
            "flash_done": rom_flash_done,
            "fw_seen": rom_fw_seen,
            "runtime_invalid": rom_fw_invalid_runtime,
            "runtime_status_unconfirmed": rom_runtime_status_unconfirmed,
        },
        "live_df_apply": {
            "attempted": live_df_apply_attempted,
            "applied": live_df_apply_applied,
            "done": live_df_apply_done,
            "writes": live_df_apply_writes,
            "errors": dict(sorted(live_df_apply_errors.items())),
            "readback": {
                "seen": live_df_readback_seen,
                "fet_options": live_df_readback_fet_options,
                "prot_cfg": live_df_readback_prot_cfg,
                "cuv_recovery": live_df_readback_cuv_recovery,
                "matches_live_df_mainboard": live_df_readback_matches,
            },
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

    def fmt_cfg_bool(value: Optional[bool]) -> str:
        if value is None:
            return "unknown"
        return "true" if value else "false"

    def fmt_cfg_str(value: Optional[str]) -> str:
        return value if value is not None else "unknown"

    def fmt_hex(value: Optional[int]) -> str:
        if value is None:
            return "unknown"
        return f"0x{value:x}"

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
        "## Run Config",
        "",
        f"- action: `{fmt_cfg_str(run_config['action'])}`",
        f"- force_min_charge: `{fmt_cfg_bool(run_config['force_min_charge'])}`",
        f"- probe_mode: `{fmt_cfg_str(run_config['probe_mode'])}`",
        f"- rom_image: `{fmt_cfg_str(run_config['rom_image'])}`",
        f"- repair_profile: `{fmt_cfg_str(run_config['repair_profile'])}`",
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
            f"- fw_seen: `{summary['rom_events']['fw_seen']}`",
            f"- runtime_invalid: `{summary['rom_events']['runtime_invalid']}`",
            f"- runtime_status_unconfirmed: `{summary['rom_events']['runtime_status_unconfirmed']}`",
            "",
            "## Live DF Apply",
            "",
            f"- attempted: `{summary['live_df_apply']['attempted']}`",
            f"- applied: `{summary['live_df_apply']['applied']}`",
            f"- done: `{summary['live_df_apply']['done']}`",
            f"- writes: `{summary['live_df_apply']['writes']}`",
            f"- readback_seen: `{summary['live_df_apply']['readback']['seen']}`",
            f"- readback_fet_options: `{fmt_hex(summary['live_df_apply']['readback']['fet_options'])}`",
            f"- readback_prot_cfg: `{fmt_hex(summary['live_df_apply']['readback']['prot_cfg'])}`",
            f"- readback_cuv_recovery: `{fmt_hex(summary['live_df_apply']['readback']['cuv_recovery'])}`",
            f"- readback_matches_live_df_mainboard: `{summary['live_df_apply']['readback']['matches_live_df_mainboard']}`",
        ]
    )

    if summary["live_df_apply"]["errors"]:
        for key, value in summary["live_df_apply"]["errors"].items():
            md.append(f"- {key}: {value}")
    else:
        md.append("- errors: none")

    md.extend(
        [
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
