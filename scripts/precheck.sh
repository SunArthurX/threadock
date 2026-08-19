#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────
# precheck.sh — 提交前本地预检：逐条镜像 .github/workflows/ci.yml 的命令。
#
# 背景：CI 的 clippy 带 -W clippy::pedantic -D warnings 全套旗标，本地裸
# `cargo clippy` 不带同款旗标 → 本地全绿、CI 红（v1.1.1 后 master 曾因此挂掉）。
# 本脚本保证「本地过 = CI 过」。
#
# 用法：
#   scripts/precheck.sh          # 默认 = lint（快，秒级~1分钟）→ pre-commit 钩子跑这个
#   scripts/precheck.sh lint     # 同上：fmt×2 + clippy×2 + tsc + eslint
#   scripts/precheck.sh test     # lint + 全部测试（cargo/workspace + src-tauri + jieba + 前端）+ 构建 → pre-push 钩子跑这个
#   scripts/precheck.sh all      # test + cargo audit（装了才跑）+ MSRV 1.88 check（装了才跑）
#
# 钩子安装（一次性）：scripts/precheck.sh --install-hooks
# 跳过钩子（紧急情况）：git commit --no-verify
# ─────────────────────────────────────────────────────────────────────
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# 与 ci.yml 完全一致的 clippy 旗标（改 CI 时同步改这里）
CLIPPY_FLAGS=(-D warnings
  -W clippy::unwrap_used
  -W clippy::pedantic
  -A clippy::missing_errors_doc
  -A clippy::missing_panics_doc
  -A clippy::cast_precision_loss
  -A clippy::cast_possible_wrap
  -A clippy::cast_sign_loss
  -A clippy::cast_possible_truncation
  -A clippy::doc_markdown)

PASS=()
FAIL=()
step() { printf '\n\033[1;36m▶ %s\033[0m\n' "$1"; }
ok()   { PASS+=("$1"); printf '\033[1;32m✓ %s\033[0m\n' "$1"; }
bad()  { FAIL+=("$1"); printf '\033[1;31m✗ %s\033[0m\n' "$1"; }
run() { # run <名称> <命令...>；失败不中断（收尾汇总）
  local name="$1"; shift
  step "$name"
  if "$@"; then ok "$name"; else bad "$name"; fi
}

MODE="${1:-lint}"

install_hooks() {
  git config core.hooksPath .githooks
  echo "✓ git 钩子已指向 .githooks/（pre-commit=lint，pre-push=test）"
  echo "  取消：git config --unset core.hooksPath；跳过一次：git commit --no-verify"
}

case "$MODE" in
  --install-hooks) install_hooks; exit 0 ;;
  lint|test|all) ;;
  *) echo "用法: $0 [lint|test|all|--install-hooks]"; exit 2 ;;
esac

# ── lint（CI: Rust Tests & Lints 前两步 + Frontend Build 的 tsc/lint）──
run "cargo fmt --check（workspace，CI 同款）" cargo fmt --all -- --check
run "cargo clippy（workspace，CI 同款旗标）" cargo clippy --workspace --all-targets -- "${CLIPPY_FLAGS[@]}"
run "cargo fmt --check（src-tauri，CI 未覆盖、提前查）" bash -c 'cd apps/desktop/src-tauri && cargo fmt --all -- --check'
run "cargo clippy（src-tauri，CI 同款）" bash -c 'cd apps/desktop/src-tauri && cargo clippy --all-targets -- -D warnings'
run "tsc --noEmit（前端，CI 同款）" bash -c 'cd apps/desktop && npx tsc --noEmit'
run "eslint（前端，CI 同款）" bash -c 'cd apps/desktop && npm run lint'

# ── test（CI: Tests×3 平台取本机一份 + jieba + Frontend 单测/构建 + src-tauri 测试）──
if [[ "$MODE" != "lint" ]]; then
  # CI 注释：process_isolation.rs 需要 adapter 二进制先编译
  run "cargo build --bin ch-adapter-markdown（测试前置，CI 同款）" cargo build --bin ch-adapter-markdown
  run "cargo test --workspace（CI 同款）" cargo test --workspace
  run "cargo test（src-tauri，CI 未覆盖、提前查）" bash -c 'cd apps/desktop/src-tauri && cargo test'
  run "cargo test -p ch-search --features jieba（CI 同款）" cargo test -p ch-search --features jieba
  run "typegen 示例可运行（CI 同款）" bash -c 'cd apps/desktop/src-tauri && cargo run --example typegen'
  run "npm test（前端，CI 同款）" bash -c 'cd apps/desktop && npm test'
  run "vite build（前端，CI 同款）" bash -c 'cd apps/desktop && npx vite build'
fi

# ── all（CI: Supply-chain audit + MSRV；本机没装对应工具则提示跳过）──
if [[ "$MODE" == "all" ]]; then
  if command -v cargo-audit >/dev/null 2>&1; then
    run "cargo audit（CI 同款，忽略 RUSTSEC-2026-0253）" cargo audit --ignore RUSTSEC-2026-0253
  else
    step "cargo audit：未安装（cargo install cargo-audit --locked），跳过"
  fi
  if rustup toolchain list 2>/dev/null | grep -q "1.88"; then
    run "MSRV 1.88 cargo check（CI 同款）" cargo +1.88 check --workspace
  else
    step "MSRV 1.88：未安装该工具链（rustup toolchain install 1.88），跳过"
  fi
fi

# ── 汇总 ──
echo
printf '\033[1m────── 预检结果：通过 %d' "${#PASS[@]}"
[[ ${#FAIL[@]} -gt 0 ]] && printf '，\033[1;31m失败 %d：%s\033[0m' "${#FAIL[@]}" "${FAIL[*]}" || printf ' \033[1;32m（全部通过 ✅）\033[0m'
printf '\033[1m ──────\033[0m\n'
[[ ${#FAIL[@]} -eq 0 ]]
