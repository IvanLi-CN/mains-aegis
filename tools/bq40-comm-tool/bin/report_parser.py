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

CHARGE_PROFILE_PREFIX_RE = re.compile(rf"^{LOG_LEVEL_PREFIX}bms_df_apply:")

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
CHARGE_PROFILE_APPLY_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=(?P<profile>[a-zA-Z0-9_]+) stage=(?P<stage>[a-zA-Z0-9_]+)"
    rf"(?:\s.*)?$"
)
CHARGE_PROFILE_APPLY_ANY_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=(?P<profile>[^\s]+) stage=(?P<stage>[^\s]+)(?:\s.*)?$"
)


def charge_profile_evidence_shape_is_valid(text: str) -> bool:
    """Require every apparent profile event to use the complete key=value grammar."""
    prefix = CHARGE_PROFILE_PREFIX_RE.match(text)
    if prefix is None:
        return False
    tokens = text[prefix.end() :].strip().split()
    if len(tokens) < 3:
        return False
    if re.fullmatch(r"addr=0x[0-9a-fA-F]+", tokens[0]) is None:
        return False
    if re.fullmatch(r"profile=\S+", tokens[1]) is None:
        return False
    if re.fullmatch(r"stage=\S+", tokens[2]) is None:
        return False
    stage = tokens[2].split("=", 1)[1]
    tail = tokens[3:]

    def matches(patterns: tuple[str, ...]) -> bool:
        return len(tail) == len(patterns) and all(
            re.fullmatch(pattern, token) is not None
            for pattern, token in zip(patterns, tail)
        )

    if stage in {"snapshot_before_hex", "final_after_hex"}:
        return len(tail) == 32 and all(
            re.fullmatch(rf"b{index}=0x[0-9a-fA-F]{{1,2}}", token) is not None
            for index, token in enumerate(tail)
        )
    if stage == "field_verify":
        return matches(
            (
                r"field=[a-zA-Z0-9_]+",
                r"df_addr=0x[0-9a-fA-F]+",
                r"declared_len=\d+",
                r"payload_len=\d+",
                r"raw_lo=0x[0-9a-fA-F]{1,2}",
                r"raw_hi=0x[0-9a-fA-F]{1,2}",
                r"decoded=0x[0-9a-fA-F]{1,4}",
            )
        )
    if stage == "done":
        return matches(
            (
                r"fields=\d+",
                r"snapshot_addr=0x[0-9a-fA-F]+",
                r"snapshot_len=\d+",
                r"final_addr=0x[0-9a-fA-F]+",
                r"final_len=\d+",
            )
        )
    if stage in {
        "security_unseal_0414",
        "security_unseal_3672",
        "security_full_access_ffff_1",
        "security_full_access_ffff_2",
    }:
        return matches((r"write=ok",))
    if stage == "charge_disabled":
        return matches((r"ce_high=true", r"charger_enabled=false"))
    if stage == "provenance":
        return matches((r"probe_mode=strict", r"rom_image=r2", r"mode=canonical"))
    if stage == "identity_verified":
        return matches((r"manufacturer=texas", r"device=bq40"))
    if stage == "security_after":
        return matches((r"op=0x[0-9a-fA-F]+", r"sec=\S+"))
    if stage in {
        "identity_mfg_error",
        "identity_device_error",
        "security_error",
        "snapshot_transport",
        "snapshot_address",
        "snapshot_length",
        "echo_address",
        "echo_length",
        "echo_raw",
        "echo_decoded",
        "final_transport",
        "final_address",
        "final_length",
        "final_raw",
        "final_decoded",
    }:
        return matches((r"err=\S+",))
    if stage == "write_transport":
        return matches((r"via=direct", r"df_addr=0x[0-9a-fA-F]+", r"err=\S+"))
    if stage == "identity_error":
        return matches((r"reason=\S+",))
    if stage == "error":
        return matches((r"error=\S+",))
    if stage == "halt":
        return matches((r"reason=(?:complete|profile_failed)", r"charger_enabled=false"))

    # Unknown stages are rejected semantically below. Keep their shape check
    # conservative so a bare stage can still produce the explicit unknown-stage
    # reason, while arbitrary trailing text cannot be mistaken for evidence.
    return all(re.fullmatch(r"[a-zA-Z0-9_]+=[^\s]+", token) for token in tail)


CHARGE_PROFILE_SNAPSHOT_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=bq40_charge_(?:temperature|termination) stage=(?P<stage>snapshot_before_hex|final_after_hex) "
    + " ".join(
        rf"b{index}=0x(?P<b{index}>[0-9a-fA-F]{{1,2}})" for index in range(32)
    )
    + r"$"
)
CHARGE_PROFILE_DONE_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=bq40_charge_(?:temperature|termination) stage=done fields=(?P<fields>\d+) "
    rf"snapshot_addr=0x(?P<snapshot_addr>[0-9a-fA-F]+) snapshot_len=(?P<snapshot_len>\d+) "
    rf"final_addr=0x(?P<final_addr>[0-9a-fA-F]+) final_len=(?P<final_len>\d+)$"
)
CHARGE_PROFILE_FIELD_VERIFY_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=bq40_charge_(?:temperature|termination) stage=field_verify field=(?P<field>[a-zA-Z0-9_]+) "
    rf"df_addr=0x(?P<df_addr>[0-9a-fA-F]+) "
    rf"declared_len=(?P<declared_len>\d+) payload_len=(?P<payload_len>\d+) "
    rf"raw_lo=0x(?P<raw_lo>[0-9a-fA-F]{{1,2}}) "
    rf"raw_hi=0x(?P<raw_hi>[0-9a-fA-F]{{1,2}}) decoded=0x(?P<decoded>[0-9a-fA-F]{{1,4}})$"
)
CHARGE_PROFILE_ERROR_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=(?P<profile>[a-zA-Z0-9_]+) stage=error error=(?P<error>[a-zA-Z0-9_]+)$"
)
CHARGE_PROFILE_CE_DISABLED_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=bq40_charge_(?:temperature|termination) stage=charge_disabled "
    rf"ce_high=(?P<ce_high>true|false) charger_enabled=(?P<charger_enabled>true|false)$"
)
CHARGE_PROFILE_POST_HALT_CHARGE_ENABLE_RE = re.compile(
    r"(?:\bcharger:\s+enabled=true\b|\bcharger_enabled=true\b|\bce_low=true\b|\bEN_CHG=1\b)",
    re.IGNORECASE,
)
CHARGE_PROFILE_IDENTITY_VERIFIED_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=bq40_charge_(?:temperature|termination) stage=identity_verified "
    rf"manufacturer=(?P<manufacturer>[a-zA-Z0-9_]+) device=(?P<device>[a-zA-Z0-9_]+)$"
)
CHARGE_PROFILE_PROVENANCE_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=bq40_charge_(?:temperature|termination) stage=provenance "
    rf"probe_mode=(?P<probe_mode>strict|mac-only) "
    rf"rom_image=(?P<rom_image>r2|r3|r5) "
    rf"mode=(?P<mode>canonical|dual-diag)$"
)
CHARGE_PROFILE_SECURITY_AFTER_RE = re.compile(
    rf"{LOG_LEVEL_PREFIX}bms_df_apply: addr=0x(?P<addr>[0-9a-fA-F]+) "
    rf"profile=bq40_charge_(?:temperature|termination) stage=security_after "
    rf"op=0x(?P<op>[0-9a-fA-F]+) sec=(?P<sec>[a-zA-Z0-9_]+)$"
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
# The explicit ROM event flags above feed the human-readable summary.  The
# profile verdict uses this broader boundary because ROM/repair paths have
# several terminal and failure stages; accepting an unlisted one would let a
# mixed recovery/profile log pass offline verification.
ROM_OR_RECOVERY_EVENT_RE = re.compile(
    r"(?:\bstage=[^\s]*(?:rom|wake|post_flash|recovery|recover|flash|mode_exit|emshut|addr_scan)[^\s]*"
    r"|\brom=[^\s]+|\brecover=[^\s]+|\bbms_df_(?:repair|prepare):)",
    re.IGNORECASE,
)
ADDR16_RE = re.compile(r"addr=0x16\b")

CHARGE_PROFILE_EXPECTED_FIELDS = {
    "t1": (0x4A0B, (0xAC, 0x0A), 0x0AAC),
    "t2": (0x4A0D, (0x42, 0x0B), 0x0B42),
    "t5": (0x4A0F, (0x74, 0x0B), 0x0B74),
    "t6": (0x4A11, (0x0A, 0x0C), 0x0C0A),
    "t3": (0x4A13, (0x3C, 0x0C), 0x0C3C),
    "t4": (0x4A15, (0x6E, 0x0C), 0x0C6E),
    "hysteresis": (0x4A17, (0x14, 0x00), 0x0014),
}
CHARGE_PROFILE_KNOWN_STAGES = {
    "snapshot_before_hex",
    "final_after_hex",
    "field_verify",
    "done",
    "security_unseal_0414",
    "security_unseal_3672",
    "security_full_access_ffff_1",
    "security_full_access_ffff_2",
    "security_after",
    "charge_disabled",
    "provenance",
    "identity_verified",
    "security_error",
    "identity_mfg_error",
    "identity_device_error",
    "identity_error",
    "snapshot_transport",
    "snapshot_address",
    "snapshot_length",
    "write_transport",
    "echo_address",
    "echo_length",
    "echo_raw",
    "echo_decoded",
    "final_transport",
    "final_address",
    "final_length",
    "final_raw",
    "final_decoded",
    "error",
    "halt",
}
CHARGE_PROFILE_SECURITY_STAGE_ORDER = (
    "security_unseal_0414",
    "security_unseal_3672",
    "security_full_access_ffff_1",
    "security_full_access_ffff_2",
)


def charge_profile_address_allowed(mode: str, address: int) -> bool:
    return mode == "canonical" and address == 0x0B


def charge_profile_final_vectors_match(payload_hex: Optional[str]) -> bool:
    if payload_hex is None or len(payload_hex) != 64:
        return False
    payload = bytes.fromhex(payload_hex)
    for address, raw, _decoded in CHARGE_PROFILE_EXPECTED_FIELDS.values():
        offset = address - PROFILE_BLOCK_ADDRESS
        if payload[offset : offset + 2] != bytes(raw):
            return False
    return True


def charge_profile_non_allowlisted_bytes_unchanged(
    before_hex: Optional[str], after_hex: Optional[str]
) -> bool:
    if (
        before_hex is None
        or after_hex is None
        or len(before_hex) != 64
        or len(after_hex) != 64
    ):
        return False
    before = bytes.fromhex(before_hex)
    after = bytes.fromhex(after_hex)
    allowlisted_offsets = {
        offset
        for address, _raw, _decoded in CHARGE_PROFILE_EXPECTED_FIELDS.values()
        for offset in (address - PROFILE_BLOCK_ADDRESS, address - PROFILE_BLOCK_ADDRESS + 1)
    }
    return all(
        before[offset] == after[offset]
        for offset in range(32)
        if offset not in allowlisted_offsets
    )


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


def is_mirrored_profile_evidence(
    seen_sources: set[str], source: object
) -> bool:
    """Identify the trace/log mirror of one defmt event without hiding real duplicates."""
    if source not in {"trace", "log"}:
        return False
    other_source = "log" if source == "trace" else "trace"
    return seen_sources == {other_source}


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
        choices=["none", "afe-default", "bq40-charge-temperature", "bq40-charge-termination", "asset-df-mainboard"],
        required=True,
    )
    parser.add_argument("--report-out", required=True)
    return parser.parse_args()


EXPECTED_PROFILE_NAME = "bq40_charge_temperature"
PROFILE_BLOCK_ADDRESS = 0x4A00

def main() -> int:
    global EXPECTED_PROFILE_NAME, PROFILE_BLOCK_ADDRESS, CHARGE_PROFILE_EXPECTED_FIELDS
    args = parse_args()
    if args.repair_profile == "bq40-charge-termination":
        EXPECTED_PROFILE_NAME = "bq40_charge_termination"
        PROFILE_BLOCK_ADDRESS = 0x4A60
        CHARGE_PROFILE_EXPECTED_FIELDS = {"charge_term_taper_current": (0x4A6D, (120, 0), 120)}

    if args.repair_profile in {"bq40-charge-temperature", "bq40-charge-termination"} and args.action not in {
        "apply-df",
        "verify",
    }:
        print(
            "bq40-charge-temperature profile verification requires apply-df or verify action",
            file=sys.stderr,
        )
        return 24

    if args.repair_profile in {"bq40-charge-temperature", "bq40-charge-termination"} and args.mode != "canonical":
        print(
            "bq40-charge-temperature profile verification requires canonical mode; "
            "dual-diag is diagnostic-only",
            file=sys.stderr,
        )
        return 24

    if args.repair_profile in {"bq40-charge-temperature", "bq40-charge-termination"}:
        if args.action == "apply-df" and (
            args.probe_mode != "strict" or args.rom_image != "r2"
        ):
            print(
                "bq40-charge-temperature apply-df verification requires strict probe "
                "and r2 ROM provenance",
                file=sys.stderr,
            )
            return 24
        if args.action == "verify" and any(
            value is not None
            for value in (args.force_min_charge, args.probe_mode, args.rom_image)
        ):
            print(
                "bq40-charge-temperature verify does not accept live run provenance flags",
                file=sys.stderr,
            )
            return 24

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
    charge_profile_provenance_conflict = False
    charge_profile_apply_attempted = False
    charge_profile_apply_done = False
    charge_profile_apply_writes = 0
    charge_profile_apply_errors: Counter[str] = Counter()
    charge_profile_readback_seen = False
    charge_profile_snapshot_before_hex: Optional[str] = None
    charge_profile_final_after_hex: Optional[str] = None
    charge_profile_addresses: set[int] = set()
    charge_profile_done_fields: Optional[int] = None
    charge_profile_done_snapshot_addr: Optional[int] = None
    charge_profile_done_snapshot_len: Optional[int] = None
    charge_profile_done_final_addr: Optional[int] = None
    charge_profile_done_final_len: Optional[int] = None
    charge_profile_verified_fields: set[str] = set()
    charge_profile_snapshot_before_count = 0
    charge_profile_final_after_count = 0
    charge_profile_done_count = 0
    charge_profile_field_verify_counts: Counter[str] = Counter()
    charge_profile_field_order: list[str] = []
    charge_profile_sequence_error = False
    charge_profile_unknown_stage = False
    charge_profile_ce_disabled = False
    charge_profile_provenance_verified = False
    charge_profile_provenance_count = 0
    charge_profile_identity_verified = False
    charge_profile_full_access_verified = False
    charge_profile_security_stages_seen: set[str] = set()
    charge_profile_security_stage_index = 0
    charge_profile_ce_disabled_count = 0
    charge_profile_identity_verified_count = 0
    charge_profile_security_after_count = 0
    charge_profile_halt_seen = False
    charge_profile_post_halt_charge_enabled = False
    canonical_touched_0x16 = False
    last_sample_ts: Optional[float] = None
    in_preexisting_segment = False
    parse_preexisting_segment = False
    allow_preexisting_parse = False
    seen_entries: set[tuple[int, str]] = set()
    seen_charge_profile_entries: dict[tuple[int, str], set[str]] = {}
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

    is_charge_profile_requested = args.repair_profile in {"bq40-charge-temperature", "bq40-charge-termination"}

    def mark_malformed_charge_profile_evidence(kind: str) -> None:
        nonlocal charge_profile_apply_attempted
        nonlocal charge_profile_sequence_error
        nonlocal charge_profile_unknown_stage
        charge_profile_apply_attempted = True
        charge_profile_sequence_error = True
        charge_profile_unknown_stage = True
        charge_profile_apply_errors[kind] += 1

    try:
        with monitor_file.open("r", encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    if is_charge_profile_requested and "bms_df_apply:" in line:
                        mark_malformed_charge_profile_evidence("malformed_json")
                    continue

                if not isinstance(entry, dict):
                    if is_charge_profile_requested and "bms_df_apply:" in line:
                        mark_malformed_charge_profile_evidence("malformed_entry")
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
                        if is_charge_profile_requested and charge_profile_apply_attempted:
                            # A profile verdict must never be assembled from evidence emitted by
                            # multiple monitor sessions. Ordinary telemetry may span re-attaches,
                            # but the non-atomic Data Flash audit stream must remain single-session.
                            charge_profile_provenance_conflict = True
                            charge_profile_sequence_error = True
                            charge_profile_apply_errors["session_boundary"] += 1
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
                    if is_charge_profile_requested and "text" in entry:
                        mark_malformed_charge_profile_evidence("malformed_text")
                    continue
                if (
                    is_charge_profile_requested
                    and charge_profile_halt_seen
                    and CHARGE_PROFILE_POST_HALT_CHARGE_ENABLE_RE.search(text)
                ):
                    charge_profile_post_halt_charge_enabled = True
                    charge_profile_sequence_error = True
                    charge_profile_apply_errors["post_halt_charge_enable"] += 1
                if is_charge_profile_requested and ROM_OR_RECOVERY_EVENT_RE.search(text):
                    charge_profile_provenance_conflict = True
                is_charge_profile_evidence = "bms_df_apply:" in text
                if (
                    is_charge_profile_requested
                    and is_charge_profile_evidence
                    and not charge_profile_evidence_shape_is_valid(text)
                ):
                    mark_malformed_charge_profile_evidence("malformed_evidence")
                key = dedupe_key(entry, text)
                if key is not None:
                    if is_charge_profile_evidence:
                        source = entry.get("src")
                        source_name = source if isinstance(source, str) else ""
                        profile_sources = seen_charge_profile_entries.setdefault(key, set())
                        if is_mirrored_profile_evidence(profile_sources, source_name):
                            profile_sources.add(source_name)
                            continue
                        profile_sources.add(source_name)
                    else:
                        # Ordinary monitor telemetry can be duplicated when a session tail is
                        # reattached.
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

                charge_profile_match = CHARGE_PROFILE_APPLY_RE.search(text)
                charge_profile_any_match = CHARGE_PROFILE_APPLY_ANY_RE.search(text)
                if charge_profile_any_match:
                    any_profile = charge_profile_any_match.group("profile")
                    any_stage = charge_profile_any_match.group("stage")
                    if any_profile != EXPECTED_PROFILE_NAME:
                        charge_profile_apply_attempted = True
                        charge_profile_addresses.add(
                            int(charge_profile_any_match.group("addr"), 16)
                        )
                        charge_profile_sequence_error = True
                        charge_profile_unknown_stage = True
                        charge_profile_apply_errors["unknown_profile"] += 1
                    elif (
                        charge_profile_match is None
                        or charge_profile_match.group("profile") != any_profile
                        or charge_profile_match.group("stage") != any_stage
                    ):
                        charge_profile_apply_attempted = True
                        charge_profile_addresses.add(
                            int(charge_profile_any_match.group("addr"), 16)
                        )
                        charge_profile_sequence_error = True
                        charge_profile_unknown_stage = True
                        charge_profile_apply_errors["malformed_evidence"] += 1
                if charge_profile_match and charge_profile_match.group("profile") == EXPECTED_PROFILE_NAME:
                    charge_profile_apply_attempted = True
                    charge_profile_addresses.add(int(charge_profile_match.group("addr"), 16))
                    stage = charge_profile_match.group("stage")
                    if stage == "charge_disabled":
                        charge_profile_ce_disabled_count += 1
                        gate_match = CHARGE_PROFILE_CE_DISABLED_RE.search(text)
                        if (
                            charge_profile_ce_disabled_count != 1
                            or gate_match is None
                            or gate_match.group("ce_high") != "true"
                            or gate_match.group("charger_enabled") != "false"
                        ):
                            charge_profile_sequence_error = True
                        else:
                            charge_profile_ce_disabled = True
                    elif stage == "provenance":
                        charge_profile_provenance_count += 1
                        provenance_match = CHARGE_PROFILE_PROVENANCE_RE.search(text)
                        if (
                            not charge_profile_ce_disabled
                            or charge_profile_provenance_count != 1
                            or provenance_match is None
                            or provenance_match.group("probe_mode") != "strict"
                            or provenance_match.group("rom_image") != "r2"
                            or provenance_match.group("mode") != "canonical"
                        ):
                            charge_profile_sequence_error = True
                        else:
                            charge_profile_provenance_verified = True
                    elif stage == "identity_verified":
                        charge_profile_identity_verified_count += 1
                        identity_match = CHARGE_PROFILE_IDENTITY_VERIFIED_RE.search(text)
                        if (
                            not charge_profile_ce_disabled
                            or not charge_profile_provenance_verified
                            or charge_profile_identity_verified_count != 1
                            or identity_match is None
                            or identity_match.group("manufacturer") != "texas"
                            or identity_match.group("device") != "bq40"
                        ):
                            charge_profile_sequence_error = True
                        else:
                            charge_profile_identity_verified = True
                    elif stage in {
                        "security_unseal_0414",
                        "security_unseal_3672",
                        "security_full_access_ffff_1",
                        "security_full_access_ffff_2",
                    }:
                        if (
                            not charge_profile_identity_verified
                            or stage in charge_profile_security_stages_seen
                            or charge_profile_security_stage_index
                            >= len(CHARGE_PROFILE_SECURITY_STAGE_ORDER)
                            or stage
                            != CHARGE_PROFILE_SECURITY_STAGE_ORDER[
                                charge_profile_security_stage_index
                            ]
                        ):
                            charge_profile_sequence_error = True
                        else:
                            charge_profile_security_stage_index += 1
                        charge_profile_security_stages_seen.add(stage)
                    elif stage == "security_after":
                        charge_profile_security_after_count += 1
                        security_match = CHARGE_PROFILE_SECURITY_AFTER_RE.search(text)
                        if (
                            not charge_profile_identity_verified
                            or charge_profile_security_after_count != 1
                            or len(charge_profile_security_stages_seen) != 4
                            or charge_profile_security_stage_index
                            != len(CHARGE_PROFILE_SECURITY_STAGE_ORDER)
                            or security_match is None
                            or security_match.group("sec") != "full_access"
                        ):
                            charge_profile_sequence_error = True
                        else:
                            op = int(security_match.group("op"), 16)
                            if ((op >> 8) & 0x3) != 0b01:
                                charge_profile_sequence_error = True
                                charge_profile_apply_errors["full_access_op"] += 1
                                continue
                            charge_profile_full_access_verified = True
                    elif stage == "snapshot_before_hex":
                        if (
                            charge_profile_snapshot_before_count != 0
                            or charge_profile_final_after_count != 0
                            or charge_profile_done_count != 0
                            or not charge_profile_full_access_verified
                        ):
                            charge_profile_sequence_error = True
                        charge_profile_snapshot_before_count += 1
                    elif stage == "final_after_hex":
                        if (
                            charge_profile_snapshot_before_count != 1
                            or charge_profile_final_after_count != 0
                            or charge_profile_done_count != 0
                            or not charge_profile_full_access_verified
                        ):
                            charge_profile_sequence_error = True
                        charge_profile_final_after_count += 1
                    elif stage == "field_verify":
                        if (
                            charge_profile_snapshot_before_count != 1
                            or charge_profile_final_after_count != 1
                            or charge_profile_done_count != 0
                            or not charge_profile_full_access_verified
                        ):
                            charge_profile_sequence_error = True
                    elif stage == "done":
                        if (
                            charge_profile_snapshot_before_count != 1
                            or charge_profile_final_after_count != 1
                            or charge_profile_done_count != 0
                        ):
                            charge_profile_sequence_error = True
                        charge_profile_done_count += 1
                    elif stage in {
                        "security_error",
                        "identity_mfg_error",
                        "identity_device_error",
                        "identity_error",
                        "snapshot_transport",
                        "snapshot_address",
                        "snapshot_length",
                        "write_transport",
                        "echo_address",
                        "echo_length",
                        "echo_raw",
                        "echo_decoded",
                        "final_transport",
                        "final_address",
                        "final_length",
                        "final_raw",
                        "final_decoded",
                        "error",
                    }:
                        charge_profile_apply_errors[stage] += 1
                    elif stage == "halt":
                        halt_match = re.search(
                            r"\bstage=halt reason=(?P<reason>complete|profile_failed) "
                            r"charger_enabled=false$",
                            text,
                        )
                        if halt_match is None or halt_match.group("reason") != "complete":
                            charge_profile_apply_errors[stage] += 1
                        else:
                            charge_profile_halt_seen = True
                    elif stage not in CHARGE_PROFILE_KNOWN_STAGES:
                        charge_profile_unknown_stage = True
                        charge_profile_sequence_error = True
                        charge_profile_apply_errors["unknown_stage"] += 1

                charge_profile_done_match = CHARGE_PROFILE_DONE_RE.search(text)
                if charge_profile_done_match:
                    charge_profile_apply_attempted = True
                    charge_profile_apply_done = True
                    charge_profile_addresses.add(
                        int(charge_profile_done_match.group("addr"), 16)
                    )
                    charge_profile_done_fields = int(charge_profile_done_match.group("fields"))
                    charge_profile_done_snapshot_addr = int(
                        charge_profile_done_match.group("snapshot_addr"), 16
                    )
                    charge_profile_done_snapshot_len = int(
                        charge_profile_done_match.group("snapshot_len")
                    )
                    charge_profile_done_final_addr = int(
                        charge_profile_done_match.group("final_addr"), 16
                    )
                    charge_profile_done_final_len = int(
                        charge_profile_done_match.group("final_len")
                    )
                    charge_profile_apply_writes = max(
                        charge_profile_apply_writes, charge_profile_done_fields
                    )

                charge_profile_field_match = CHARGE_PROFILE_FIELD_VERIFY_RE.search(text)
                if charge_profile_field_match:
                    charge_profile_apply_attempted = True
                    address = int(charge_profile_field_match.group("addr"), 16)
                    charge_profile_addresses.add(address)
                    field_name = charge_profile_field_match.group("field")
                    charge_profile_field_verify_counts[field_name] += 1
                    expected_field_order = tuple(CHARGE_PROFILE_EXPECTED_FIELDS)
                    if (
                        charge_profile_field_verify_counts[field_name] != 1
                        or len(charge_profile_field_order) >= len(expected_field_order)
                        or field_name != expected_field_order[len(charge_profile_field_order)]
                    ):
                        charge_profile_sequence_error = True
                    charge_profile_field_order.append(field_name)
                    expected = CHARGE_PROFILE_EXPECTED_FIELDS.get(field_name)
                    actual = (
                        int(charge_profile_field_match.group("df_addr"), 16),
                        int(charge_profile_field_match.group("declared_len")),
                        int(charge_profile_field_match.group("payload_len")),
                        (
                            int(charge_profile_field_match.group("raw_lo"), 16),
                            int(charge_profile_field_match.group("raw_hi"), 16),
                        ),
                        int(charge_profile_field_match.group("decoded"), 16),
                    )
                    if (
                        expected is None
                        or actual[0] != expected[0]
                        or actual[1:3] != (32, 32)
                        or actual[3:] != expected[1:]
                    ):
                        charge_profile_apply_errors["field_verify"] += 1
                    else:
                        charge_profile_verified_fields.add(field_name)

                charge_profile_snapshot_match = CHARGE_PROFILE_SNAPSHOT_RE.search(text)
                if charge_profile_snapshot_match:
                    charge_profile_apply_attempted = True
                    charge_profile_addresses.add(
                        int(charge_profile_snapshot_match.group("addr"), 16)
                    )
                    payload_hex = bytes(
                        int(charge_profile_snapshot_match.group(f"b{index}"), 16)
                        for index in range(32)
                    ).hex()
                    if charge_profile_snapshot_match.group("stage") == "snapshot_before_hex":
                        charge_profile_snapshot_before_hex = payload_hex
                    else:
                        charge_profile_final_after_hex = payload_hex
                        charge_profile_readback_seen = True

                charge_profile_error_match = CHARGE_PROFILE_ERROR_RE.search(text)
                if (
                    charge_profile_error_match
                    and charge_profile_error_match.group("profile") == EXPECTED_PROFILE_NAME
                ):
                    charge_profile_apply_attempted = True
                    charge_profile_apply_errors[charge_profile_error_match.group("error")] += 1

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
    if samples_total == 0 and not (
        is_charge_profile_requested and charge_profile_apply_done
    ):
        reasons.append("no_bms_samples")
    if max_streak < 10 and not is_charge_profile_requested:
        reasons.append("max_valid_streak_lt_10")
    if args.mode == "canonical" and canonical_touched_0x16:
        reasons.append("canonical_mode_touched_0x16")
    if is_charge_profile_requested:
        if not charge_profile_ce_disabled:
            reasons.append("charge_profile_ce_disabled_evidence_missing")
        if not charge_profile_provenance_verified:
            reasons.append("charge_profile_provenance_missing")
        if (
            charge_profile_provenance_conflict
            or rom_detected
            or rom_flash_attempted
            or rom_flash_image_done
            or rom_flash_done
            or rom_fw_seen
            or rom_fw_invalid_runtime
            or rom_runtime_status_unconfirmed
        ):
            reasons.append("charge_profile_provenance_conflict")
        if not charge_profile_identity_verified:
            reasons.append("charge_profile_identity_evidence_missing")
        if not charge_profile_full_access_verified:
            reasons.append("charge_profile_full_access_evidence_missing")
        if not charge_profile_apply_done:
            reasons.append("charge_profile_apply_not_done")
        if charge_profile_apply_writes != len(CHARGE_PROFILE_EXPECTED_FIELDS):
            reasons.append("charge_profile_field_count_not_7" if EXPECTED_PROFILE_NAME == "bq40_charge_temperature" else "charge_profile_field_count_not_1")
        if charge_profile_done_fields != len(CHARGE_PROFILE_EXPECTED_FIELDS):
            reasons.append("charge_profile_done_evidence_invalid")
        if charge_profile_done_snapshot_addr != PROFILE_BLOCK_ADDRESS:
            reasons.append("charge_profile_snapshot_address_missing")
        if charge_profile_done_snapshot_len != 32:
            reasons.append("charge_profile_snapshot_length_missing")
        if charge_profile_done_final_addr != PROFILE_BLOCK_ADDRESS:
            reasons.append("charge_profile_final_address_missing")
        if charge_profile_done_final_len != 32:
            reasons.append("charge_profile_final_length_missing")
        if charge_profile_apply_errors:
            reasons.append("charge_profile_apply_errors")
        if not charge_profile_readback_seen:
            reasons.append("charge_profile_readback_missing")
        if charge_profile_snapshot_before_hex is None:
            reasons.append("charge_profile_snapshot_missing")
        if not charge_profile_final_after_hex:
            reasons.append("charge_profile_final_snapshot_missing")
        if not charge_profile_final_vectors_match(charge_profile_final_after_hex):
            reasons.append("charge_profile_final_vectors_mismatch")
        if not charge_profile_non_allowlisted_bytes_unchanged(
            charge_profile_snapshot_before_hex, charge_profile_final_after_hex
        ):
            reasons.append("charge_profile_non_allowlisted_bytes_changed")
        if charge_profile_sequence_error:
            reasons.append("charge_profile_evidence_sequence_invalid")
        if charge_profile_unknown_stage:
            reasons.append("charge_profile_unknown_stage")
        if charge_profile_field_order != list(CHARGE_PROFILE_EXPECTED_FIELDS):
            reasons.append("charge_profile_field_order_invalid")
        if charge_profile_addresses and not all(
            charge_profile_address_allowed(args.mode, address)
            for address in charge_profile_addresses
        ):
            reasons.append("charge_profile_address_invalid")
        if len(charge_profile_addresses) != 1:
            reasons.append("charge_profile_address_inconsistent")
        if set(CHARGE_PROFILE_EXPECTED_FIELDS) != charge_profile_verified_fields:
            reasons.append("charge_profile_field_verification_missing")
        if charge_profile_post_halt_charge_enabled:
            reasons.append("charge_profile_post_halt_charge_enable")

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
        "charge_temperature_apply": {
            "attempted": charge_profile_apply_attempted,
            "done": charge_profile_apply_done,
            "ce_disabled": charge_profile_ce_disabled,
            "provenance_verified": charge_profile_provenance_verified,
            "provenance_conflict": "charge_profile_provenance_conflict" in reasons,
            "identity_verified": charge_profile_identity_verified,
            "full_access_verified": charge_profile_full_access_verified,
            "writes": charge_profile_apply_writes,
            "errors": dict(sorted(charge_profile_apply_errors.items())),
            "readback_seen": charge_profile_readback_seen,
            "snapshot_before_hex": charge_profile_snapshot_before_hex,
            "final_after_hex": charge_profile_final_after_hex,
            "snapshot_before_count": charge_profile_snapshot_before_count,
            "final_after_count": charge_profile_final_after_count,
            "done_count": charge_profile_done_count,
            "field_order": charge_profile_field_order,
            "evidence_sequence_error": charge_profile_sequence_error,
            "unknown_stage": charge_profile_unknown_stage,
            "addresses": [f"0x{address:02x}" for address in sorted(charge_profile_addresses)],
            "verified_fields": sorted(charge_profile_verified_fields),
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
        f"- charge_temperature_snapshot_before_hex: `{fmt_cfg_str(summary['charge_temperature_apply']['snapshot_before_hex'])}`",
        f"- charge_temperature_final_after_hex: `{fmt_cfg_str(summary['charge_temperature_apply']['final_after_hex'])}`",
        f"- charge_temperature_verified_fields: `{', '.join(summary['charge_temperature_apply']['verified_fields']) or 'none'}`",
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
            "## BQ40 Charge Temperature Apply",
            "",
            f"- attempted: `{summary['charge_temperature_apply']['attempted']}`",
            f"- done: `{summary['charge_temperature_apply']['done']}`",
            f"- writes: `{summary['charge_temperature_apply']['writes']}`",
            f"- readback_seen: `{summary['charge_temperature_apply']['readback_seen']}`",
        ]
    )

    if summary["charge_temperature_apply"]["errors"]:
        for key, value in summary["charge_temperature_apply"]["errors"].items():
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
