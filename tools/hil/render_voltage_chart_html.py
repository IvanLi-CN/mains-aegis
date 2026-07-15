#!/usr/bin/env python3
from __future__ import annotations

import argparse
import html
import json
from pathlib import Path


SERIES = [
    ("source_v", "Power Source Output", "#1f77b4"),
    ("vin_v", "UPS DC VIN", "#d62728"),
    ("ups_vout", "UPS INA VOUT", "#2ca02c"),
    ("load_v", "Load Actual Voltage", "#ff7f0e"),
]

TAG_COLORS = {
    "pre": "#f2f2f4",
    "hold": "#dcecff",
    "backup": "#ffe1e1",
    "restore": "#e2f4e4",
    "post": "#f2f2f4",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render a self-contained interactive HTML chart for HIL voltage traces."
    )
    parser.add_argument("--input", required=True, help="Path to timeseries.jsonl")
    parser.add_argument("--output", required=True, help="Output HTML path")
    parser.add_argument(
        "--gap-seconds",
        type=float,
        default=0.5,
        help="Break line segments across larger sample gaps.",
    )
    parser.add_argument(
        "--title",
        default="Interactive HIL Voltage Chart",
        help="HTML page title",
    )
    return parser.parse_args()


def load_report_status(timeseries_path: Path) -> dict:
    report_dir = timeseries_path.parent
    results_path = report_dir / "results.json"
    summary_path = report_dir / "summary.json"
    if results_path.exists():
        payload = json.loads(results_path.read_text())
        overall = payload.get("summary", {}).get("all", {})
        completeness = overall.get("completeness", {})
        acceptance = overall.get("acceptance", {})
        return {
            "source": "results.json",
            "run_validity": acceptance.get("run_validity"),
            "scene_complete": bool(completeness.get("scene_complete")),
            "failures": list(completeness.get("failures") or []),
            "failed_acceptance_checks": list(acceptance.get("failed_acceptance_checks") or []),
            "required_voltage_series": dict(acceptance.get("required_voltage_series") or {}),
            "effective_sample_rate_hz": completeness.get("effective_sample_rate_hz"),
            "max_sample_gap_s": completeness.get("max_sample_gap_s"),
            "signoff_valid": acceptance.get("signoff_valid"),
            "load_status_max_age_s": completeness.get("load_status_max_age_s"),
            "source_status_max_age_s": completeness.get("source_status_max_age_s"),
            "ups_status_max_age_s": completeness.get("ups_status_max_age_s"),
            "diag_snapshot_max_age_s": completeness.get("diag_snapshot_max_age_s"),
        }
    if summary_path.exists():
        payload = json.loads(summary_path.read_text())
        overall = payload.get("all", {})
        completeness = overall.get("completeness", {})
        acceptance = overall.get("acceptance", {})
        return {
            "source": "summary.json",
            "run_validity": acceptance.get("run_validity"),
            "scene_complete": bool(completeness.get("scene_complete")),
            "failures": list(completeness.get("failures") or []),
            "failed_acceptance_checks": list(acceptance.get("failed_acceptance_checks") or []),
            "required_voltage_series": dict(acceptance.get("required_voltage_series") or {}),
            "effective_sample_rate_hz": completeness.get("effective_sample_rate_hz"),
            "max_sample_gap_s": completeness.get("max_sample_gap_s"),
            "signoff_valid": acceptance.get("signoff_valid"),
            "load_status_max_age_s": completeness.get("load_status_max_age_s"),
            "source_status_max_age_s": completeness.get("source_status_max_age_s"),
            "ups_status_max_age_s": completeness.get("ups_status_max_age_s"),
            "diag_snapshot_max_age_s": completeness.get("diag_snapshot_max_age_s"),
        }
    return {
        "source": None,
        "run_validity": "invalid_diagnostic_only",
        "scene_complete": False,
        "failures": ["missing_results_or_summary"],
        "failed_acceptance_checks": ["missing_results_or_summary"],
        "required_voltage_series": {},
        "effective_sample_rate_hz": None,
        "max_sample_gap_s": None,
        "signoff_valid": False,
        "load_status_max_age_s": None,
        "source_status_max_age_s": None,
        "ups_status_max_age_s": None,
        "diag_snapshot_max_age_s": None,
    }


def load_rows(path: Path) -> list[dict]:
    rows = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        raw = json.loads(line)
        t_s = raw.get("t_s")
        if not isinstance(t_s, (int, float)):
            continue
        phase = raw.get("phase")
        if not isinstance(phase, str) or not phase:
            phase = raw.get("tag")
        ups_vout_mv = raw.get("ups_vout_mv")
        if not isinstance(ups_vout_mv, (int, float)):
            fallback_a = raw.get("out_a_vbus_mv")
            fallback_b = raw.get("out_b_vbus_mv")
            if isinstance(fallback_a, (int, float)):
                ups_vout_mv = fallback_a
            elif isinstance(fallback_b, (int, float)):
                ups_vout_mv = fallback_b
        rows.append(
            {
                "t_s": round(float(t_s), 3),
                "phase": phase,
                "stage": raw.get("stage"),
                "mode": raw.get("mode"),
                "backup_reason": raw.get("backup_reason") or raw.get("diag_backup_reason"),
                "charger_state": raw.get("charger_state"),
                "charger_allow_charge": raw.get("charger_allow_charge"),
                "target_ma": raw.get("load_target_i_ma"),
                "port_c_enabled": raw.get("port_c_enabled"),
                "mains_present": raw.get("mains_present"),
                "assist_target_vout_mv": raw.get("assist_target_vout_mv"),
                "vin_vbus_mv": raw.get("vin_vbus_mv"),
                "vin_iin_ma": raw.get("vin_iin_ma"),
                "tps_total_iout_ma": raw.get("tps_total_iout_ma"),
                "battery_current_ma": raw.get("battery_current_ma"),
                "diag_stage": raw.get("diag_stage"),
                "diag_assist_target_vout_mv": raw.get("diag_assist_target_vout_mv"),
                "diag_vin_baseline_mv": raw.get("diag_vin_baseline_mv"),
                "diag_vin_drop_mv": raw.get("diag_vin_drop_mv"),
                "diag_tps_total_iout_ma": raw.get("diag_tps_total_iout_ma"),
                "out_a_vbus_mv": raw.get("out_a_vbus_mv"),
                "out_b_vbus_mv": raw.get("out_b_vbus_mv"),
                "out_a_iout_ma": raw.get("out_a_iout_ma"),
                "out_b_iout_ma": raw.get("out_b_iout_ma"),
                "load_output_enabled": raw.get("load_output_enabled"),
                "load_i_total_ma": raw.get("load_i_total_ma"),
                "load_status_generation": raw.get("load_status_generation"),
                "load_status_age_s": raw.get("load_status_age_s"),
                "source_v": mv_to_v(raw.get("isolapurr_port_c_mv")),
                "vin_v": mv_to_v(raw.get("vin_vbus_mv")),
                "ups_vout": mv_to_v(ups_vout_mv),
                "load_v": mv_to_v(raw.get("load_v_local_mv")),
            }
        )
    if not rows:
        raise SystemExit(f"no usable rows found in {path}")
    return rows


def mv_to_v(value: object) -> float | None:
    if isinstance(value, (int, float)):
        return round(float(value) / 1000.0, 4)
    return None


def row_mains_present(row: dict) -> bool | None:
    value = row.get("mains_present")
    if isinstance(value, bool):
        return value
    return None


def row_vin_vbus_mv(row: dict) -> int | float | None:
    value = row.get("vin_vbus_mv")
    if isinstance(value, (int, float)):
        return value
    return None


def row_is_backup(row: dict) -> bool:
    return row.get("mode") == "backup" or row.get("stage") == "backup"


def normalized_span_phases(rows: list[dict]) -> list[str]:
    phases = [str(row.get("phase") or "unknown") for row in rows]

    try:
        first_transition_idx = phases.index("transition_backup")
    except ValueError:
        return phases

    hold_like_idx = None
    for idx in range(first_transition_idx - 1, -1, -1):
        if phases[idx] in {"hold", "transition_load", "backup_online"}:
            hold_like_idx = idx
            break

    hold_mains_present = (
        row_mains_present(rows[hold_like_idx]) if hold_like_idx is not None else None
    )
    hold_vin_vbus_mv = (
        row_vin_vbus_mv(rows[hold_like_idx]) if hold_like_idx is not None else None
    )

    first_effect_idx = None
    first_backup_idx = None
    for idx in range(first_transition_idx, len(rows)):
        if phases[idx] not in {"transition_backup", "backup"}:
            continue
        row = rows[idx]
        mains_changed = (
            hold_mains_present is not None
            and row_mains_present(row) is not None
            and row_mains_present(row) != hold_mains_present
        )
        vin_changed = (
            hold_vin_vbus_mv is not None
            and row_vin_vbus_mv(row) is not None
            and abs(row_vin_vbus_mv(row) - hold_vin_vbus_mv) >= 200
        )
        if first_effect_idx is None and (
            row_is_backup(row)
            or row.get("backup_reason") == "input_absent"
            or mains_changed
            or vin_changed
        ):
            first_effect_idx = idx
        if first_backup_idx is None and row_is_backup(row):
            first_backup_idx = idx

    if first_effect_idx is None:
        return phases

    for idx in range(first_transition_idx, first_effect_idx):
        if phases[idx] == "transition_backup":
            phases[idx] = "hold"

    if first_backup_idx is None:
        first_backup_idx = first_effect_idx + 1
    backup_start_idx = (
        first_effect_idx + 1 if first_backup_idx == first_effect_idx else first_backup_idx
    )
    for idx in range(backup_start_idx, len(phases)):
        if phases[idx] == "transition_backup":
            phases[idx] = "backup"

    return phases


def build_tag_spans(rows: list[dict]) -> list[dict]:
    spans: list[dict] = []
    phases = normalized_span_phases(rows)
    current: dict | None = None
    for idx, row in enumerate(rows):
        phase = phases[idx]
        if current is None or current["phase"] != phase:
            if current is not None:
                current["end"] = row["t_s"]
            current = {
                "phase": phase,
                "start": row["t_s"],
                "end": row["t_s"],
                "label": span_label(phase, row.get("target_ma")),
            }
            spans.append(current)
        else:
            current["end"] = row["t_s"]
    return spans


def build_stage_transitions(rows: list[dict]) -> list[dict]:
    transitions: list[dict] = []
    last_stage = last_mode = last_reason = last_charger = None
    for row in rows:
        stage = row.get("stage")
        mode = row.get("mode")
        reason = row.get("backup_reason")
        charger = row.get("charger_state")
        if (
            stage != last_stage
            or mode != last_mode
            or reason != last_reason
            or charger != last_charger
        ):
            transitions.append(
                {
                    "t_s": row["t_s"],
                    "stage": stage,
                    "mode": mode,
                    "backup_reason": reason,
                    "charger_state": charger,
                    "charger_allow_charge": row.get("charger_allow_charge"),
                }
            )
            last_stage = stage
            last_mode = mode
            last_reason = reason
            last_charger = charger
    return transitions


def span_label(tag: str, target_ma: object) -> str:
    if tag == "pre":
        return "pre / idle"
    if tag == "hold":
        if isinstance(target_ma, (int, float)):
            return f"hold / {int(target_ma)}mA"
        return "hold"
    if tag == "backup":
        return "backup / input cut"
    if tag == "restore":
        if isinstance(target_ma, (int, float)):
            return f"restore / {int(target_ma)}mA"
        return "restore / input on"
    if tag == "post":
        return "post / unload"
    return tag


def stats(rows: list[dict]) -> dict:
    result: dict[str, dict] = {}
    for key, label, _color in SERIES:
        points = [(row["t_s"], row[key], row["phase"], row["stage"]) for row in rows if row[key] is not None]
        if not points:
            continue
        t_s, value, phase, stage = min(points, key=lambda item: item[1])
        result[key] = {
            "label": label,
            "min_v": round(value, 4),
            "min_t_s": t_s,
            "phase": phase,
            "stage": stage,
        }
    return result


def script_safe_json(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")


def render_html(
    title: str,
    source_path: Path,
    rows: list[dict],
    gap_seconds: float,
) -> str:
    page_title = html.escape(title)
    data_json = script_safe_json(rows)
    spans_json = script_safe_json(build_tag_spans(rows))
    transitions_json = script_safe_json(build_stage_transitions(rows))
    series_json = script_safe_json(
        [{"key": key, "label": label, "color": color} for key, label, color in SERIES]
    )
    tag_colors_json = script_safe_json(TAG_COLORS)
    stats_json = script_safe_json(stats(rows))
    source_text = html.escape(str(source_path))
    report_status = load_report_status(source_path)
    report_sample_rate = report_status.get("effective_sample_rate_hz")
    report_max_gap = report_status.get("max_sample_gap_s")
    report_signoff_valid = report_status.get("signoff_valid") is True
    report_run_validity = report_status.get("run_validity")
    report_acceptance_failures = list(report_status.get("failed_acceptance_checks") or [])
    report_required_voltage_series = dict(report_status.get("required_voltage_series") or {})
    load_max_age = report_status.get("load_status_max_age_s")
    source_max_age = report_status.get("source_status_max_age_s")
    ups_max_age = report_status.get("ups_status_max_age_s")
    diag_max_age = report_status.get("diag_snapshot_max_age_s")
    ups_vout_complete = all(row.get("ups_vout") is not None for row in rows)
    ups_vout_required_ok = report_required_voltage_series.get("ups_output_voltage")
    if ups_vout_required_ok is None:
        ups_vout_required_ok = ups_vout_complete
    rate_ok = isinstance(report_sample_rate, (int, float)) and report_sample_rate >= 2.0
    gap_ok = isinstance(report_max_gap, (int, float)) and report_max_gap <= 0.5
    acceptance_ok = (
        report_signoff_valid
        and report_status["scene_complete"]
        and rate_ok
        and gap_ok
        and ups_vout_required_ok
        and not report_acceptance_failures
        and not report_status["failures"]
    )
    if acceptance_ok:
        acceptance_class = "note-success"
        acceptance_html = (
            "Acceptance status: "
            f"<strong>{html.escape(str(report_run_validity or 'valid_for_signoff'))}</strong>. "
            "This chart is backed by one complete formal scene with full source / UPS VIN / UPS INA VOUT / load voltage series, "
            f"effective_sample_rate_hz={html.escape(str(report_sample_rate))}, "
            f"max_sample_gap_s={html.escape(str(report_max_gap))}, "
            "max realtime ages "
            f"(load/source/ups/diag)="
            f"{html.escape(str(load_max_age))}/"
            f"{html.escape(str(source_max_age))}/"
            f"{html.escape(str(ups_max_age))}/"
            f"{html.escape(str(diag_max_age))}. "
            f"ups_output_voltage_complete={html.escape(str(ups_vout_required_ok))}."
        )
    else:
        failures_text = ", ".join(report_status["failures"]) or "unknown"
        acceptance_failures_text = ", ".join(report_acceptance_failures) or "unknown"
        acceptance_class = "note-danger"
        acceptance_html = (
            "Acceptance status: "
            f"<strong>{html.escape(str(report_run_validity or 'invalid_diagnostic_only'))}</strong>. "
            "This report is diagnostic-only because the formal acceptance contract did not fully pass. "
            f"effective_sample_rate_hz={html.escape(str(report_sample_rate))}, "
            f"max_sample_gap_s={html.escape(str(report_max_gap))}. "
            "max realtime ages "
            f"(load/source/ups/diag)="
            f"{html.escape(str(load_max_age))}/"
            f"{html.escape(str(source_max_age))}/"
            f"{html.escape(str(ups_max_age))}/"
            f"{html.escape(str(diag_max_age))}. "
            f"ups_output_voltage_complete={html.escape(str(ups_vout_required_ok))}. "
            f"Completeness source={html.escape(str(report_status['source']))}, "
            f"acceptance_failures={html.escape(acceptance_failures_text)}, "
            f"completeness_failures={html.escape(failures_text)}."
        )
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{page_title}</title>
  <style>
    :root {{
      --bg: #f6f7f9;
      --panel: #ffffff;
      --ink: #1c1f24;
      --muted: #5b6470;
      --grid: #d8dde5;
      --border: #c8d0db;
      --shadow: 0 12px 30px rgba(20, 31, 51, 0.08);
      --accent: #1f77b4;
      font-family: "SF Mono", "Menlo", "Consolas", monospace;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      background: linear-gradient(180deg, #eef3f8 0%, var(--bg) 160px);
      color: var(--ink);
    }}
    .page {{
      max-width: 1560px;
      margin: 0 auto;
      padding: 28px 24px 40px;
    }}
    body.embed {{
      background: #fff;
    }}
    body.embed .page {{
      max-width: none;
      padding: 10px;
    }}
    body.embed .page > h1,
    body.embed .lede,
    body.embed .note,
    body.embed .sidebar {{
      display: none;
    }}
    body.embed .layout {{
      margin-top: 0;
      display: block;
    }}
    body.embed .chart-panel {{
      padding: 10px;
      border: 0;
      border-radius: 0;
      box-shadow: none;
    }}
    body.embed .controls {{
      margin-bottom: 8px;
      gap: 8px 12px;
    }}
    body.embed .control-group button,
    body.embed .series-toggle {{
      padding: 4px 8px;
      font-size: 12px;
    }}
    body.embed .control-group input[type="range"] {{
      width: 150px;
    }}
    body.embed .chart-wrap {{
      border-radius: 10px;
    }}
    h1 {{
      margin: 0 0 10px;
      font-size: 32px;
      line-height: 1.2;
    }}
    .lede {{
      margin: 0;
      color: var(--muted);
      font-size: 15px;
      line-height: 1.5;
    }}
    .note {{
      margin-top: 12px;
      padding: 12px 14px;
      background: #fff9df;
      border: 1px solid #e8d98f;
      border-radius: 12px;
      color: #5a4c13;
      font-size: 14px;
    }}
    .note.note-danger {{
      background: #fff0f0;
      border-color: #e4a6a6;
      color: #7a1f1f;
    }}
    .note.note-success {{
      background: #edf9ef;
      border-color: #93d3a0;
      color: #165a24;
    }}
    .layout {{
      margin-top: 18px;
      display: grid;
      grid-template-columns: minmax(0, 1fr) 320px;
      gap: 18px;
      align-items: start;
    }}
    .panel {{
      background: var(--panel);
      border: 1px solid var(--border);
      border-radius: 18px;
      box-shadow: var(--shadow);
    }}
    .chart-panel {{
      padding: 18px 18px 12px;
    }}
    .controls {{
      display: flex;
      flex-wrap: wrap;
      gap: 12px 18px;
      margin-bottom: 12px;
      align-items: center;
    }}
    .control-group {{
      display: flex;
      align-items: center;
      gap: 8px;
      flex-wrap: wrap;
    }}
    .control-group label {{
      font-size: 13px;
      color: var(--muted);
    }}
    .control-group button {{
      border: 1px solid var(--border);
      background: #f8fafc;
      color: var(--ink);
      border-radius: 999px;
      padding: 7px 12px;
      cursor: pointer;
      font: inherit;
      font-size: 13px;
    }}
    .control-group button:hover {{
      border-color: #97aac4;
      background: #eef5ff;
    }}
    .control-group input[type="range"] {{
      width: 180px;
    }}
    .chart-wrap {{
      position: relative;
      border: 1px solid var(--border);
      border-radius: 14px;
      overflow: hidden;
      background: #fff;
    }}
    svg {{
      display: block;
      width: 100%;
      height: auto;
      background: #fff;
    }}
    .tooltip {{
      position: fixed;
      left: 0;
      top: 0;
      pointer-events: none;
      min-width: 320px;
      max-width: min(420px, calc(100vw - 24px));
      max-height: calc(100vh - 24px);
      padding: 10px 12px;
      border-radius: 12px;
      background: rgba(19, 24, 32, 0.94);
      color: #fff;
      font-size: 12px;
      line-height: 1.35;
      box-shadow: 0 12px 28px rgba(0, 0, 0, 0.22);
      opacity: 0;
      transform: translate(-9999px, -9999px);
      transition: opacity 0.12s ease;
      z-index: 2147483647;
      overflow-wrap: anywhere;
      overflow-y: auto;
    }}
    .tooltip strong {{ color: #ffe594; }}
    .sidebar {{
      padding: 18px;
    }}
    .sidebar h2 {{
      margin: 0 0 12px;
      font-size: 18px;
    }}
    .sidebar-section + .sidebar-section {{
      margin-top: 18px;
      padding-top: 18px;
      border-top: 1px dashed var(--border);
    }}
    .list {{
      display: grid;
      gap: 10px;
      margin: 0;
      padding: 0;
      list-style: none;
    }}
    .list li {{
      font-size: 14px;
      line-height: 1.45;
    }}
    .swatch {{
      display: inline-block;
      width: 12px;
      height: 12px;
      border-radius: 999px;
      margin-right: 8px;
      vertical-align: -1px;
    }}
    .small {{
      color: var(--muted);
      font-size: 13px;
      line-height: 1.45;
    }}
    .series-toggle {{
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 4px 8px;
      border-radius: 999px;
      background: #f6f8fb;
      border: 1px solid #e2e8f0;
    }}
    .series-toggle input {{
      accent-color: var(--series-color, var(--accent));
    }}
    .series-mark {{
      width: 26px;
      height: 10px;
      display: inline-flex;
      align-items: center;
    }}
    .series-mark::before {{
      content: "";
      width: 26px;
      height: 4px;
      border-radius: 999px;
      background: var(--series-color, var(--accent));
      box-shadow: 0 0 0 1px rgba(255,255,255,0.85) inset;
    }}
    @media (max-width: 1180px) {{
      .layout {{
        grid-template-columns: 1fr;
      }}
    }}
  </style>
</head>
<body>
  <div class="page">
    <h1>{page_title}</h1>
    <p class="lede">Real sample points only: power source output voltage, UPS DC VIN, UPS INA VOUT, and load actual voltage from the same HIL report. Hover any point to inspect the exact sample.</p>
    <div class="note">This page does <strong>not</strong> synthesize oscilloscope-like waveforms. Line segments are drawn only between adjacent samples when the gap is ≤ {gap_seconds:.1f}s. Larger gaps are intentionally broken.</div>
    <div class="note {acceptance_class}">{acceptance_html}</div>
    <div class="layout">
      <section class="panel chart-panel">
        <div class="controls">
          <div class="control-group">
            <label>Windows</label>
            <button data-window="full">Full</button>
            <button data-window="hold">Hold</button>
            <button data-window="backup_restore">Backup + Restore</button>
            <button data-window="restore">Restore</button>
          </div>
          <div class="control-group">
            <label>Series</label>
            <label class="series-toggle" style="--series-color: #1f77b4"><input type="checkbox" data-series="source_v" checked><span class="series-mark"></span> Source</label>
            <label class="series-toggle" style="--series-color: #d62728"><input type="checkbox" data-series="vin_v" checked><span class="series-mark"></span> UPS VIN</label>
            <label class="series-toggle" style="--series-color: #2ca02c"><input type="checkbox" data-series="ups_vout" checked><span class="series-mark"></span> UPS VOUT</label>
            <label class="series-toggle" style="--series-color: #ff7f0e"><input type="checkbox" data-series="load_v" checked><span class="series-mark"></span> Load V</label>
          </div>
        </div>
        <div class="controls">
          <div class="control-group">
            <label for="startRange">Start</label>
            <input id="startRange" type="range">
            <span id="startValue" class="small"></span>
          </div>
          <div class="control-group">
            <label for="endRange">End</label>
            <input id="endRange" type="range">
            <span id="endValue" class="small"></span>
          </div>
        </div>
        <div class="chart-wrap">
          <svg id="chart" viewBox="0 0 1360 760" aria-label="Interactive voltage chart"></svg>
          <div id="tooltip" class="tooltip"></div>
        </div>
      </section>
      <aside class="panel sidebar">
        <div class="sidebar-section">
          <h2>Trace Map</h2>
          <ul id="legend" class="list"></ul>
        </div>
        <div class="sidebar-section">
          <h2>Phase Map</h2>
          <ul id="phaseMap" class="list"></ul>
        </div>
        <div class="sidebar-section">
          <h2>Minima</h2>
          <ul id="stats" class="list"></ul>
        </div>
        <div class="sidebar-section">
          <h2>Source Report</h2>
          <p class="small">{source_text}</p>
          <p class="small">Rows: {len(rows)}<br>Gap break threshold: {gap_seconds:.1f}s</p>
        </div>
      </aside>
    </div>
  </div>
  <script>
    const rows = {data_json};
    const tagSpans = {spans_json};
    const stageTransitions = {transitions_json};
    const seriesConfig = {series_json};
    const tagColors = {tag_colors_json};
    const minima = {stats_json};
    const gapSeconds = {gap_seconds:.3f};

    const svg = document.getElementById("chart");
    const tooltip = document.getElementById("tooltip");
    document.body.appendChild(tooltip);
    const startRange = document.getElementById("startRange");
    const endRange = document.getElementById("endRange");
    const startValue = document.getElementById("startValue");
    const endValue = document.getElementById("endValue");
    const legend = document.getElementById("legend");
    const phaseMap = document.getElementById("phaseMap");
    const stats = document.getElementById("stats");

    const fullStart = rows[0].t_s;
    const fullEnd = rows[rows.length - 1].t_s;
    const state = {{
      start: fullStart,
      end: fullEnd,
      visible: Object.fromEntries(seriesConfig.map(item => [item.key, true])),
    }};

    startRange.min = String(fullStart);
    startRange.max = String(fullEnd);
    startRange.step = "0.001";
    endRange.min = String(fullStart);
    endRange.max = String(fullEnd);
    endRange.step = "0.001";
    startRange.value = String(fullStart);
    endRange.value = String(fullEnd);

    function fmtV(value) {{
      return value == null ? "n/a" : value.toFixed(3) + "V";
    }}

    function fmtT(value) {{
      return value.toFixed(3) + "s";
    }}

    function fmtMv(value) {{
      return Number.isFinite(value) ? `${{value}}mV` : "n/a";
    }}

    function fmtMa(value) {{
      return Number.isFinite(value) ? `${{value}}mA` : "n/a";
    }}

    function fmtBool(value) {{
      return typeof value === "boolean" ? String(value) : "n/a";
    }}

    function fmtToken(value) {{
      return value == null || value === "" ? "n/a" : String(value);
    }}

    function escapeHtml(value) {{
      return String(value).replace(/[&<>"']/g, ch => ({{
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      }})[ch]);
    }}

    function phaseMeaning(phase) {{
      switch (phase) {{
        case "pre":
          return "Idle baseline before the runner applies any load or input switching.";
        case "transition_load":
          return "Load-apply transient while the electronic load ramps toward the requested current with VIN still online.";
        case "hold":
          return "Main online hold window under the programmed load.";
        case "transition_source_limited":
          return "Handoff transient: TPS output has started participating, but source-limited backup is not fully latched yet.";
        case "backup_online":
          return "Source-limited backup is latched while VIN remains online, so UPS is actively supporting the load.";
        case "transition_backup":
          return "Backup handoff transient after input loss or an input-off decision, before backup fully settles.";
        case "backup":
          return "Stable backup window after UPS has taken over the load.";
        case "restore":
          return "Restore window while the load is handed back from backup to the online path.";
        case "transition_unload":
          return "Unload transient while the runner disables the electronic load and returns toward post-idle.";
        case "post":
          return "Post-idle window after the load has been removed.";
        default:
          return "Recorded scene phase window.";
      }}
    }}

    function findActiveSpanIndex(targetT) {{
      for (let index = 0; index < tagSpans.length; index += 1) {{
        const span = tagSpans[index];
        if (targetT >= span.start && targetT <= span.end) return index;
      }}
      let fallback = -1;
      for (let index = 0; index < tagSpans.length; index += 1) {{
        if (tagSpans[index].start <= targetT) fallback = index;
      }}
      return fallback;
    }}

    function findActiveTransitionIndex(targetT) {{
      let activeIndex = -1;
      for (let index = 0; index < stageTransitions.length; index += 1) {{
        if (stageTransitions[index].t_s <= targetT) {{
          activeIndex = index;
        }} else {{
          break;
        }}
      }}
      return activeIndex;
    }}

    function transitionChangeSummary(previous, current) {{
      if (!current) return [];
      if (!previous) return [];
      const changes = [];
      const trackedFields = [
        ["stage", "stage"],
        ["mode", "mode"],
        ["backup_reason", "backup_reason"],
        ["charger_state", "charger"],
      ];
      for (const [field, label] of trackedFields) {{
        if (current[field] !== previous[field]) {{
          changes.push(`${{label}} ${{fmtToken(previous[field])}} -> ${{fmtToken(current[field])}}`);
        }}
      }}
      if (current.charger_allow_charge !== previous.charger_allow_charge) {{
        changes.push(
          `allow_charge ${{fmtBool(previous.charger_allow_charge)}} -> ${{fmtBool(current.charger_allow_charge)}}`
        );
      }}
      return changes;
    }}

    function transitionMeaning(previous, current) {{
      if (!current) return "No state marker matched this sample.";
      if (!previous) {{
        return "Scene baseline state.";
      }}
      if (
        current.backup_reason === "source_limited" &&
        previous.backup_reason !== "source_limited"
      ) {{
        return "UPS latched source-limited backup: VIN is still online, but the upstream source can no longer carry the load.";
      }}
      if (
        current.backup_reason === "input_absent" &&
        previous.backup_reason !== "input_absent"
      ) {{
        return "UPS confirmed the input as absent/cut and switched the backup reason to input_absent.";
      }}
      if (current.stage === "backup" && previous.stage !== "backup") {{
        return "Runtime stage changed to backup: UPS has taken over the load.";
      }}
      if (current.mode === "backup" && previous.mode !== "backup") {{
        return "Published mode changed to backup.";
      }}
      if (current.charger_state !== previous.charger_state) {{
        return `Charger state changed to ${{fmtToken(current.charger_state)}}.`;
      }}
      if (current.charger_allow_charge !== previous.charger_allow_charge) {{
        return `Charge permission changed to ${{fmtBool(current.charger_allow_charge)}}.`;
      }}
      return "";
    }}

    function chartTagDetails(targetT) {{
      const spanIndex = findActiveSpanIndex(targetT);
      const transitionIndex = findActiveTransitionIndex(targetT);
      const span = spanIndex >= 0 ? tagSpans[spanIndex] : null;
      const transition = transitionIndex >= 0 ? stageTransitions[transitionIndex] : null;
      const previousTransition = transitionIndex > 0 ? stageTransitions[transitionIndex - 1] : null;
      const transitionWindowS = Math.max(0.35, Math.min(1.2, (state.end - state.start) * 0.03));
      const showTransition = transition && Math.abs(targetT - transition.t_s) <= transitionWindowS;
      const transitionMeaningText = transitionMeaning(previousTransition, transition);
      const phaseHtml = span
        ? `
          <div><strong>P${{spanIndex + 1}}</strong> ${{escapeHtml(span.label)}}</div>
          <div>${{escapeHtml(phaseMeaning(span.phase))}}</div>
          <div>range=${{fmtT(span.start)}}..${{fmtT(span.end)}} | phase=${{escapeHtml(span.phase)}}</div>
        `
        : `
          <div><strong>P*</strong></div>
          <div>No phase span matched this sample.</div>
        `;
      const transitionChanges = transitionChangeSummary(previousTransition, transition);
      const transitionHtml = !showTransition
        ? ``
        : `
          <div style="margin-top:4px"><strong>S${{transitionIndex + 1}}</strong> ${{previousTransition ? "state change" : "initial state"}}</div>
          ${{transitionMeaningText ? `<div>${{escapeHtml(transitionMeaningText)}}</div>` : ""}}
          <div>t=${{fmtT(transition.t_s)}} | stage=${{escapeHtml(fmtToken(transition.stage))}} | mode=${{escapeHtml(fmtToken(transition.mode))}}</div>
          <div>backup_reason=${{escapeHtml(fmtToken(transition.backup_reason))}} | charger=${{escapeHtml(fmtToken(transition.charger_state))}} | allow_charge=${{fmtBool(transition.charger_allow_charge)}}</div>
          ${{transitionChanges.length ? `<div>${{escapeHtml(transitionChanges.join(" | "))}}</div>` : ""}}
        `;
      return `
        <div style="margin-top:6px"><strong>Chart Tags</strong></div>
        ${{phaseHtml}}
        ${{transitionHtml}}
      `;
    }}

    function updateLegend() {{
      legend.innerHTML = "";
      for (const item of seriesConfig) {{
        const li = document.createElement("li");
        li.innerHTML = `<span class="swatch" style="background:${{item.color}}"></span>${{item.label}}`;
        legend.appendChild(li);
      }}
    }}

    function updateStats() {{
      stats.innerHTML = "";
      for (const item of seriesConfig) {{
        const metric = minima[item.key];
        if (!metric) continue;
        const li = document.createElement("li");
        li.innerHTML = `<strong>${{metric.label}}</strong><br>min=${{metric.min_v.toFixed(3)}}V @ ${{fmtT(metric.min_t_s)}}<br>phase=${{metric.phase}}, stage=${{metric.stage}}`;
        stats.appendChild(li);
      }}
    }}

    function updatePhaseMap() {{
      phaseMap.innerHTML = "";
      tagSpans.forEach((span, index) => {{
        const li = document.createElement("li");
        const color = tagColors[span.phase] || "#f1f1f1";
        const transitions = stageTransitions
          .filter(item => item.t_s >= span.start && item.t_s <= span.end)
          .map(item => {{
            const state = item.stage || item.mode || "unknown";
            const reason = item.backup_reason ? ` / ${{item.backup_reason}}` : "";
            const charger = item.charger_state ? ` / ${{item.charger_state}}` : "";
            return `${{fmtT(item.t_s)}} ${{state}}${{reason}}${{charger}}`;
          }})
          .join(" · ");
        li.innerHTML = `<strong><span class="swatch" style="background:${{color}}"></span>P${{index + 1}}</strong> ${{span.label}}<br><span class="small">${{fmtT(span.start)}}..${{fmtT(span.end)}} | phase=${{span.phase}}${{transitions ? "<br>stage: " + transitions : ""}}</span>`;
        phaseMap.appendChild(li);
      }});
    }}

    function windowFromPhase(phaseName) {{
      const span = tagSpans.find(item => item.phase === phaseName);
      if (!span) return null;
      return [span.start, span.end];
    }}

    function setWindow(start, end) {{
      state.start = Math.max(fullStart, Math.min(start, fullEnd));
      state.end = Math.max(state.start + 0.001, Math.min(end, fullEnd));
      startRange.value = String(state.start);
      endRange.value = String(state.end);
      render();
    }}

    document.querySelectorAll("button[data-window]").forEach(button => {{
      button.addEventListener("click", () => {{
        const name = button.dataset.window;
        if (name === "full") {{
          setWindow(fullStart, fullEnd);
          return;
        }}
        if (name === "hold") {{
          const w = windowFromPhase("hold");
          if (w) setWindow(Math.max(fullStart, w[0] - 3), Math.min(fullEnd, w[1] + 3));
          return;
        }}
        if (name === "backup_restore") {{
          const b = windowFromPhase("backup");
          const r = windowFromPhase("restore");
          if (b && r) setWindow(Math.max(fullStart, b[0] - 3), Math.min(fullEnd, r[1] + 3));
          return;
        }}
        if (name === "restore") {{
          const w = windowFromPhase("restore");
          if (w) setWindow(Math.max(fullStart, w[0] - 3), Math.min(fullEnd, w[1] + 3));
        }}
      }});
    }});

    document.querySelectorAll("input[data-series]").forEach(input => {{
      input.addEventListener("change", () => {{
        state.visible[input.dataset.series] = input.checked;
        render();
      }});
    }});

    startRange.addEventListener("input", () => {{
      const value = parseFloat(startRange.value);
      state.start = Math.min(value, state.end - 0.001);
      startRange.value = String(state.start);
      render();
    }});

    endRange.addEventListener("input", () => {{
      const value = parseFloat(endRange.value);
      state.end = Math.max(value, state.start + 0.001);
      endRange.value = String(state.end);
      render();
    }});

    function withinWindow(row) {{
      return row.t_s >= state.start && row.t_s <= state.end;
    }}

    function visibleRows() {{
      return rows.filter(withinWindow);
    }}

    function overlap(span) {{
      return span.end >= state.start && span.start <= state.end;
    }}

    function scaleX(t, left, width) {{
      return left + ((t - state.start) / (state.end - state.start)) * width;
    }}

    function scaleY(v, top, height, yMin, yMax) {{
      return top + height - ((v - yMin) / (yMax - yMin)) * height;
    }}

    function svgEl(name, attrs) {{
      const el = document.createElementNS("http://www.w3.org/2000/svg", name);
      for (const [key, value] of Object.entries(attrs)) {{
        el.setAttribute(key, String(value));
      }}
      return el;
    }}

    function niceStep(span, target) {{
      const rough = span / Math.max(1, target);
      const exponent = Math.floor(Math.log10(rough || 1));
      const fraction = rough / Math.pow(10, exponent);
      for (const candidate of [1, 2, 2.5, 5, 10]) {{
        if (fraction <= candidate) return candidate * Math.pow(10, exponent);
      }}
      return 10 * Math.pow(10, exponent);
    }}

    function render() {{
      const filtered = visibleRows();
      const left = 82;
      const top = 48;
      const width = 1230;
      const height = 620;

      startValue.textContent = fmtT(state.start);
      endValue.textContent = fmtT(state.end);
      svg.innerHTML = "";

      const values = [];
      for (const row of filtered) {{
        for (const item of seriesConfig) {{
          if (!state.visible[item.key]) continue;
          const value = row[item.key];
          if (value != null) values.push(value);
        }}
      }}
      const fallbackValues = values.length ? values : [0, 1];
      const rawMin = Math.min(...fallbackValues);
      const rawMax = Math.max(...fallbackValues);
      const pad = Math.max(0.08, (rawMax - rawMin) * 0.12);
      let yMin = Math.floor((rawMin - pad) * 10) / 10;
      let yMax = Math.ceil((rawMax + pad) * 10) / 10;
      if (yMax <= yMin) yMax = yMin + 1;

      const visibleSpans = tagSpans.filter(overlap).map((span, index) => {{
        const x0 = scaleX(Math.max(span.start, state.start), left, width);
        const x1 = scaleX(Math.min(span.end, state.end), left, width);
        return {{ ...span, index, x0, x1, pixelWidth: x1 - x0 }};
      }});
      for (const span of visibleSpans) {{
        svg.appendChild(svgEl("rect", {{
          x: span.x0,
          y: top,
          width: Math.max(1, span.pixelWidth),
          height,
          fill: tagColors[span.phase] || "#f1f1f1",
          opacity: 0.7,
        }}));
        const phaseId = `P${{span.index + 1}}`;
        const phaseText = span.pixelWidth < 150 ? phaseId : `${{phaseId}} ${{span.label}}`;
        if (span.pixelWidth < 34) continue;
        const labelX = Math.min(span.x1 - 8, Math.max(left + 8, (span.x0 + span.x1) / 2));
        const labelY = top + 22 + ((span.index % 2) * 22);
        const labelWidth = Math.min(span.pixelWidth - 14, Math.max(62, phaseText.length * 8.6 + 18));
        svg.appendChild(svgEl("rect", {{
          x: labelX - labelWidth / 2,
          y: labelY - 17,
          width: labelWidth,
          height: 21,
          rx: 8,
          fill: "rgba(255,255,255,0.82)",
          stroke: "rgba(148,163,184,0.55)",
          "stroke-width": 1,
        }}));
        const text = svgEl("text", {{
          x: labelX,
          y: labelY,
          "text-anchor": "middle",
          "font-size": span.pixelWidth < 150 ? 12 : 14,
          fill: "#2b3138",
        }});
        text.textContent = phaseText;
        svg.appendChild(text);
      }}

      const yStep = niceStep(yMax - yMin, 6);
      for (let y = Math.floor(yMin / yStep) * yStep; y <= yMax + 1e-9; y += yStep) {{
        const py = scaleY(y, top, height, yMin, yMax);
        svg.appendChild(svgEl("line", {{
          x1: left,
          y1: py,
          x2: left + width,
          y2: py,
          stroke: "#d8dde5",
          "stroke-width": 1,
        }}));
        const label = svgEl("text", {{
          x: left - 12,
          y: py + 5,
          "text-anchor": "end",
          "font-size": 15,
          fill: "#66707d",
        }});
        label.textContent = y.toFixed(2) + "V";
        svg.appendChild(label);
      }}

      const xStep = niceStep(state.end - state.start, 8);
      for (let t = Math.ceil(state.start / xStep) * xStep; t <= state.end + 1e-9; t += xStep) {{
        const px = scaleX(t, left, width);
        svg.appendChild(svgEl("line", {{
          x1: px,
          y1: top,
          x2: px,
          y2: top + height,
          stroke: "#edf0f4",
          "stroke-width": 1,
        }}));
        const label = svgEl("text", {{
          x: px,
          y: top + height + 28,
          "text-anchor": "middle",
          "font-size": 15,
          fill: "#66707d",
        }});
        label.textContent = t.toFixed(0) + "s";
        svg.appendChild(label);
      }}

      svg.appendChild(svgEl("rect", {{
        x: left,
        y: top,
        width,
        height,
        fill: "none",
        stroke: "#95a2b3",
        "stroke-width": 1.5,
      }}));

      const visibleTransitions = stageTransitions
        .filter(item => item.t_s >= state.start && item.t_s <= state.end)
        .map((item, index) => ({{ ...item, index }}));
      const transitionMinGapPx = 72;
      let lastTransitionLabelX = -Infinity;
      for (const transition of visibleTransitions) {{
        const px = scaleX(transition.t_s, left, width);
        svg.appendChild(svgEl("line", {{
          x1: px,
          y1: top,
          x2: px,
          y2: top + height,
          stroke: "#8996a7",
          "stroke-width": 1,
          "stroke-dasharray": "4 4",
        }}));
        const transitionText = `S${{transition.index + 1}}`;
        const canShowLabel = px - lastTransitionLabelX >= transitionMinGapPx;
        if (!canShowLabel) continue;
        lastTransitionLabelX = px;
        const labelX = Math.min(left + width - 8, px + 6);
        const labelY = top + 56 + ((transition.index % 3) * 22);
        const labelWidth = Math.max(54, Math.min(130, transitionText.length * 8 + 14));
        svg.appendChild(svgEl("rect", {{
          x: labelX - 4,
          y: labelY - 16,
          width: labelWidth,
          height: 20,
          rx: 7,
          fill: "rgba(255,255,255,0.78)",
          stroke: "rgba(148,163,184,0.55)",
          "stroke-width": 1,
        }}));
        const label = svgEl("text", {{
          x: labelX,
          y: labelY,
          "font-size": 12,
          fill: "#404852",
        }});
        label.textContent = transitionText;
        svg.appendChild(label);
      }}

        for (const item of seriesConfig) {{
        if (!state.visible[item.key]) continue;
        let segment = [];
        const pathParts = [];
        for (const row of filtered) {{
          const value = row[item.key];
          if (value == null) {{
            if (segment.length >= 2) pathParts.push(segment.map(point => point.cmd).join(" "));
            segment = [];
            continue;
          }}
          const x = scaleX(row.t_s, left, width);
          const y = scaleY(value, top, height, yMin, yMax);
          const previous = segment.length ? segment[segment.length - 1] : null;
          if (previous && row.t_s - previous.t_s > gapSeconds) {{
            if (segment.length >= 2) pathParts.push(segment.map(point => point.cmd).join(" "));
            segment = [];
          }}
          segment.push({{ t_s: row.t_s, cmd: `${{segment.length ? "L" : "M"}}${{x.toFixed(2)}},${{y.toFixed(2)}}` }});
        }}
        if (segment.length >= 2) pathParts.push(segment.map(point => point.cmd).join(" "));
        for (const d of pathParts) {{
          svg.appendChild(svgEl("path", {{
            d,
            fill: "none",
            stroke: item.color,
            "stroke-width": 2.8,
            "stroke-linejoin": "round",
            "stroke-linecap": "round",
          }}));
        }}
        for (const row of filtered) {{
          const value = row[item.key];
          if (value == null) continue;
          svg.appendChild(svgEl("circle", {{
            cx: scaleX(row.t_s, left, width),
            cy: scaleY(value, top, height, yMin, yMax),
            r: 3.2,
            fill: item.color,
            stroke: "#fff",
            "stroke-width": 1,
          }}));
        }}
      }}

      const hoverLayer = svgEl("rect", {{
        x: left,
        y: top,
        width,
        height,
        fill: "transparent",
      }});
      const cursor = svgEl("line", {{
        x1: left,
        y1: top,
        x2: left,
        y2: top + height,
        stroke: "#111827",
        "stroke-width": 1,
        "stroke-dasharray": "3 3",
        opacity: 0,
      }});
      svg.appendChild(cursor);
      svg.appendChild(hoverLayer);

      hoverLayer.addEventListener("mouseleave", () => {{
        cursor.setAttribute("opacity", "0");
        tooltip.style.opacity = "0";
        tooltip.style.transform = "translate(-9999px, -9999px)";
      }});

      hoverLayer.addEventListener("mousemove", event => {{
        const pt = svg.createSVGPoint();
        pt.x = event.clientX;
        pt.y = event.clientY;
        const local = pt.matrixTransform(svg.getScreenCTM().inverse());
        const ratio = Math.max(0, Math.min(1, (local.x - left) / width));
        const targetT = state.start + ratio * (state.end - state.start);
        const candidates = filtered;
        if (!candidates.length) return;
        let nearest = candidates[0];
        for (const row of candidates) {{
          if (Math.abs(row.t_s - targetT) < Math.abs(nearest.t_s - targetT)) nearest = row;
        }}
        const tagHelp = chartTagDetails(nearest.t_s);
        const cx = scaleX(nearest.t_s, left, width);
        cursor.setAttribute("x1", cx);
        cursor.setAttribute("x2", cx);
        cursor.setAttribute("opacity", "1");
        const seriesLines = seriesConfig
          .map(item => {{
            const visible = state.visible[item.key];
            const value = nearest[item.key];
            const label = item.label;
            const marker = `<span style="display:inline-block;width:10px;height:10px;border-radius:999px;background:${{item.color}};margin-right:6px;vertical-align:-1px;"></span>`;
            return `<div>${{marker}}${{label}}: ${{visible ? fmtV(value) : "hidden"}}</div>`;
          }})
          .join("");
        tooltip.innerHTML = `
          <div><strong>${{fmtT(nearest.t_s)}}</strong></div>
          <div>phase=${{nearest.phase || "n/a"}} | stage=${{nearest.stage || "n/a"}} | mode=${{nearest.mode || "n/a"}}</div>
          <div>mains_present=${{fmtBool(nearest.mains_present)}} | load_enabled=${{fmtBool(nearest.load_output_enabled)}} | load_gen=${{nearest.load_status_generation ?? "n/a"}}</div>
          ${{tagHelp}}
          <div style="margin-top:6px"><strong>Voltage traces</strong></div>
          ${{seriesLines}}
          <div style="margin-top:6px"><strong>UPS</strong></div>
          <div>target=${{fmtMv(nearest.assist_target_vout_mv)}} | VIN=${{fmtMv(nearest.vin_vbus_mv)}} | VIN_I=${{fmtMa(nearest.vin_iin_ma)}}</div>
          <div>TPS=${{fmtMa(nearest.tps_total_iout_ma)}} | batt=${{fmtMa(nearest.battery_current_ma)}}</div>
          <div>OUTA=${{fmtMv(nearest.out_a_vbus_mv)}} / ${{fmtMa(nearest.out_a_iout_ma)}} | OUTB=${{fmtMv(nearest.out_b_vbus_mv)}} / ${{fmtMa(nearest.out_b_iout_ma)}}</div>
          <div style="margin-top:6px"><strong>Diagnostics</strong></div>
          <div>d_stage=${{nearest.diag_stage || "n/a"}} | d_target=${{fmtMv(nearest.diag_assist_target_vout_mv)}}</div>
          <div>vbase=${{fmtMv(nearest.diag_vin_baseline_mv)}} | vdrop=${{fmtMv(nearest.diag_vin_drop_mv)}} | d_tps=${{fmtMa(nearest.diag_tps_total_iout_ma)}}</div>
          <div style="margin-top:6px"><strong>Load</strong></div>
          <div>target=${{fmtMa(nearest.target_ma)}} | actual=${{fmtMa(nearest.load_i_total_ma)}}</div>
          <div>age=${{typeof nearest.load_status_age_s === "number" ? nearest.load_status_age_s.toFixed(3) + "s" : "n/a"}}</div>
        `;
        tooltip.style.opacity = "1";
        tooltip.style.transform = "translate(-9999px, -9999px)";
        const tooltipRect = tooltip.getBoundingClientRect();
        const tooltipWidth = tooltipRect.width || 360;
        const tooltipHeight = tooltipRect.height || 260;
        const margin = 12;
        const offset = 18;
        const viewportWidth = document.documentElement.clientWidth;
        const viewportHeight = document.documentElement.clientHeight;
        let tipX = event.clientX + offset;
        if (tipX + tooltipWidth + margin > viewportWidth) {{
          tipX = event.clientX - tooltipWidth - offset;
        }}
        tipX = Math.max(margin, Math.min(tipX, viewportWidth - tooltipWidth - margin));

        let tipY = event.clientY + offset;
        if (tipY + tooltipHeight + margin > viewportHeight) {{
          tipY = event.clientY - tooltipHeight - offset;
        }}
        tipY = Math.max(margin, Math.min(tipY, viewportHeight - tooltipHeight - margin));

        tooltip.style.transform = `translate3d(${{tipX}}px, ${{tipY}}px, 0)`;
      }});
    }}

    updateLegend();
    updatePhaseMap();
    updateStats();
    if (new URLSearchParams(window.location.search).get("embed") === "1") {{
      document.body.classList.add("embed");
    }}
    render();
  </script>
</body>
</html>
"""


def main() -> None:
    args = parse_args()
    input_path = Path(args.input)
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    rows = load_rows(input_path)
    html_text = render_html(args.title, input_path, rows, args.gap_seconds)
    output_path.write_text(html_text)


if __name__ == "__main__":
    main()
