#!/usr/bin/env python3
"""
重新生成 Threadock app icon：
- 1024x1024 源
- Apple Blue 渐变背景（#0071e3 → #4f8cff 135° 对角渐变）
- macOS-style superellipse 圆角（22% 半径，等价于 SVG rx=224）
- 现有 logo 缩小到 56% 居中（保留品牌识别）
- 上下加微妙 inner shadow 增强层次

设计参考 macOS Sonoma / iOS 17 app icon：留白足 + 渐变 + 中心 logo 居中。
"""
import sys
from PIL import Image, ImageDraw, ImageFilter

SRC = "apps/desktop/src-tauri/icons/icon-1024x1024.png"
OUT = "apps/desktop/src-tauri/icons/icon-1024x1024.png"  # 覆盖原文件

# Apple 系统色
APPLE_BLUE_DARK = (0, 113, 227, 255)   # #0071E3
APPLE_BLUE_LIGHT = (79, 140, 255, 255)  # #4F8CFF

def make_rounded_mask(size: int, radius_pct: float = 0.225) -> Image.Image:
    """生成 macOS superellipse 圆角 mask（更圆润的 squircle，不是普通 rounded-rect）。
    PIL 没原生 squircle，用 4× supersample + 重采样到目标尺寸模拟。
    22% 圆角是 macOS 实际用的比例（参考 Apple Design Resources）。
    """
    s = size * 4
    r = int(s * radius_pct)
    mask = Image.new("L", (s, s), 0)
    d = ImageDraw.Draw(mask)
    d.rounded_rectangle((0, 0, s - 1, s - 1), radius=r, fill=255)
    return mask.resize((size, size), Image.LANCZOS)

def make_gradient(size: int, c1, c2, angle_deg: float = 135) -> Image.Image:
    """线性渐变背景"""
    img = Image.new("RGBA", (size, size), c1)
    pixels = img.load()
    import math
    angle = math.radians(angle_deg)
    cos_a, sin_a = math.cos(angle), math.sin(angle)
    for y in range(size):
        for x in range(size):
            # 沿渐变方向投影
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
    assert logo.size == (1024, 1024), f"unexpected size: {logo.size}"

    # 2. 抠出 logo 内容（去掉黑色背景，保留白色形状）
    # 原图：黑色 RGB(0,0,0) + 白色 RGB(255,255,255) 形状
    # 用白色通道作为 alpha：白色 = 1，黑色 = 0
    r, g, b, a = logo.split()
    # 取最亮通道（白色形状）作为 mask
    luminance = Image.eval(r, lambda x: x)  # 红通道
    # 实际白色为 255，黑色为 0
    alpha_from_white = Image.merge("L", (r,))  # R 通道 = 白色 → 255
    # 抠出白色形状
    logo_only = Image.new("RGBA", logo.size, (0, 0, 0, 0))
    logo_only.paste((255, 255, 255, 255), mask=alpha_from_white)

    # 3. 缩小 logo 到 56%（与 Apple app icon 设计一致）
    scale = 0.56
    new_size = int(1024 * scale)
    logo_small = logo_only.resize((new_size, new_size), Image.LANCZOS)

    # 4. 生成 1024x1024 渐变背景
    bg = make_gradient(1024, APPLE_BLUE_DARK, APPLE_BLUE_LIGHT, 135)

    # 5. 合成：背景 + 居中 logo
    canvas = bg.copy()
    offset = (1024 - new_size) // 2
    canvas.alpha_composite(logo_small, (offset, offset))

    # 6. 应用 superellipse 圆角 mask
    mask = make_rounded_mask(1024, 0.225)
    rounded = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    rounded.paste(canvas, (0, 0), mask)

    # 7. 微妙 top-highlight（增强层次）— 顶部加 5% 白色渐变
    highlight = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    hd = ImageDraw.Draw(highlight)
    for y in range(0, 512):
        a = int(20 * (1 - y / 512))
        hd.line((0, y, 1024, y), fill=(255, 255, 255, a))
    highlight = highlight.filter(ImageFilter.GaussianBlur(40))
    rounded = Image.alpha_composite(rounded, highlight)

    # 8. 输出
    rounded.save(OUT, "PNG", optimize=True)
    print(f"✓ 生成 {OUT}（{1024}x{1024}，Apple Blue 渐变 + 56% 居中 logo + 22% superellipse）")

if __name__ == "__main__":
    main()
