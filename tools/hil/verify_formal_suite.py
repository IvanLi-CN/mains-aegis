#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify a formal HIL suite summary against its referenced reports."
    )
    parser.add_argument(
        "--summary",
        required=True,
        help="Path to a suite summary JSON file.",
    )
    return parser.parse_args()


def load_json(path: Path) -> Any:
    return json.loads(path.read_text())


def load_samples(report_dir: Path, results: dict[str, Any]) -> list[dict[str, Any]]:
    samples = results.get("samples") or []
    if samples:
        return list(samples)
    timeseries_path = report_dir / "timeseries.jsonl"
    if not timeseries_path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for line in timeseries_path.read_text().splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def voltage_series_present(series: dict[str, Any], canonical: str, legacy: str) -> bool:
    if series.get(canonical) is True:
        return True
    return series.get(legacy) is True


def is_profile_window_phase(sample: dict[str, Any]) -> bool:
    phase = str(sample.get("phase") or "")
    if phase.startswith("transition_"):
        return False
    return phase in {"pre", "hold", "post"}


def expected_source_window(entry: dict[str, Any]) -> tuple[int | None, int | None]:
    profile = entry.get("output_profile")
    if profile == "12v":
        return 11000, 12500
    if profile == "19v":
        return 18000, 19500
    min_mv = entry.get("source_online_mv_min")
    max_mv = entry.get("source_online_mv_max")
    return (
        int(min_mv) if isinstance(min_mv, (int, float)) else None,
        int(max_mv) if isinstance(max_mv, (int, float)) else None,
    )


def resolve_report_dir(summary_root: Path, rel_or_abs: str) -> Path:
    report_dir = Path(rel_or_abs)
    if report_dir.is_absolute():
        return report_dir
    return (summary_root / report_dir).resolve()


def main() -> int:
    args = parse_args()
    summary_path = Path(args.summary)
    suite = load_json(summary_path)
    root = summary_path.parent

    expected_transport = suite.get("transport") or {}
    expected_power = suite.get("advanced_power")
    report_entries = suite.get("reports") or []

    failures: list[str] = []
    verified: list[dict[str, Any]] = []

    for entry in report_entries:
        rel_dir = entry["report_dir"]
        report_dir = resolve_report_dir(root, rel_dir)
        results_path = report_dir / "results.json"
        if not results_path.exists():
            failures.append(f"missing_results:{report_dir}")
            continue
        results = load_json(results_path)
        metadata = results.get("metadata") or {}
        overall = ((results.get("summary") or {}).get("all") or {})
        completeness = overall.get("completeness") or {}
        acceptance = overall.get("acceptance") or {}
        settings_power = ((results.get("settings_snapshot") or {}).get("advanced_power")) or {}
        if not settings_power:
            settings_power = dict(entry.get("advanced_power") or {})

        report_failures: list[str] = []
        if metadata.get("target_ma") != entry.get("target_ma"):
            report_failures.append("target_ma_mismatch")
        if bool(metadata.get("include_backup")) != bool(entry.get("include_backup")):
            report_failures.append("include_backup_mismatch")
        if metadata.get("output_profile") != entry.get("output_profile"):
            report_failures.append("output_profile_mismatch")
        if metadata.get("scene_type") != entry.get("scene_type"):
            report_failures.append("scene_type_mismatch")
        if metadata.get("source_voltage_mv") != entry.get("source_voltage_mv"):
            report_failures.append("source_voltage_mv_mismatch")
        if metadata.get("source_current_limit_ma") != entry.get("source_current_limit_ma"):
            report_failures.append("source_current_limit_ma_mismatch")
        if metadata.get("load_min_v_mv") != entry.get("load_min_v_mv"):
            report_failures.append("load_min_v_mv_mismatch")
        if metadata.get("max_i_ma_total") != entry.get("load_max_i_ma_total"):
            report_failures.append("load_max_i_ma_total_mismatch")
        if metadata.get("max_p_mw") != entry.get("load_max_p_mw"):
            report_failures.append("load_max_p_mw_mismatch")
        if not completeness.get("scene_complete"):
            report_failures.append("scene_not_complete")
        if acceptance.get("run_validity") != "valid_for_signoff":
            report_failures.append("run_validity_not_signoff")
        if acceptance.get("signoff_valid") is not True:
            report_failures.append("signoff_not_valid")
        failed_acceptance_checks = list(acceptance.get("failed_acceptance_checks") or [])
        if failed_acceptance_checks:
            report_failures.append("failed_acceptance_checks_not_empty")
        required_voltage_series = (
            acceptance.get("required_voltage_series")
            or completeness.get("required_voltage_series")
            or {}
        )
        if not voltage_series_present(required_voltage_series, "source_output_voltage", "source_v"):
            report_failures.append("missing_required_voltage_series:source_output_voltage")
        if not voltage_series_present(required_voltage_series, "ups_dcin_voltage", "ups_vin"):
            report_failures.append("missing_required_voltage_series:ups_dcin_voltage")
        if not voltage_series_present(required_voltage_series, "ups_output_voltage", "ups_vout"):
            report_failures.append("missing_required_voltage_series:ups_output_voltage")
        if not voltage_series_present(required_voltage_series, "load_actual_voltage", "load_v"):
            report_failures.append("missing_required_voltage_series:load_actual_voltage")
        if list(completeness.get("failures") or []) != list(entry.get("failures") or []):
            report_failures.append("failures_mismatch")
        if metadata.get("load_status_source") != expected_transport.get("load_status_source"):
            report_failures.append("load_status_source_mismatch")
        if metadata.get("load_usb_port") != expected_transport.get("load_usb_port"):
            report_failures.append("load_usb_port_mismatch")
        if metadata.get("load_ipc") != expected_transport.get("load_ipc"):
            report_failures.append("load_ipc_mismatch")
        if metadata.get("load_cli") != expected_transport.get("load_cli"):
            report_failures.append("load_cli_mismatch")
        if expected_power is not None and settings_power != expected_power:
            report_failures.append("advanced_power_mismatch")
        if dict(entry.get("advanced_power") or {}) != settings_power:
            report_failures.append("report_entry_advanced_power_mismatch")

        max_age = completeness.get("load_status_max_age_s")
        if isinstance(max_age, (int, float)) and max_age > 0.5:
            report_failures.append("load_status_too_stale")
        for key in (
            "source_status_max_age_s",
            "ups_status_max_age_s",
            "diag_snapshot_max_age_s",
        ):
            age = completeness.get(key)
            if isinstance(age, (int, float)) and age > 0.5:
                report_failures.append(f"{key}_too_stale")
        samples = load_samples(report_dir, results)
        if len(samples) < 2:
            report_failures.append("too_few_samples")
            effective_sample_rate_hz = None
            max_sample_gap_s = None
        else:
            span_s = float(samples[-1]["t_s"]) - float(samples[0]["t_s"])
            gaps = [
                float(curr["t_s"]) - float(prev["t_s"])
                for prev, curr in zip(samples, samples[1:])
            ]
            max_sample_gap_s = max(gaps)
            effective_sample_rate_hz = ((len(samples) - 1) / span_s) if span_s > 0 else None
            if effective_sample_rate_hz is None or effective_sample_rate_hz < 2.0:
                report_failures.append("sample_rate_below_2hz")
            if max_sample_gap_s > 0.5:
                report_failures.append("sample_gap_exceeds_0.5s")
        source_online = [
            sample.get("isolapurr_port_c_mv")
            for sample in samples
            if sample.get("port_c_enabled") is True
            and isinstance(sample.get("isolapurr_port_c_mv"), (int, float))
        ]
        if not source_online:
            report_failures.append("missing_online_source_voltage")
            source_online_min = None
            source_online_max = None
        else:
            source_online_min = min(source_online)
            source_online_max = max(source_online)
        ups_online = [
            sample.get("vin_vbus_mv")
            for sample in samples
            if sample.get("port_c_enabled") is True
            and is_profile_window_phase(sample)
            and isinstance(sample.get("vin_vbus_mv"), (int, float))
        ]
        if not ups_online:
            report_failures.append("missing_online_ups_dcin_voltage")
            ups_online_min = None
            ups_online_max = None
        else:
            ups_online_min = min(ups_online)
            ups_online_max = max(ups_online)
            expected_min_mv, expected_max_mv = expected_source_window(entry)
            if isinstance(expected_max_mv, int) and ups_online_max > expected_max_mv:
                report_failures.append("ups_dcin_voltage_above_expected_guard")
            if isinstance(expected_min_mv, int) and ups_online_min < expected_min_mv:
                report_failures.append("ups_dcin_voltage_below_expected_range")
        if entry.get("source_online_mv_min") is not None and entry.get("source_online_mv_min") != source_online_min:
            report_failures.append("source_online_mv_min_mismatch")
        if entry.get("source_online_mv_max") is not None and entry.get("source_online_mv_max") != source_online_max:
            report_failures.append("source_online_mv_max_mismatch")
        for surface in ("ups_status", "diag_snapshot", "isolapurr_power", "load_control", "load_status"):
            if surface in completeness and completeness.get(surface) is not True:
                report_failures.append(f"surface_missing:{surface}")

        verified.append(
            {
                "report_dir": rel_dir,
                "output_profile": entry.get("output_profile"),
                "scene_type": entry.get("scene_type"),
                "target_ma": entry.get("target_ma"),
                "scene_complete": completeness.get("scene_complete"),
                "run_validity": acceptance.get("run_validity"),
                "signoff_valid": acceptance.get("signoff_valid"),
                "failed_acceptance_checks": failed_acceptance_checks,
                "load_status_generation_count": completeness.get("load_status_generation_count"),
                "load_status_max_age_s": max_age,
                "effective_sample_rate_hz": effective_sample_rate_hz,
                "max_sample_gap_s": max_sample_gap_s,
                "source_online_mv_min": source_online_min,
                "source_online_mv_max": source_online_max,
                "ups_online_mv_min": ups_online_min,
                "ups_online_mv_max": ups_online_max,
                "report_failures": report_failures,
            }
        )
        for failure in report_failures:
            failures.append(f"{Path(rel_dir).name}:{failure}")

    payload = {
        "suite_id": suite.get("suite_id"),
        "summary": str(summary_path),
        "effective_test_contract": {
            "run_validity": "valid_for_signoff",
            "target_sample_rate_hz": 3.0,
            "minimum_effective_sample_rate_hz": 2.0,
            "maximum_sample_gap_s": 0.5,
            "maximum_realtime_sample_age_s": 0.5,
            "required_voltage_series": [
                "source_output_voltage",
                "ups_dcin_voltage",
                "ups_output_voltage",
                "load_actual_voltage",
            ],
            "failed_acceptance_checks_must_be_empty": True,
        },
        "verified_reports": verified,
        "ok": not failures,
        "failures": failures,
    }
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
