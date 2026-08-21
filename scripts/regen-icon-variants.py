#!/usr/bin/env python3
"""生成多个 logo 尺寸变体（60% / 70% / 80% / 88%），供用户对比选择。"""
import math
from PIL import Image, ImageDraw, ImageFilter

SRC_ORIG = "/tmp/icon-orig-1024.png"
APPLE_BLUE_DARK = (0, 113, 227, 255)
APPLE_BLUE_LIGHT = (79, 140, 255, 255)

def make_gradient(size, c1, c2, angle_deg=135):
    img = Image.new("RGBA", (size, size), c1)
    pixels = img.load()
    a = math.radians(angle_deg)
    cos_a, sin_a = math.cos(a), math.sin(a)
    for y in range(size):
        for x in range(size):
            t = (x * cos_a + y * sin_a) / (size * (abs(cos_a) + abs(sin_a)))
            t = max(0.0, min(1.0, t))
            r = int(c1[0] * (1 - t) + c2[0] * t)
            g = int(c1[1] * (1 - t) + c2[1] * t)
            b = int(c1[2] * (1 - t) + c2[2] * t)
            pixels[x, y] = (r, g, b, 255)
    return img

def make_icon(scale):
    logo = Image.open(SRC_ORIG).convert("RGBA")
    r_ch, _, _, _ = logo.split()
    logo_only = Image.new("RGBA", logo.size, (0, 0, 0, 0))
    logo_only.paste((255, 255, 255, 255), mask=r_ch)
    new_size = int(1024 * scale)
    logo_small = logo_only.resize((new_size, new_size), Image.LANCZOS)
    bg = make_gradient(1024, APPLE_BLUE_DARK, APPLE_BLUE_LIGHT, 135)
    canvas = bg.copy()
    offset = (1024 - new_size) // 2
    canvas.alpha_composite(logo_small, (offset, offset))
    # 顶部 8% 高光
    highlight = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    hd = ImageDraw.Draw(highlight)
    for y in range(0, int(1024 * 0.4)):
        a = int(28 * (1 - y / (1024 * 0.4)))
        hd.line((0, y, 1024, y), fill=(255, 255, 255, a))
    highlight = highlight.filter(ImageFilter.GaussianBlur(50))
    return Image.alpha_composite(canvas, highlight)

for s in [0.60, 0.70, 0.78, 0.85]:
    out = f"/tmp/icon-{int(s*100)}.png"
    make_icon(s).save(out, "PNG", optimize=True)
    print(f"✓ {out} (logo {int(s*100)}%)")
