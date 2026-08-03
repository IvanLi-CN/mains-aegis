#!/usr/bin/env python3
"""Build and validate the Mains Aegis 06 ribbon mark.

THESIS: Two uninterrupted power ribbons pass through an asymmetric protected
handoff. The lower detached wedge is an intentional termination, not a gap to
be filled or mirrored.
OWN-WORLD: Flat functional color, clean white field, and a dark-green/ink pair
that belongs to the existing light Web App theme.
STORY: The mark reads as continuity of power before it reads as a letterform.
FORM: A 743 x 300 construction canvas with three named closed silhouettes.

The 1024 px selected monochrome source is the only geometry authority. Its
low-resolution color-study grid supplies only the nine flat color pairings.
The three path strings below are fixed, hand-curated construction geometry;
this program never calls a raster tracing tool.

Run from the repository root:
  python3 tools/brand/reconstruct_mains_aegis_logo.py
  python3 tools/brand/reconstruct_mains_aegis_logo.py --strict
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import shutil
import subprocess
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy import ndimage


ROOT = Path(__file__).resolve().parents[2]
MONO_REFERENCE = ROOT / (
    "output/imagegen/mains-aegis-logo-round-4/06-continuous-ribbon-low-wide.png"
)
COLOR_REFERENCE = ROOT / (
    "output/imagegen/mains-aegis-logo-06-light-color-model-grid-r2/"
    "mains-aegis-logo-06-light-color-model-grid-r2.png"
)
ASSET_DIR = ROOT / "web/public/brand/mains-aegis"
PWA_DIR = ROOT / "web/public/pwa"
FAVICON_ASSET = ROOT / "web/public/favicon.svg"
DARK_FAVICON_ASSET = ROOT / "web/public/favicon-dark.svg"
VALIDATION_DIR = ROOT / "output/logo-vector-validation"
SQUARE_LOCKUP_VALIDATION_DIR = ROOT / "output/logo-square-lockup-validation"
PLATFORM_VALIDATION_DIR = ROOT / "output/logo-platform-validation"
MARK_LOGO_ASSET = ASSET_DIR / "mains-aegis-logo-mark.svg"
MARK_COLOR_LIGHT_ASSET = ASSET_DIR / "mains-aegis-logo-mark-color-light.svg"
MARK_COLOR_DARK_ASSET = ASSET_DIR / "mains-aegis-logo-mark-color-dark.svg"
MANIFEST_ASSET = ASSET_DIR / "mains-aegis-logo-manifest.json"
LEGACY_MARK_ASSETS = (
    ASSET_DIR / "mains-aegis-06-master.svg",
    ASSET_DIR / "mains-aegis-06-monochrome.svg",
    ASSET_DIR / "mains-aegis-06-theme-light.svg",
    ASSET_DIR / "mains-aegis-06-theme-dark.svg",
    ASSET_DIR / "mains-aegis-06-colorways.svg",
    ASSET_DIR / "mains-aegis-06-manifest.json",
)
LEGACY_VARIANT_DIR = ASSET_DIR / "variants"
FINAL_SQUARE_LOCKUP_ASSET = ASSET_DIR / "mains-aegis-logo-square.svg"
LEGACY_SQUARE_LOCKUP_ASSET = ASSET_DIR / "mains-aegis-06-square-lockup.svg"
SQUARE_COLOR_LIGHT_ASSET = ASSET_DIR / "mains-aegis-logo-square-color-light.svg"
SQUARE_COLOR_DARK_ASSET = ASSET_DIR / "mains-aegis-logo-square-color-dark.svg"
STALE_SQUARE_MONO_ASSETS = (
    ASSET_DIR / "mains-aegis-logo-square-monochrome-dark.svg",
    ASSET_DIR / "mains-aegis-logo-square-monochrome-light.svg",
)
WIDE_LOGO_ASSET = ASSET_DIR / "mains-aegis-logo-wide.svg"
WIDE_COLOR_LIGHT_ASSET = ASSET_DIR / "mains-aegis-logo-wide-color-light.svg"
WIDE_COLOR_DARK_ASSET = ASSET_DIR / "mains-aegis-logo-wide-color-dark.svg"
APPROVED_WORDMARK_REFERENCE = ROOT / (
    "output/imagegen/mains-aegis-wordmark-grid-r1/"
    "mains-aegis-wordmark-grid-r1.png"
)
APPROVED_WORDMARK_LOCKUP_REFERENCE = ROOT / (
    "output/imagegen/mains-aegis-wordmark-lockup-r1/"
    "mains-aegis-06-wordmark-09-review.png"
)

# The tight construction crop in the 1024 px source. It is never auto-aligned
# during validation; the SVG starts at the same top-left point.
SOURCE_CROP = (141, 361, 884, 661)
VIEWBOX_WIDTH = SOURCE_CROP[2] - SOURCE_CROP[0]
VIEWBOX_HEIGHT = SOURCE_CROP[3] - SOURCE_CROP[1]
LUMA_CUTOFF = 128

COLOR_GRID_MARK_WIDTH = 395
COLOR_GRID_MARK_HEIGHT = 161
COLOR_GRID_POSITIONS = (
    (68, 294),
    (570, 294),
    (1070, 294),
    (67, 727),
    (569, 727),
    (1070, 727),
    (65, 1160),
    (567, 1160),
    (1066, 1160),
)
STALE_EVIDENCE = (
    "edge-diff-colorways.png",
    "expected-flat-colorways.png",
    "pixel-diff-colorways.png",
    "rendered-colorways.png",
    "theme-light.png",
)
STALE_SQUARE_LOCKUP_EVIDENCE = (
    "render-sources",
    "renders",
    "mains-aegis-06-square-lockup-grid-review-v2.png",
    "mains-aegis-06-square-lockup-grid-review.png",
    "mains-aegis-06-square-lockup-grid.png",
    "mains-aegis-06-square-lockup-grid.svg",
    "mains-aegis-06-square-lockup-construction-review.png",
    "mains-aegis-06-square-lockup-final.svg",
    "mains-aegis-06-square-lockup-final.png",
    "mains-aegis-06-square-lockup-construction.svg",
    "mains-aegis-06-square-lockup-construction.png",
)

# Literal sRGB values sampled from the flat terminal interiors of the r2 grid.
# The grid is deliberately not used to recover geometry: each model tile has
# slightly different antialiasing and contour noise.
LIGHT_THEME_LEFT = "#07251C"
LIGHT_THEME_HANDOFF = "#A86300"
LIGHT_THEME_RIGHT = "#0B7258"
# Exact sRGB conversion of the Web App light canvas token:
# oklch(98.8% 0.006 105) -> #FCFBF7.
LIGHT_CANVAS = "#FCFBF7"
# Exact sRGB conversion of the Web App dark canvas token:
# oklch(19% 0.014 250) -> #0F141A.
DARK_CANVAS = "#0F141A"
SQUARE_LOCKUP_CANVAS = 1024
SQUARE_LOCKUP_WORD = "MAINS AEGIS"
SQUARE_LOCKUP_MARK_WIDTH = 720.0
SQUARE_LOCKUP_MARK_X = 152.0
SQUARE_LOCKUP_MARK_Y = 252.0
SQUARE_LOCKUP_WORD_WIDTH = 640.0
SQUARE_LOCKUP_WORD_HEIGHT = 69.0
SQUARE_LOCKUP_WORD_X = 192.0
SQUARE_LOCKUP_WORD_Y = 704.0
SQUARE_LOCKUP_MIN_GAP = 128.0
WIDE_LOGO_WIDTH = 1600
WIDE_LOGO_HEIGHT = 260
WIDE_MARK_X = 50.0
WIDE_MARK_Y = 47.5
WIDE_MARK_SCALE = 0.55
WIDE_WORDMARK_X = 513.7
WIDE_WORDMARK_Y = 74.11
WIDE_WORDMARK_SCALE = 1.62
PLATFORM_MASKABLE_SCALE = 0.9
FINAL_WORDMARK_NAME = "09-source-normalized"
FINAL_WORDMARK_ALPHA_THRESHOLD = 128
FINAL_WORDMARK_TRACE_SETTINGS = {
    "turn_policy": "minority",
    "turd_size": 0,
    "alpha_max": 0.2,
    "optimization_tolerance": 0.2,
}
WORDMARK_REFERENCE_LIMITS = {
    "minimum_iou": 0.98,
    "maximum_xor_pixels": 400,
    "maximum_p99_contour_offset_px": 1.0,
    "maximum_contour_offset_px": 1.0,
}
SQUARE_LOCKUP_SURFACES = (
    ("white-proof", "#FFFFFF", "#000000"),
    ("light-web", LIGHT_CANVAS, "#161B20"),
    ("dark-web", DARK_CANVAS, "#EAF7F0"),
)
@dataclass(frozen=True)
class Variant:
    name: str
    left_color: str
    right_color: str


@dataclass(frozen=True)
class AppVariant:
    name: str
    incoming_color: str
    handoff_color: str
    outgoing_color: str


@dataclass(frozen=True)
class GeometryPath:
    element_id: str
    ribbon: str
    d: str


@dataclass(frozen=True)
class SquareLogoVariant:
    asset: Path
    title: str
    mark_colors: tuple[str, str, str]
    wordmark_color: str
    proof_surface: str


@dataclass(frozen=True)
class PathCommand:
    command: str
    start: tuple[float, float]
    end: tuple[float, float]
    control_1: tuple[float, float] | None = None
    control_2: tuple[float, float] | None = None


VARIANTS = (
    Variant("01-graphite-cobalt", "#313639", "#005BB2"),
    Variant("02-navy-slate", "#092E5E", "#57656E"),
    Variant("03-teal-aqua", "#005557", "#46B1B0"),
    Variant("04-deep-teal-mint", "#005654", "#6CB699"),
    Variant("05-blue-graphite", "#005BB1", "#33383B"),
    Variant("06-graphite-amber", "#45515B", "#F89F27"),
    Variant("07-navy-cyan", "#092E5D", "#22BAEE"),
    Variant("08-emerald-slate", "#007E65", "#535E66"),
    Variant("09-graphite-coral", "#33383B", "#F26D56"),
)

APP_VARIANTS = (
    AppVariant("app-flow-mint", "#07251C", "#3AA57D", "#0B7258"),
    AppVariant("app-flow-amber", "#07251C", LIGHT_THEME_HANDOFF, "#0B7258"),
    AppVariant("app-teal-flow", "#07251C", "#7D9F35", "#006C73"),
)

# All three forms are lifted for dark surfaces while retaining their input /
# handoff / output roles.
DARK_APP_VARIANTS = (
    AppVariant("dark-flow-amber", "#D6F0E1", "#E6A326", "#66D1A0"),
    AppVariant("dark-flow-mint", "#EAF7F0", "#72D5AE", "#39AA7C"),
    AppVariant("dark-flow-cyan", "#D7F3EF", "#82D7CA", "#60BCEB"),
)

SQUARE_LOGO_VARIANTS = (
    SquareLogoVariant(
        SQUARE_COLOR_LIGHT_ASSET,
        "Mains Aegis square logo, color for light surfaces",
        (LIGHT_THEME_LEFT, LIGHT_THEME_HANDOFF, LIGHT_THEME_RIGHT),
        "#161B20",
        LIGHT_CANVAS,
    ),
    SquareLogoVariant(
        SQUARE_COLOR_DARK_ASSET,
        "Mains Aegis square logo, color for dark surfaces",
        ("#D6F0E1", "#E6A326", "#66D1A0"),
        "#EAF7F0",
        DARK_CANVAS,
    ),
)

WIDE_LOGO_VARIANTS = (
    SquareLogoVariant(
        WIDE_COLOR_LIGHT_ASSET,
        "Mains Aegis wide logo, color for light surfaces",
        (LIGHT_THEME_LEFT, LIGHT_THEME_HANDOFF, LIGHT_THEME_RIGHT),
        "#161B20",
        LIGHT_CANVAS,
    ),
    SquareLogoVariant(
        WIDE_COLOR_DARK_ASSET,
        "Mains Aegis wide logo, color for dark surfaces",
        ("#D6F0E1", "#E6A326", "#66D1A0"),
        "#EAF7F0",
        DARK_CANVAS,
    ),
)

PLATFORM_ASSET_NAMES = {
    "light": {
        "icon_192": "mains-aegis-icon-192.png",
        "icon_512": "mains-aegis-icon-512.png",
        "maskable_512": "mains-aegis-icon-maskable-512.png",
        "apple_touch_180": "mains-aegis-icon-apple-touch-180.png",
    },
    "dark": {
        "icon_192": "mains-aegis-icon-dark-192.png",
        "icon_512": "mains-aegis-icon-dark-512.png",
        "maskable_512": "mains-aegis-icon-dark-maskable-512.png",
        "apple_touch_180": "mains-aegis-icon-dark-apple-touch-180.png",
    },
}

# These paths are deliberately separated by meaning, not by color or pixels:
# - left-main: incoming continuous power ribbon and its central handoff.
# - left-tail: the intentionally detached lower termination wedge.
# - right-ribbon: outgoing continuous power ribbon and right terminal.
#
# Coordinates are in the normalized source crop (0 0 743 300). Each long
# curve is represented by a tangent-matched cubic Bezier sequence. The source
# bitmap establishes the large arcs, crossings, and terminal anchors; its
# antialiased edge pixels are not copied into the final geometry.
GEOMETRY = (
    GeometryPath(
        "left-main",
        "left",
        "M 144.7 1.5 "
        "C 80.9685 11.7054 29.4 56.5 8.6 119.2 "
        "C 0.1366 144.7122 -0.1 148.0 0.2 229.0 "
        "L 0.5 299.5 L 97.5 299.5 L 98.0 229.5 "
        "C 98.5 159.5 98.5 159.5 100.9 152.5 "
        "C 120.8583 94.2884 191.5 81.5 227.4 130.1 "
        "C 231.3928 135.5053 288.8 195.4 328.1 235.1 "
        "C 375.7078 283.1923 418.9 300.9 486.5 299.8 "
        "L 504.5 299.5 L 504.8 235.7 "
        "C 504.9 200.7 504.6 172.0 504.2 172.0 "
        "C 503.7 172.0 499.7 174.4 495.2 177.4 "
        "C 461.3683 199.9545 422.2 192.4 390.1 157.7 "
        "C 384.4307 151.5715 361.0 126.2 338.2 101.5 "
        "C 279.6875 38.1115 272.2 31.5 243.4 17.1 "
        "C 214.6399 2.7199 175.3 -3.4 144.7 1.5 Z",
    ),
    GeometryPath(
        "left-tail",
        "left",
        "M 239.0 171.5 L 239.5 299.5 L 256.0 299.3 "
        "C 285.6 299.0 342.0 284.9 342.0 277.8 "
        "C 342.0 277.2 321.0 255.4 295.3 229.1 "
        "C 269.6988 202.9011 246.5 179.3 243.8 176.5 "
        "L 239.0 171.5 Z",
    ),
    GeometryPath(
        "right-ribbon",
        "right",
        "M 557.5 0.7 "
        "C 504.7104 7.5802 479.1 24.1 417.1 91.8 "
        "C 381.6773 130.4793 383.4 124.9 401.2 143.2 "
        "C 437.8346 180.8637 485.7 175.2 514.8 130.0 "
        "C 548.1055 78.2678 627.5 91.0 643.1 150.6 "
        "C 645.0591 158.0848 645.1 161.7 645.5 228.5 "
        "C 645.7305 267.0005 646.0 298.8 646.0 299.2 "
        "C 646.0 300.8 740.6 300.1 741.9 298.6 "
        "C 744.4576 295.6489 742.8 153.9 740.1 139.5 "
        "C 723.7188 52.1338 641.9 -10.3 557.5 0.7 Z",
    ),
)

# The markers sit on the intended centerlines, crossings, terminals, and the
# detached wedge. They are construction checks, not extra decorative geometry.
LANDMARKS = (
    ("left-crown", 169, 1),
    ("left-outer-sweep", 3, 125),
    ("left-inner-sweep", 169, 100),
    ("left-terminal", 1, 244),
    ("left-handoff-entry", 385, 151),
    ("left-handoff-exit", 504, 178),
    ("left-baseline-end", 503, 299),
    ("tail-tip", 239, 172),
    ("tail-leading-edge", 239, 241),
    ("tail-terminal", 260, 299),
    ("right-crown", 575, 1),
    ("right-outer-sweep", 740, 125),
    ("right-inner-sweep", 575, 100),
    ("right-terminal", 742, 244),
)

# The bitmap remains a fixed, unregistered visual reference. It constrains the
# construction without treating renderer antialiasing as a geometry defect.
REFERENCE_ALIGNMENT_LIMITS = {
    "minimum_iou": 0.99,
    "maximum_xor_ratio": 0.005,
    "maximum_p99_contour_offset_px": 2.0,
    "maximum_contour_offset_px": 2.5,
    "maximum_landmark_offset_px": 1.5,
}

SMOOTHNESS_LIMITS = {
    "maximum_drawing_commands": 42,
    "maximum_curve_segments": 26,
    "maximum_curve_join_angle_degrees": 1.5,
}


def geometry_hash() -> str:
    payload = "\n".join(entry.d for entry in GEOMETRY)
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def color_for(entry: GeometryPath, left_color: str, right_color: str) -> str:
    return left_color if entry.ribbon == "left" else right_color


def path_markup(
    left_color: str,
    right_color: str,
    *,
    include_ids: bool,
    id_prefix: str = "",
) -> str:
    entries = []
    for entry in GEOMETRY:
        element_id = f' id="{id_prefix}{entry.element_id}"' if include_ids else ""
        entries.append(
            f'  <path{element_id} d="{entry.d}" '
            f'fill="{color_for(entry, left_color, right_color)}"/>'
        )
    return "\n".join(entries)


def mark_svg(left_color: str, right_color: str, title: str) -> str:
    return "\n".join(
        (
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 743 300" role="img"',
            '  aria-labelledby="title">',
            f'  <title id="title">{title}</title>',
            path_markup(left_color, right_color, include_ids=True),
            "</svg>",
            "",
        )
    )


def app_mark_svg(
    incoming_color: str,
    handoff_color: str,
    outgoing_color: str,
    title: str,
) -> str:
    entries = []
    for entry in GEOMETRY:
        fill = {
            "left-main": incoming_color,
            "left-tail": handoff_color,
            "right-ribbon": outgoing_color,
        }[entry.element_id]
        entries.append(f'  <path id="{entry.element_id}" d="{entry.d}" fill="{fill}"/>')
    return "\n".join(
        (
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 743 300" role="img"',
            '  aria-labelledby="title">',
            f'  <title id="title">{title}</title>',
            *entries,
            "</svg>",
            "",
        )
    )


FINAL_WORDMARK_09_D = (
    "M 236.5 705.5 L 235.2 706.5 L 234 713.5 C 232.5 722.8 227.8 742.6 226.9 744.2 L 226.1 745.5 L "
    "225.1 744.5 C 224.6 743.9 223.2 738.5 222 732.5 C 220.9 726.4 219.3 718.2 218.5 714.1 L 217 "
    "706.6 L 215.8 705.8 L 214.5 705 L 205.7 705 L 196.9 705 L 194.5 707.6 L 192 710.3 L 192 740.1 L "
    "192 769.9 L 193.9 771 L 195.9 772 L 199.8 772 L 203.8 772 L 205 770.5 L 206.3 768.9 L 205.6 "
    "743.6 L 204.9 718.2 L 205.8 717.6 L 206.8 717 L 207.9 718.4 L 209.1 719.8 L 211.1 732.1 L 213.1 "
    "744.5 L 215.8 752.5 L 218.5 760.5 L 225.1 760.8 L 231.8 761.1 L 234.9 754.4 L 238 747.8 L 240.6 "
    "733.9 L 243.3 720 L 244.1 720 L 245 720 L 245.2 745.7 L 245.5 771.5 L 251.5 771.5 L 257.5 771.5 "
    "L 257.8 742 L 258.1 712.5 L 256.4 709.3 L 254.7 706 L 252.6 705.3 C 251.4 705 247.6 704.6 244.1 "
    "704.6 L 237.7 704.5 L 236.5 705.5 Z M 451 705.6 C 449.5 706.3 447.5 707.8 446.6 709.1 L 445 "
    "711.4 L 445 724.2 L 445 737 L 447.9 740.2 L 450.8 743.5 L 468.7 744 L 486.5 744.5 L 486.5 752 L "
    "486.5 759.5 L 472.5 759.5 L 458.5 759.5 L 458 756.5 L 457.5 753.5 L 451.5 753.5 L 445.5 753.5 L "
    "445.2 759.8 L 444.9 766.1 L 447.9 769 L 450.8 772 L 471.1 772 L 491.3 772 L 494.6 769.9 C 496.5 "
    "768.8 498.5 767 499 766 L 500 764.1 L 500 752.1 L 500 740 L 496.6 736.2 L 493.2 732.5 L 475.8 "
    "732 L 458.5 731.5 L 458.5 724 L 458.5 716.5 L 471.4 716.2 L 484.2 715.9 L 485.6 717 C 486.3 "
    "717.6 487 719.1 487.2 720.3 L 487.5 722.5 L 493 722.5 L 498.5 722.5 L 498.8 716.7 L 499.1 710.9 "
    "L 495.6 707.4 L 492 703.8 L 472.9 704.2 L 453.8 704.5 L 451 705.6 Z M 554 704.7 C 553.2 705 "
    "551.5 706.2 550.3 707.3 L 548.1 709.3 L 547.5 713.9 C 546.6 721.3 545.1 730.5 542.4 745.5 C 541 "
    "753.2 539.5 762 539 765 L 538 770.4 L 539.3 771.2 L 540.5 772 L 545.4 771.8 L 550.4 771.5 L "
    "551.6 766 C 552.3 763 552.9 759.1 553 757.4 L 553 754.3 L 554.8 752.9 L 556.5 751.5 L 570 751.5 "
    "L 583.4 751.5 L 585.6 760.7 C 586.8 765.8 588.1 770.4 588.5 771 L 589.1 772 L 593.9 772 L 598.6 "
    "772 L 599.7 770.9 L 600.7 769.9 L 597.4 753.2 C 595.5 744 592.6 730 590.9 722 L 587.9 707.5 L "
    "585.8 705.7 L 583.8 704 L 569.6 704.1 C 561.9 704.1 554.8 704.4 554 704.7 Z M 577.1 718 C 577.9 "
    "719.6 580 731.7 580.3 736.5 L 580.5 739.5 L 569.6 739.8 L 558.7 740.1 L 557.8 739.2 L 556.9 "
    "738.3 L 558.1 729.4 C 558.7 724.5 559.6 719.5 560.1 718.2 L 561 715.9 L 568.6 716.2 L 576.3 "
    "716.5 L 577.1 718 Z M 635.5 705.1 C 624 705.4 614.4 705.7 614.3 705.8 C 614.2 705.9 613.4 707.3 "
    "612.5 709 L 611 711.9 L 611 739 L 611 766.2 L 613.9 769.1 L 616.8 772 L 639.7 772 L 662.5 772 L "
    "663.8 771.1 L 665.1 770.3 L 664.8 765.4 L 664.5 760.5 L 644.2 760.2 L 623.9 760 L 624.2 752.2 L "
    "624.5 744.5 L 640 744 L 655.5 743.5 L 655.5 738 L 655.5 732.5 L 640 732 L 624.5 731.5 L 624.5 "
    "724 L 624.5 716.5 L 644.5 716 L 664.5 715.5 L 664.8 711.1 L 665.1 706.7 L 664 705.6 L 663 704.6 "
    "L 659.7 704.5 C 658 704.5 647.1 704.7 635.5 705.1 Z M 684.3 705.5 L 681.2 706.8 L 679.3 709.6 L "
    "677.5 712.4 L 677.5 738 L 677.5 763.6 L 679.2 766.1 L 680.9 768.7 L 684.3 770.4 L 687.8 772 L "
    "708.4 772 L 729.1 772 L 731.6 769.5 L 734 767 L 733.8 750.8 L 733.5 734.5 L 719.5 734.5 L 705.5 "
    "734.5 L 704.9 738.5 C 704.5 740.7 704.5 743 704.7 743.7 L 705.2 744.9 L 712.3 745.2 L 719.5 "
    "745.5 L 719.5 752.5 L 719.5 759.5 L 705.5 759.5 L 691.5 759.5 L 691 739.2 L 690.5 718.8 L 691.3 "
    "717.4 L 692.1 716 L 704.8 716 L 717.5 716 L 718.6 716.8 C 719.3 717.2 719.9 718.4 720.1 719.5 L "
    "720.5 721.5 L 726.5 721.5 L 732.5 721.5 L 732.8 717.1 L 733.1 712.7 L 731.9 709.8 C 731.3 708.2 "
    "729.8 706.3 728.6 705.5 L 726.6 704 L 707 704 L 687.5 704.1 L 684.3 705.5 Z M 783.5 704.9 C "
    "782.4 705.4 780.3 706.9 778.9 708.2 L 776.3 710.6 L 775.6 722.6 L 774.9 734.6 L 775.9 737.2 L "
    "776.9 739.8 L 779.8 741.9 L 782.7 744 L 800.1 744.2 L 817.5 744.5 L 817.5 752 L 817.5 759.5 L "
    "803.3 759.8 L 789 760.1 L 789 757 L 789 754 L 786.7 753.4 C 785.4 753.1 782.6 753 780.4 753.2 L "
    "776.5 753.5 L 776.2 759.3 L 775.9 765.1 L 779.4 768.6 L 782.9 772.1 L 803.9 771.8 L 825 771.5 L "
    "828.1 768.4 L 831.2 765.3 L 831.1 751.9 L 830.9 738.5 L 828 735.5 L 825 732.5 L 807.2 732 L "
    "789.5 731.5 L 789.5 724 L 789.5 716.5 L 802.4 716.2 L 815.2 715.9 L 816.6 717 C 817.3 717.6 818 "
    "719.1 818.2 720.3 L 818.5 722.5 L 823.3 722.8 L 828.2 723.1 L 829.4 722.1 L 830.7 721 L 830.2 "
    "715.7 L 829.7 710.5 L 828.1 708.3 C 827.2 707.1 825.5 705.7 824.2 705.1 L 821.9 704 L 803.7 "
    "704.1 L 785.5 704.1 L 783.5 704.9 Z M 288.9 707.2 C 288.2 708.4 284.2 720 279.9 733 C 275.7 "
    "745.9 271 759.6 269.5 763.4 L 266.9 770.3 L 268.2 771.1 C 268.9 771.6 271.5 772 274 772 L 278.5 "
    "772 L 279.9 771.1 L 281.4 770.1 L 283.9 762.8 L 286.5 755.5 L 299.9 755.5 L 313.3 755.5 L 315.7 "
    "762.5 L 318 769.5 L 319.8 770.8 L 321.5 772.1 L 326.3 771.6 C 329 771.4 331.4 771 331.6 770.7 C "
    "331.8 770.5 329.3 762.3 326 752.4 C 322.8 742.6 317.9 728 315.3 720 L 310.5 705.5 L 300.3 705.2 "
    "L 290.1 704.9 L 288.9 707.2 Z M 304.5 728.5 C 306.5 734.9 308.3 740.7 308.7 741.5 L 309.2 743 L "
    "300.1 743 L 291 743 L 291 741.9 C 291 740.4 297 720.9 298.2 718.7 C 298.7 717.8 299.5 717 300 "
    "717 L 300.9 717 L 304.5 728.5 Z M 343.5 706 L 342.8 707.1 L 343.1 738.6 L 343.4 770.2 L 345 "
    "770.9 C 346.3 771.5 350.7 771.6 355.8 771.1 L 357 771 L 356.8 738.2 L 356.5 705.5 L 350.3 705.2 "
    "L 344.2 704.9 L 343.5 706 Z M 373.7 705.7 L 373 706.3 L 373 738.6 L 373 770.8 L 374.6 771.4 C "
    "376.6 772.2 382.4 772.2 384.4 771.4 L 386 770.8 L 386 748.9 L 386 727 L 386.9 727 C 387.4 727 "
    "394.1 736.6 401.7 748.3 C 409.4 759.9 416.4 770.1 417.3 770.8 L 418.8 772.1 L 424.2 771.8 L "
    "429.5 771.5 L 429.8 738.2 L 430 704.9 L 423.8 705.2 L 417.5 705.5 L 417 726.5 L 416.5 747.5 L "
    "405.6 731 C 399.6 721.9 393.1 712.4 391.2 709.7 L 387.6 705 L 381 705 C 377.3 705 374 705.3 "
    "373.7 705.7 Z M 747.5 706.3 C 747.2 707 747.1 721.9 747.2 739.5 L 747.5 771.5 L 751.9 771.8 C "
    "754.4 772 757.4 771.9 758.7 771.5 L 761 771 L 761 738 L 761 705 L 754.5 705 L 747.9 705 L 747.5 "
    "706.3 Z"
)


def final_wordmark_09_path() -> str:
    """Return the normalized vector contour of the selected raster 09 wordmark."""
    return FINAL_WORDMARK_09_D


def square_mark_paths(
    *,
    include_ids: bool,
    indent: str,
    fills: tuple[str, str, str] | None = None,
) -> str:
    return "\n".join(
        f'{indent}<path{f" id=\"{entry.element_id}\"" if include_ids else ""} '
        f'd="{entry.d}" fill="{fills[index] if fills is not None else "currentColor"}"/>'
        for index, entry in enumerate(GEOMETRY)
    )


def square_mark_transform() -> str:
    mark_scale = SQUARE_LOCKUP_MARK_WIDTH / VIEWBOX_WIDTH
    return (
        f"translate({SQUARE_LOCKUP_MARK_X:.4f} {SQUARE_LOCKUP_MARK_Y:.4f}) "
        f"scale({mark_scale:.8f})"
    )


def final_square_lockup_svg(
    *,
    color: str | None = None,
    title: str = "Mains Aegis square logo",
    mark_colors: tuple[str, str, str] | None = None,
    wordmark_color: str | None = None,
) -> str:
    """Emit the selected 09 lockup without a raster or font dependency."""
    color_attribute = f' color="{color}"' if color is not None else ""
    wordmark_fill = wordmark_color or "currentColor"
    return "\n".join(
        (
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" role="img"',
            f'  aria-labelledby="title"{color_attribute}>',
            f'  <title id="title">{title}</title>',
            f'  <g id="mark-06" transform="{square_mark_transform()}">',
            square_mark_paths(include_ids=True, indent="    ", fills=mark_colors),
            "  </g>",
            f'  <path id="wordmark-mains-aegis" d="{final_wordmark_09_path()}" fill="{wordmark_fill}"/>',
            "</svg>",
            "",
        )
    )


def wide_mark_transform() -> str:
    return f"translate({WIDE_MARK_X:.4f} {WIDE_MARK_Y:.4f}) scale({WIDE_MARK_SCALE:.8f})"


def wide_wordmark_transform() -> str:
    return (
        f"translate({WIDE_WORDMARK_X:.4f} {WIDE_WORDMARK_Y:.4f}) "
        f"scale({WIDE_WORDMARK_SCALE:.8f}) "
        f"translate({-SQUARE_LOCKUP_WORD_X:.4f} {-SQUARE_LOCKUP_WORD_Y:.4f})"
    )


def wide_logo_svg(
    *,
    title: str = "Mains Aegis wide logo",
    mark_colors: tuple[str, str, str] | None = None,
    wordmark_color: str | None = None,
) -> str:
    """Emit the horizontal site-banner lockup from the approved vector master."""
    return "\n".join(
        (
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {WIDE_LOGO_WIDTH} {WIDE_LOGO_HEIGHT}" role="img"',
            '  aria-labelledby="title">',
            f'  <title id="title">{title}</title>',
            f'  <g id="mark-06" transform="{wide_mark_transform()}">',
            square_mark_paths(include_ids=True, indent="    ", fills=mark_colors),
            "  </g>",
            f'  <path id="wordmark-mains-aegis" transform="{wide_wordmark_transform()}" '
            f'd="{final_wordmark_09_path()}" fill="{wordmark_color or "currentColor"}"/>',
            "</svg>",
            "",
        )
    )


def platform_lockup_svg(
    *,
    mark_colors: tuple[str, str, str],
    wordmark_color: str,
    background: str,
    title: str,
    maskable: bool = False,
) -> str:
    """Wrap the approved 09 Lockup in a flat platform-icon surface.

    The platform export may add one solid background and one uniform outer
    scale for the maskable safe zone. The four foreground paths remain the
    exact square-lockup geometry and are never redrawn or rearranged.
    """
    outer_transform = (
        f' transform="translate(512 512) scale({PLATFORM_MASKABLE_SCALE:.4f}) '
        'translate(-512 -512)"'
        if maskable
        else ""
    )
    return "\n".join(
        (
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" role="img"',
            '  aria-labelledby="title">',
            f'  <title id="title">{title}</title>',
            f'  <rect id="platform-background" width="1024" height="1024" fill="{background}"/>',
            f'  <g id="platform-lockup"{outer_transform}>',
            f'    <g id="mark-06" transform="{square_mark_transform()}">',
            square_mark_paths(include_ids=True, indent="      ", fills=mark_colors),
            "    </g>",
            f'    <path id="wordmark-mains-aegis" d="{final_wordmark_09_path()}" '
            f'fill="{wordmark_color}"/>',
            "  </g>",
            "</svg>",
            "",
        )
    )


def platform_variant_definitions() -> tuple[tuple[str, SquareLogoVariant], ...]:
    return (("light", SQUARE_LOGO_VARIANTS[0]), ("dark", SQUARE_LOGO_VARIANTS[1]))


def platform_svg_path(theme: str, kind: str) -> Path:
    return PLATFORM_VALIDATION_DIR / f"mains-aegis-{theme}-{kind}.svg"


def platform_png_path(theme: str, kind: str) -> Path:
    return PWA_DIR / PLATFORM_ASSET_NAMES[theme][kind]


def write_platform_assets() -> None:
    """Rasterize both approved 09 colorways for install surfaces."""
    require_renderer()
    PWA_DIR.mkdir(parents=True, exist_ok=True)
    PLATFORM_VALIDATION_DIR.mkdir(parents=True, exist_ok=True)
    for stale_path in PLATFORM_VALIDATION_DIR.glob("*"):
        if stale_path.is_file():
            stale_path.unlink()
    for theme, variant in platform_variant_definitions():
        standard_svg = platform_lockup_svg(
            mark_colors=variant.mark_colors,
            wordmark_color=variant.wordmark_color,
            background=variant.proof_surface,
            title=f"Mains Aegis {theme} application icon",
        )
        maskable_svg = platform_lockup_svg(
            mark_colors=variant.mark_colors,
            wordmark_color=variant.wordmark_color,
            background=variant.proof_surface,
            title=f"Mains Aegis {theme} maskable application icon",
            maskable=True,
        )
        standard_source = platform_svg_path(theme, "standard")
        maskable_source = platform_svg_path(theme, "maskable")
        standard_source.write_text(standard_svg, encoding="utf-8")
        maskable_source.write_text(maskable_svg, encoding="utf-8")
        render_svg(standard_source, platform_png_path(theme, "icon_192"), 192, 192, background=None)
        render_svg(standard_source, platform_png_path(theme, "icon_512"), 512, 512, background=None)
        render_svg(maskable_source, platform_png_path(theme, "maskable_512"), 512, 512, background=None)
        render_svg(standard_source, platform_png_path(theme, "apple_touch_180"), 180, 180, background=None)


def colorways_svg() -> str:
    tile_width = 500
    tile_height = 390
    margin = 54
    scale = 0.49
    content_width = int(VIEWBOX_WIDTH * scale)
    content_height = int(VIEWBOX_HEIGHT * scale)
    entries = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="1608" height="1278" viewBox="0 0 1608 1278"',
        '  role="img" aria-labelledby="title">',
        '  <title id="title">Mains Aegis 06 flat colorways</title>',
        '  <rect width="1608" height="1278" fill="#F7F8F6"/>',
    ]
    for index, variant in enumerate(VARIANTS):
        row, column = divmod(index, 3)
        x = margin + column * (tile_width + margin)
        y = margin + row * (tile_height + margin)
        mark_x = x + (tile_width - content_width) / 2
        mark_y = y + 64
        entries.extend(
            (
                f'  <g aria-label="{variant.name}" transform="translate({x} {y})">',
                f'    <rect width="{tile_width}" height="{tile_height}" fill="#FFFFFF"/>',
                f'    <g transform="translate({mark_x - x:.3f} {mark_y - y:.3f}) scale({scale})">',
                path_markup(variant.left_color, variant.right_color, include_ids=False),
                "    </g>",
                f'    <text x="28" y="354" fill="#3B4346" font-family="system-ui, sans-serif" font-size="22">{index + 1:02d}</text>',
                "  </g>",
            )
        )
    entries.extend(("</svg>", ""))
    return "\n".join(entries)


def theme_preview_svg() -> str:
    component_colors = {
        "left-main": LIGHT_THEME_LEFT,
        "left-tail": LIGHT_THEME_HANDOFF,
        "right-ribbon": LIGHT_THEME_RIGHT,
    }
    entries = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 743 300" role="img"',
        '  aria-labelledby="title">',
        '  <title id="title">Mains Aegis 06 on the light Web App canvas</title>',
        f'  <rect width="743" height="300" fill="{LIGHT_CANVAS}"/>',
        *(
            f'  <path d="{entry.d}" fill="{component_colors[entry.element_id]}"/>'
            for entry in GEOMETRY
        ),
        "</svg>",
        "",
    )
    return "\n".join(entries)


def construction_svg() -> str:
    component_colors = {
        "left-main": "#075A54",
        "left-tail": "#D66F38",
        "right-ribbon": "#284C67",
    }
    entries = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 743 300" role="img"',
        '  aria-label="Mains Aegis 06 construction components">',
        '  <rect width="743" height="300" fill="#FFFFFF"/>',
        *(
            f'  <path d="{entry.d}" fill="{component_colors[entry.element_id]}"/>'
            for entry in GEOMETRY
        ),
        "</svg>",
        "",
    )
    return "\n".join(entries)


def require_renderer() -> None:
    if shutil.which("rsvg-convert") is None:
        raise SystemExit("rsvg-convert is required to render the deterministic validation assets.")


def render_svg(
    svg: Path, output: Path, width: int, height: int, *, background: str | None = "white"
) -> None:
    command = ["rsvg-convert"]
    if background is not None:
        command.extend(("--background-color", background))
    command.extend(
        (
            "--width",
            str(width),
            "--height",
            str(height),
            "--output",
            str(output),
            str(svg),
        )
    )
    subprocess.run(command, check=True)


def source_crop() -> np.ndarray:
    if not MONO_REFERENCE.exists():
        raise SystemExit(f"Missing monochrome geometry source: {MONO_REFERENCE}")
    image = np.asarray(Image.open(MONO_REFERENCE).convert("RGB"))
    x0, y0, x1, y1 = SOURCE_CROP
    return image[y0:y1, x0:x1]


def hex_color(rgb: np.ndarray) -> str:
    return "#{:02X}{:02X}{:02X}".format(*(int(value) for value in rgb))


def sampled_palette() -> list[dict[str, str]]:
    if not COLOR_REFERENCE.exists():
        raise SystemExit(f"Missing color-study palette source: {COLOR_REFERENCE}")
    image = np.asarray(Image.open(COLOR_REFERENCE).convert("RGB"))
    result = []
    radius = 7
    for variant, (origin_x, origin_y) in zip(VARIANTS, COLOR_GRID_POSITIONS, strict=True):
        def sample(point_x: int, point_y: int) -> str:
            swatch = image[
                origin_y + point_y - radius : origin_y + point_y + radius + 1,
                origin_x + point_x - radius : origin_x + point_x + radius + 1,
            ]
            return hex_color(np.median(swatch.reshape(-1, 3), axis=0).round().astype(np.uint8))

        result.append(
            {
                "name": variant.name,
                "left_color": sample(20, 130),
                "right_color": sample(370, 130),
            }
        )
    return result


def mask_from_rgb(image: np.ndarray) -> np.ndarray:
    return np.min(image, axis=2) < LUMA_CUTOFF


def component_masks(mask: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    labels, label_count = ndimage.label(mask, structure=np.ones((3, 3), dtype=np.uint8))
    candidates: list[tuple[int, np.ndarray]] = []
    for label in range(1, label_count + 1):
        component = labels == label
        if int(component.sum()) >= 100:
            candidates.append((int(component.sum()), component))
    if len(candidates) != 3:
        raise RuntimeError(f"Expected three semantic source components, received {len(candidates)}.")
    candidates.sort(reverse=True, key=lambda item: item[0])
    left_main = candidates[0][1]
    right_ribbon = candidates[1][1]
    left_tail = candidates[2][1]
    return left_main, left_tail, right_ribbon


def contour_metrics(expected: np.ndarray, actual: np.ndarray) -> dict[str, int | float]:
    difference = np.logical_xor(expected, actual)
    overlap = expected & actual
    union = expected | actual
    extra = actual & ~expected
    missing = expected & ~actual
    distances: list[np.ndarray] = []
    if extra.any():
        distances.append(ndimage.distance_transform_edt(~expected)[extra])
    if missing.any():
        distances.append(ndimage.distance_transform_edt(~actual)[missing])
    contour_offsets = np.concatenate(distances) if distances else np.array([0.0])
    return {
        "expected_pixels": int(expected.sum()),
        "rendered_pixels": int(actual.sum()),
        "xor_pixels": int(difference.sum()),
        "xor_ratio": float(difference.mean()),
        "iou": float(overlap.sum() / union.sum()),
        "mean_contour_offset_px": float(contour_offsets.mean()),
        "p99_contour_offset_px": float(np.quantile(contour_offsets, 0.99)),
        "max_contour_offset_px": float(contour_offsets.max()),
    }


def outline_mask(mask: np.ndarray) -> np.ndarray:
    return mask & ~ndimage.binary_erosion(mask)


def nearest_point(mask: np.ndarray, x: int, y: int) -> tuple[int, int]:
    ys, xs = np.where(mask)
    if not len(xs):
        raise RuntimeError("Unable to locate a construction landmark on an empty contour.")
    distances = (xs - x) ** 2 + (ys - y) ** 2
    index = int(np.argmin(distances))
    return int(xs[index]), int(ys[index])


def landmark_metrics(
    source_mask: np.ndarray, rendered_mask: np.ndarray
) -> list[dict[str, int | float | str]]:
    source_outline = outline_mask(source_mask)
    rendered_outline = outline_mask(rendered_mask)
    result = []
    for name, x, y in LANDMARKS:
        source_x, source_y = nearest_point(source_outline, x, y)
        rendered_x, rendered_y = nearest_point(rendered_outline, source_x, source_y)
        result.append(
            {
                "name": name,
                "source_x": source_x,
                "source_y": source_y,
                "rendered_x": rendered_x,
                "rendered_y": rendered_y,
                "source_to_rendered_offset_px": float(
                    math.hypot(rendered_x - source_x, rendered_y - source_y)
                ),
            }
        )
    return result


def write_assets() -> None:
    ASSET_DIR.mkdir(parents=True, exist_ok=True)
    PWA_DIR.mkdir(parents=True, exist_ok=True)
    for stale_asset in LEGACY_MARK_ASSETS:
        stale_asset.unlink(missing_ok=True)
    (ASSET_DIR / "mains-aegis-app-icon.svg").unlink(missing_ok=True)
    DARK_FAVICON_ASSET.unlink(missing_ok=True)
    if LEGACY_VARIANT_DIR.exists():
        shutil.rmtree(LEGACY_VARIANT_DIR)
    MARK_LOGO_ASSET.write_text(
        mark_svg("currentColor", "currentColor", "Mains Aegis mark"),
        encoding="utf-8",
    )
    MARK_COLOR_LIGHT_ASSET.write_text(
        app_mark_svg(
            LIGHT_THEME_LEFT,
            LIGHT_THEME_HANDOFF,
            LIGHT_THEME_RIGHT,
            "Mains Aegis mark, color for light surfaces",
        ),
        encoding="utf-8",
    )
    selected_dark_variant = DARK_APP_VARIANTS[0]
    MARK_COLOR_DARK_ASSET.write_text(
        app_mark_svg(
            selected_dark_variant.incoming_color,
            selected_dark_variant.handoff_color,
            selected_dark_variant.outgoing_color,
            "Mains Aegis mark, color for dark surfaces",
        ),
        encoding="utf-8",
    )
    LEGACY_SQUARE_LOCKUP_ASSET.unlink(missing_ok=True)
    for stale_asset in STALE_SQUARE_MONO_ASSETS:
        stale_asset.unlink(missing_ok=True)
    FINAL_SQUARE_LOCKUP_ASSET.write_text(final_square_lockup_svg(), encoding="utf-8")
    for variant in SQUARE_LOGO_VARIANTS:
        variant.asset.write_text(
            final_square_lockup_svg(
                title=variant.title,
                mark_colors=variant.mark_colors,
                wordmark_color=variant.wordmark_color,
            ),
            encoding="utf-8",
        )
    WIDE_LOGO_ASSET.write_text(wide_logo_svg(), encoding="utf-8")
    for variant in WIDE_LOGO_VARIANTS:
        variant.asset.write_text(
            wide_logo_svg(
                title=variant.title,
                mark_colors=variant.mark_colors,
                wordmark_color=variant.wordmark_color,
            ),
            encoding="utf-8",
        )
    FAVICON_ASSET.write_text(
        final_square_lockup_svg(
            color="#161B20",
            title="Mains Aegis monochrome favicon",
        ),
        encoding="utf-8",
    )
    DARK_FAVICON_ASSET.write_text(
        final_square_lockup_svg(
            color="#EAF7F0",
            title="Mains Aegis monochrome favicon for dark surfaces",
        ),
        encoding="utf-8",
    )
    write_platform_assets()
    manifest = {
        "geometry_source": str(MONO_REFERENCE.relative_to(ROOT)),
        "palette_source": str(COLOR_REFERENCE.relative_to(ROOT)),
        "source_crop": list(SOURCE_CROP),
        "view_box": [VIEWBOX_WIDTH, VIEWBOX_HEIGHT],
        "geometry_sha256": geometry_hash(),
        "semantic_paths": [
            {"id": entry.element_id, "ribbon": entry.ribbon} for entry in GEOMETRY
        ],
        "construction": {
            "curve_model": "tangent-continuous cubic Bezier sequences",
            "commands": ["M", "L", "C", "Z"],
            "raster_role": "fixed visual reference; antialiasing is not path geometry",
        },
        "light_theme": {
            "incoming": LIGHT_THEME_LEFT,
            "handoff": LIGHT_THEME_HANDOFF,
            "outgoing": LIGHT_THEME_RIGHT,
        },
        "dark_theme": selected_dark_variant.__dict__,
        "square_lockup": {
            "canvas": [SQUARE_LOCKUP_CANVAS, SQUARE_LOCKUP_CANVAS],
            "status": "approved 09 source-normalized square logo master",
            "purpose": "brand lockup and sole geometry source for platform application icons",
            "geometry": {
                "mark_x": SQUARE_LOCKUP_MARK_X,
                "mark_y": SQUARE_LOCKUP_MARK_Y,
                "mark_width": SQUARE_LOCKUP_MARK_WIDTH,
                "wordmark_x": SQUARE_LOCKUP_WORD_X,
                "wordmark_y": SQUARE_LOCKUP_WORD_Y,
                "wordmark_width": SQUARE_LOCKUP_WORD_WIDTH,
                "wordmark_height": SQUARE_LOCKUP_WORD_HEIGHT,
                "minimum_clear_gap": SQUARE_LOCKUP_MIN_GAP,
            },
            "wordmark": {
                "content": SQUARE_LOCKUP_WORD,
                "line_count": 1,
                "source": "fixed vector contour normalized from the selected 09 raster reference",
                "font_dependency": None,
                "style": "rounded instrument-panel capitals",
                "alpha_threshold": FINAL_WORDMARK_ALPHA_THRESHOLD,
                "trace_settings": FINAL_WORDMARK_TRACE_SETTINGS,
                "reference": {
                    "asset": str(APPROVED_WORDMARK_REFERENCE.relative_to(ROOT)),
                    "lockup": str(
                        APPROVED_WORDMARK_LOCKUP_REFERENCE.relative_to(ROOT)
                    ),
                    "cell": [3, 3],
                    "selection": "09",
                },
            },
            "approved_asset": str(FINAL_SQUARE_LOCKUP_ASSET.relative_to(ROOT)),
            "delivery_variants": [
                {
                    "asset": str(variant.asset.relative_to(ROOT)),
                    "mark_colors": list(variant.mark_colors),
                    "wordmark_color": variant.wordmark_color,
                    "proof_surface": variant.proof_surface,
                }
                for variant in SQUARE_LOGO_VARIANTS
            ],
            "wordmark_geometry_sha256": hashlib.sha256(
                final_wordmark_09_path().encode("utf-8")
            ).hexdigest(),
        },
        "platform_assets": {
            "source": str(FINAL_SQUARE_LOCKUP_ASSET.relative_to(ROOT)),
            "foreground": "approved 09 multi-color square lockup",
            "favicon": {
                "light": str(FAVICON_ASSET.relative_to(ROOT)),
                "dark": str(DARK_FAVICON_ASSET.relative_to(ROOT)),
                "mode": "single-color foreground with transparent canvas",
            },
            "application_icons": {
                "manifest_default": "light",
                "maskable_scale": PLATFORM_MASKABLE_SCALE,
                "light": {
                    key: str(platform_png_path("light", key).relative_to(ROOT))
                    for key in PLATFORM_ASSET_NAMES["light"]
                },
                "dark": {
                    key: str(platform_png_path("dark", key).relative_to(ROOT))
                    for key in PLATFORM_ASSET_NAMES["dark"]
                },
            },
        },
        "wide_logo": {
            "status": "site banner master",
            "canvas": [WIDE_LOGO_WIDTH, WIDE_LOGO_HEIGHT],
            "master_asset": str(WIDE_LOGO_ASSET.relative_to(ROOT)),
            "mark_transform": wide_mark_transform(),
            "wordmark_transform": wide_wordmark_transform(),
            "delivery_variants": [
                {
                    "asset": str(variant.asset.relative_to(ROOT)),
                    "mark_colors": list(variant.mark_colors),
                    "wordmark_color": variant.wordmark_color,
                    "proof_surface": variant.proof_surface,
                }
                for variant in WIDE_LOGO_VARIANTS
            ],
        },
        "variants": [variant.__dict__ for variant in VARIANTS],
    }
    MANIFEST_ASSET.write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )


def parse_paths(path: Path) -> list[ET.Element]:
    tree = ET.parse(path)
    return list(tree.getroot().iter("{http://www.w3.org/2000/svg}path"))


PATH_TOKEN_RE = re.compile(r"[MLCZ]|-?(?:\d+(?:\.\d*)?|\.\d+)")
PATH_ARITY = {"M": 2, "L": 2, "C": 6, "Z": 0}


def parse_path_commands(d: str) -> list[PathCommand]:
    """Parse the deliberately small, absolute SVG command vocabulary we emit."""
    tokens = PATH_TOKEN_RE.findall(d)
    if not tokens:
        raise ValueError("empty path data")
    if "".join(tokens) != re.sub(r"[\s,]+", "", d):
        raise ValueError("path data contains commands outside M/L/C/Z")

    commands: list[PathCommand] = []
    index = 0
    cursor = (0.0, 0.0)
    subpath_start: tuple[float, float] | None = None
    while index < len(tokens):
        command = tokens[index]
        if command not in PATH_ARITY:
            raise ValueError(f"unsupported path command: {command}")
        index += 1
        arity = PATH_ARITY[command]
        if command == "Z":
            if subpath_start is None:
                raise ValueError("closed path has no starting point")
            commands.append(PathCommand("Z", cursor, subpath_start))
            cursor = subpath_start
            continue
        if index + arity > len(tokens) or any(token in PATH_ARITY for token in tokens[index : index + arity]):
            raise ValueError(f"incomplete {command} command")
        values = tuple(float(token) for token in tokens[index : index + arity])
        index += arity
        if command == "M":
            end = (values[0], values[1])
            commands.append(PathCommand("M", cursor, end))
            cursor = end
            subpath_start = end
        elif command == "L":
            end = (values[0], values[1])
            commands.append(PathCommand("L", cursor, end))
            cursor = end
        else:
            control_1 = (values[0], values[1])
            control_2 = (values[2], values[3])
            end = (values[4], values[5])
            commands.append(PathCommand("C", cursor, end, control_1, control_2))
            cursor = end
    return commands


def tangent_angle_degrees(previous: PathCommand, current: PathCommand) -> float:
    assert previous.control_2 is not None
    assert current.control_1 is not None
    outgoing = (
        previous.end[0] - previous.control_2[0],
        previous.end[1] - previous.control_2[1],
    )
    incoming = (
        current.control_1[0] - current.start[0],
        current.control_1[1] - current.start[1],
    )
    outgoing_length = math.hypot(*outgoing)
    incoming_length = math.hypot(*incoming)
    if outgoing_length == 0 or incoming_length == 0:
        return 180.0
    cosine = (outgoing[0] * incoming[0] + outgoing[1] * incoming[1]) / (
        outgoing_length * incoming_length
    )
    return math.degrees(math.acos(max(-1.0, min(1.0, cosine))))


def curve_smoothness() -> dict[str, object]:
    """Check that curves are a small C1 construction rather than a pixel fit."""
    drawing_command_count = 0
    curve_segment_count = 0
    joins: list[dict[str, float | str]] = []
    violations: list[str] = []
    for entry in GEOMETRY:
        try:
            commands = parse_path_commands(entry.d)
        except ValueError as error:
            violations.append(f"{entry.element_id}: {error}")
            continue
        drawing_command_count += len(commands)
        curve_segment_count += sum(command.command == "C" for command in commands)
        previous: PathCommand | None = None
        first_curve: PathCommand | None = None
        for command in commands:
            if command.command == "M":
                previous = command
                first_curve = None
                continue
            if command.command == "C":
                if previous is not None and previous.command == "C":
                    joins.append(
                        {
                            "path_id": entry.element_id,
                            "at_x": command.start[0],
                            "at_y": command.start[1],
                            "angle_degrees": tangent_angle_degrees(previous, command),
                        }
                    )
                if first_curve is None:
                    first_curve = command
                previous = command
                continue
            if command.command == "Z":
                if previous is not None and previous.command == "C" and first_curve is not None:
                    joins.append(
                        {
                            "path_id": entry.element_id,
                            "at_x": command.end[0],
                            "at_y": command.end[1],
                            "angle_degrees": tangent_angle_degrees(previous, first_curve),
                        }
                    )
                previous = command
                continue
            previous = command

    max_join_angle = max((float(join["angle_degrees"]) for join in joins), default=0.0)
    if drawing_command_count > SMOOTHNESS_LIMITS["maximum_drawing_commands"]:
        violations.append(
            f"construction has too many drawing commands: {drawing_command_count}"
        )
    if curve_segment_count > SMOOTHNESS_LIMITS["maximum_curve_segments"]:
        violations.append(f"construction has too many cubic segments: {curve_segment_count}")
    if max_join_angle > SMOOTHNESS_LIMITS["maximum_curve_join_angle_degrees"]:
        violations.append(f"curve join angle is too large: {max_join_angle:.3f} degrees")
    return {
        "passed": not violations,
        "drawing_command_count": drawing_command_count,
        "curve_segment_count": curve_segment_count,
        "curve_join_count": len(joins),
        "max_curve_join_angle_degrees": max_join_angle,
        "joins": joins,
        "limits": SMOOTHNESS_LIMITS,
        "violations": violations,
    }


def vector_integrity() -> dict[str, object]:
    main_assets = [
        MARK_LOGO_ASSET,
        MARK_COLOR_LIGHT_ASSET,
        MARK_COLOR_DARK_ASSET,
    ]
    expected_d = [entry.d for entry in GEOMETRY]
    banned_tags = {"image", "filter", "mask", "pattern", "linearGradient", "radialGradient"}
    violations: list[str] = []
    hashes: dict[str, str] = {}
    for asset in main_assets:
        root = ET.parse(asset).getroot()
        for element in root.iter():
            tag = element.tag.rsplit("}", 1)[-1]
            if tag in banned_tags:
                violations.append(f"{asset.name}: banned <{tag}>")
            if any("data:" in value for value in element.attrib.values()):
                violations.append(f"{asset.name}: embedded data URI")
        paths = parse_paths(asset)
        found_d = [path.attrib.get("d", "") for path in paths]
        if len(paths) != 3:
            violations.append(f"{asset.name}: expected three paths, received {len(paths)}")
        if found_d != expected_d:
            violations.append(f"{asset.name}: geometry differs from master")
        hashes[asset.name] = hashlib.sha256("\n".join(found_d).encode("utf-8")).hexdigest()
    master_paths = parse_paths(MARK_LOGO_ASSET)
    expected_ids = [entry.element_id for entry in GEOMETRY]
    actual_ids = [path.attrib.get("id") for path in master_paths]
    if actual_ids != expected_ids:
        violations.append(f"master path ids differ: {actual_ids}")
    selected_dark_variant = DARK_APP_VARIANTS[0]
    expected_dark_fills = [
        selected_dark_variant.incoming_color,
        selected_dark_variant.handoff_color,
        selected_dark_variant.outgoing_color,
    ]
    dark_theme_paths = parse_paths(MARK_COLOR_DARK_ASSET)
    actual_dark_fills = [path.attrib.get("fill") for path in dark_theme_paths]
    if actual_dark_fills != expected_dark_fills:
        violations.append(
            "theme-dark fills differ from approved dark-flow-amber palette: "
            f"{actual_dark_fills}"
        )
    command_count = sum(len(re.findall(r"[MLCZ]", entry.d)) for entry in GEOMETRY)
    for entry in GEOMETRY:
        try:
            parse_path_commands(entry.d)
        except ValueError as error:
            violations.append(f"{entry.element_id}: {error}")
    return {
        "passed": not violations,
        "geometry_sha256": geometry_hash(),
        "asset_geometry_hashes": hashes,
        "path_count": len(master_paths),
        "path_ids": actual_ids,
        "drawing_command_count": command_count,
        "theme_dark_fills": actual_dark_fills,
        "banned_features": sorted(banned_tags),
        "violations": violations,
    }


def foreground_bounds(image: np.ndarray) -> tuple[int, int, int, int] | None:
    """Return the inclusive foreground bounds for a white-backed SVG render."""
    mask = mask_from_rgb(image)
    return mask_bounds(mask)


def mask_bounds(mask: np.ndarray) -> tuple[int, int, int, int] | None:
    ys, xs = np.where(mask)
    if not len(xs):
        return None
    return (int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max()))


def foreground_mask_against(image: np.ndarray, background: str) -> np.ndarray:
    rgb = np.array(tuple(bytes.fromhex(background[1:])), dtype=np.int16)
    difference = np.abs(image.astype(np.int16) - rgb)
    return np.max(difference, axis=2) >= 16


def selected_wordmark_reference_mask() -> np.ndarray:
    """Return the fixed-position 09 mask used in the approved raster review."""
    image = Image.open(APPROVED_WORDMARK_LOCKUP_REFERENCE).convert("RGBA")
    if image.size != (SQUARE_LOCKUP_CANVAS, SQUARE_LOCKUP_CANVAS):
        raise RuntimeError(f"Unexpected selected 09 lockup size: {image.size}")
    alpha = np.asarray(image)[:, :, 3]
    mask = np.zeros(alpha.shape, dtype=bool)
    x0 = int(SQUARE_LOCKUP_WORD_X)
    y0 = int(SQUARE_LOCKUP_WORD_Y)
    x1 = x0 + int(SQUARE_LOCKUP_WORD_WIDTH)
    y1 = y0 + int(SQUARE_LOCKUP_WORD_HEIGHT)
    mask[y0:y1, x0:x1] = alpha[y0:y1, x0:x1] >= FINAL_WORDMARK_ALPHA_THRESHOLD
    return mask


def make_wordmark_comparison(
    reference_mask: np.ndarray,
    vector_mask: np.ndarray,
    output: Path,
) -> None:
    """Write a fixed-coordinate source/vector/overlay proof for the 09 wordmark."""
    x0 = int(SQUARE_LOCKUP_WORD_X)
    y0 = int(SQUARE_LOCKUP_WORD_Y)
    x1 = x0 + int(SQUARE_LOCKUP_WORD_WIDTH)
    y1 = y0 + int(SQUARE_LOCKUP_WORD_HEIGHT)
    source = reference_mask[y0:y1, x0:x1]
    vector = vector_mask[y0:y1, x0:x1]
    scale = 3
    panel_width = source.shape[1] * scale
    panel_height = source.shape[0] * scale
    header = 48
    gutter = 26
    footer = 52
    canvas = Image.new(
        "RGB",
        (panel_width, (panel_height + header) * 3 + gutter * 2 + footer),
        "#F7F8F6",
    )
    draw = ImageDraw.Draw(canvas)

    source_image = np.full((*source.shape, 3), 255, dtype=np.uint8)
    source_image[source] = (18, 22, 25)
    vector_image = np.full((*vector.shape, 3), 255, dtype=np.uint8)
    vector_image[vector] = (18, 22, 25)
    overlay_image = np.full((*source.shape, 3), 255, dtype=np.uint8)
    overlay_image[source & vector] = (24, 29, 33)
    overlay_image[source & ~vector] = (221, 66, 70)
    overlay_image[vector & ~source] = (10, 116, 181)
    panels = (
        ("Selected 09 raster mask", source_image),
        ("Normalized SVG path", vector_image),
        ("Fixed-coordinate overlay", overlay_image),
    )
    y = 0
    for label, pixels in panels:
        draw.text((18, y + 11), label, fill="#30383C", font=sheet_font(24))
        panel = Image.fromarray(pixels, mode="RGB").resize(
            (panel_width, panel_height), Image.Resampling.NEAREST
        )
        canvas.paste(panel, (0, y + header))
        y += header + panel_height + gutter
    draw.text(
        (18, canvas.height - footer + 14),
        "Overlay: dark = shared, red = raster only, blue = SVG only",
        fill="#4C565B",
        font=sheet_font(20),
    )
    canvas.save(output)


def final_square_lockup_layer_svg(layer: str, color: str) -> str:
    if layer not in {"mark", "wordmark"}:
        raise ValueError(f"Unknown final lockup layer: {layer}")
    if layer == "mark":
        content = (
            f'  <g transform="{square_mark_transform()}">',
            square_mark_paths(include_ids=False, indent="    "),
            "  </g>",
        )
    else:
        content = (f'  <path d="{final_wordmark_09_path()}" fill="currentColor"/>',)
    return "\n".join(
        (
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"',
            f'  color="{color}">',
            *content,
            "</svg>",
            "",
        )
    )


def final_square_lockup_preview_svg() -> str:
    return "\n".join(
        (
            '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">',
            '  <rect width="1024" height="1024" fill="#FFFFFF"/>',
            f'  <g transform="{square_mark_transform()}" color="#000000">',
            square_mark_paths(include_ids=False, indent="    "),
            "  </g>",
            f'  <path d="{final_wordmark_09_path()}" fill="#000000"/>',
            "</svg>",
            "",
        )
    )


def final_square_lockup_construction_svg() -> str:
    mark_height = VIEWBOX_HEIGHT * SQUARE_LOCKUP_MARK_WIDTH / VIEWBOX_WIDTH
    actual_gap = SQUARE_LOCKUP_WORD_Y - (SQUARE_LOCKUP_MARK_Y + mark_height)
    return "\n".join(
        (
            '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">',
            '  <rect width="1024" height="1024" fill="#FFFFFF"/>',
            f'  <rect x="{SQUARE_LOCKUP_MARK_X}" y="{SQUARE_LOCKUP_MARK_Y}" width="{SQUARE_LOCKUP_MARK_WIDTH}" height="{mark_height:.4f}" fill="none" stroke="#C6C6C6" stroke-dasharray="6 6"/>',
            f'  <rect x="{SQUARE_LOCKUP_WORD_X}" y="{SQUARE_LOCKUP_WORD_Y}" width="{SQUARE_LOCKUP_WORD_WIDTH}" height="{SQUARE_LOCKUP_WORD_HEIGHT}" fill="none" stroke="#C6C6C6" stroke-dasharray="6 6"/>',
            '  <path d="M 512 120 L 512 870" fill="none" stroke="#E7E7E7" stroke-dasharray="4 8"/>',
            f'  <g transform="{square_mark_transform()}" color="#000000">',
            square_mark_paths(include_ids=False, indent="    "),
            "  </g>",
            f'  <path d="{final_wordmark_09_path()}" fill="#000000"/>',
            f'  <text x="152" y="224" fill="#717171" font-family="system-ui, sans-serif" font-size="18">06 mark: 720 x {mark_height:.1f}</text>',
            f'  <text x="192" y="814" fill="#717171" font-family="system-ui, sans-serif" font-size="18">09 source-normalized wordmark: 640 x {SQUARE_LOCKUP_WORD_HEIGHT:.0f}</text>',
            f'  <text x="532" y="625" fill="#717171" font-family="system-ui, sans-serif" font-size="18">clear gap: {actual_gap:.1f} (min {SQUARE_LOCKUP_MIN_GAP:.0f})</text>',
            "</svg>",
            "",
        )
    )


def square_lockup_integrity() -> dict[str, object]:
    """Validate the approved 09 lockup as an independent true-vector asset."""
    expected_mark_d = [entry.d for entry in GEOMETRY]
    expected_mark_ids = [entry.element_id for entry in GEOMETRY]
    expected_ids = [*expected_mark_ids, "wordmark-mains-aegis"]
    allowed_tags = {"svg", "title", "g", "path"}
    banned_tags = {
        "defs",
        "image",
        "filter",
        "mask",
        "pattern",
        "linearGradient",
        "radialGradient",
        "rect",
        "text",
        "use",
    }
    violations: list[str] = []
    if not FINAL_SQUARE_LOCKUP_ASSET.exists():
        violations.append("approved square lockup asset is missing")
        return {"passed": False, "violations": violations}
    if not APPROVED_WORDMARK_REFERENCE.exists():
        violations.append("selected 09 wordmark reference is missing")
    if not APPROVED_WORDMARK_LOCKUP_REFERENCE.exists():
        violations.append("selected 09 lockup reference is missing")

    raw = FINAL_SQUARE_LOCKUP_ASSET.read_text(encoding="utf-8")
    root = ET.parse(FINAL_SQUARE_LOCKUP_ASSET).getroot()
    if root.attrib.get("viewBox") != "0 0 1024 1024":
        violations.append("approved lockup does not use the 1024 square viewBox")
    if "color" in root.attrib:
        violations.append("approved lockup must inherit currentColor")
    for element in root.iter():
        tag = element.tag.rsplit("}", 1)[-1]
        if tag in banned_tags or tag not in allowed_tags:
            violations.append(f"approved lockup contains non-delivery <{tag}> element")
        if any("data:" in value for value in element.attrib.values()):
            violations.append("approved lockup contains an embedded data URI")
        if "font" in " ".join(element.attrib.keys()).lower():
            violations.append("approved lockup contains a font dependency attribute")
    if any(token in raw.lower() for token in ("data:", "rotate(", "font-", "<style")):
        violations.append("approved lockup contains a banned raster, rotation, or font feature")

    groups = list(root.iter("{http://www.w3.org/2000/svg}g"))
    if len(groups) != 1 or groups[0].attrib.get("id") != "mark-06":
        violations.append("approved lockup must contain exactly one mark-06 group")
    elif groups[0].attrib.get("transform") != square_mark_transform():
        violations.append("approved lockup changes the fixed 06 transform")

    paths = parse_paths(FINAL_SQUARE_LOCKUP_ASSET)
    ids = [path.attrib.get("id") for path in paths]
    path_data = [path.attrib.get("d", "") for path in paths]
    if len(paths) != 4:
        violations.append(f"approved lockup must contain four paths, received {len(paths)}")
    if ids != expected_ids:
        violations.append(f"approved lockup path ids/order differ: {ids}")
    if path_data[:3] != expected_mark_d:
        violations.append("approved lockup 06 geometry differs from the horizontal master")
    expected_word = final_wordmark_09_path()
    if len(path_data) < 4 or path_data[3] != expected_word:
        violations.append("approved wordmark geometry differs from the selected 09 construction")
    word_path = paths[3] if len(paths) > 3 else None
    if word_path is not None:
        if word_path.attrib.get("fill") != "currentColor":
            violations.append("approved wordmark must use currentColor")
        if "transform" in word_path.attrib or "stroke" in word_path.attrib:
            violations.append("approved wordmark must be a direct filled path")
        try:
            commands = parse_path_commands(word_path.attrib.get("d", ""))
            open_contour = False
            for command in commands:
                if command.command == "M":
                    if open_contour:
                        violations.append("approved wordmark has an unclosed contour")
                    open_contour = True
                elif command.command == "Z":
                    open_contour = False
            if open_contour:
                violations.append("approved wordmark final contour is open")
        except ValueError as error:
            violations.append(f"approved wordmark path parse failed: {error}")
    return {
        "passed": not violations,
        "asset": str(FINAL_SQUARE_LOCKUP_ASSET.relative_to(ROOT)),
        "selection": "09",
        "style": FINAL_WORDMARK_NAME,
        "expected_mark_geometry_sha256": geometry_hash(),
        "wordmark_geometry_sha256": hashlib.sha256(expected_word.encode("utf-8")).hexdigest(),
        "expected_path_ids": expected_ids,
        "allowed_tags": sorted(allowed_tags),
        "banned_features": sorted(banned_tags),
        "violations": violations,
    }


def square_logo_variant_integrity() -> dict[str, object]:
    """Verify that delivery variants change fills only, never logo geometry."""
    expected_ids = [entry.element_id for entry in GEOMETRY] + ["wordmark-mains-aegis"]
    expected_paths = [entry.d for entry in GEOMETRY] + [final_wordmark_09_path()]
    results: list[dict[str, object]] = []
    all_passed = True
    for variant in SQUARE_LOGO_VARIANTS:
        violations: list[str] = []
        if not variant.asset.exists():
            results.append(
                {
                    "asset": str(variant.asset.relative_to(ROOT)),
                    "passed": False,
                    "violations": ["delivery variant is missing"],
                }
            )
            all_passed = False
            continue
        raw = variant.asset.read_text(encoding="utf-8")
        root = ET.parse(variant.asset).getroot()
        paths = parse_paths(variant.asset)
        ids = [path.attrib.get("id") for path in paths]
        path_data = [path.attrib.get("d", "") for path in paths]
        fills = [path.attrib.get("fill") for path in paths]
        expected_fills = [*variant.mark_colors, variant.wordmark_color]
        groups = list(root.iter("{http://www.w3.org/2000/svg}g"))
        if root.attrib.get("viewBox") != "0 0 1024 1024":
            violations.append("variant does not use the canonical square viewBox")
        if ids != expected_ids:
            violations.append("variant path ids or drawing order differ from the master")
        if path_data != expected_paths:
            violations.append("variant path geometry differs from the master")
        if fills != expected_fills:
            violations.append(f"variant fills differ: {fills}")
        if len(groups) != 1 or groups[0].attrib.get("transform") != square_mark_transform():
            violations.append("variant mark transform differs from the master")
        if any(token in raw.lower() for token in ("<rect", "<image", "data:", "<style", "filter", "mask")):
            violations.append("variant contains a background, raster, style, filter, or mask")
        contrast = {
            color: contrast_ratio(color, variant.proof_surface)
            for color in dict.fromkeys(expected_fills)
        }
        if min(contrast.values()) < 4.5:
            violations.append("variant foreground contrast is below 4.5:1 on its target surface")
        passed = not violations
        all_passed = all_passed and passed
        results.append(
            {
                "asset": str(variant.asset.relative_to(ROOT)),
                "passed": passed,
                "mark_colors": list(variant.mark_colors),
                "wordmark_color": variant.wordmark_color,
                "proof_surface": variant.proof_surface,
                "contrast_ratios": contrast,
                "path_data_sha256": hashlib.sha256(
                    "\n".join(path_data).encode("utf-8")
                ).hexdigest(),
                "violations": violations,
            }
        )
    return {"passed": all_passed, "variants": results}


def make_square_logo_variant_proof() -> Path:
    """Render the four transparent delivery variants on their intended surfaces."""
    proof_path = SQUARE_LOCKUP_VALIDATION_DIR / "mains-aegis-logo-square-variants.png"
    tile_size = 500
    label_height = 44
    gutter = 24
    columns = 2
    rows = math.ceil(len(SQUARE_LOGO_VARIANTS) / columns)
    canvas = Image.new(
        "RGB",
        (
            tile_size * columns + gutter * (columns + 1),
            (tile_size + label_height) * rows + gutter * (rows + 1),
        ),
        "#D9DEDB",
    )
    labels = ("COLOR / LIGHT", "COLOR / DARK")
    for index, (variant, label) in enumerate(zip(SQUARE_LOGO_VARIANTS, labels, strict=True)):
        row, column = divmod(index, columns)
        x = gutter + column * (tile_size + gutter)
        y = gutter + row * (tile_size + label_height + gutter)
        panel = Image.new("RGB", (tile_size, tile_size + label_height), variant.proof_surface)
        rendered_path = SQUARE_LOCKUP_VALIDATION_DIR / f"{variant.asset.stem}-proof.png"
        render_svg(
            variant.asset,
            rendered_path,
            tile_size,
            tile_size,
            background=variant.proof_surface,
        )
        panel.paste(Image.open(rendered_path).convert("RGB"), (0, label_height))
        draw = ImageDraw.Draw(panel)
        label_color = "#161B20" if relative_luminance(variant.proof_surface) > 0.5 else "#EAF7F0"
        draw.text((18, 12), label, fill=label_color, font=sheet_font(18))
        canvas.paste(panel, (x, y))
    canvas.save(proof_path)
    return proof_path


def wide_logo_integrity() -> dict[str, object]:
    """Validate the wide master and its theme color variants."""
    expected_ids = [entry.element_id for entry in GEOMETRY] + ["wordmark-mains-aegis"]
    expected_paths = [entry.d for entry in GEOMETRY] + [final_wordmark_09_path()]
    definitions: list[tuple[Path, list[str], str]] = [
        (WIDE_LOGO_ASSET, ["currentColor"] * 4, "#FFFFFF"),
        *[
            (
                variant.asset,
                [*variant.mark_colors, variant.wordmark_color],
                variant.proof_surface,
            )
            for variant in WIDE_LOGO_VARIANTS
        ],
    ]
    results: list[dict[str, object]] = []
    all_passed = True
    for asset, expected_fills, proof_surface in definitions:
        violations: list[str] = []
        if not asset.exists():
            results.append(
                {
                    "asset": str(asset.relative_to(ROOT)),
                    "passed": False,
                    "violations": ["wide logo asset is missing"],
                }
            )
            all_passed = False
            continue
        raw = asset.read_text(encoding="utf-8")
        root = ET.parse(asset).getroot()
        paths = parse_paths(asset)
        ids = [path.attrib.get("id") for path in paths]
        path_data = [path.attrib.get("d", "") for path in paths]
        fills = [path.attrib.get("fill") for path in paths]
        groups = list(root.iter("{http://www.w3.org/2000/svg}g"))
        if root.attrib.get("viewBox") != f"0 0 {WIDE_LOGO_WIDTH} {WIDE_LOGO_HEIGHT}":
            violations.append("wide logo viewBox differs from the banner master")
        if ids != expected_ids or path_data != expected_paths:
            violations.append("wide logo path ids, order, or geometry differ from the master")
        if fills != expected_fills:
            violations.append(f"wide logo fills differ: {fills}")
        if len(groups) != 1 or groups[0].attrib.get("transform") != wide_mark_transform():
            violations.append("wide logo mark transform differs from the fixed layout")
        if len(paths) != 4 or paths[-1].attrib.get("transform") != wide_wordmark_transform():
            violations.append("wide logo wordmark transform differs from the fixed layout")
        if any(token in raw.lower() for token in ("<rect", "<image", "data:", "<style", "filter", "mask")):
            violations.append("wide logo contains a background, raster, style, filter, or mask")
        contrast: dict[str, float] = {}
        if "currentColor" not in expected_fills:
            contrast = {
                color: contrast_ratio(color, proof_surface)
                for color in dict.fromkeys(expected_fills)
            }
            if min(contrast.values()) < 4.5:
                violations.append("wide logo contrast is below 4.5:1 on its target surface")
        render_sizes: dict[str, dict[str, object]] = {}
        bounds: tuple[int, int, int, int] | None = None
        for render_width in (1600, 800, 400, 300, 200):
            render_height = render_width * WIDE_LOGO_HEIGHT // WIDE_LOGO_WIDTH
            render_path = SQUARE_LOCKUP_VALIDATION_DIR / f"{asset.stem}-{render_width}.png"
            render_svg(
                asset,
                render_path,
                render_width,
                render_height,
                background=proof_surface,
            )
            image = np.asarray(Image.open(render_path).convert("RGB"))
            foreground = foreground_mask_against(image, proof_surface)
            current_bounds = mask_bounds(foreground)
            split = round(render_width * 650 / WIDE_LOGO_WIDTH)
            both_parts_visible = bool(foreground[:, :split].any() and foreground[:, split:].any())
            size_passed = current_bounds is not None and both_parts_visible
            if not size_passed:
                violations.append(
                    f"wide logo mark or wordmark is empty at {render_width}x{render_height}"
                )
            render_sizes[str(render_width)] = {
                "height": render_height,
                "bounds": list(current_bounds) if current_bounds is not None else None,
                "mark_and_wordmark_visible": both_parts_visible,
                "passed": size_passed,
            }
            if render_width == WIDE_LOGO_WIDTH:
                bounds = current_bounds
        if bounds is not None:
            x0, y0, x1, y1 = bounds
            if x0 < 45 or y0 < 45 or x1 > WIDE_LOGO_WIDTH - 45 or y1 > WIDE_LOGO_HEIGHT - 45:
                violations.append(f"wide logo violates the fixed optical margins: {bounds}")
        passed = not violations
        all_passed = all_passed and passed
        results.append(
            {
                "asset": str(asset.relative_to(ROOT)),
                "passed": passed,
                "fills": expected_fills,
                "proof_surface": proof_surface,
                "contrast_ratios": contrast,
                "bounds": list(bounds) if bounds is not None else None,
                "render_sizes": render_sizes,
                "path_data_sha256": hashlib.sha256(
                    "\n".join(path_data).encode("utf-8")
                ).hexdigest(),
                "violations": violations,
            }
        )
    return {"passed": all_passed, "assets": results}


def make_wide_logo_proof() -> Path:
    proof_path = SQUARE_LOCKUP_VALIDATION_DIR / "mains-aegis-logo-wide-variants.png"
    panel_width = 1200
    panel_height = 195
    label_height = 42
    gutter = 20
    entries = (
        (WIDE_LOGO_ASSET, "MONO / CURRENTCOLOR", "#FFFFFF"),
        (WIDE_COLOR_LIGHT_ASSET, "COLOR / LIGHT", LIGHT_CANVAS),
        (WIDE_COLOR_DARK_ASSET, "COLOR / DARK", DARK_CANVAS),
    )
    canvas = Image.new(
        "RGB",
        (panel_width + gutter * 2, (panel_height + label_height) * len(entries) + gutter * 4),
        "#D9DEDB",
    )
    for index, (asset, label, surface) in enumerate(entries):
        y = gutter + index * (panel_height + label_height + gutter)
        panel = Image.new("RGB", (panel_width, panel_height + label_height), surface)
        rendered_path = SQUARE_LOCKUP_VALIDATION_DIR / f"{asset.stem}-proof.png"
        render_svg(asset, rendered_path, panel_width, panel_height, background=surface)
        panel.paste(Image.open(rendered_path).convert("RGB"), (0, label_height))
        label_color = "#161B20" if relative_luminance(surface) > 0.5 else "#EAF7F0"
        ImageDraw.Draw(panel).text((18, 11), label, fill=label_color, font=sheet_font(18))
        canvas.paste(panel, (gutter, y))
    canvas.save(proof_path)
    return proof_path


def platform_asset_integrity() -> dict[str, object]:
    """Validate exact 09 geometry, palette, dimensions, and maskable bounds."""
    expected_ids = [entry.element_id for entry in GEOMETRY] + ["wordmark-mains-aegis"]
    expected_paths = [entry.d for entry in GEOMETRY] + [final_wordmark_09_path()]
    violations: list[str] = []
    favicon_results: list[dict[str, object]] = []
    for favicon, color in ((FAVICON_ASSET, "#161B20"), (DARK_FAVICON_ASSET, "#EAF7F0")):
        favicon_violations: list[str] = []
        if not favicon.exists():
            favicon_violations.append("favicon is missing")
        else:
            raw = favicon.read_text(encoding="utf-8")
            root = ET.parse(favicon).getroot()
            paths = parse_paths(favicon)
            if root.attrib.get("viewBox") != "0 0 1024 1024":
                favicon_violations.append("favicon does not use the square 09 viewBox")
            if root.attrib.get("color") != color:
                favicon_violations.append("favicon foreground color differs from the selected monochrome color")
            if [path.attrib.get("id") for path in paths] != expected_ids:
                favicon_violations.append("favicon path ids/order differ from 09")
            if [path.attrib.get("d", "") for path in paths] != expected_paths:
                favicon_violations.append("favicon geometry differs from 09")
            if [path.attrib.get("fill") for path in paths] != ["currentColor"] * 4:
                favicon_violations.append("favicon is not single-color currentColor geometry")
            if any(element.tag.rsplit("}", 1)[-1] in {"rect", "image", "filter", "mask", "pattern", "text"} for element in root.iter()):
                favicon_violations.append("favicon contains a background or raster feature")
            if "M18 42V20" in raw:
                favicon_violations.append("favicon contains the retired M monogram")
        favicon_results.append(
            {
                "asset": str(favicon.relative_to(ROOT)),
                "color": color,
                "passed": not favicon_violations,
                "violations": favicon_violations,
            }
        )
        violations.extend(f"{favicon.name}: {item}" for item in favicon_violations)

    platform_results: list[dict[str, object]] = []
    for theme, variant in platform_variant_definitions():
        expected_fills = [*variant.mark_colors, variant.wordmark_color]
        theme_violations: list[str] = []
        for kind in ("standard", "maskable"):
            source = platform_svg_path(theme, kind)
            if not source.exists():
                theme_violations.append(f"missing platform SVG source: {source.name}")
                continue
            root = ET.parse(source).getroot()
            paths = parse_paths(source)
            raw = source.read_text(encoding="utf-8")
            if root.attrib.get("viewBox") != "0 0 1024 1024":
                theme_violations.append(f"{source.name}: wrong viewBox")
            if [path.attrib.get("id") for path in paths] != expected_ids:
                theme_violations.append(f"{source.name}: path ids/order differ from 09")
            if [path.attrib.get("d", "") for path in paths] != expected_paths:
                theme_violations.append(f"{source.name}: path geometry differs from 09")
            if [path.attrib.get("fill") for path in paths] != expected_fills:
                theme_violations.append(f"{source.name}: multicolor fills differ")
            if "M18 42V20" in raw:
                theme_violations.append(f"{source.name}: contains the retired M monogram")
            if any(token in raw.lower() for token in ("<image", "data:", "<filter", "<mask", "<pattern", "gradient", "<text")):
                theme_violations.append(f"{source.name}: contains a forbidden asset feature")

        image_results: list[dict[str, object]] = []
        for kind, size in (("icon_192", 192), ("icon_512", 512), ("maskable_512", 512), ("apple_touch_180", 180)):
            image_path = platform_png_path(theme, kind)
            image_violations: list[str] = []
            if not image_path.exists():
                image_violations.append("platform PNG is missing")
                image_results.append({"asset": str(image_path.relative_to(ROOT)), "passed": False, "violations": image_violations})
                theme_violations.extend(f"{image_path.name}: {item}" for item in image_violations)
                continue
            image = Image.open(image_path).convert("RGB")
            if image.size != (size, size):
                image_violations.append(f"expected {size}x{size}, received {image.size}")
            background_rgb = tuple(int(variant.proof_surface[offset : offset + 2], 16) for offset in (1, 3, 5))
            colors_present = {
                color: any(
                    pixel == tuple(int(color[offset : offset + 2], 16) for offset in (1, 3, 5))
                    for pixel in image.getdata()
                )
                for color in expected_fills
            }
            if not all(colors_present.values()):
                image_violations.append(f"missing exact multicolor interior pixels: {colors_present}")
            bounds = mask_bounds(np.any(np.asarray(image) != background_rgb, axis=2))
            if kind == "maskable_512":
                safe_inset = round(size / 6)
                safe_limit = size - safe_inset
                if bounds is None or bounds[0] < safe_inset or bounds[1] < safe_inset or bounds[2] > safe_limit or bounds[3] > safe_limit:
                    image_violations.append(f"foreground escapes 66% maskable safe zone: {bounds}")
            image_results.append(
                {
                    "asset": str(image_path.relative_to(ROOT)),
                    "size": list(image.size),
                    "background": variant.proof_surface,
                    "foreground_bounds": list(bounds) if bounds else None,
                    "colors_present": colors_present,
                    "passed": not image_violations,
                    "violations": image_violations,
                }
            )
            theme_violations.extend(f"{image_path.name}: {item}" for item in image_violations)
        platform_results.append(
            {
                "theme": theme,
                "background": variant.proof_surface,
                "foreground_colors": expected_fills,
                "images": image_results,
                "passed": not theme_violations,
                "violations": theme_violations,
            }
        )
        violations.extend(f"{theme}: {item}" for item in theme_violations)
    return {
        "passed": not violations,
        "favicons": favicon_results,
        "application_icons": platform_results,
        "violations": violations,
    }


def make_platform_proof() -> Path:
    """Create a three-group proof: light app, dark app, monochrome favicon."""
    proof_path = PLATFORM_VALIDATION_DIR / "mains-aegis-platform-assets.png"
    panel_width = 520
    panel_height = 420
    gutter = 24
    canvas = Image.new("RGB", (panel_width * 3 + gutter * 4, panel_height + gutter * 2), "#D9DEDB")
    entries = (
        ("LIGHT MULTICOLOR APP", platform_png_path("light", "icon_512"), LIGHT_CANVAS),
        ("DARK MULTICOLOR APP", platform_png_path("dark", "icon_512"), DARK_CANVAS),
        ("MONOCHROME FAVICON", FAVICON_ASSET, "#FFFFFF"),
    )
    for index, (label, source, surface) in enumerate(entries):
        x = gutter + index * (panel_width + gutter)
        panel = Image.new("RGB", (panel_width, panel_height), surface)
        if source.suffix.lower() == ".svg":
            rendered = PLATFORM_VALIDATION_DIR / f"proof-favicon-{index}.png"
            render_svg(source, rendered, 320, 320, background=surface)
            image = Image.open(rendered).convert("RGB")
        else:
            image = Image.open(source).convert("RGB")
            image.thumbnail((320, 320), Image.Resampling.LANCZOS)
        panel.paste(image, ((panel_width - image.width) // 2, 70))
        label_color = "#161B20" if relative_luminance(surface) > 0.5 else "#EAF7F0"
        ImageDraw.Draw(panel).text((20, 22), label, fill=label_color, font=sheet_font(18))
        canvas.paste(panel, (x, gutter))
    canvas.save(proof_path)
    return proof_path


def square_lockup_render_validation() -> dict[str, object]:
    """Render the approved 09 lockup at fixed sizes and theme surfaces."""
    output_dir = SQUARE_LOCKUP_VALIDATION_DIR / "final-renders"
    source_dir = SQUARE_LOCKUP_VALIDATION_DIR / "final-render-sources"
    output_dir.mkdir(parents=True, exist_ok=True)
    source_dir.mkdir(parents=True, exist_ok=True)
    violations: list[str] = []
    mark_source = source_dir / "09-mark.svg"
    word_source = source_dir / "09-wordmark.svg"
    mark_source.write_text(final_square_lockup_layer_svg("mark", "#000000"), encoding="utf-8")
    word_source.write_text(final_square_lockup_layer_svg("wordmark", "#000000"), encoding="utf-8")
    mark_png = output_dir / "09-mark-1024.png"
    word_png = output_dir / "09-wordmark-1024.png"
    render_svg(mark_source, mark_png, 1024, 1024)
    render_svg(word_source, word_png, 1024, 1024)
    mark_image = np.asarray(Image.open(mark_png).convert("RGB"))
    word_image = np.asarray(Image.open(word_png).convert("RGB"))
    mark_mask = foreground_mask_against(mark_image, "#FFFFFF")
    word_mask = foreground_mask_against(word_image, "#FFFFFF")
    reference_word_mask = selected_wordmark_reference_mask()
    comparison_word_mask = mask_from_rgb(word_image)
    wordmark_metrics = contour_metrics(reference_word_mask, comparison_word_mask)
    wordmark_reference_passed = (
        wordmark_metrics["iou"] >= WORDMARK_REFERENCE_LIMITS["minimum_iou"]
        and wordmark_metrics["xor_pixels"]
        <= WORDMARK_REFERENCE_LIMITS["maximum_xor_pixels"]
        and wordmark_metrics["p99_contour_offset_px"]
        <= WORDMARK_REFERENCE_LIMITS["maximum_p99_contour_offset_px"]
        and wordmark_metrics["max_contour_offset_px"]
        <= WORDMARK_REFERENCE_LIMITS["maximum_contour_offset_px"]
    )
    if not wordmark_reference_passed:
        violations.append("approved wordmark differs materially from the selected 09 raster")
    wordmark_comparison = SQUARE_LOCKUP_VALIDATION_DIR / "wordmark-09-reference-comparison.png"
    make_wordmark_comparison(reference_word_mask, comparison_word_mask, wordmark_comparison)
    mark_bounds = mask_bounds(mark_mask)
    word_bounds = mask_bounds(word_mask)
    if mark_bounds is None or word_bounds is None:
        violations.append("approved lockup mark or wordmark is empty at 1024px")
        layout: dict[str, object] = {
            "passed": False,
            "mark_bounds": mark_bounds,
            "wordmark_bounds": word_bounds,
        }
    else:
        mark_x0, mark_y0, mark_x1, mark_y1 = mark_bounds
        word_x0, word_y0, word_x1, word_y1 = word_bounds
        mark_width = mark_x1 - mark_x0 + 1
        word_width = word_x1 - word_x0 + 1
        word_height = word_y1 - word_y0 + 1
        mark_center = (mark_x0 + mark_x1) / 2
        word_center = (word_x0 + word_x1) / 2
        gap = word_y0 - mark_y1 - 1
        layout_passed = (
            abs(mark_width - SQUARE_LOCKUP_MARK_WIDTH) <= 3
            and abs(word_width - SQUARE_LOCKUP_WORD_WIDTH) <= 3
            and abs(word_height - SQUARE_LOCKUP_WORD_HEIGHT) <= 3
            and abs(mark_center - SQUARE_LOCKUP_CANVAS / 2) <= 2
            and abs(word_center - SQUARE_LOCKUP_CANVAS / 2) <= 2
            and gap >= SQUARE_LOCKUP_MIN_GAP
            and mark_mask.sum() >= word_mask.sum() * 2.5
            and mark_x0 > 0
            and mark_y0 > 0
            and mark_x1 < SQUARE_LOCKUP_CANVAS - 1
            and mark_y1 < SQUARE_LOCKUP_CANVAS - 1
            and word_x0 > 0
            and word_y0 > 0
            and word_x1 < SQUARE_LOCKUP_CANVAS - 1
            and word_y1 < SQUARE_LOCKUP_CANVAS - 1
        )
        if not layout_passed:
            violations.append("approved 09 lockup fixed layout constraints failed")
        layout = {
            "passed": layout_passed,
            "mark_bounds": list(mark_bounds),
            "wordmark_bounds": list(word_bounds),
            "mark_width": mark_width,
            "wordmark_width": word_width,
            "wordmark_height": word_height,
            "mark_center": mark_center,
            "wordmark_center": word_center,
            "clear_gap": gap,
            "area_ratio": float(mark_mask.sum() / word_mask.sum()),
        }

    surfaces: dict[str, dict[str, object]] = {}
    for surface_name, background, foreground in SQUARE_LOCKUP_SURFACES:
        source = source_dir / f"09-{surface_name}.svg"
        source.write_text(final_square_lockup_svg(color=foreground), encoding="utf-8")
        size_results: dict[str, dict[str, object]] = {}
        for size in (1024, 512, 256, 128, 64):
            png = output_dir / f"09-{surface_name}-{size}.png"
            render_svg(source, png, size, size, background=background)
            image = np.asarray(Image.open(png).convert("RGB"))
            bounds = mask_bounds(foreground_mask_against(image, background))
            passed = bounds is not None
            if not passed:
                violations.append(f"approved lockup is empty on {surface_name} at {size}px")
            size_results[str(size)] = {
                "bounds": list(bounds) if bounds is not None else None,
                "passed": passed,
            }
        surfaces[surface_name] = {
            "background": background,
            "foreground": foreground,
            "sizes": size_results,
        }

    preview_svg = SQUARE_LOCKUP_VALIDATION_DIR / "mains-aegis-logo-square-proof.svg"
    preview_png = SQUARE_LOCKUP_VALIDATION_DIR / "mains-aegis-logo-square-proof.png"
    construction_svg = SQUARE_LOCKUP_VALIDATION_DIR / "mains-aegis-logo-square-construction.svg"
    construction_png = SQUARE_LOCKUP_VALIDATION_DIR / "mains-aegis-logo-square-construction.png"
    preview_svg.write_text(final_square_lockup_preview_svg(), encoding="utf-8")
    construction_svg.write_text(final_square_lockup_construction_svg(), encoding="utf-8")
    render_svg(preview_svg, preview_png, 1024, 1024)
    render_svg(construction_svg, construction_png, 1024, 1024)
    return {
        "passed": not violations,
        "renderer": "rsvg-convert",
        "asset": str(FINAL_SQUARE_LOCKUP_ASSET.relative_to(ROOT)),
        "sizes": [1024, 512, 256, 128, 64],
        "surfaces": [
            {"name": name, "background": background, "foreground": foreground}
            for name, background, foreground in SQUARE_LOCKUP_SURFACES
        ],
        "registration": "fixed SVG coordinates; no crop, translation, or fit alignment",
        "preview_svg": str(preview_svg.relative_to(ROOT)),
        "preview_png": str(preview_png.relative_to(ROOT)),
        "construction_svg": str(construction_svg.relative_to(ROOT)),
        "construction_png": str(construction_png.relative_to(ROOT)),
        "wordmark_reference": {
            "passed": wordmark_reference_passed,
            "source": str(APPROVED_WORDMARK_REFERENCE.relative_to(ROOT)),
            "source_cell": [3, 3],
            "lockup_reference": str(
                APPROVED_WORDMARK_LOCKUP_REFERENCE.relative_to(ROOT)
            ),
            "alpha_threshold": FINAL_WORDMARK_ALPHA_THRESHOLD,
            "trace_settings": FINAL_WORDMARK_TRACE_SETTINGS,
            "limits": WORDMARK_REFERENCE_LIMITS,
            "metrics": wordmark_metrics,
            "comparison": str(wordmark_comparison.relative_to(ROOT)),
            "registration": "fixed 1024 coordinates; no translation, scaling, or fit alignment",
        },
        "layout": layout,
        "surfaces_result": surfaces,
        "violations": violations,
    }


def color_metrics() -> list[dict[str, int | str]]:
    result = []
    for variant in VARIANTS:
        source_path = VALIDATION_DIR / f"color-source-{variant.name}.svg"
        rendered_path = VALIDATION_DIR / f"{variant.name}.png"
        source_path.write_text(
            mark_svg(variant.left_color, variant.right_color, f"palette validation {variant.name}"),
            encoding="utf-8",
        )
        render_svg(source_path, rendered_path, VIEWBOX_WIDTH, VIEWBOX_HEIGHT)
        image = np.asarray(Image.open(rendered_path).convert("RGB"))
        # At three pixels from an edge, SVG flat fills must be byte-exact.
        image_mask = mask_from_rgb(image)
        components = component_masks(image_mask)
        component_colours = (variant.left_color, variant.left_color, variant.right_color)
        mismatch = 0
        tested = 0
        for component, colour in zip(components, component_colours, strict=True):
            interior = ndimage.binary_erosion(component, iterations=3)
            expected_rgb = np.array(tuple(bytes.fromhex(colour[1:])), dtype=np.uint8)
            actual = image[interior]
            tested += int(interior.sum())
            mismatch += int(np.any(actual != expected_rgb, axis=1).sum())
        result.append(
            {
                "name": variant.name,
                "left_color": variant.left_color,
                "right_color": variant.right_color,
                "interior_pixels": tested,
                "interior_rgb_diff_pixels": mismatch,
            }
        )
    return result


def make_native_overlay(reference: np.ndarray, rendered: np.ndarray, output: Path) -> None:
    reference_mask = mask_from_rgb(reference)
    rendered_mask = mask_from_rgb(rendered)
    image = np.full_like(reference, 255)
    shared = reference_mask & rendered_mask
    source_only = reference_mask & ~rendered_mask
    vector_only = rendered_mask & ~reference_mask
    image[shared] = (20, 27, 32)
    image[source_only] = (222, 65, 70)
    image[vector_only] = (11, 116, 181)
    Image.fromarray(image, mode="RGB").save(output)


def make_eight_x_overlay(reference: np.ndarray, rendered: np.ndarray, output: Path) -> None:
    reference_mask = mask_from_rgb(reference)
    rendered_mask = mask_from_rgb(rendered)
    scale = 8
    reference_8x = np.repeat(np.repeat(reference_mask, scale, axis=0), scale, axis=1)
    rendered_8x = np.repeat(np.repeat(rendered_mask, scale, axis=0), scale, axis=1)
    image = np.full((*reference_8x.shape, 3), 255, dtype=np.uint8)
    image[reference_8x & rendered_8x] = (25, 31, 35)
    image[reference_8x & ~rendered_8x] = (225, 79, 78)
    image[rendered_8x & ~reference_8x] = (22, 122, 187)
    Image.fromarray(image, mode="RGB").save(output)


def sheet_font(size: int) -> ImageFont.ImageFont:
    for candidate in (
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    ):
        try:
            return ImageFont.truetype(candidate, size)
        except OSError:
            continue
    return ImageFont.load_default()


def draw_arrow(
    draw: ImageDraw.ImageDraw,
    start: tuple[int, int],
    end: tuple[int, int],
    *,
    fill: str,
    width: int,
) -> None:
    draw.line((start, end), fill=fill, width=width)
    angle = math.atan2(end[1] - start[1], end[0] - start[0])
    length = width * 3.2
    left = (
        end[0] - length * math.cos(angle - math.pi / 6),
        end[1] - length * math.sin(angle - math.pi / 6),
    )
    right = (
        end[0] - length * math.cos(angle + math.pi / 6),
        end[1] - length * math.sin(angle + math.pi / 6),
    )
    draw.polygon((end, left, right), fill=fill)


def make_construction_sheet(reference: np.ndarray, output: Path) -> None:
    scale = 2
    gutter = 70
    header = 92
    footer = 146
    panel_width = VIEWBOX_WIDTH * scale
    panel_height = VIEWBOX_HEIGHT * scale
    canvas = Image.new(
        "RGB",
        (panel_width * 2 + gutter * 3, panel_height + header + footer + gutter * 2),
        "#F7F8F6",
    )
    construction_svg_path = VALIDATION_DIR / "construction-vector.svg"
    construction_png_path = VALIDATION_DIR / "construction-vector.png"
    construction_svg_path.write_text(construction_svg(), encoding="utf-8")
    render_svg(construction_svg_path, construction_png_path, panel_width, panel_height)
    draw = ImageDraw.Draw(canvas)
    source_panel = Image.fromarray(reference, mode="RGB").resize(
        (panel_width, panel_height), Image.Resampling.NEAREST
    )
    vector_panel = Image.open(construction_png_path).convert("RGB")
    left_x = gutter
    right_x = left_x + panel_width + gutter
    panel_y = header + gutter
    canvas.paste(source_panel, (left_x, panel_y))
    canvas.paste(vector_panel, (right_x, panel_y))
    draw.text((left_x, 25), "Selected 06: fixed geometry reference", fill="#1E292E", font=sheet_font(31))
    draw.text((right_x, 25), "Vector construction: three semantic paths", fill="#1E292E", font=sheet_font(31))
    draw.rectangle(
        (right_x, panel_y, right_x + panel_width - 1, panel_y + panel_height - 1),
        outline="#94A2A7",
        width=2,
    )

    # Arrows make the intended read explicit without changing the actual logo:
    # the energy path enters through the green arc, crosses the protected
    # handoff, and exits through the blue arc. The orange wedge stays detached.
    draw_arrow(
        draw,
        (right_x + 130, panel_y + 295),
        (right_x + 280, panel_y + 160),
        fill="#FFFFFF",
        width=8,
    )
    draw_arrow(
        draw,
        (right_x + 1070, panel_y + 158),
        (right_x + 1230, panel_y + 290),
        fill="#FFFFFF",
        width=8,
    )
    draw_arrow(
        draw,
        (right_x + 566, panel_y + 495),
        (right_x + 492, panel_y + 400),
        fill="#FFFFFF",
        width=7,
    )
    draw.line(
        (right_x + 785, panel_y + 250, right_x + 905, panel_y + 195),
        fill="#1E292E",
        width=4,
    )

    footer_y = panel_y + panel_height + 38
    legend = (
        ("#075A54", "incoming continuous ribbon"),
        ("#D66F38", "independent lower termination"),
        ("#284C67", "outgoing continuous ribbon"),
        ("#1E292E", "white channel retained as intentional negative space"),
    )
    for index, (colour, label) in enumerate(legend):
        column = index % 2
        row = index // 2
        x = gutter + column * (panel_width + gutter)
        y = footer_y + row * 52
        draw.ellipse((x, y, x + 26, y + 26), fill=colour)
        draw.text((x + 40, y - 2), label, fill="#29343A", font=sheet_font(24))
    canvas.save(output)


def relative_luminance(colour: str) -> float:
    rgb = [int(colour[offset : offset + 2], 16) / 255 for offset in (1, 3, 5)]
    linear = [
        value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4
        for value in rgb
    ]
    return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]


def contrast_ratio(foreground: str, background: str) -> float:
    high, low = sorted((relative_luminance(foreground), relative_luminance(background)), reverse=True)
    return (high + 0.05) / (low + 0.05)


def validate() -> dict[str, object]:
    require_renderer()
    VALIDATION_DIR.mkdir(parents=True, exist_ok=True)
    for stale_name in STALE_EVIDENCE:
        (VALIDATION_DIR / stale_name).unlink(missing_ok=True)
    SQUARE_LOCKUP_VALIDATION_DIR.mkdir(parents=True, exist_ok=True)
    # These names belong exclusively to superseded candidate boards. Keep the
    # selected 09 evidence separate so rerunning validation cannot resurrect
    # or preserve an obsolete review path.
    for stale_name in STALE_SQUARE_LOCKUP_EVIDENCE:
        stale_path = SQUARE_LOCKUP_VALIDATION_DIR / stale_name
        if stale_path.is_dir():
            shutil.rmtree(stale_path)
        else:
            stale_path.unlink(missing_ok=True)
    reference = source_crop()
    render_svg(
        MARK_LOGO_ASSET,
        VALIDATION_DIR / "rendered-monochrome.png",
        VIEWBOX_WIDTH,
        VIEWBOX_HEIGHT,
    )
    render_svg(
        MARK_LOGO_ASSET,
        VALIDATION_DIR / "smoothness-8x.png",
        VIEWBOX_WIDTH * 8,
        VIEWBOX_HEIGHT * 8,
    )
    rendered = np.asarray(Image.open(VALIDATION_DIR / "rendered-monochrome.png").convert("RGB"))
    source_mask = mask_from_rgb(reference)
    rendered_mask = mask_from_rgb(rendered)
    raw_metrics = contour_metrics(source_mask, rendered_mask)
    landmarks = landmark_metrics(source_mask, rendered_mask)
    colors = color_metrics()
    make_native_overlay(reference, rendered, VALIDATION_DIR / "native-overlay.png")
    make_eight_x_overlay(reference, rendered, VALIDATION_DIR / "eight-x-overlay.png")
    make_construction_sheet(reference, VALIDATION_DIR / "construction-sheet.png")
    theme_preview_path = VALIDATION_DIR / "theme-light-preview.svg"
    theme_preview_path.write_text(theme_preview_svg(), encoding="utf-8")
    render_svg(
        theme_preview_path,
        VALIDATION_DIR / "theme-light-preview.png",
        VIEWBOX_WIDTH * 2,
        VIEWBOX_HEIGHT * 2,
        background=LIGHT_CANVAS,
    )
    integrity = vector_integrity()
    square_lockup = square_lockup_integrity()
    square_logo_variants = square_logo_variant_integrity()
    wide_logo = wide_logo_integrity()
    platform_assets = platform_asset_integrity()
    square_lockup_renders = square_lockup_render_validation()
    square_logo_variant_proof = make_square_logo_variant_proof()
    wide_logo_proof = make_wide_logo_proof()
    platform_proof = make_platform_proof()
    square_lockup_report = {
        "canvas": [SQUARE_LOCKUP_CANVAS, SQUARE_LOCKUP_CANVAS],
        "layout": {
            "mark": {
                "x": SQUARE_LOCKUP_MARK_X,
                "y": SQUARE_LOCKUP_MARK_Y,
                "width": SQUARE_LOCKUP_MARK_WIDTH,
            },
            "wordmark": {
                "x": SQUARE_LOCKUP_WORD_X,
                "y": SQUARE_LOCKUP_WORD_Y,
                "width": SQUARE_LOCKUP_WORD_WIDTH,
                "cap_height": SQUARE_LOCKUP_WORD_HEIGHT,
            },
            "minimum_clear_gap": SQUARE_LOCKUP_MIN_GAP,
        },
        "xml_and_hash_integrity": square_lockup,
        "delivery_variants": square_logo_variants,
        "rendering": square_lockup_renders,
        "variant_proof": str(square_logo_variant_proof.relative_to(ROOT)),
        "passed": bool(
            square_lockup["passed"]
            and square_logo_variants["passed"]
            and square_lockup_renders["passed"]
        ),
        "canonical_asset": str(FINAL_SQUARE_LOCKUP_ASSET.relative_to(ROOT)),
        "selection": "09 instrument reference",
        "wide_logo": {
            "integrity": wide_logo,
            "proof": str(wide_logo_proof.relative_to(ROOT)),
        },
        "platform_assets": {
            "integrity": platform_assets,
            "proof": str(platform_proof.relative_to(ROOT)),
        },
    }
    SQUARE_LOCKUP_VALIDATION_DIR.mkdir(parents=True, exist_ok=True)
    (SQUARE_LOCKUP_VALIDATION_DIR / "report.json").write_text(
        json.dumps(square_lockup_report, indent=2) + "\n", encoding="utf-8"
    )
    sampled = sampled_palette()
    palette_passed = all(
        sample["left_color"] == variant.left_color and sample["right_color"] == variant.right_color
        for sample, variant in zip(sampled, VARIANTS, strict=True)
    )
    colors_passed = palette_passed and all(item["interior_rgb_diff_pixels"] == 0 for item in colors)
    landmarks_passed = all(
        item["source_to_rendered_offset_px"]
        <= REFERENCE_ALIGNMENT_LIMITS["maximum_landmark_offset_px"]
        for item in landmarks
    )
    theme_light = {
        "canvas": LIGHT_CANVAS,
        "incoming_color": LIGHT_THEME_LEFT,
        "handoff_color": LIGHT_THEME_HANDOFF,
        "outgoing_color": LIGHT_THEME_RIGHT,
        "incoming_contrast_ratio": contrast_ratio(LIGHT_THEME_LEFT, LIGHT_CANVAS),
        "handoff_contrast_ratio": contrast_ratio(LIGHT_THEME_HANDOFF, LIGHT_CANVAS),
        "outgoing_contrast_ratio": contrast_ratio(LIGHT_THEME_RIGHT, LIGHT_CANVAS),
    }
    theme_light["passed"] = min(
        theme_light["incoming_contrast_ratio"],
        theme_light["handoff_contrast_ratio"],
        theme_light["outgoing_contrast_ratio"],
    ) >= 4.5
    dark_variants = [
        {
            "name": variant.name,
            "incoming_contrast_ratio": contrast_ratio(variant.incoming_color, DARK_CANVAS),
            "handoff_contrast_ratio": contrast_ratio(variant.handoff_color, DARK_CANVAS),
            "outgoing_contrast_ratio": contrast_ratio(variant.outgoing_color, DARK_CANVAS),
        }
        for variant in DARK_APP_VARIANTS
    ]
    for variant in dark_variants:
        variant["passed"] = min(
            variant["incoming_contrast_ratio"],
            variant["handoff_contrast_ratio"],
            variant["outgoing_contrast_ratio"],
        ) >= 4.5
    theme_dark = {
        "canvas": DARK_CANVAS,
        "variants": dark_variants,
        "passed": all(variant["passed"] for variant in dark_variants),
    }
    reference_alignment_passed = (
        raw_metrics["iou"] >= REFERENCE_ALIGNMENT_LIMITS["minimum_iou"]
        and raw_metrics["xor_ratio"] <= REFERENCE_ALIGNMENT_LIMITS["maximum_xor_ratio"]
        and raw_metrics["p99_contour_offset_px"]
        <= REFERENCE_ALIGNMENT_LIMITS["maximum_p99_contour_offset_px"]
        and raw_metrics["max_contour_offset_px"]
        <= REFERENCE_ALIGNMENT_LIMITS["maximum_contour_offset_px"]
        and landmarks_passed
    )
    smoothness = curve_smoothness()
    report: dict[str, object] = {
        "reference": {
            "geometry": str(MONO_REFERENCE.relative_to(ROOT)),
            "palette": str(COLOR_REFERENCE.relative_to(ROOT)),
            "crop": list(SOURCE_CROP),
            "view_box": [VIEWBOX_WIDTH, VIEWBOX_HEIGHT],
            "luma_cutoff": LUMA_CUTOFF,
            "renderer": "rsvg-convert",
            "registration": "disabled; the source crop and SVG viewBox share a fixed origin",
        },
        "geometry": integrity,
        "square_lockup": {
            "integrity": square_lockup,
            "delivery_variants": square_logo_variants,
            "renders": square_lockup_renders,
            "variant_proof": str(square_logo_variant_proof.relative_to(ROOT)),
        },
        "wide_logo": {
            "integrity": wide_logo,
            "proof": str(wide_logo_proof.relative_to(ROOT)),
        },
        "curve_smoothness": smoothness,
        "raw_source_overlay": raw_metrics,
        "landmarks": landmarks,
        "reference_alignment": {
            "role": (
                "fixed source registration check; permits normal rasterization and "
                "antialiasing loss instead of treating every edge pixel as geometry"
            ),
            "limits": REFERENCE_ALIGNMENT_LIMITS,
            "passed": reference_alignment_passed,
        },
        "palette_source_check": {
            "sampled": sampled,
            "passed": palette_passed,
        },
        "colorways": colors,
        "theme_light": theme_light,
        "theme_dark": theme_dark,
        "acceptance": {
            "limits": {
                "reference_alignment": REFERENCE_ALIGNMENT_LIMITS,
                "curve_smoothness": SMOOTHNESS_LIMITS,
            },
            "vector_integrity_passed": integrity["passed"],
            "square_lockup_integrity_passed": square_lockup["passed"],
            "square_logo_variants_passed": square_logo_variants["passed"],
            "wide_logo_passed": wide_logo["passed"],
            "platform_assets_passed": platform_assets["passed"],
            "square_lockup_render_passed": square_lockup_renders["passed"],
            "curve_smoothness_passed": smoothness["passed"],
            "color_passed": colors_passed,
            "theme_light_passed": theme_light["passed"],
            "theme_dark_passed": theme_dark["passed"],
            "landmarks_passed": landmarks_passed,
            "reference_alignment_passed": reference_alignment_passed,
            "passed": bool(
                integrity["passed"]
                and square_lockup["passed"]
                and square_logo_variants["passed"]
                and wide_logo["passed"]
                and platform_assets["passed"]
                and square_lockup_renders["passed"]
                and smoothness["passed"]
                and colors_passed
                and theme_light["passed"]
                and theme_dark["passed"]
                and reference_alignment_passed
            ),
            "note": (
                "The selected source is an AI-rendered raster. Its fixed-origin overlay guards "
                "the construction, while normal antialiasing and one-pixel renderer loss are "
                "permitted. Curve continuity, finite node count, semantic topology, vector "
                "integrity, flat fills, and light-theme contrast are the delivery gates; this "
                "report does not claim raw pixel-for-pixel identity."
            ),
        },
    }
    (VALIDATION_DIR / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return report


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Fail when any vector, smoothness, bounded-reference, color, or theme gate fails.",
    )
    args = parser.parse_args()
    write_assets()
    report = validate()
    acceptance = report["acceptance"]
    assert isinstance(acceptance, dict)
    if args.strict and not acceptance["passed"]:
        raise SystemExit("Logo acceptance did not pass; inspect output/logo-vector-validation/report.json.")


if __name__ == "__main__":
    main()
