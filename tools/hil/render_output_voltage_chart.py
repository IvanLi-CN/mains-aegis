#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Iterable

from PIL import Image, ImageDraw, ImageFont


SERIES = (
    ("out_a_vbus_mv", "OUT A", (11, 132, 243)),
    ("out_b_vbus_mv", "OUT B", (43, 182, 115)),
    ("load_v_local_mv", "LoadLynx v_local", (243, 156, 18)),
)

TAG_COLORS = {
    "pre": (244, 244, 246, 255),
    "hold": (226, 239, 255, 255),
    "backup": (255, 232, 232, 255),
    "restore": (229, 245, 231, 255),
    "post": (244, 244, 246, 255),
}

STAGE_LABELS = {
    "standby": "standby",
    "assist_low": "assist_low",
    "assist_rated": "assist_rated",
    "backup": "backup",
}

FONT_CANDIDATES = (
    "/System/Library/Fonts/Supplemental/Menlo.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render an annotated output-voltage chart from HIL timeseries.jsonl."
    )
    parser.add_argument("--input", required=True, help="Path to timeseries.jsonl")
    parser.add_argument("--output", required=True, help="Path to output PNG")
    parser.add_argument("--title", required=True)
    parser.add_argument("--subtitle", default="")
    parser.add_argument("--time-window-start", type=float, default=None)
    parser.add_argument("--time-window-end", type=float, default=None)
    parser.add_argument("--width", type=int, default=1800)
    parser.add_argument("--height", type=int, default=1080)
    parser.add_argument(
        "--gap-seconds",
        type=float,
        default=0.5,
        help="Break the trace when adjacent samples are farther apart than this.",
    )
    return parser.parse_args()


def load_rows(path: Path) -> list[dict]:
    rows = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    if not rows:
        raise SystemExit(f"no samples found in {path}")
    return rows


def clip_rows(
    rows: list[dict], start: float | None, end: float | None
) -> list[dict]:
    clipped = []
    for row in rows:
        t_s = row.get("t_s")
        if not isinstance(t_s, (int, float)):
            continue
        if start is not None and t_s < start:
            continue
        if end is not None and t_s > end:
            continue
        clipped.append(row)
    if not clipped:
        raise SystemExit("no samples left after time-window clipping")
    return clipped


def load_font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for candidate in FONT_CANDIDATES:
        try:
            return ImageFont.truetype(candidate, size=size)
        except OSError:
            continue
    return ImageFont.load_default()


def measure(draw: ImageDraw.ImageDraw, text: str, font: ImageFont.ImageFont) -> tuple[int, int]:
    bbox = draw.textbbox((0, 0), text, font=font)
    return bbox[2] - bbox[0], bbox[3] - bbox[1]


def nice_step(span: float, target_ticks: int) -> float:
    if span <= 0:
        return 1.0
    rough = span / max(1, target_ticks)
    exponent = math.floor(math.log10(rough))
    fraction = rough / (10**exponent)
    for candidate in (1.0, 2.0, 2.5, 5.0, 10.0):
        if fraction <= candidate:
            return candidate * (10**exponent)
    return 10.0 * (10**exponent)


def ceil_to(value: float, step: float) -> float:
    return math.ceil(value / step) * step


def floor_to(value: float, step: float) -> float:
    return math.floor(value / step) * step


def build_tag_spans(rows: list[dict]) -> list[dict]:
    spans: list[dict] = []
    current: dict | None = None
    for row in rows:
        tag = row.get("tag") or "unknown"
        t_s = row["t_s"]
        target_ma = row.get("load_target_i_ma")
        if current is None or current["tag"] != tag:
            current = {
                "tag": tag,
                "start": t_s,
                "end": t_s,
                "targets": [target_ma] if isinstance(target_ma, (int, float)) else [],
            }
            spans.append(current)
        else:
            current["end"] = t_s
            if isinstance(target_ma, (int, float)):
                current["targets"].append(target_ma)
    return spans


def build_stage_transitions(rows: list[dict]) -> list[dict]:
    transitions: list[dict] = []
    last_stage = last_mode = None
    for row in rows:
        stage = row.get("stage")
        mode = row.get("mode")
        if stage != last_stage or mode != last_mode:
            transitions.append(
                {
                    "t_s": row["t_s"],
                    "stage": stage,
                    "mode": mode,
                }
            )
            last_stage = stage
            last_mode = mode
    return transitions


def volts(values_mv: Iterable[float]) -> list[float]:
    return [value / 1000.0 for value in values_mv]


def value_or_none(row: dict, key: str) -> float | None:
    value = row.get(key)
    if isinstance(value, (int, float)):
        return float(value)
    return None


def min_sample(rows: list[dict], key: str) -> dict | None:
    best = None
    for row in rows:
        value = value_or_none(row, key)
        if value is None:
            continue
        if best is None or value < best[key]:
            best = dict(row)
            best[key] = value
    return best


def stage_label(stage: str | None) -> str:
    if not stage:
        return "unknown"
    return STAGE_LABELS.get(stage, stage)


def tag_label(span: dict) -> str:
    tag = span["tag"]
    targets = [value for value in span["targets"] if value is not None]
    target_ma = None
    if targets:
        target_ma = int(round(sum(targets) / len(targets)))
    if tag == "pre":
        return "pre / idle"
    if tag == "hold":
        return f"hold / {target_ma}mA" if target_ma else "hold"
    if tag == "backup":
        return "backup / input cut"
    if tag == "restore":
        return f"restore / {target_ma}mA" if target_ma else "restore / input on"
    if tag == "post":
        return "post / unload"
    return tag


def time_to_x(t_s: float, x0: int, x1: int, t_min: float, t_max: float) -> float:
    if t_max <= t_min:
        return float(x0)
    return x0 + (t_s - t_min) * (x1 - x0) / (t_max - t_min)


def value_to_y(value_v: float, y0: int, y1: int, v_min: float, v_max: float) -> float:
    if v_max <= v_min:
        return float(y1)
    return y1 - (value_v - v_min) * (y1 - y0) / (v_max - v_min)


def draw_polyline(
    draw: ImageDraw.ImageDraw,
    rows: list[dict],
    key: str,
    color: tuple[int, int, int],
    bounds: tuple[int, int, int, int],
    t_min: float,
    t_max: float,
    v_min: float,
    v_max: float,
    gap_seconds: float,
) -> None:
    x0, y0, x1, y1 = bounds
    segment: list[tuple[float, float]] = []
    previous_t: float | None = None
    for row in rows:
        value = value_or_none(row, key)
        if value is None:
            if len(segment) >= 2:
                draw.line(segment, fill=color, width=3, joint="curve")
            segment = []
            previous_t = None
            continue
        current_t = float(row["t_s"])
        point = (
            time_to_x(current_t, x0, x1, t_min, t_max),
            value_to_y(value / 1000.0, y0, y1, v_min, v_max),
        )
        if previous_t is not None and current_t - previous_t > gap_seconds:
            if len(segment) >= 2:
                draw.line(segment, fill=color, width=3, joint="curve")
            segment = []
        segment.append(point)
        previous_t = current_t
    if len(segment) >= 2:
        draw.line(segment, fill=color, width=3, joint="curve")

    for row in rows:
        value = value_or_none(row, key)
        if value is None:
            continue
        x = time_to_x(float(row["t_s"]), x0, x1, t_min, t_max)
        y = value_to_y(value / 1000.0, y0, y1, v_min, v_max)
        draw.ellipse((x - 2, y - 2, x + 2, y + 2), fill=color)


def main() -> None:
    args = parse_args()
    source_path = Path(args.input)
    rows = clip_rows(
        load_rows(source_path),
        args.time_window_start,
        args.time_window_end,
    )
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    title_font = load_font(36)
    subtitle_font = load_font(22)
    body_font = load_font(20)
    small_font = load_font(18)

    image = Image.new("RGBA", (args.width, args.height), (255, 255, 255, 255))
    draw = ImageDraw.Draw(image)

    left = 130
    right = args.width - 60
    top = 170
    bottom = args.height - 160
    plot_bounds = (left, top, right, bottom)
    x0, y0, x1, y1 = plot_bounds

    t_min = float(rows[0]["t_s"])
    t_max = float(rows[-1]["t_s"])

    output_values_v = []
    for key, _, _ in SERIES:
        output_values_v.extend(
            volts(
                value
                for value in (value_or_none(row, key) for row in rows)
                if value is not None
            )
        )
    if not output_values_v:
        raise SystemExit("no output-voltage series found")
    value_span = max(output_values_v) - min(output_values_v)
    pad = max(0.08, value_span * 0.08)
    v_min = floor_to(min(output_values_v) - pad, 0.1)
    v_max = ceil_to(max(output_values_v) + pad, 0.1)

    draw.text((left, 48), args.title, font=title_font, fill=(20, 20, 20))
    if args.subtitle:
        draw.text((left, 94), args.subtitle, font=subtitle_font, fill=(90, 90, 90))

    for span in build_tag_spans(rows):
        sx0 = time_to_x(span["start"], x0, x1, t_min, t_max)
        sx1 = time_to_x(span["end"], x0, x1, t_min, t_max)
        color = TAG_COLORS.get(span["tag"], (240, 240, 240, 255))
        overlay = Image.new("RGBA", image.size, (255, 255, 255, 0))
        overlay_draw = ImageDraw.Draw(overlay)
        overlay_draw.rectangle((sx0, y0, sx1, y1), fill=color)
        image.alpha_composite(overlay)
        draw = ImageDraw.Draw(image)
        label = tag_label(span)
        label_w, label_h = measure(draw, label, small_font)
        label_x = max(x0 + 4, min((sx0 + sx1 - label_w) / 2, x1 - label_w - 4))
        draw.rounded_rectangle(
            (
                label_x - 8,
                y0 + 8,
                label_x + label_w + 8,
                y0 + label_h + 16,
            ),
            radius=10,
            fill=(255, 255, 255, 210),
            outline=(210, 210, 210),
            width=1,
        )
        draw.text((label_x, y0 + 12), label, font=small_font, fill=(40, 40, 40))

    tick_step_v = nice_step(v_max - v_min, 6)
    tick_v = floor_to(v_min, tick_step_v)
    while tick_v <= v_max + 1e-9:
        y = value_to_y(tick_v, y0, y1, v_min, v_max)
        draw.line((x0, y, x1, y), fill=(230, 230, 230), width=1)
        label = f"{tick_v:.2f}V"
        lw, lh = measure(draw, label, body_font)
        draw.text((left - lw - 16, y - lh / 2), label, font=body_font, fill=(90, 90, 90))
        tick_v += tick_step_v

    tick_step_t = nice_step(t_max - t_min, 8)
    tick_t = ceil_to(t_min, tick_step_t)
    while tick_t <= t_max + 1e-9:
        x = time_to_x(tick_t, x0, x1, t_min, t_max)
        draw.line((x, y0, x, y1), fill=(236, 236, 236), width=1)
        label = f"{tick_t:.0f}s"
        lw, lh = measure(draw, label, body_font)
        draw.text((x - lw / 2, y1 + 14), label, font=body_font, fill=(90, 90, 90))
        tick_t += tick_step_t

    draw.rectangle(plot_bounds, outline=(140, 140, 140), width=2)

    for key, _, color in SERIES:
        draw_polyline(
            draw,
            rows,
            key,
            color,
            plot_bounds,
            t_min,
            t_max,
            v_min,
            v_max,
            args.gap_seconds,
        )

    transitions = build_stage_transitions(rows)
    previous_label_x = -10_000.0
    for index, transition in enumerate(transitions):
        tx = time_to_x(transition["t_s"], x0, x1, t_min, t_max)
        draw.line((tx, y0, tx, y1), fill=(120, 120, 120), width=1)
        label = stage_label(transition["stage"])
        label_w, label_h = measure(draw, label, small_font)
        label_y = y0 + 48 + (index % 3) * (label_h + 8)
        label_x = tx + 6
        if label_x - previous_label_x < 110:
            label_y += 42
        label_x = min(label_x, x1 - label_w - 8)
        draw.rounded_rectangle(
            (
                label_x - 5,
                label_y - 3,
                label_x + label_w + 5,
                label_y + label_h + 3,
            ),
            radius=8,
            fill=(255, 255, 255, 230),
            outline=(180, 180, 180),
            width=1,
        )
        draw.text((label_x, label_y), label, font=small_font, fill=(55, 55, 55))
        previous_label_x = label_x

    legend_x = left
    legend_y = args.height - 120
    for key, label, color in SERIES:
        draw.line((legend_x, legend_y + 12, legend_x + 40, legend_y + 12), fill=color, width=5)
        draw.text((legend_x + 52, legend_y), label, font=body_font, fill=(40, 40, 40))
        legend_x += 290

    ups_a_min = min_sample(rows, "out_a_vbus_mv")
    ups_b_min = min_sample(rows, "out_b_vbus_mv")
    load_min = min_sample(rows, "load_v_local_mv")
    restore_low = None
    restore_rows = [row for row in rows if row.get("tag") == "restore"]
    if restore_rows:
        restore_low = min_sample(restore_rows, "out_a_vbus_mv")

    footer_lines = []
    if ups_a_min and ups_b_min:
        footer_lines.append(
            "UPS min: "
            f"OUT A {ups_a_min['out_a_vbus_mv'] / 1000.0:.3f}V @ {ups_a_min['t_s']:.3f}s, "
            f"OUT B {ups_b_min['out_b_vbus_mv'] / 1000.0:.3f}V @ {ups_b_min['t_s']:.3f}s"
        )
    if load_min:
        footer_lines.append(
            f"LoadLynx local min: {load_min['load_v_local_mv'] / 1000.0:.3f}V @ {load_min['t_s']:.3f}s"
        )
    if restore_low:
        footer_lines.append(
            "Restore dip: "
            f"OUT A {restore_low['out_a_vbus_mv'] / 1000.0:.3f}V @ {restore_low['t_s']:.3f}s "
            f"during {stage_label(restore_low.get('stage'))}"
        )
    footer_lines.append(
        f"Observed-sample plot only; lines break at gaps > {args.gap_seconds:.1f}s"
    )
    footer_lines.append(f"Source: {source_path}")

    footer_y = args.height - 84
    for line in footer_lines:
        draw.text((left, footer_y), line, font=small_font, fill=(85, 85, 85))
        footer_y += 22

    image.convert("RGB").save(output_path)


if __name__ == "__main__":
    main()
