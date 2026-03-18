#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import textwrap
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


@dataclass(frozen=True)
class IconSpec:
    section: str
    label: str
    const_name: str
    provenance: str


ICON_SPECS: tuple[IconSpec, ...] = (
    IconSpec(
        "Source",
        "USB-C input",
        "CARBON_USB_C_OUTLINE_24",
        "Iconify / mdi:usb-c-port",
    ),
    IconSpec(
        "Source",
        "DC5025 input",
        "CARBON_DC_BARREL_OUTLINE_24",
        "Iconify / mdi:audio-input-stereo-minijack",
    ),
    IconSpec(
        "Source",
        "Charge battery",
        "RI_BATTERY_CHARGE_LINE_24",
        "Iconify / ri:battery-charge-line",
    ),
    IconSpec(
        "Source",
        "Idle battery",
        "RI_BATTERY_LINE_24",
        "Iconify / ri:battery-line",
    ),
    IconSpec(
        "Thermal",
        "Fan frame A",
        "CARBON_FAN_OUTLINE_CARDINAL_24",
        "Iconify / material-symbols-light:mode-fan-outline",
    ),
    IconSpec(
        "Thermal",
        "Fan frame B",
        "CARBON_FAN_OUTLINE_DIAGONAL_24",
        "Iconify / material-symbols-light:mode-fan-outline (rotated 45deg, scaled 1.08x)",
    ),
    IconSpec(
        "Footer",
        "Live data",
        "CARBON_CHECKMARK_OUTLINE_18",
        "Iconify / carbon:checkmark-outline",
    ),
    IconSpec(
        "Footer",
        "Mock data",
        "CARBON_CHECKBOX_INDETERMINATE_18",
        "Iconify / carbon:checkbox-indeterminate",
    ),
    IconSpec(
        "Footer",
        "Warning",
        "CARBON_WARNING_ALT_18",
        "Iconify / carbon:warning-alt",
    ),
    IconSpec(
        "Footer",
        "Fault",
        "CARBON_ERROR_OUTLINE_18",
        "Iconify / carbon:error-outline",
    ),
    IconSpec(
        "Footer",
        "No data / source next",
        "CARBON_HELP_18",
        "Iconify / carbon:help",
    ),
)


SECTION_COLORS = {
    "Source": (212, 175, 127),
    "Thermal": (225, 198, 135),
    "Footer": (168, 191, 216),
}

PANEL_BG = (28, 38, 54)
PANEL_BORDER = (70, 84, 104)
CANVAS_BG = (18, 23, 32)
TEXT_PRIMARY = (238, 242, 246)
TEXT_SECONDARY = (132, 146, 162)
ICON_COLOR = (24, 29, 34)
ICON_CARD_BG = (245, 246, 248)
ICON_CARD_BORDER = (200, 205, 212)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def load_source() -> str:
    scene = repo_root() / "firmware" / "src" / "front_panel_scene.rs"
    return scene.read_text(encoding="utf-8")


def parse_blocks(source: str, const_name: str) -> list[tuple[int, int, int, int]]:
    pattern = re.compile(
        rf"const\s+{re.escape(const_name)}:\s*&\[\(u8, u8, u8, u8\)\]\s*=\s*&\[(.*?)\];",
        re.S,
    )
    match = pattern.search(source)
    if not match:
        raise ValueError(f"icon constant not found: {const_name}")

    blocks: list[tuple[int, int, int, int]] = []
    for ax, ay, aw, ah in re.findall(r"\((\d+),\s*(\d+),\s*(\d+),\s*(\d+)\)", match.group(1)):
        blocks.append((int(ax), int(ay), int(aw), int(ah)))
    if not blocks:
        raise ValueError(f"icon constant has no blocks: {const_name}")
    return blocks


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


def draw_icon_preview(
    draw: ImageDraw.ImageDraw,
    blocks: list[tuple[int, int, int, int]],
    x1: int,
    y1: int,
    x2: int,
    y2: int,
) -> None:
    draw.rounded_rectangle(
        (x1, y1, x2, y2),
        radius=12,
        fill=ICON_CARD_BG,
        outline=ICON_CARD_BORDER,
        width=1,
    )
    min_x = min(bx for bx, _, _, _ in blocks)
    min_y = min(by for _, by, _, _ in blocks)
    max_x = max(bx + bw for bx, _, bw, _ in blocks)
    max_y = max(by + bh for _, by, _, bh in blocks)
    icon_w = max_x - min_x
    icon_h = max_y - min_y
    inner_w = x2 - x1 - 24
    inner_h = y2 - y1 - 24
    scale = max(2, min(inner_w // icon_w, inner_h // icon_h))
    ox = x1 + ((x2 - x1) - icon_w * scale) // 2 - min_x * scale
    oy = y1 + ((y2 - y1) - icon_h * scale) // 2 - min_y * scale

    for bx, by, bw, bh in blocks:
        draw.rectangle(
            (
                ox + bx * scale,
                oy + by * scale,
                ox + (bx + bw) * scale - 1,
                oy + (by + bh) * scale - 1,
            ),
            fill=ICON_COLOR,
        )


def wrap_lines(text: str, width: int) -> str:
    return "\n".join(textwrap.wrap(text, width=width, break_long_words=False))


def render_sheet(output: Path) -> None:
    source = load_source()

    cols = 2
    card_w = 480
    card_h = 176
    gap = 20
    margin = 32
    header_h = 88
    rows = (len(ICON_SPECS) + cols - 1) // cols
    width = margin * 2 + cols * card_w + (cols - 1) * gap
    height = header_h + rows * card_h + (rows - 1) * gap + 40

    image = Image.new("RGB", (width, height), CANVAS_BG)
    draw = ImageDraw.Draw(image)
    font_title = load_font(30)
    font_subtitle = load_font(16)
    font_section = load_font(16)
    font_label = load_font(22)
    font_meta = load_font(14)
    font_const = load_font(13)

    draw.text((margin, 18), "Dashboard Detail Icons", fill=TEXT_PRIMARY, font=font_title)
    draw.text(
        (margin, 54),
        "Auto-generated from firmware/src/front_panel_scene.rs",
        fill=TEXT_SECONDARY,
        font=font_subtitle,
    )

    for index, spec in enumerate(ICON_SPECS):
        x = margin + (index % cols) * (card_w + gap)
        y = header_h + (index // cols) * (card_h + gap)
        draw.rounded_rectangle(
            (x, y, x + card_w, y + card_h),
            radius=18,
            fill=PANEL_BG,
            outline=PANEL_BORDER,
            width=2,
        )
        draw.rounded_rectangle(
            (x + 12, y + 12, x + card_w - 12, y + 38),
            radius=10,
            fill=SECTION_COLORS[spec.section],
        )
        draw.text((x + 24, y + 17), spec.section.upper(), fill=(20, 25, 30), font=font_section)

        blocks = parse_blocks(source, spec.const_name)
        draw_icon_preview(
            draw,
            blocks,
            x + 24,
            y + 56,
            x + 126,
            y + 128,
        )
        draw.text((x + 148, y + 68), spec.label, fill=TEXT_PRIMARY, font=font_label)
        draw.multiline_text(
            (x + 148, y + 100),
            wrap_lines(spec.provenance, 34),
            fill=TEXT_SECONDARY,
            font=font_meta,
            spacing=4,
        )
        draw.multiline_text(
            (x + 148, y + 132),
            wrap_lines(spec.const_name, 28),
            fill=TEXT_SECONDARY,
            font=font_const,
            spacing=3,
        )

    output.parent.mkdir(parents=True, exist_ok=True)
    image.save(output)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Render a contact sheet for all currently used dashboard detail icons."
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("/tmp/mains-aegis-dashboard-detail-icons-sheet.png"),
        help="Output PNG path.",
    )
    args = parser.parse_args()
    render_sheet(args.out)
    print(args.out)


if __name__ == "__main__":
    main()
