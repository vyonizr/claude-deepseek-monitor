"""Generates the app icon set for claude-deepseek-monitor.

Concept: a pace gauge (mirrors the widget's session/weekly pacing arcs)
with a small status dot (mirrors the DeepSeek peak/off-peak indicator).
Colors match dist/index.html: dark widget bg, blue/green/red pacing states.
"""
import math
from PIL import Image, ImageDraw

BG = (26, 26, 31, 255)          # widget background, opaque
BG_EDGE = (255, 255, 255, 20)   # subtle border like the widget
BLUE = (90, 130, 240, 255)      # under pace
GREEN = (74, 222, 128, 255)     # on pace / off-peak
RED = (235, 87, 87, 255)        # over pace / peak
NEEDLE = (240, 240, 245, 255)
HUB = (18, 18, 22, 255)

SIZE = 1024
SS = 4  # supersample factor for smooth edges
CANVAS = SIZE * SS


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(4))


def draw_arc_gradient(draw, bbox, start, end, width, steps=240):
    span = end - start
    for i in range(steps):
        t0 = i / steps
        t1 = (i + 1) / steps
        a0 = start + span * t0
        a1 = start + span * t1 + 0.6
        if t0 < 0.5:
            color = lerp(BLUE, GREEN, t0 / 0.5)
        else:
            color = lerp(GREEN, RED, (t0 - 0.5) / 0.5)
        draw.arc(bbox, a0, a1, fill=color, width=width)


def rounded_square(draw, box, radius, fill):
    draw.rounded_rectangle(box, radius=radius, fill=fill)


def build_base():
    img = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    pad = int(CANVAS * 0.04)
    radius = int(CANVAS * 0.22)
    rounded_square(d, [pad, pad, CANVAS - pad, CANVAS - pad], radius, BG)
    d.rounded_rectangle(
        [pad, pad, CANVAS - pad, CANVAS - pad],
        radius=radius,
        outline=BG_EDGE,
        width=int(CANVAS * 0.006),
    )

    cx, cy = CANVAS / 2, CANVAS * 0.47
    r = CANVAS * 0.30
    arc_width = int(CANVAS * 0.085)
    bbox = [cx - r, cy - r, cx + r, cy + r]
    start_angle, end_angle = 145, 395  # 250 degree sweep, gap at bottom
    draw_arc_gradient(d, bbox, start_angle, end_angle, arc_width)

    needle_angle_deg = start_angle + (end_angle - start_angle) * 0.62
    needle_len = r - arc_width * 0.15
    ang = math.radians(needle_angle_deg)
    tip = (cx + needle_len * math.cos(ang), cy + needle_len * math.sin(ang))
    perp = math.radians(needle_angle_deg + 90)
    base_half = CANVAS * 0.018
    b1 = (cx + base_half * math.cos(perp), cy + base_half * math.sin(perp))
    b2 = (cx - base_half * math.cos(perp), cy - base_half * math.sin(perp))
    d.polygon([b1, tip, b2], fill=NEEDLE)

    hub_r = CANVAS * 0.045
    d.ellipse([cx - hub_r, cy - hub_r, cx + hub_r, cy + hub_r], fill=NEEDLE)
    hub_inner = CANVAS * 0.022
    d.ellipse(
        [cx - hub_inner, cy - hub_inner, cx + hub_inner, cy + hub_inner],
        fill=HUB,
    )

    dot_r = CANVAS * 0.075
    dot_cx = CANVAS - pad - CANVAS * 0.185
    dot_cy = CANVAS - pad - CANVAS * 0.185
    ring_r = dot_r + CANVAS * 0.02
    d.ellipse(
        [dot_cx - ring_r, dot_cy - ring_r, dot_cx + ring_r, dot_cy + ring_r],
        fill=BG,
    )
    d.ellipse(
        [dot_cx - dot_r, dot_cy - dot_r, dot_cx + dot_r, dot_cy + dot_r],
        fill=GREEN,
    )

    return img.resize((SIZE, SIZE), Image.LANCZOS)


def main():
    import os

    base = build_base()
    out_dir = os.path.join(os.path.dirname(__file__), "..", "icons")
    out_dir = os.path.abspath(out_dir)
    os.makedirs(out_dir, exist_ok=True)

    sizes = {
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon.png": 512,
        "Square30x30Logo.png": 30,
        "Square44x44Logo.png": 44,
        "Square71x71Logo.png": 71,
        "Square89x89Logo.png": 89,
        "Square107x107Logo.png": 107,
        "Square142x142Logo.png": 142,
        "Square150x150Logo.png": 150,
        "Square284x284Logo.png": 284,
        "Square310x310Logo.png": 310,
        "StoreLogo.png": 50,
    }
    for name, sz in sizes.items():
        base.resize((sz, sz), Image.LANCZOS).save(os.path.join(out_dir, name))

    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    base.save(
        os.path.join(out_dir, "icon.ico"),
        sizes=[(s, s) for s in ico_sizes],
    )

    print("Icons written to", out_dir)


if __name__ == "__main__":
    main()
