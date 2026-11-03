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


def session_boundary_log(reset_on_attach: bool = True) -> str:
    return json.dumps(
        {
            "src": "meta",
            "event": "monitor_session_start",
            "reset_on_attach": reset_on_attach,
        }
    )


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
            "bms_charge_profile: profile=bq40_charge_temperature stage=armed "
            "wait_for_identity=true charger_enabled=false"
        ),
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            "stage=charge_disabled ce_high=true charger_enabled=false"
        ),
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            "stage=provenance probe_mode=strict rom_image=r2 mode=canonical"
        ),
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            "stage=identity_verified manufacturer=texas device=bq40"
        ),
    ]
    for stage in (
        "security_unseal_0414",
        "security_unseal_3672",
        "security_full_access_ffff_1",
        "security_full_access_ffff_2",
    ):
        lines.append(
            log(
                "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
                f"stage={stage} write=ok"
            )
        )
    lines.extend(
        [
            log(
                "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
                "stage=security_after op=0x00000100 sec=full_access"
            ),
            log(
                "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
                f"stage=snapshot_before_hex {before}"
            ),
            log(
                "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
                f"stage=final_after_hex {after}"
            ),
        ]
    )
    for name, (address, raw_lo, raw_hi, decoded) in FIELD_VALUES.items():
        lines.append(
            log(
                "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
                f"stage=field_verify field={name} df_addr=0x{address:x} "
                "declared_len=32 payload_len=32 "
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
    lines.append(
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            "stage=halt reason=complete charger_enabled=false"
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
    charge_disabled_index = next(
        index for index, line in enumerate(lines) if "stage=charge_disabled" in line
    )
    provenance_index = next(
        index for index, line in enumerate(lines) if "stage=provenance" in line
    )
    lines[charge_disabled_index], lines[provenance_index] = (
        lines[provenance_index],
        lines[charge_disabled_index],
    )
    return lines


def duplicate_field_evidence_log() -> list[str]:
    lines = successful_log()
    field_line = next(line for line in lines if "field=t1" in line)
    lines.insert(-1, field_line)
    return lines


def duplicate_timestamp_field_evidence_log() -> list[str]:
    timestamp = "2026-11-02T00:00:00.000Z"
    lines = []
    for line in successful_log():
        entry = json.loads(line)
        entry["ts"] = timestamp
        lines.append(json.dumps(entry))
    field_line = next(line for line in lines if "field=t1" in line)
    lines.insert(-1, field_line)
    return lines


def mirrored_profile_evidence_log() -> list[str]:
    lines = []
    for index, line in enumerate(successful_log()):
        entry = json.loads(line)
        timestamp = f"2026-11-02T00:00:00.{index:03d}Z"
        entry["ts"] = timestamp
        entry["src"] = "trace"
        lines.append(json.dumps(entry))
        mirror = dict(entry)
        mirror["src"] = "log"
        mirror["ts"] = timestamp
        lines.append(json.dumps(mirror))
    return lines


def reordered_field_evidence_log() -> list[str]:
    lines = successful_log()
    t1_index = next(index for index, line in enumerate(lines) if "field=t1" in line)
    t2_index = next(index for index, line in enumerate(lines) if "field=t2" in line)
    lines[t1_index], lines[t2_index] = lines[t2_index], lines[t1_index]
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


def malformed_stage_log() -> list[str]:
    lines = successful_log()
    lines.insert(
        -1,
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            "stage=-unexpected"
        ),
    )
    return lines


def malformed_known_stage_log() -> list[str]:
    lines = successful_log()
    lines.insert(
        -1,
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            "stage=done-typo fields=7"
        ),
    )
    return lines


def malformed_profile_log() -> list[str]:
    lines = successful_log()
    lines.insert(
        -1,
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature-typo "
            "stage=done fields=7"
        ),
    )
    return lines


def malformed_json_log() -> list[str]:
    lines = successful_log()
    lines.insert(
        -1,
        '{"text":"bms_df_apply: addr=0xb profile=bq40_charge_temperature '
        'stage=done"',
    )
    return lines


def malformed_text_log() -> list[str]:
    lines = successful_log()
    lines.insert(
        -1,
        json.dumps(
            {
                "text": {
                    "bms_df_apply": "addr=0xb profile=bq40_charge_temperature stage=done"
                }
            }
        ),
    )
    return lines


def missing_stage_log() -> list[str]:
    lines = successful_log()
    lines.insert(
        -1,
        log("bms_df_apply: addr=0xb profile=bq40_charge_temperature"),
    )
    return lines


def trailing_garbage_log() -> list[str]:
    lines = successful_log()
    lines.insert(
        -1,
        log(
            "bms_df_apply: addr=0xb profile=bq40_charge_temperature "
            "stage=done fields=7 trailing=garbage"
        ),
    )
    return lines


def missing_gate_evidence_log() -> list[str]:
    return [
        line
        for line in successful_log()
        if "stage=charge_disabled" not in line
        and "stage=identity_verified" not in line
        and "stage=security_" not in line
    ]


def mixed_session_profile_log() -> list[str]:
    return [session_boundary_log()] + successful_log() + [session_boundary_log()]


def repair_event_profile_log() -> list[str]:
    return successful_log() + [log("bms_df_repair: addr=0xb field=afe_prot_ctrl")]


def post_halt_charge_enable_log() -> list[str]:
    return successful_log() + [
        log(
            "charger: enabled=true charge_allowed=true allow_charge=true "
            "ce_low=true ctrl0=0x20"
        )
    ]


def ordinary_sample_log() -> list[str]:
    return [
        log(
            "bms: addr=0xb temp_c_x10=250 voltage_mv=14600 "
            "current_ma=500 soc_pct=50 status=0x0"
        )
        for _ in range(10)
    ]


def run_parser(
    log_lines: list[str],
    root: Path,
    action: str = "apply-df",
    mode: str = "canonical",
    probe_mode: str | None = None,
    rom_image: str | None = None,
    force_min_charge: str | None = None,
    include_provenance: bool = True,
) -> subprocess.CompletedProcess[str]:
    monitor = root / "monitor.jsonl"
    report = root / "report"
    monitor.write_text("\n".join(log_lines) + "\n", encoding="utf-8")
    args = [
        "python3",
        str(SCRIPT),
        "--action",
        action,
        "--mode",
        mode,
        "--duration-sec",
        "1",
        "--monitor-file",
        str(monitor),
        "--repair-profile",
        "bq40-charge-temperature",
        "--report-out",
        str(report),
    ]
    if include_provenance and action == "apply-df":
        probe_mode = "strict" if probe_mode is None else probe_mode
        rom_image = "r2" if rom_image is None else rom_image
    if probe_mode is not None:
        args.extend(["--probe-mode", probe_mode])
    if rom_image is not None:
        args.extend(["--rom-image", rom_image])
    if force_min_charge is not None:
        args.extend(["--force-min-charge", force_min_charge])
    return subprocess.run(
        args,
        check=False,
        capture_output=True,
        text=True,
    )


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="mains-aegis-report-parser-") as temp:
        root = Path(temp)
        good = run_parser(
            successful_log(),
            root,
            probe_mode="strict",
            rom_image="r2",
        )
        assert good.returncode == 0, good.stderr + good.stdout
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert summary["verdict"]["pass"] is True
        assert summary["charge_temperature_apply"]["snapshot_before_hex"]
        assert summary["charge_temperature_apply"]["final_after_hex"]

        mirrored = run_parser(mirrored_profile_evidence_log(), root)
        assert mirrored.returncode == 0, mirrored.stderr + mirrored.stdout
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert summary["verdict"]["pass"] is True
        assert summary["charge_temperature_apply"]["field_order"] == list(FIELD_VALUES)

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

        bad = run_parser(mixed_session_profile_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_provenance_conflict" in summary["verdict"]["reason"]

        bad = run_parser(repair_event_profile_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_provenance_conflict" in summary["verdict"]["reason"]

        bad = run_parser(post_halt_charge_enable_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_post_halt_charge_enable" in summary["verdict"]["reason"]

        bad = run_parser(duplicate_field_evidence_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_evidence_sequence_invalid" in summary["verdict"]["reason"]

        bad = run_parser(duplicate_timestamp_field_evidence_log(), root)
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

        bad = run_parser(malformed_stage_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_unknown_stage" in summary["verdict"]["reason"]

        bad = run_parser(malformed_known_stage_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_unknown_stage" in summary["verdict"]["reason"]

        bad = run_parser(malformed_profile_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_unknown_stage" in summary["verdict"]["reason"]

        for malformed_log in (
            malformed_json_log(),
            malformed_text_log(),
            missing_stage_log(),
            trailing_garbage_log(),
        ):
            bad = run_parser(malformed_log, root)
            assert bad.returncode == 20
            summary = json.loads((root / "report" / "summary.json").read_text())
            assert (
                "charge_profile_evidence_sequence_invalid"
                in summary["verdict"]["reason"]
            )

        bad = run_parser(missing_gate_evidence_log(), root)
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_identity_evidence_missing" in summary["verdict"]["reason"]

        bad = run_parser(successful_log(), root, mode="dual-diag")
        assert bad.returncode == 24
        assert "requires canonical mode" in bad.stderr

        for kwargs, expected_text in (
            ({"probe_mode": "mac-only", "rom_image": "r2"}, "strict probe"),
            ({"probe_mode": "strict", "rom_image": "r3"}, "r2 ROM provenance"),
            ({"include_provenance": False}, "strict probe"),
        ):
            bad = run_parser(successful_log(), root, **kwargs)
            assert bad.returncode == 24
            assert expected_text in bad.stderr

        bad = run_parser(
            successful_log(),
            root,
            action="verify",
            probe_mode="strict",
            rom_image="r2",
        )
        assert bad.returncode == 24
        assert "does not accept live run provenance flags" in bad.stderr

        invalid_full_access_op = [
            line.replace("op=0x00000100", "op=0x00000000")
            for line in successful_log()
        ]
        bad = run_parser(
            invalid_full_access_op,
            root,
            probe_mode="strict",
            rom_image="r2",
        )
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_evidence_sequence_invalid" in summary["verdict"]["reason"]

        reordered_security = successful_log()
        reordered_security[3], reordered_security[5] = (
            reordered_security[5],
            reordered_security[3],
        )
        bad = run_parser(
            reordered_security,
            root,
            probe_mode="strict",
            rom_image="r2",
        )
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_evidence_sequence_invalid" in summary["verdict"]["reason"]

        for replacement in (
            ("probe_mode=strict", "probe_mode=mac-only"),
            ("rom_image=r2", "rom_image=r3"),
        ):
            bad = run_parser(
                [line.replace(*replacement) for line in successful_log()],
                root,
                probe_mode="strict",
                rom_image="r2",
            )
            assert bad.returncode == 20
            summary = json.loads((root / "report" / "summary.json").read_text())
            assert "charge_profile_provenance_missing" in summary["verdict"]["reason"]

        for rom_stage in (
            "rom_mode_detected",
            "rom_mode_exit_failed",
            "wake_window_rom",
            "wake_touch_temp",
            "addr_scan_hit",
            "emshut_shutdown_exit_window",
            "probe_rom_flash_skipped",
            "rom_post_flash_resume_bad_temp",
        ):
            bad = run_parser(
                successful_log() + [log(f"bms: stage={rom_stage}")],
                root,
                probe_mode="strict",
                rom_image="r2",
            )
            assert bad.returncode == 20
            summary = json.loads((root / "report" / "summary.json").read_text())
            assert "charge_profile_provenance_conflict" in summary["verdict"]["reason"]

        for invalid_action in ("diagnose", "recover"):
            bad = run_parser(successful_log(), root, action=invalid_action)
            assert bad.returncode == 24
            assert "requires apply-df or verify action" in bad.stderr

        bad = run_parser(ordinary_sample_log(), root, action="verify")
        assert bad.returncode == 20
        summary = json.loads((root / "report" / "summary.json").read_text())
        assert "charge_profile_apply_not_done" in summary["verdict"]["reason"]

    print("report parser contract: ok")


if __name__ == "__main__":
    main()
