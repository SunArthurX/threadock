#!/usr/bin/env bash
# Round 10 截图：Chrome headless，polling 文件大小稳定 + pkill 兜底
set -e
OUT=docs/optimization-rounds
mkdir -p "$OUT"
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
W=1440; H=900

snap() {
  local url="$1"
  local out="$2"
  local prev_size=-1
  local stable=0
  # 启动 Chrome
  "$CHROME" --headless --disable-gpu --no-sandbox --hide-scrollbars \
    --window-size=$W,$H --force-device-scale-factor=1 \
    --user-data-dir=/tmp/chrome-r10 \
    --virtual-time-budget=3000 \
    --screenshot="$out" "$url" >/dev/null 2>&1 &
  local pid=$!
  # polling 文件大小 3 秒稳定就 kill
  for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
    sleep 1
    if [ -f "$out" ]; then
      local sz=$(stat -f%z "$out" 2>/dev/null || echo 0)
      if [ "$sz" -eq "$prev_size" ] && [ "$sz" -gt 1000 ]; then
        stable=$((stable+1))
        if [ "$stable" -ge 3 ]; then break; fi
      else
        stable=0
        prev_size=$sz
      fi
    fi
  done
  pkill -9 "Google Chrome" 2>/dev/null || true
  wait $pid 2>/dev/null || true
  if [ -f "$out" ]; then
    local final=$(stat -f%z "$out")
    echo "✓ $out ($final bytes)"
  else
    echo "✗ $out"
  fi
}

# 浅色 sm（默认）
snap "http://localhost:1420/" "$OUT/r10-light-sm-overview.png"
snap "http://localhost:1420/" "$OUT/r10-light-sm-overview-2.png"
