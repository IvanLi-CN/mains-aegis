#!/usr/bin/env python3
from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


@dataclass(frozen=True)
class ReviewImage:
    title: str
    path: Path


CANVAS_BG = (24, 28, 36)
CARD_BG = (239, 241, 245)
CARD_BORDER = (205, 211, 220)
TEXT = (18, 24, 32)
SUBTLE = (90, 102, 118)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def load_font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    candidates = (
        "/System/Library/Fonts/Supplemental/Menlo.ttc",
        "/System/Library/Fonts/Supplemental/Courier New Bold.ttf",
    )
    for candidate in candidates:
        path = Path(candidate)
        if path.exists():
            return ImageFont.truetype(str(path), size)
    return ImageFont.load_default()


def review_images() -> tuple[ReviewImage, ...]:
    base = repo_root() / "firmware" / "ui" / "assets"
    return (
        ReviewImage("Home", base / "dashboard-b-detail-home.png"),
        ReviewImage("Cells", base / "dashboard-b-detail-cells.png"),
        ReviewImage("Battery Flow", base / "dashboard-b-detail-battery-flow.png"),
        ReviewImage("Output", base / "dashboard-b-detail-output.png"),
        ReviewImage("Charger", base / "dashboard-b-detail-charger.png"),
        ReviewImage("Thermal", base / "dashboard-b-detail-thermal.png"),
    )


def render_review_set(output: Path) -> None:
    items = review_images()
    cols = 2
    margin = 28
    gap = 16
    header_h = 52
    label_h = 34
    screenshot_w = 320
    screenshot_h = 172
    card_w = screenshot_w + 28
    card_h = label_h + screenshot_h + 18
    rows = (len(items) + cols - 1) // cols

    canvas_w = margin * 2 + cols * card_w + (cols - 1) * gap
    canvas_h = margin * 2 + header_h + rows * card_h + (rows - 1) * gap

    image = Image.new("RGB", (canvas_w, canvas_h), CANVAS_BG)
    draw = ImageDraw.Draw(image)
    font_title = load_font(18)
    font_label = load_font(14)
    font_subtle = load_font(12)

    draw.text((margin, margin - 2), "Dashboard Review Set", fill=(240, 244, 248), font=font_title)
    draw.text(
        (margin, margin + 20),
        "Auto-generated from firmware/ui/assets",
        fill=SUBTLE,
        font=font_subtle,
    )

    for index, item in enumerate(items):
        col = index % cols
        row = index // cols
        x = margin + col * (card_w + gap)
        y = margin + header_h + row * (card_h + gap)

        draw.rounded_rectangle(
            (x, y, x + card_w, y + card_h),
            radius=16,
            fill=CARD_BG,
            outline=CARD_BORDER,
            width=1,
        )
        draw.text((x + 16, y + 12), item.title, fill=TEXT, font=font_label)

        preview = Image.open(item.path).convert("RGB")
        image.paste(preview, (x + 14, y + label_h))

    output.parent.mkdir(parents=True, exist_ok=True)
    image.save(output)


def main() -> None:
    parser = argparse.ArgumentParser(description="Render the dashboard review contact sheet.")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("/tmp/mains-aegis-dashboard-review-set.png"),
        help="Output PNG path.",
    )
    args = parser.parse_args()
    render_review_set(args.out)
    print(args.out)


if __name__ == "__main__":
    main()
