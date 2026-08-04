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


@dataclass(frozen=True)
class ReviewProfile:
    title: str
    subtitle: str
    images: tuple[ReviewImage, ...]


@dataclass(frozen=True)
class StoryboardStep:
    title: str
    note: str
    path: Path


@dataclass(frozen=True)
class StoryboardRow:
    title: str
    note: str
    steps: tuple[StoryboardStep, ...]


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


def detail_review_images() -> tuple[ReviewImage, ...]:
    base = repo_root() / "firmware" / "ui" / "assets"
    return (
        ReviewImage("Home", base / "dashboard-b-detail-home.png"),
        ReviewImage("Cells", base / "dashboard-b-detail-cells.png"),
        ReviewImage("Battery Flow", base / "dashboard-b-detail-battery-flow.png"),
        ReviewImage("Output", base / "dashboard-b-detail-output.png"),
        ReviewImage("Charger", base / "dashboard-b-detail-charger.png"),
        ReviewImage("Thermal", base / "dashboard-b-detail-thermal.png"),
    )


def menu_beeper_review_images() -> tuple[ReviewImage, ...]:
    base = repo_root() / "docs" / "specs" / "front-panel-industrial-ui-preview" / "assets"
    return (
        ReviewImage("Home Focus / Output", base / "dashboard-home-focus-output.png"),
        ReviewImage("Home Focus / Dischg", base / "dashboard-home-focus-battery-flow.png"),
        ReviewImage("Menu / Dashboard", base / "dashboard-menu-dashboard.png"),
        ReviewImage("Menu / Audio", base / "dashboard-menu-beeper.png"),
        ReviewImage("Audio / Action", base / "dashboard-audio-action-focus.png"),
        ReviewImage("Audio / System", base / "dashboard-audio-system-focus.png"),
        ReviewImage("Audio / System Off", base / "dashboard-audio-system-off.png"),
        ReviewImage("Transition / Mid", base / "dashboard-menu-transition-mid.png"),
        ReviewImage("Transition / End", base / "dashboard-menu-transition-end.png"),
    )


def menu_concept_review_images() -> tuple[ReviewImage, ...]:
    base = repo_root() / "docs" / "specs" / "front-panel-industrial-ui-preview" / "assets"
    return (
        ReviewImage("A. Plate", base / "dashboard-menu-concept-dense-badge.png"),
        ReviewImage("B. Bar", base / "dashboard-menu-concept-dock-bar.png"),
        ReviewImage("C. Pedestal", base / "dashboard-menu-concept-split-rail.png"),
        ReviewImage("D. Rail", base / "dashboard-menu-concept-signal-plate.png"),
    )


def review_profile(name: str) -> ReviewProfile:
    if name == "detail":
        return ReviewProfile(
            title="Dashboard Review Set",
            subtitle="Auto-generated from firmware/ui/assets",
            images=detail_review_images(),
        )
    if name == "menu-beeper":
        return ReviewProfile(
            title="Dashboard / Menu / Audio Preview Set",
            subtitle="Firmware renderer, 320x172 RGB565 1:1 scenes",
            images=menu_beeper_review_images(),
        )
    if name == "menu-concepts":
        return ReviewProfile(
            title="Dashboard Menu Clean Studies",
            subtitle="Four simplified footer directions rendered from the firmware scene",
            images=menu_concept_review_images(),
        )
    raise ValueError(f"unsupported profile: {name}")


def menu_beeper_storyboard_rows() -> tuple[StoryboardRow, ...]:
    base = repo_root() / "docs" / "specs" / "front-panel-industrial-ui-preview" / "assets"
    return (
        StoryboardRow(
            title="1. DOWN on the bottom card opens MENU",
            note="The dashboard and menu behave as one vertical stack, not as separate modes.",
            steps=(
                StoryboardStep(
                    "Battery Flow Focus",
                    "Bottom card is focused before the handoff.",
                    base / "dashboard-home-focus-battery-flow.png",
                ),
                StoryboardStep(
                    "Slide Transition",
                    "The 320x344 stack is mid-way through the 220 ms slide.",
                    base / "dashboard-menu-transition-mid.png",
                ),
                StoryboardStep(
                    "Menu Landed",
                    "Menu becomes the active page after the vertical slide settles.",
                    base / "dashboard-menu-dashboard.png",
                ),
            ),
        ),
        StoryboardRow(
            title="2. LEFT/RIGHT keeps the selected icon centered",
            note="The icon rail translates as a whole so the active item stays on center.",
            steps=(
                StoryboardStep(
                    "Dashboard Selected",
                    "Entry state centers DASHBOARD.",
                    base / "dashboard-menu-dashboard.png",
                ),
                StoryboardStep(
                    "Beeper Selected",
                    "Moving right recenters BEEPER without changing page layout.",
                    base / "dashboard-menu-beeper.png",
                ),
            ),
        ),
        StoryboardRow(
            title="3. CENTER opens AUDIO, UP backs out directly",
            note="Audio settings is a direct page transition, not a long scroll extension.",
            steps=(
                StoryboardStep(
                    "Menu / Audio",
                    "CENTER on AUDIO enters settings.",
                    base / "dashboard-menu-beeper.png",
                ),
                StoryboardStep(
                    "Audio Settings",
                    "Two controls exist: ACTION and SYSTEM, each = 0 + 1..6.",
                    base / "dashboard-audio-system-focus.png",
                ),
                StoryboardStep(
                    "Back Path",
                    "UP returns to the same centered AUDIO tile.",
                    base / "dashboard-menu-beeper.png",
                ),
            ),
        ),
    )


def render_review_set(output: Path, profile_name: str) -> None:
    profile = review_profile(profile_name)
    items = profile.images
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

    draw.text((margin, margin - 2), profile.title, fill=(240, 244, 248), font=font_title)
    draw.text(
        (margin, margin + 20),
        profile.subtitle,
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


def render_storyboard(output: Path) -> None:
    rows = menu_beeper_storyboard_rows()
    margin = 28
    section_gap = 26
    step_gap = 18
    header_h = 54
    row_title_h = 46
    screenshot_w = 320
    screenshot_h = 172
    note_h = 36
    card_pad = 12
    arrow_w = 22
    card_w = screenshot_w + card_pad * 2
    card_h = 28 + screenshot_h + note_h
    max_steps = max(len(row.steps) for row in rows)

    canvas_w = margin * 2 + max_steps * card_w + (max_steps - 1) * step_gap + (max_steps - 1) * arrow_w
    canvas_h = margin * 2 + header_h
    for row in rows:
        canvas_h += row_title_h + card_h + section_gap
    canvas_h -= section_gap

    image = Image.new("RGB", (canvas_w, canvas_h), CANVAS_BG)
    draw = ImageDraw.Draw(image)
    font_title = load_font(18)
    font_subtle = load_font(12)
    font_row = load_font(16)
    font_card = load_font(13)
    font_note = load_font(11)

    draw.text((margin, margin - 2), "Dashboard / Menu / Audio Interaction Storyboard", fill=(240, 244, 248), font=font_title)
    draw.text(
        (margin, margin + 20),
        "This sheet answers the interaction questions directly instead of showing unrelated static states.",
        fill=SUBTLE,
        font=font_subtle,
    )

    cursor_y = margin + header_h
    for row in rows:
        draw.text((margin, cursor_y), row.title, fill=(240, 244, 248), font=font_row)
        draw.text((margin, cursor_y + 20), row.note, fill=SUBTLE, font=font_subtle)
        cursor_y += row_title_h

        row_width = len(row.steps) * card_w + (len(row.steps) - 1) * step_gap + (len(row.steps) - 1) * arrow_w
        x = margin + (canvas_w - margin * 2 - row_width) // 2
        for index, step in enumerate(row.steps):
            draw.rounded_rectangle(
                (x, cursor_y, x + card_w, cursor_y + card_h),
                radius=16,
                fill=CARD_BG,
                outline=CARD_BORDER,
                width=1,
            )
            draw.text((x + 14, cursor_y + 10), step.title, fill=TEXT, font=font_card)
            preview = Image.open(step.path).convert("RGB")
            image.paste(preview, (x + card_pad, cursor_y + 30))
            draw.text((x + 14, cursor_y + 30 + screenshot_h + 10), step.note, fill=SUBTLE, font=font_note)
            x += card_w
            if index != len(row.steps) - 1:
                mid_y = cursor_y + card_h // 2
                draw.line((x + 2, mid_y, x + arrow_w - 6, mid_y), fill=(173, 184, 198), width=3)
                draw.polygon(
                    (
                        (x + arrow_w - 8, mid_y - 6),
                        (x + arrow_w - 8, mid_y + 6),
                        (x + arrow_w - 2, mid_y),
                    ),
                    fill=(173, 184, 198),
                )
                x += arrow_w + step_gap
        cursor_y += card_h + section_gap

    output.parent.mkdir(parents=True, exist_ok=True)
    image.save(output)


def main() -> None:
    parser = argparse.ArgumentParser(description="Render the dashboard review contact sheet.")
    parser.add_argument(
        "--profile",
        choices=("detail", "menu-beeper", "menu-beeper-flow", "menu-concepts"),
        default="detail",
        help="Review-set layout to render.",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("/tmp/mains-aegis-dashboard-review-set.png"),
        help="Output PNG path.",
    )
    args = parser.parse_args()
    if args.profile == "menu-beeper-flow":
        render_storyboard(args.out)
    else:
        render_review_set(args.out, args.profile)
    print(args.out)


if __name__ == "__main__":
    main()
