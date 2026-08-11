#!/usr/bin/env python3
"""Export the active-alert preview matrix from the firmware renderer.

This script only orchestrates the Rust preview binary and composes review
sheets from the PNGs it produces. It never rasterizes a firmware screen itself.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


@dataclass(frozen=True)
class Entry:
    group: str
    title: str
    args: tuple[str, ...]


ROOT = Path(__file__).resolve().parents[3]
MANIFEST = ROOT / "tools" / "front-panel-preview" / "Cargo.toml"
BINARY = ROOT / "tools" / "front-panel-preview" / "target" / "debug" / "front-panel-preview"


def alert_detail_entries(state: str) -> list[Entry]:
    kinds = (
        "mains-absent-dc",
        "high-stress",
        "battery-low-no-mains",
        "battery-low-with-mains",
        "shutdown-protection",
        "io-over-voltage",
        "io-over-current",
        "module-fault",
        "battery-protection",
    )
    entries: list[Entry] = []
    for kind in kinds:
        args = ["--scenario", "alert-detail", "--alert-kind", kind]
        if state == "active":
            args.extend(["--alert-sound", "audible"])
        elif state == "muted":
            args.extend(["--alert-sound", "muted"])
        else:
            args.append("--alert-cleared")
        entries.append(Entry(f"details-{state}", f"{kind} / {state}", tuple(args)))
    return entries


def matrix() -> list[Entry]:
    entries = [
        Entry("dashboard", "No active alert", ("--scenario", "dashboard-runtime-standby")),
        Entry(
            "dashboard",
            "Warning audible / white phase",
            ("--scenario", "dashboard-alert", "--alert-severity", "warning", "--alert-sound", "audible", "--frame-no", "0"),
        ),
        Entry(
            "dashboard",
            "Warning audible / severity phase",
            ("--scenario", "dashboard-alert", "--alert-severity", "warning", "--alert-sound", "audible", "--frame-no", "1"),
        ),
        Entry(
            "dashboard",
            "Critical audible / white phase",
            ("--scenario", "dashboard-alert", "--alert-severity", "critical", "--alert-sound", "audible", "--frame-no", "0"),
        ),
        Entry(
            "dashboard",
            "Critical audible / severity phase",
            ("--scenario", "dashboard-alert", "--alert-severity", "critical", "--alert-sound", "audible", "--frame-no", "1"),
        ),
        Entry(
            "dashboard",
            "Muted / static",
            ("--scenario", "dashboard-alert", "--alert-severity", "warning", "--alert-sound", "muted"),
        ),
        Entry(
            "dashboard",
            "System silent / static",
            ("--scenario", "dashboard-alert", "--alert-severity", "critical", "--alert-sound", "system-silent"),
        ),
        Entry(
            "dashboard",
            "Policy silent / static",
            ("--scenario", "dashboard-alert", "--alert-severity", "warning", "--alert-sound", "policy-silent"),
        ),
        Entry(
            "dashboard",
            "Mixed / highest critical audible",
            ("--scenario", "dashboard-alert", "--alert-mixed", "--frame-no", "3"),
        ),
        Entry(
            "hotspots",
            "Dashboard touch zones / A = Alerts",
            ("--scenario", "dashboard-alert", "--alert-severity", "warning", "--alert-sound", "audible", "--alert-touch-zones"),
        ),
        Entry("list", "Empty", ("--scenario", "alert-list", "--alert-list", "empty")),
        Entry(
            "list",
            "Single / audible",
            ("--scenario", "alert-list", "--alert-list", "single", "--alert-sound", "audible"),
        ),
        Entry(
            "list",
            "Single / muted",
            ("--scenario", "alert-list", "--alert-list", "single", "--alert-sound", "muted"),
        ),
        Entry("list", "Mixed / selection", ("--scenario", "alert-list", "--alert-list", "mixed", "--alert-selected", "1")),
        Entry(
            "list",
            "Overflow / top",
            ("--scenario", "alert-list", "--alert-list", "overflow", "--alert-selected", "0", "--alert-top", "0"),
        ),
        Entry(
            "list",
            "Overflow / middle",
            ("--scenario", "alert-list", "--alert-list", "overflow", "--alert-selected", "4", "--alert-top", "3"),
        ),
        Entry(
            "list",
            "Overflow / end",
            ("--scenario", "alert-list", "--alert-list", "overflow", "--alert-selected", "8", "--alert-top", "6"),
        ),
        Entry(
            "hotspots",
            "Alert list / rows = Detail, M = Mute",
            ("--scenario", "alert-list", "--alert-list", "overflow", "--alert-touch-zones"),
        ),
        Entry(
            "detail-touch",
            "Detail touch zones",
            ("--scenario", "alert-detail", "--alert-kind", "mains-absent-dc", "--alert-touch-zones"),
        ),
    ]
    entries.extend(alert_detail_entries("active"))
    entries.extend(alert_detail_entries("muted"))
    entries.extend(alert_detail_entries("cleared"))
    return entries


def font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for candidate in (
        "/System/Library/Fonts/Supplemental/Menlo.ttc",
        "/System/Library/Fonts/Supplemental/Courier New Bold.ttf",
    ):
        if Path(candidate).exists():
            return ImageFont.truetype(candidate, size)
    return ImageFont.load_default()


def make_sheet(title: str, images: list[tuple[str, Path]], destination: Path) -> None:
    columns = 2 if len(images) <= 9 else 3
    margin = 22
    gap = 14
    label_h = 30
    screen_w, screen_h = 320, 172
    card_w = screen_w + 16
    card_h = label_h + screen_h + 14
    rows = (len(images) + columns - 1) // columns
    canvas = Image.new(
        "RGB",
        (
            margin * 2 + columns * card_w + (columns - 1) * gap,
            margin * 2 + 36 + rows * card_h + (rows - 1) * gap,
        ),
        (21, 25, 32),
    )
    draw = ImageDraw.Draw(canvas)
    draw.text((margin, margin), title, fill=(238, 242, 246), font=font(17))
    for index, (label, path) in enumerate(images):
        col = index % columns
        row = index // columns
        x = margin + col * (card_w + gap)
        y = margin + 36 + row * (card_h + gap)
        draw.rectangle((x, y, x + card_w, y + card_h), fill=(239, 241, 245), outline=(204, 211, 220))
        draw.text((x + 8, y + 7), label, fill=(18, 24, 32), font=font(12))
        canvas.paste(Image.open(path).convert("RGB"), (x + 8, y + label_h))
    canvas.save(destination)


def render_entry(entry: Entry, output: Path) -> Path:
    before = set(output.rglob("preview.png"))
    started_at_ns = time.time_ns()
    command = [
        str(BINARY),
        "--variant",
        "B",
        "--focus",
        "idle",
        "--mode",
        "standby",
        *entry.args,
        "--out-dir",
        str(output),
    ]
    subprocess.run(command, cwd=ROOT, check=True)
    created = set(output.rglob("preview.png")) - before
    if len(created) != 1:
        updated = {
            path
            for path in output.rglob("preview.png")
            if path.stat().st_mtime_ns >= started_at_ns
        }
        if len(updated) == 1:
            return updated.pop()
        raise RuntimeError(
            f"expected one new or updated preview for {entry.title}, "
            f"found {len(created)} new and {len(updated)} updated"
        )
    return created.pop()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    output = args.out_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)

    subprocess.run(["cargo", "build", "--quiet", "--manifest-path", str(MANIFEST)], cwd=ROOT, check=True)
    rendered: list[tuple[Entry, Path]] = []
    for entry in matrix():
        rendered.append((entry, render_entry(entry, output)))

    # The renderer owns this output tree. Remove preview/framebuffer pairs left by
    # older matrices so the checked-in manifest remains a complete inventory.
    rendered_paths = {path.resolve() for _, path in rendered}
    for preview in output.rglob("preview.png"):
        if preview.resolve() in rendered_paths:
            continue
        preview.unlink()
        framebuffer = preview.with_name("framebuffer.bin")
        if framebuffer.exists():
            framebuffer.unlink()

    grouped: dict[str, list[tuple[str, Path]]] = {}
    for entry, path in rendered:
        grouped.setdefault(entry.group, []).append((entry.title, path))
    review_dir = output / "review-sheets"
    review_dir.mkdir(exist_ok=True)
    for group, images in grouped.items():
        make_sheet(f"Active Alert Muting / {group}", images, review_dir / f"{group}.png")

    source_revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    manifest = []
    for entry, path in rendered:
        framebuffer = path.with_name("framebuffer.bin")
        with Image.open(path) as image:
            png_dimensions = list(image.size)
        manifest.append(
            {
                "group": entry.group,
                "title": entry.title,
                "renderer": "firmware_preview",
                "source_revision": source_revision,
                "args": list(entry.args),
                "preview": str(path.relative_to(output)),
                "preview_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "framebuffer": str(framebuffer.relative_to(output)),
                "framebuffer_sha256": hashlib.sha256(framebuffer.read_bytes()).hexdigest(),
                "framebuffer_bytes": framebuffer.stat().st_size,
                "png_dimensions": png_dimensions,
            }
        )
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
