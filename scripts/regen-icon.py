#!/usr/bin/env python3
"""
重新生成 Threadock app icon（v2 — 标准 macOS/iOS 源图）。

设计原则（按 Apple HIG / Material Design 平台约定）：
- 1024x1024 满铺 PNG，**不带圆角**——各平台会自己加 superellipse / continuous / circular mask
- 单一源图适用 macOS / iOS / Android / Windows，tauri icon CLI 自动分配各分辨率
- Logo 居中放大到 70%（在 824x824 safe area 内，留 12% 内 padding 不被 mask 切到）
- Apple Blue 渐变（#0071E3 → #4F8CFF，135° 对角），对齐应用内 Apple HIG 主题
- 顶部 8% 白色高光（增强玻璃感，参考 iOS 17 / macOS Sonoma 渐变 app icon）

修复历史：
- v1: 加了 superellipse 圆角 → 渲染时被 macOS 重复 mask，圆角被切
- v2: 不加圆角，让 macOS 自己处理（2026-08-21）
"""
import math
from PIL import Image, ImageDraw, ImageFilter

SRC = "/tmp/icon-orig-1024.png"  # 原始黑白 logo，避免重复读取已生成的蓝色背景
OUT = "apps/desktop/src-tauri/icons/icon-1024x1024.png"

# Apple 系统色
APPLE_BLUE_DARK = (0, 113, 227, 255)   # #0071E3
APPLE_BLUE_LIGHT = (79, 140, 255, 255)  # #4F8CFF

# Logo 缩放比例（mask 切到的外圈 ~22%，safe area ~80%，留 12% padding）
LOGO_SCALE = 0.60  # 614x614，增加留白，降低 Dock 中的视觉占比
ICON_FACE_SCALE = 0.84  # 缩小整个蓝色底，避免 Dock 中的视觉边界偏大

def make_gradient(size: int, c1, c2, angle_deg: float = 135) -> Image.Image:
    """线性渐变背景（无圆角 — 满铺）"""
    img = Image.new("RGBA", (size, size), c1)
    pixels = img.load()
    angle = math.radians(angle_deg)
    cos_a, sin_a = math.cos(angle), math.sin(angle)
    for y in range(size):
        for x in range(size):
            t = (x * cos_a + y * sin_a) / (size * (abs(cos_a) + abs(sin_a)))
            t = max(0.0, min(1.0, t))
            r = int(c1[0] * (1 - t) + c2[0] * t)
            g = int(c1[1] * (1 - t) + c2[1] * t)
            b = int(c1[2] * (1 - t) + c2[2] * t)
            pixels[x, y] = (r, g, b, 255)
    return img

def main():
    # 1. 加载原 logo
    logo = Image.open(SRC).convert("RGBA")
    if logo.size != (1024, 1024):
        raise ValueError(f"原图尺寸 {logo.size} != 1024x1024")

    # 2. 抠白色形状：原图 = 黑底 + 白色 logo
    r_ch, g_ch, b_ch, a_ch = logo.split()
    # 白色像素 = 255 通道值，黑色 = 0；用 R 通道当 alpha mask
    logo_only = Image.new("RGBA", logo.size, (0, 0, 0, 0))
    logo_only.paste((255, 255, 255, 255), mask=r_ch)

    # 3. 缩放到 LOGO_SCALE（70% = 717x717）
    new_size = int(1024 * LOGO_SCALE)
    logo_small = logo_only.resize((new_size, new_size), Image.LANCZOS)

    # 4. 缩小 Apple Blue 图标底，并保留透明留白
    face_size = int(1024 * ICON_FACE_SCALE)
    bg = make_gradient(face_size, APPLE_BLUE_DARK, APPLE_BLUE_LIGHT, 135)
    face_mask = Image.new("L", (face_size, face_size), 0)
    ImageDraw.Draw(face_mask).rounded_rectangle(
        (0, 0, face_size - 1, face_size - 1),
        radius=int(face_size * 0.22),
        fill=255,
    )
    canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    face_offset = (1024 - face_size) // 2
    canvas.paste(bg, (face_offset, face_offset), face_mask)

    # 5. 合成：背景 + 居中 logo
    offset = (1024 - new_size) // 2
    canvas.alpha_composite(logo_small, (offset, offset))

    # 6. 顶部 8% 白色渐变（微妙高光，增强玻璃感）
    highlight = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    hd = ImageDraw.Draw(highlight)
    for y in range(0, int(1024 * 0.4)):
        a = int(28 * (1 - y / (1024 * 0.4)))
        hd.line((0, y, 1024, y), fill=(255, 255, 255, a))
    highlight = highlight.filter(ImageFilter.GaussianBlur(50))
    canvas = Image.alpha_composite(canvas, highlight)

    # 7. 输出（无圆角满铺，让 macOS / iOS / Android mask 自己处理）
    canvas.save(OUT, "PNG", optimize=True)
    print(f"✓ {OUT}（1024x1024 满铺，Apple Blue 渐变 + {int(LOGO_SCALE*100)}% 居中 logo，无圆角）")

if __name__ == "__main__":
    main()
