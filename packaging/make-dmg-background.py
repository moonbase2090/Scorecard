#!/usr/bin/env python3
"""Regenerate the Scorecard DMG background (1x and @2x).

Dark 600x400 panel with an arrow pointing at the installer icon, which
dmgbuild places at content position (150, 170). Regenerating needs
Pillow; the PNGs are committed so the release only reads them.
"""
import os

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
WIDTH, HEIGHT = 600, 400
TOP = (26, 29, 36)
BOTTOM = (15, 17, 22)
TEXT = (230, 232, 238)
DIM = (154, 160, 174)
ACCENT = (74, 222, 128)


def font(size):
    for path in (
        "/System/Library/Fonts/Helvetica.ttc",
        "/System/Library/Fonts/SFNSDisplay.ttf",
    ):
        try:
            return ImageFont.truetype(path, size)
        except OSError:
            continue
    return ImageFont.load_default()


def draw(scale):
    img = Image.new("RGB", (WIDTH * scale, HEIGHT * scale))
    px = img.load()
    for y in range(HEIGHT * scale):
        t = y / (HEIGHT * scale)
        r = int(TOP[0] + (BOTTOM[0] - TOP[0]) * t)
        g = int(TOP[1] + (BOTTOM[1] - TOP[1]) * t)
        b = int(TOP[2] + (BOTTOM[2] - TOP[2]) * t)
        for x in range(WIDTH * scale):
            px[x, y] = (r, g, b)
    d = ImageDraw.Draw(img)
    s = scale
    # Arrow from the headline down to the installer icon at (150, 170).
    # Icon art is ~96px, so the arrowhead lands just above it.
    x0, y0, x1, y1 = 150 * s, 88 * s, 150 * s, 118 * s
    d.line([x0, y0, x1, y1], fill=ACCENT, width=3 * s)
    head = 9 * s
    d.polygon([(x1, y1), (x1 - head, y1 - head), (x1 + head, y1 - head)], fill=ACCENT)
    d.text((150 * s, 52 * s), "Double-click Install Scorecard", font=font(22 * s), fill=TEXT, anchor="mm")
    d.text(
        (300 * s, 372 * s),
        "Command-line tools — installs sc and sc-mcp to /usr/local/bin",
        font=font(13 * s),
        fill=DIM,
        anchor="mm",
    )
    return img


draw(1).save(os.path.join(HERE, "dmg-background.png"))
draw(2).save(os.path.join(HERE, "dmg-background@2x.png"))
print("wrote dmg-background.png and dmg-background@2x.png")
