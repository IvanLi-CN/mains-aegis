#!/usr/bin/env python3
"""Contract tests for the fail-closed BQ40 charge-profile report parser."""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path


SCRIPT = Path(__file__).with_name("report_parser.py")
FIELD_VALUES = {
    "t1": (0x4A0B, 0xAC, 0x0A, 0x0AAC),
    "t2": (0x4A0D, 0x42, 0x0B, 0x0B42),
    "t5": (0x4A0F, 0x74, 0x0B, 0x0B74),
    "t6": (0x4A11, 0x0A, 0x0C, 0x0C0A),
    "t3": (0x4A13, 0x3C, 0x0C, 0x0C3C),
    "t4": (0x4A15, 0x6E, 0x0C, 0x0C6E),
    "hysteresis": (0x4A17, 0x14, 0x00, 0x0014),
}


def log(text: str) -> str:
    return json.dumps({"text": text})


def block_payload() -> bytearray:
    payload = bytearray([0xA5] * 32)
    for address, raw_lo, raw_hi, _decoded in FIELD_VALUES.values():
        offset = address - 0x4A00
        payload[offset : offset + 2] = bytes((raw_lo, raw_hi))
    return payload


def successful_log() -> list[str]:
    before = " ".join(f"b{index}=0xa5" for index in range(32))
    after_payload = block_payload()
    after = " ".join(f"b{index}=0x{value:02x}" for index, value in enumerate(after_payload))
    lines = [
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            f"stage=snapshot_before_hex {before}"
        ),
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            f"stage=final_after_hex {after}"
        ),
    ]
    for name, (address, raw_lo, raw_hi, decoded) in FIELD_VALUES.items():
        lines.append(
            log(
                "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
                f"stage=field_verify field={name} df_addr=0x{address:x} "
                f"raw_lo=0x{raw_lo:x} raw_hi=0x{raw_hi:x} decoded=0x{decoded:x}"
            )
        )
    lines.append(
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature stage=done "
            "fields=7 snapshot_addr=0x4a00 snapshot_len=32 "
            "final_addr=0x4a00 final_len=32"
        )
    )
    return lines


def non_allowlisted_byte_change_log() -> list[str]:
    lines = successful_log()
    for index, line in enumerate(lines):
        if "stage=final_after_hex" in line:
            lines[index] = line.replace("b0=0xa5", "b0=0xa6")
            break
    return lines


def reordered_evidence_log() -> list[str]:
    lines = successful_log()
    lines[0], lines[1] = lines[1], lines[0]
    return lines


def duplicate_field_evidence_log() -> list[str]:
    lines = successful_log()
    lines.insert(-1, lines[2])
    return lines


def reordered_field_evidence_log() -> list[str]:
    lines = successful_log()
    lines[2], lines[3] = lines[3], lines[2]
    return lines


def unknown_stage_log() -> list[str]:
    lines = successful_log()
    lines.insert(
        -1,
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            "stage=unexpected"
        ),
    )
    return lines


def ordinary_sample_log() -> list[str]:
    return [
        log(
            "bms: addr=0xb temp_c_x10=250 voltage_mv=14600 "
            "current_ma=500 soc_pct=50 status=0x0"
        )
        for _ in range(10)
    ]


def run_parser(
    log_lines: list[str], root: Path, action: str = "apply-df"
) -> subprocess.CompletedProcess[str]:
    monitor = root / "monitor.jsonl"
    report = root / "report"
    monitor.write_text("\n".join(log_lines) + "\n", encoding="utf-8")
    return subprocess.run(
        [
            "python3",
            str(SCRIPT),
            "--action",
            action,
            "--mode",
            "canonical",
            "--duration-sec",
            "1",
            "--monitor-file",
            str(monitor),
            "--repair-profile",
            "bq40-charge-temperature",
            "--report-out",
            str(report),
        ],
        check=False,
        capture_output=True,
        text=True,
    )


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="mains-aegis-report-parser-") as temp:
        root = Path(temp)
        good = run_parser(successful_log(), root)
        assert good.returncode == 0, good.stderr + good.stdout
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert summary["verdict"]["pass"] is True
        assert summary["charge_temperature_apply"]["snapshot_before_hex"]
        assert summary["charge_temperature_apply"]["final_after_hex"]

        incomplete = successful_log()[:1] + [successful_log()[-1]]
        bad = run_parser(incomplete, root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert summary["verdict"]["pass"] is False
        assert "charge_profile_final_snapshot_missing" in summary["verdict"]["reason"]

        wrong_address = [
            line.replace("addr=0xb", "addr=0x16") for line in successful_log()
        ]
        bad = run_parser(wrong_address, root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_address_invalid" in summary["verdict"]["reason"]

        bad = run_parser(non_allowlisted_byte_change_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert summary["verdict"]["pass"] is False
        assert (
            "charge_profile_non_allowlisted_bytes_changed"
            in summary["verdict"]["reason"]
        )

        bad = run_parser(reordered_evidence_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_evidence_sequence_invalid" in summary["verdict"]["reason"]

        bad = run_parser(duplicate_field_evidence_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_evidence_sequence_invalid" in summary["verdict"]["reason"]

        bad = run_parser(reordered_field_evidence_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_field_order_invalid" in summary["verdict"]["reason"]

        bad = run_parser(unknown_stage_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_unknown_stage" in summary["verdict"]["reason"]

        bad = run_parser(ordinary_sample_log(), root, action="verify")
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_apply_not_done" in summary["verdict"]["reason"]

    print("report parser contract: ok")


if __name__ == "__main__":
    main()
