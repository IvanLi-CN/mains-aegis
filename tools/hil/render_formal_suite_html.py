#!/usr/bin/env python3
from __future__ import annotations

import argparse
import html
import json
import os
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render one overview HTML for the dual-voltage formal HIL suite."
    )
    parser.add_argument("--summary", required=True, help="Suite summary JSON path.")
    parser.add_argument("--output", required=True, help="Output HTML path.")
    return parser.parse_args()


def load_json(path: Path) -> Any:
    return json.loads(path.read_text())


def rel_href(from_path: Path, target_path: Path) -> str:
    return os.path.relpath(target_path.resolve(), from_path.resolve().parent).replace(
        os.sep, "/"
    )


def status_class(entry: dict[str, Any]) -> str:
    if entry.get("scene_complete") and entry.get("signoff_valid") is True:
        return "valid"
    return "invalid"


def render_card(entry: dict[str, Any], *, summary_path: Path, output_path: Path) -> str:
    report_dir = Path(entry["report_dir"])
    if not report_dir.is_absolute():
        report_dir = (summary_path.parent / report_dir).resolve()
    chart_path = report_dir / "voltage-chart.html"
    chart_href = f"{rel_href(output_path, chart_path)}?embed=1"
    profile = html.escape(str(entry.get("output_profile") or "n/a"))
    scene = html.escape(str(entry.get("scene_type") or "n/a"))
    target_ma = html.escape(str(entry.get("target_ma") or "n/a"))
    source_voltage_mv = html.escape(str(entry.get("source_voltage_mv") or "n/a"))
    source_current_limit_ma = html.escape(str(entry.get("source_current_limit_ma") or "n/a"))
    signoff = html.escape(str(entry.get("run_validity") or "n/a"))
    sample_rate = html.escape(str(entry.get("effective_sample_rate_hz") or "n/a"))
    max_gap = html.escape(str(entry.get("max_sample_gap_s") or "n/a"))
    failures = ", ".join(entry.get("failures") or []) or "none"
    acceptance_failures = ", ".join(entry.get("failed_acceptance_checks") or []) or "none"
    status = status_class(entry)
    status_text = "valid_for_signoff" if status == "valid" else "diagnostic_only"
    advanced_power = entry.get("advanced_power") or {}
    advanced_power_summary = ", ".join(
        f"{key}={value}" for key, value in advanced_power.items()
    ) or "n/a"
    report_dir_text = html.escape(str(report_dir))
    return f"""
    <section class="card {status}">
      <div class="card-header">
        <div>
          <h2>{profile} / {scene}</h2>
          <p>source={source_voltage_mv}mV / {source_current_limit_ma}mA | load={target_ma}mA</p>
        </div>
        <span class="pill">{status_text}</span>
      </div>
      <iframe src="{html.escape(chart_href)}" loading="lazy"></iframe>
      <div class="meta-grid">
        <div class="meta-item"><strong>Run validity</strong><span>{signoff}</span></div>
        <div class="meta-item"><strong>Scene complete</strong><span>{html.escape(str(entry.get("scene_complete")))}</span></div>
        <div class="meta-item"><strong>Sample rate</strong><span>{sample_rate}Hz</span></div>
        <div class="meta-item"><strong>Max gap</strong><span>{max_gap}s</span></div>
      </div>
      <p class="meta-text"><strong>Completeness failures:</strong> {html.escape(failures)}</p>
      <p class="meta-text"><strong>Acceptance failures:</strong> {html.escape(acceptance_failures)}</p>
      <p class="meta-text"><strong>Advanced power:</strong> {html.escape(advanced_power_summary)}</p>
      <p class="meta-text"><strong>Report dir:</strong> {report_dir_text}</p>
    </section>
    """


def render_profiles(payload: dict[str, Any]) -> str:
    profiles = payload.get("profiles") or {}
    blocks = []
    for key, info in profiles.items():
        features = ", ".join(info.get("artifact_features") or []) or "(none)"
        window = info.get("expected_source_window_mv") or {}
        blocks.append(
            f"""
            <div class="item">
              <strong>{html.escape(str(key))}</strong><br>
              source={html.escape(str(info.get("source_voltage_mv")))}mV /
              {html.escape(str(info.get("source_current_limit_ma")))}mA<br>
              guard={html.escape(str(window.get("min_mv")))}..{html.escape(str(window.get("max_mv")))}mV<br>
              features={html.escape(features)}
            </div>
            """
        )
    return "\n".join(blocks)


def render_html(summary_path: Path, output_path: Path, payload: dict[str, Any]) -> str:
    cards = "\n".join(
        render_card(entry, summary_path=summary_path, output_path=output_path)
        for entry in payload.get("reports") or []
    )
    suite_id = html.escape(str(payload.get("suite_id") or "formal-suite"))
    protection = payload.get("load_protection") or {}
    transport = payload.get("transport") or {}
    reports = payload.get("reports") or []
    valid_reports = sum(1 for report in reports if status_class(report) == "valid")
    profiles_html = render_profiles(payload)
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{suite_id}</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #f4f6f9;
      --panel: #ffffff;
      --text: #15202b;
      --muted: #5b6672;
      --line: #d6dde6;
      --ok: #dff6e7;
      --bad: #fde5e5;
      --accent: #0d6efd;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      padding: 20px;
      font: 16px/1.45 ui-monospace, SFMono-Regular, Menlo, monospace;
      color: var(--text);
      background: linear-gradient(180deg, #eef3f8 0%, #f8fafc 100%);
    }}
    .page {{
      max-width: 1700px;
      margin: 0 auto;
      display: grid;
      gap: 16px;
    }}
    .hero, .card {{
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 16px;
      padding: 16px;
      box-shadow: 0 10px 35px rgba(21, 32, 43, 0.06);
    }}
    .hero h1 {{ margin: 0 0 8px; font-size: 28px; }}
    .hero p {{ margin: 0; color: var(--muted); }}
    .meta {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 12px;
      margin-top: 16px;
    }}
    .meta .item {{
      background: #f8fafc;
      border: 1px solid var(--line);
      border-radius: 14px;
      padding: 12px 14px;
    }}
    .cards {{
      display: grid;
      gap: 20px;
      grid-template-columns: repeat(auto-fit, minmax(min(100%, 1000px), 1fr));
    }}
    .card.valid {{ border-color: #b9e5c8; }}
    .card.invalid {{ border-color: #efb7b7; }}
    .card-header {{
      display: flex;
      justify-content: space-between;
      align-items: flex-start;
      gap: 12px;
    }}
    .card h2 {{ margin: 0 0 4px; font-size: 22px; }}
    .card p {{ margin: 0; color: var(--muted); }}
    .pill {{
      padding: 6px 10px;
      border-radius: 999px;
      font-size: 12px;
      background: #eef4ff;
      color: var(--accent);
      border: 1px solid #cfe0ff;
    }}
    .meta-grid {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
      gap: 10px;
      margin-top: 14px;
    }}
    .meta-item {{
      background: #f8fafc;
      border: 1px solid var(--line);
      border-radius: 12px;
      padding: 10px 12px;
      display: grid;
      gap: 4px;
    }}
    .meta-item strong {{
      font-size: 12px;
      text-transform: uppercase;
      color: var(--muted);
    }}
    .meta-text {{
      margin-top: 10px !important;
      font-size: 13px;
      line-height: 1.5;
      color: var(--text) !important;
    }}
    iframe {{
      width: 100%;
      height: min(72vh, 760px);
      min-height: 560px;
      border: 1px solid var(--line);
      border-radius: 12px;
      margin-top: 10px;
      background: white;
    }}
  </style>
</head>
<body>
  <div class="page">
    <section class="hero">
      <h1>{suite_id}</h1>
      <p>Formal dual-voltage HIL suite overview. Four required scenes, one page, same sampling and protection contract.</p>
      <div class="meta">
        <div class="item">reports={len(reports)}</div>
        <div class="item">signoff_valid_reports={valid_reports}</div>
        <div class="item">load_uvp={html.escape(str(protection.get("min_v_mv") or "n/a"))}mV</div>
        <div class="item">load_ocp={html.escape(str(protection.get("max_i_ma_total") or "n/a"))}mA</div>
        <div class="item">load_opp={html.escape(str(protection.get("max_p_mw") or "n/a"))}mW</div>
        <div class="item">usb_port={html.escape(str(transport.get("load_usb_port") or "n/a"))}</div>
        <div class="item">ups_status_url={html.escape(str(transport.get("ups_status_url") or "n/a"))}</div>
        <div class="item">isolapurr_url={html.escape(str(transport.get("isolapurr_url") or "n/a"))}</div>
        {profiles_html}
      </div>
    </section>
    <div class="cards">
      {cards}
    </div>
  </div>
</body>
</html>
"""


def main() -> int:
    args = parse_args()
    summary_path = Path(args.summary).resolve()
    output_path = Path(args.output).resolve()
    payload = load_json(summary_path)
    output_path.write_text(render_html(summary_path, output_path, payload), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
