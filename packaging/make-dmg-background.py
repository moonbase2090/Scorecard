#!/usr/bin/env python3
"""Regenerate the Scorecard DMG background (1x and @2x).

800x560 dark panel in the scorecardcli.com dark theme (#141414 page,
#ededed text, #a3a3a3 muted, #fb923c orange accent). The logo mark is
redrawn from https://scorecardcli.com/assets/logo-mark.svg primitives.
Regenerating needs Pillow; the PNGs are committed so the release only
reads them. Headline uses Helvetica Bold: Inter is the site font but
is not installed on build machines.
"""
import os

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
WIDTH, HEIGHT = 800, 560
BG_TOP = (23, 23, 23)
BG_BOTTOM = (16, 16, 16)
PANEL = (38, 38, 38)
TEXT = (237, 237, 237)
MUTED = (163, 163, 163)
ACCENT = (251, 146, 60)


def font(size, bold=False):
    candidates = []
    if bold:
        candidates += [
            ("/System/Library/Fonts/Helvetica.ttc", 1),
            ("/System/Library/Fonts/HelveticaNeue.ttc", 1),
        ]
    candidates += [
        ("/System/Library/Fonts/Helvetica.ttc", 0),
        ("/System/Library/Fonts/HelveticaNeue.ttc", 0),
        ("/System/Library/Fonts/SFNSDisplay.ttf", 0),
    ]
    for path, index in candidates:
        try:
            face = ImageFont.truetype(path, size, index=index)
            if not bold or "Bold" in face.getname():
                return face
        except OSError:
            continue
    return ImageFont.load_default()


def label_band(d, x0, x1, cy, s):
    """Orange band behind a label row so Finder's black text reads on
    the dark background. Symmetric about the row center."""
    h = 30 * s
    d.rounded_rectangle([x0 * s, cy * s - h / 2, x1 * s, cy * s + h / 2], radius=h / 2, fill=ACCENT)


def draw_logo(d, cx, cy, size):
    """Scorecard logo mark: ring, doc, grid ticks, orange check."""
    u = size / 100.0

    def pt(x, y):
        return (cx + (x - 50) * u, cy + (y - 50) * u)

    lite, orange, dark = TEXT, ACCENT, (20, 20, 20)
    d.ellipse([pt(4, 4), pt(96, 96)], outline=lite, width=max(1, int(4.5 * u)))
    d.ellipse([pt(10.5, 10.5), pt(89.5, 89.5)], outline=orange, width=max(1, int(2.5 * u)))
    d.rounded_rectangle(
        [pt(24, 24), pt(68, 74)],
        radius=int(6 * u),
        outline=lite,
        width=max(1, int(4 * u)),
    )
    d.rounded_rectangle(
        [pt(27, 27), pt(65, 35)], radius=int(2.5 * u), fill=orange
    )
    for segs, color, w in (
        ([(38.7, 38, 38.7, 74), (53.3, 38, 53.3, 74), (24, 50.7, 68, 50.7), (24, 62.3, 68, 62.3)], orange, 2),
        ([(28.2, 44.3, 30.5, 46.6, 34.5, 42.1), (42.9, 44.3, 45.2, 46.6, 49.2, 42.1), (57.6, 44.3, 59.9, 46.6, 63.9, 42.1)], lite, 2.4),
    ):
        for s in segs:
            pts = [pt(s[i], s[i + 1]) for i in range(0, len(s), 2)]
            d.line(pts, fill=color, width=max(1, int(w * u)), joint="curve")
    check = [pt(39, 57), pt(52.5, 70.5), pt(86, 23)]
    for color, w in ((dark, 21), (lite, 13), (orange, 5.5)):
        d.line(check, fill=color, width=max(1, int(w * u)), joint="curve")


def draw(scale):
    img = Image.new("RGB", (WIDTH * scale, HEIGHT * scale))
    px = img.load()
    for y in range(HEIGHT * scale):
        t = y / (HEIGHT * scale)
        rgb = tuple(int(BG_TOP[i] + (BG_BOTTOM[i] - BG_TOP[i]) * t) for i in range(3))
        for x in range(WIDTH * scale):
            px[x, y] = rgb
    d = ImageDraw.Draw(img)
    s = scale
    # Orange label bands so Finder's black text reads on the dark
    # background. The top band is shortened to the installer pair so
    # the row stays centered; icons stay on dark.
    label_band(d, 200, 600, 272, s)
    label_band(d, 60, 740, 430, s)
    # Heading group: logo plus centered headline, 40px+ top padding.
    face = font(28 * s, bold=True)
    label = "Double-click Install Scorecard"
    left, _, right, _ = d.textbbox((0, 0), label, font=face)
    gap = 16 * s
    logo_size = 44 * s
    start = 400 * s - (logo_size + gap + (right - left)) / 2
    draw_logo(d, start + logo_size / 2, 72 * s, logo_size)
    d.text((start + logo_size + gap, 72 * s), label, font=face, fill=TEXT, anchor="lm")
    # Arrow above the installer icon at (330, 210).
    x, y0, y1 = 330 * s, 128 * s, 158 * s
    d.line([x, y0, x, y1], fill=ACCENT, width=4 * s)
    head = 11 * s
    d.polygon([(x, y1), (x - head, y1 - head), (x + head, y1 - head)], fill=ACCENT)
    d.text(
        (400 * s, 508 * s),
        "Command-line tools \u2014 installs sc and sc-mcp to /usr/local/bin",
        font=font(14 * s),
        fill=MUTED,
        anchor="mm",
    )
    return img


draw(1).save(os.path.join(HERE, "dmg-background.png"))
draw(2).save(os.path.join(HERE, "dmg-background@2x.png"))
print("wrote dmg-background.png and dmg-background@2x.png")
