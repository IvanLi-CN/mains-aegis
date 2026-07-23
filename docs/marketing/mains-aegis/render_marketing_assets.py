#!/usr/bin/env python3
"""Generate editable deterministic Mains Aegis marketing composites."""

from __future__ import annotations

from collections import deque
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont


ROOT = Path(__file__).resolve().parent
PRODUCT_RENDER = ROOT / "product-render-white-studio.png"

SOCIAL_SIZE = (1280, 640)
POSTER_SIZE = (1600, 2000)

BRAND = "MAINS AEGIS"
SOCIAL_HEADLINE = ["12V / 19V", "DC UPS for", "critical loads."]
SOCIAL_BODY = (
    "Battery-backed DC5025 output with dual TPS55288 regulation, "
    "3-channel INA3221 telemetry, and hardware fault alerts."
)
POSTER_HEADLINE = ["12V / 19V", "DC UPS for", "critical loads."]
POSTER_BODY = (
    "Mains Aegis backs the DC5025 output with a 4S battery path, dual "
    "TPS55288 regulation, 3-channel INA3221 telemetry, and hardware "
    "UV / OC / thermal protection."
)
CHIPS = [
    ("12V / 19V OUTPUT", "teal"),
    ("6.32A UPS OUT", "teal"),
    ("3-CH POWER SENSE", "amber"),
]
FEATURES = [
    (
        "12V / 19V output",
        "Firmware-selected DC output profiles for different load classes.",
    ),
    ("6.32A UPS OUT", "DC5025 output target for routers, sensors, and high-current test loads."),
    (
        "3-channel telemetry",
        "UPS VIN plus dual output current / voltage sensing via INA3221.",
    ),
    ("Hardware protection", "Undervoltage, overcurrent, and thermal kill paths visible to firmware."),
]

FONT_AVENIR = "/System/Library/Fonts/Avenir Next.ttc"
FONT_AVENIR_COND = "/System/Library/Fonts/Avenir Next Condensed.ttc"
FONT_DIN_COND = "/System/Library/Fonts/Supplemental/DIN Condensed Bold.ttf"
FONT_HELVETICA = "/System/Library/Fonts/HelveticaNeue.ttc"
FONT_MENLO = "/System/Library/Fonts/Menlo.ttc"

INK = (22, 36, 39, 246)
MUTED = (63, 81, 83, 226)
TEAL = (8, 104, 128, 230)
AMBER = (223, 143, 37, 235)


def load_font(path: str, size: int, index: int = 0) -> ImageFont.FreeTypeFont:
    for candidate in (path, FONT_AVENIR, FONT_HELVETICA):
        try:
            return ImageFont.truetype(candidate, size=size, index=index)
        except OSError:
            continue
    return ImageFont.load_default(size=size)


def body(size: int) -> ImageFont.FreeTypeFont:
    return load_font(FONT_AVENIR, size)


def head(size: int) -> ImageFont.FreeTypeFont:
    return load_font(FONT_AVENIR_COND, size)


def kicker(size: int) -> ImageFont.FreeTypeFont:
    return load_font(FONT_DIN_COND, size)


def mono(size: int) -> ImageFont.FreeTypeFont:
    return load_font(FONT_MENLO, size)


def make_background(size: tuple[int, int]) -> Image.Image:
    w, h = size
    yy, xx = np.mgrid[0:h, 0:w]
    x = xx / (w - 1)
    y = yy / (h - 1)

    warm = np.array([249, 246, 238], dtype=float)
    cool = np.array([226, 240, 244], dtype=float)
    sand = np.array([241, 229, 207], dtype=float)

    pixels = warm * (1 - x[..., None]) + cool * x[..., None]
    pixels = pixels * (1 - (y * 0.16)[..., None]) + sand * ((y * 0.16)[..., None])

    blue_glow = np.exp(-(((x - 0.82) / 0.42) ** 2 + ((y - 0.24) / 0.42) ** 2))
    warm_glow = np.exp(-(((x - 0.24) / 0.55) ** 2 + ((y - 0.92) / 0.30) ** 2))
    pixels += blue_glow[..., None] * np.array([0, 16, 22])
    pixels += warm_glow[..., None] * np.array([9, 3, -8])
    pixels += np.random.default_rng(37).normal(0, 0.35, (h, w, 1))

    return Image.fromarray(np.clip(pixels, 0, 255).astype("uint8"), "RGB").convert("RGBA")


def product_cutout() -> Image.Image:
    image = Image.open(PRODUCT_RENDER).convert("RGBA")
    arr = np.asarray(image).astype(np.int16)
    rgb = arr[:, :, :3]
    luminance = 0.2126 * rgb[:, :, 0] + 0.7152 * rgb[:, :, 1] + 0.0722 * rgb[:, :, 2]
    chroma = rgb.max(axis=2) - rgb.min(axis=2)
    candidate = (luminance > 176) & (chroma < 58)

    h, w = candidate.shape
    seen = np.zeros((h, w), dtype=bool)
    queue: deque[tuple[int, int]] = deque()

    for x in range(w):
        if candidate[0, x]:
            seen[0, x] = True
            queue.append((0, x))
        if candidate[h - 1, x]:
            seen[h - 1, x] = True
            queue.append((h - 1, x))

    for y in range(h):
        if candidate[y, 0] and not seen[y, 0]:
            seen[y, 0] = True
            queue.append((y, 0))
        if candidate[y, w - 1] and not seen[y, w - 1]:
            seen[y, w - 1] = True
            queue.append((y, w - 1))

    while queue:
        y, x = queue.popleft()
        for yy, xx in ((y - 1, x), (y + 1, x), (y, x - 1), (y, x + 1)):
            if 0 <= yy < h and 0 <= xx < w and candidate[yy, xx] and not seen[yy, xx]:
                seen[yy, xx] = True
                queue.append((yy, xx))

    alpha = np.full((h, w), 255, dtype=np.uint8)
    alpha[seen] = 0
    alpha[luminance < 145] = 255

    mask = Image.fromarray(alpha, "L").filter(ImageFilter.GaussianBlur(0.45))
    image.putalpha(mask)
    bbox = mask.point(lambda p: 255 if p > 20 else 0).getbbox()
    if not bbox:
        return image

    left, top, right, bottom = bbox
    pad = 14
    return image.crop(
        (
            max(0, left - pad),
            max(0, top - pad),
            min(image.width, right + pad),
            min(image.height, bottom + pad),
        )
    )


def apply_front_panel_gloss(image: Image.Image) -> Image.Image:
    """Restore the front face as one continuous glossy black panel."""
    image = image.copy()
    w, h = image.size
    panel = [(0, 128), (1056, 196), (1046, 624), (0, 505)]

    panel_mask = Image.new("L", image.size, 0)
    ImageDraw.Draw(panel_mask).polygon(panel, fill=255)
    panel_mask = panel_mask.filter(ImageFilter.GaussianBlur(1.2))

    yy, xx = np.mgrid[0:h, 0:w]
    top_line = 128 + (196 - 128) * np.clip(xx, 0, 1056) / 1056
    distance_from_top = yy - top_line

    upper_reflection = 42 * np.exp(-((distance_from_top - 34) / 58) ** 2)
    broad_reflection = 18 * np.exp(-((distance_from_top - 112) / 120) ** 2)
    side_falloff = 0.92 - 0.18 * (xx / max(w - 1, 1))
    alpha = (upper_reflection + broad_reflection) * side_falloff
    alpha *= (distance_from_top > -8) & (distance_from_top < 230)
    alpha *= np.asarray(panel_mask) / 255

    gloss = np.zeros((h, w, 4), dtype=np.uint8)
    gloss[:, :, 0] = 255
    gloss[:, :, 1] = 255
    gloss[:, :, 2] = 255
    gloss[:, :, 3] = np.clip(alpha, 0, 48).astype(np.uint8)
    image.alpha_composite(Image.fromarray(gloss, "RGBA"))

    specular = Image.new("RGBA", image.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(specular, "RGBA")
    draw.line((18, 147, 1028, 210), fill=(255, 255, 255, 42), width=3)
    draw.line((46, 175, 830, 222), fill=(255, 255, 255, 18), width=7)
    specular.putalpha(Image.composite(specular.getchannel("A"), Image.new("L", image.size, 0), panel_mask))
    image.alpha_composite(specular.filter(ImageFilter.GaussianBlur(0.8)))
    return image


PRODUCT = apply_front_panel_gloss(product_cutout())


def draw_tracking(
    draw: ImageDraw.ImageDraw,
    xy: tuple[int, int],
    text: str,
    font: ImageFont.FreeTypeFont,
    fill: tuple[int, int, int, int],
    space: int,
) -> None:
    x, y = xy
    for char in text:
        draw.text((x, y), char, font=font, fill=fill)
        x += draw.textlength(char, font=font) + space


def wrap_text(
    draw: ImageDraw.ImageDraw,
    text: str,
    font: ImageFont.FreeTypeFont,
    width: int,
) -> list[str]:
    lines: list[str] = []
    current = ""
    for word in text.split():
        proposed = (current + " " + word).strip()
        if current and draw.textlength(proposed, font=font) > width:
            lines.append(current)
            current = word
        else:
            current = proposed
    if current:
        lines.append(current)
    return lines


def draw_lines(
    draw: ImageDraw.ImageDraw,
    xy: tuple[int, int],
    lines: list[str],
    font: ImageFont.FreeTypeFont,
    fill: tuple[int, int, int, int],
    leading: int,
) -> int:
    x, y = xy
    for line in lines:
        draw.text((x, y), line, font=font, fill=fill)
        y += leading
    return y


def draw_chip(
    draw: ImageDraw.ImageDraw,
    xy: tuple[int, int],
    label: str,
    accent_name: str,
) -> tuple[int, int]:
    x, y = xy
    chip_font = body(17)
    accent = AMBER if accent_name == "amber" else TEAL
    text_width = draw.textlength(label, font=chip_font)
    width = int(text_width + 42)
    height = 34

    draw.rounded_rectangle(
        (x, y, x + width, y + height),
        radius=17,
        fill=(255, 255, 255, 168),
        outline=(9, 99, 123, 48),
        width=1,
    )
    draw.ellipse((x + 12, y + 14, x + 18, y + 20), fill=accent)
    draw.text((x + 28, y + 7), label, font=chip_font, fill=(35, 51, 53, 232))
    return width, height


def draw_soft_arcs(draw: ImageDraw.ImageDraw, size: tuple[int, int], variant: str) -> None:
    if variant == "social":
        draw.arc((705, -95, 1315, 535), 188, 310, fill=(8, 104, 128, 34), width=2)
        draw.arc((790, -12, 1250, 465), 190, 306, fill=(8, 104, 128, 22), width=1)
        draw.arc((455, 418, 1190, 880), 205, 342, fill=(223, 143, 37, 28), width=2)
    else:
        draw.arc((900, 120, 1880, 1260), 186, 318, fill=(8, 104, 128, 26), width=3)
        draw.arc((1030, 260, 1740, 1110), 188, 314, fill=(8, 104, 128, 18), width=2)
        draw.arc((-160, 1170, 670, 2100), 280, 360, fill=(223, 143, 37, 24), width=3)


def paste_product(layer: Image.Image, center: tuple[int, int], width: int, shadow_alpha: int) -> None:
    scale = width / PRODUCT.width
    product = PRODUCT.resize(
        (int(PRODUCT.width * scale), int(PRODUCT.height * scale)),
        Image.Resampling.LANCZOS,
    )
    x = int(center[0] - product.width / 2)
    y = int(center[1] - product.height / 2)

    shadow = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    shadow_draw = ImageDraw.Draw(shadow, "RGBA")
    shadow_draw.ellipse(
        (
            x + int(product.width * 0.13),
            y + int(product.height * 0.72),
            x + int(product.width * 0.90),
            y + int(product.height * 0.95),
        ),
        fill=(52, 60, 55, shadow_alpha),
    )
    layer.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(max(24, int(width * 0.035)))))
    layer.alpha_composite(product, (x, y))


def draw_social_image_layer(layer: Image.Image) -> None:
    draw = ImageDraw.Draw(layer, "RGBA")
    draw_soft_arcs(draw, SOCIAL_SIZE, "social")
    draw.ellipse((708, 46, 1260, 590), fill=(248, 242, 229, 150), outline=(255, 255, 255, 84), width=2)
    draw.rounded_rectangle((690, 454, 1222, 604), radius=58, fill=(235, 226, 208, 120))
    draw.ellipse((596, 446, 1240, 626), fill=(255, 255, 255, 84))
    paste_product(layer, (908, 362), 790, 70)


def draw_social_text_layer(layer: Image.Image) -> None:
    draw = ImageDraw.Draw(layer, "RGBA")
    draw_tracking(draw, (68, 64), BRAND, kicker(27), TEAL, 4)
    draw.line((68, 106, 166, 106), fill=AMBER, width=4)
    draw_lines(draw, (66, 152), SOCIAL_HEADLINE, head(56), INK, 63)
    draw_lines(
        draw,
        (68, 390),
        wrap_text(draw, SOCIAL_BODY, body(22), 430),
        body(22),
        MUTED,
        31,
    )

    x = 68
    y = 536
    for label, accent in CHIPS:
        chip_width, _ = draw_chip(draw, (x, y), label, accent)
        x += chip_width + 12


def draw_feature_icon(draw: ImageDraw.ImageDraw, center: tuple[int, int], index: int) -> None:
    x, y = center
    color = AMBER if index == 2 else TEAL
    white = (255, 255, 255, 230)
    draw.ellipse((x - 28, y - 28, x + 28, y + 28), fill=color)

    if index == 0:
        for dx, dy in [(-10, -8), (11, -11), (0, 13)]:
            draw.ellipse((x + dx - 4, y + dy - 4, x + dx + 4, y + dy + 4), outline=white, width=2)
        draw.line((x - 6, y - 6, x + 7, y - 10), fill=white, width=2)
        draw.line((x + 2, y + 10, x + 8, y - 7), fill=white, width=2)
        draw.line((x - 7, y - 5, x - 1, y + 10), fill=white, width=2)
    elif index == 1:
        draw.polygon(
            [(x, y - 15), (x + 14, y - 8), (x + 11, y + 12), (x, y + 20), (x - 11, y + 12), (x - 14, y - 8)],
            outline=white,
        )
        draw.line((x - 7, y + 1, x - 1, y + 8, x + 9, y - 7), fill=white, width=2)
    elif index == 2:
        draw.line((x - 7, y - 18, x - 18, y + 4, x - 3, y + 4, x - 10, y + 20, x + 17, y - 9, x + 2, y - 9, x + 7, y - 18), fill=white, width=3)
    else:
        draw.line((x - 12, y + 14, x - 12, y - 15, x, y - 15, x, y + 14), fill=white, width=2)
        draw.line((x + 8, y + 14, x + 8, y - 4, x + 18, y + 14), fill=white, width=2)
        draw.line((x - 18, y + 14, x + 22, y + 14), fill=white, width=2)


def draw_card(layer: Image.Image, xy: tuple[int, int, int, int], radius: int = 28) -> None:
    x0, y0, x1, y1 = xy
    shadow = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    shadow_draw = ImageDraw.Draw(shadow, "RGBA")
    shadow_draw.rounded_rectangle((x0, y0 + 11, x1, y1 + 11), radius=radius, fill=(54, 69, 68, 21))
    layer.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(22)))

    draw = ImageDraw.Draw(layer, "RGBA")
    draw.rounded_rectangle(xy, radius=radius, fill=(255, 255, 255, 154), outline=(9, 95, 119, 42), width=1)


def draw_poster_image_layer(layer: Image.Image) -> None:
    draw = ImageDraw.Draw(layer, "RGBA")
    draw_soft_arcs(draw, POSTER_SIZE, "poster")
    draw.ellipse((-80, -150, 820, 720), fill=(250, 245, 235, 118))
    draw.rounded_rectangle((118, 734, 1488, 1328), radius=74, fill=(229, 243, 241, 92), outline=(255, 255, 255, 76), width=1)
    draw.ellipse((205, 890, 1438, 1438), fill=(248, 241, 226, 150), outline=(255, 255, 255, 100), width=2)
    draw.ellipse((285, 1010, 1395, 1395), fill=(255, 255, 255, 72))
    paste_product(layer, (810, 1074), 1248, 72)


def draw_poster_text_layer(layer: Image.Image) -> None:
    draw = ImageDraw.Draw(layer, "RGBA")
    draw_tracking(draw, (112, 96), BRAND, kicker(42), TEAL, 6)
    draw.line((114, 158, 272, 158), fill=AMBER, width=5)
    draw.rounded_rectangle((1088, 92, 1470, 146), radius=27, fill=(255, 255, 255, 154), outline=(9, 95, 119, 48), width=1)
    draw.ellipse((1114, 112, 1131, 129), fill=AMBER)
    draw.text((1150, 105), "BATTERY-BACKED DC POWER", font=body(23), fill=(45, 63, 65, 232))
    draw_lines(draw, (112, 236), POSTER_HEADLINE, head(84), INK, 94)
    draw_lines(draw, (116, 538), wrap_text(draw, POSTER_BODY, body(29), 920), body(29), MUTED, 39)

    x0, y0 = 112, 1454
    card_width, card_height = 666, 150
    gap = 34
    for index, (title, copy) in enumerate(FEATURES):
        x = x0 + (index % 2) * (card_width + gap)
        y = y0 + (index // 2) * 180
        draw_card(layer, (x, y, x + card_width, y + card_height), 28)
        draw_feature_icon(draw, (x + 62, y + 75), index)
        draw.text((x + 112, y + 32), title, font=body(28), fill=(30, 47, 49, 244))
        draw_lines(draw, (x + 112, y + 82), wrap_text(draw, copy, body(21), card_width - 145)[:2], body(21), (75, 89, 90, 224), 29)

    draw.line((116, 1880, 130, 1880), fill=AMBER, width=5)
    draw.text((150, 1866), "12V / 19V output. 3-channel telemetry. Hardware fault protection.", font=body(25), fill=(35, 77, 88, 224))


def composite(size: tuple[int, int], draw_image_layer, draw_text_layer) -> Image.Image:
    base = make_background(size)
    image_layer = Image.new("RGBA", size, (0, 0, 0, 0))
    text_layer = Image.new("RGBA", size, (0, 0, 0, 0))
    draw_image_layer(image_layer)
    draw_text_layer(text_layer)
    base.alpha_composite(image_layer)
    base.alpha_composite(text_layer)
    return base.convert("RGB")


def save_pair(image: Image.Image, stem: str) -> None:
    png = ROOT / f"{stem}.png"
    jpg = ROOT / f"{stem}.jpg"
    image.save(png, optimize=True, compress_level=9)
    image.save(jpg, quality=92, optimize=True, progressive=True)
    print(f"{png.name}: {image.size} {png.stat().st_size}")
    print(f"{jpg.name}: {image.size} {jpg.stat().st_size}")


def main() -> None:
    save_pair(composite(SOCIAL_SIZE, draw_social_image_layer, draw_social_text_layer), "social-preview")
    save_pair(composite(POSTER_SIZE, draw_poster_image_layer, draw_poster_text_layer), "product-poster")


if __name__ == "__main__":
    main()
