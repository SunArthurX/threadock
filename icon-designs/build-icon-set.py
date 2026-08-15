#!/usr/bin/env python3
"""
Threadock / Conversation Hub — App icon 套件生成器
从 icon-designs/option-a-thread-bubbles.png (2048x2048) 生成:
  - icon.png (1024x1024)
  - 32x32.png / 128x128.png / 128x128@2x.png (256x256)
  - icon.ico (多尺寸打包: 16/24/32/48/64/128/256)
  - icon.icns (macOS, 由 iconutil 合成)
  - 同时把各尺寸 PNG 也复制到 icons 目录

特点: 自动用色度检测裁掉白边/发光阴影,只保留 squircle 本体,背景透明

输出: apps/desktop/src-tauri/icons/ (Tauri 2 标准)
"""
from pathlib import Path
from PIL import Image
import numpy as np
import shutil
import subprocess
import tempfile

ROOT = Path("/Users/sunqingguang/Downloads/06.code/threadock")
SRC_PNG = ROOT / "icon-designs" / "option-a-thread-bubbles.png"
ICONS_DIR = ROOT / "apps" / "desktop" / "src-tauri" / "icons"
ICONS_DIR.mkdir(parents=True, exist_ok=True)

# 1. 加载源图
print(f"📖 加载源图: {SRC_PNG}")
img = Image.open(SRC_PNG).convert("RGBA")
print(f"   原始尺寸: {img.size} | 模式: {img.mode}")
assert img.size[0] >= 1024, "源图至少要 1024x1024"

# 2. 自动裁掉白边/发光阴影 —— 用色度检测紫色 squircle 边界
#    (饱和像素 max-min RGB > 25,白色/灰色阴影色度接近 0)
arr = np.array(img)
chroma = arr[:, :, :3].max(axis=2) - arr[:, :, :3].min(axis=2)
colored_mask = (chroma > 25) & (arr[:, :, 3] > 0)
ys, xs = np.where(colored_mask)
bbox = (int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1)
print(f"🔍 色度裁剪 bbox: {bbox}  尺寸: {bbox[2]-bbox[0]}x{bbox[3]-bbox[1]}")
img = img.crop(bbox)

# 居中补方 (如果不是 1:1)
cw, ch = img.size
if abs(cw - ch) > 5:
    s = max(cw, ch)
    sq = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    sq.paste(img, ((s - cw) // 2, (s - ch) // 2))
    img = sq
    print(f"   居中补方: {img.size}")

# 3. 生成主图 icon.png (1024x1024, 透明背景)
master = img.resize((1024, 1024), Image.LANCZOS)
master_path = ICONS_DIR / "icon.png"
master.save(master_path, "PNG", optimize=True)
print(f"✅ icon.png (1024x1024, 透明背景) → {master_path}")

# 3. 生成标准尺寸 PNG
sizes = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,  # 256x256
}
for name, size in sizes.items():
    out = ICONS_DIR / name
    master.resize((size, size), Image.LANCZOS).save(out, "PNG", optimize=True)
    print(f"✅ {name} ({size}x{size}) → {out}")

# 4. 生成 .ico (Windows 多尺寸打包)
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
ico_path = ICONS_DIR / "icon.ico"
# Pillow 的 sizes 参数直接传 [(w,h),...] 列表 — 比 append_images 更可靠
master.save(ico_path, format="ICO", sizes=[(s, s) for s in ico_sizes])
print(f"✅ icon.ico (多尺寸 {ico_sizes}) → {ico_path}")

# 5. 生成 .icns (macOS, 用 iconutil)
#    iconutil 需要一个 .iconset 目录,文件名固定:
#      icon_16x16.png / icon_32x32.png / icon_64x64.png / icon_128x128.png
#      icon_256x256.png / icon_512x512.png / icon_512x512@2x.png (1024)
#      + 带 @2x 的 retina 尺寸
iconset_dir = Path(tempfile.mkdtemp()) / "Threadock.iconset"
iconset_dir.mkdir(parents=True, exist_ok=True)
iconset_specs = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_64x64.png", 64),
    ("icon_64x64@2x.png", 128),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]
for name, size in iconset_specs:
    out = iconset_dir / name
    master.resize((size, size), Image.LANCZOS).save(out, "PNG", optimize=True)
print(f"📦 临时 iconset: {iconset_dir}")

icns_path = ICONS_DIR / "icon.icns"
result = subprocess.run(
    ["iconutil", "-c", "icns", str(iconset_dir), "-o", str(icns_path)],
    capture_output=True, text=True
)
if result.returncode == 0:
    print(f"✅ icon.icns → {icns_path}")
else:
    print(f"❌ iconutil 失败: {result.stderr}")

# 清理临时 iconset
shutil.rmtree(iconset_dir.parent, ignore_errors=True)

# 6. 同时把主图备份到 icon-designs (源文件)
master.save(ROOT / "icon-designs" / "icon-master-1024.png", "PNG", optimize=True)
print(f"📁 源文件备份: icon-designs/icon-master-1024.png")

print("\n🎉 套件生成完成! 目录内容:")
for p in sorted(ICONS_DIR.iterdir()):
    size = p.stat().st_size
    print(f"   {p.name:30s} {size:>10,} bytes")
