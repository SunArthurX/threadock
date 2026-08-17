#!/bin/bash
# CLI 端到端真人流程测试：真实二进制 + 临时库，断言退出码与输出。
# 用法：bash scripts/e2e_cli.sh   （从仓库根目录）
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CH="$ROOT/target/release/ch"
TMP="$(mktemp -d)"
DB="$TMP/hub.db"
PASS=0; FAIL=0; FAILED_NAMES=()

check() { # check <名称> <期望子串> <命令...>
  local name="$1"; local expect="$2"; shift 2
  local out; out="$("$@" 2>&1)"; local rc=$?
  if [ $rc -eq 0 ] && echo "$out" | grep -qE "$expect"; then
    PASS=$((PASS+1)); echo "  ✓ $name"
  else
    FAIL=$((FAIL+1)); FAILED_NAMES+=("$name")
    echo "  ✗ $name (rc=$rc, 期望含「${expect}」)"
    echo "$out" | head -5 | sed 's/^/      /'
  fi
}
check_fail() { # 期望失败
  local name="$1"; shift
  local out; out="$("$@" 2>&1)"; local rc=$?
  if [ $rc -ne 0 ]; then PASS=$((PASS+1)); echo "  ✓ ${name}（按预期失败）"
  else FAIL=$((FAIL+1)); FAILED_NAMES+=("$name"); echo "  ✗ ${name} 应当失败但成功了"; fi
}

echo "── 1. 导入 ──────────────────────────────────────"
check "import md with workspace" "conversation" $CH --db "$DB" import "$ROOT/fixtures/markdown/tauri-background.md" --workspace alpha-app
check "import md no workspace" "conversation" $CH --db "$DB" import "$ROOT/fixtures/markdown/rust-error-handling.md"
check "import 幂等（重复导入同文件）" "conversation" $CH --db "$DB" import "$ROOT/fixtures/markdown/tauri-background.md" --workspace alpha-app
ID=$($CH --db "$DB" list | grep -oE 'conv_[a-f0-9]+' | head -1)
ID2=$($CH --db "$DB" list | grep -oE 'conv_[a-f0-9]+' | tail -1)
echo "  （会话 ID：${ID} / ${ID2}）"

echo "── 2. 列表与详情 ────────────────────────────────"
check "list" "conv_" $CH --db "$DB" list
check "show" "Title|标题" $CH --db "$DB" show "$ID"

echo "── 3. 收藏/标签/归档/删除 ───────────────────────"
check "favorite" "favorited" $CH --db "$DB" favorite "$ID"
check "favorites 含该会话" "$ID" $CH --db "$DB" favorites
check "unfavorite" "unfavorited" $CH --db "$DB" unfavorite "$ID"
check "tag" "tagged" $CH --db "$DB" tag "$ID" rust
check "tags 列出" "rust" $CH --db "$DB" tags "$ID"
check "untag" "untagged" $CH --db "$DB" untag "$ID" rust
check "archive" "archived" $CH --db "$DB" archive "$ID"
check "unarchive" "unarchived" $CH --db "$DB" unarchive "$ID"
check "软删除" "soft deleted" $CH --db "$DB" delete "$ID2"
check "undelete 恢复" "restored" $CH --db "$DB" undelete "$ID2"

echo "── 4. 搜索（FTS5 + 语法）──────────────────────"
check "search 纯文本" "message_id|结果|Found|conv_" $CH --db "$DB" search WorkManager
check "search provider: 语法" "conv_" $CH --db "$DB" search "provider:generic WorkManager"
check "search workspace: 语法" "conv_" $CH --db "$DB" search "workspace:alpha-app WorkManager"
check "search status:favorite" "no matches|conv_" $CH --db "$DB" search "status:favorite WorkManager"
check "search after: 语法" "conv_" $CH --db "$DB" search "after:2020-01-01 WorkManager"
check "search type:user" "conv_|no matches" $CH --db "$DB" search "type:user sync"

echo "── 5. 搜索（Tantivy + 语法）────────────────────"
check "search-tantivy 纯文本" "conv_" $CH --db "$DB" search-tantivy WorkManager
check "search-tantivy provider:" "conv_" $CH --db "$DB" search-tantivy "provider:generic WorkManager"
check "search-tantivy workspace 名字解析" "Tantivy found|no workspace matches" $CH --db "$DB" search-tantivy "workspace:alpha-app WorkManager"

echo "── 6. 知识提取与相似 ────────────────────────────"
check "knowledge 提取" "知识|knowledge|摘要" $CH --db "$DB" knowledge "$ID"
check "knowledge --save" "saved|已保存" $CH --db "$DB" knowledge "$ID" --save
check "knowledge --show" "knowledge|摘要" $CH --db "$DB" knowledge "$ID" --show
check "similar" "相似|similar|conv_|无" $CH --db "$DB" similar "$ID"

echo "── 7. 脱敏规则 ─────────────────────────────────"
check "redaction-rule add" "added" $CH --db "$DB" redaction-rule add mykey 'sk-foo-[0-9]+'
check "redaction-rule list" "mykey" $CH --db "$DB" redaction-rule list
check "redaction-rule remove" "removed" $CH --db "$DB" redaction-rule remove mykey

echo "── 8. 导出 ─────────────────────────────────────"
check "export markdown" "written|导出|exported" $CH --db "$DB" export markdown "$ID" "$TMP/out.md"
grep -q "Tauri" "$TMP/out.md" && { PASS=$((PASS+1)); echo "  ✓ 导出内容含正文"; } || { FAIL=$((FAIL+1)); FAILED_NAMES+=("导出内容"); echo "  ✗ 导出内容不含正文"; }
check "export json" "written|导出|exported" $CH --db "$DB" export json "$ID" "$TMP/out.json"
WS_ID=$($CH --db "$DB" list | grep -oE 'ws_[a-f0-9]+' | head -1)
[ -n "$WS_ID" ] && check "export workspace" "exported" $CH --db "$DB" export workspace "$WS_ID" "$TMP/wsdir"

echo "── 9. 备份恢复 ─────────────────────────────────"
export CH_BACKUP_PASSWORD="e2e-test-pass"
check "backup" "backup created" $CH --db "$DB" backup "$TMP/hub.chbak"
check "restore" "restored" $CH --db "$DB" restore "$TMP/hub.chbak" "$TMP/restored"
# 恢复出的库可用
check "恢复库可查询" "conv_" $CH --db "$TMP/restored/conversation-hub.db" list

echo "── 10. 完整性 ──────────────────────────────────"
check "integrity" "ok" $CH --db "$DB" integrity

echo "── 11. import-from（空 HOME 优雅降级）─────────"
EMPTY_HOME="$(mktemp -d)"
out=$(HOME="$EMPTY_HOME" $CH --db "$DB" import-from claude-code list 2>&1); rc=$?
if [ $rc -eq 0 ]; then PASS=$((PASS+1)); echo "  ✓ import-from claude-code list（空 HOME 优雅）"; else FAIL=$((FAIL+1)); FAILED_NAMES+=("import-from 空 HOME"); echo "  ✗ rc=$rc: $out"; fi
out=$(HOME="$EMPTY_HOME" $CH --db "$DB" import-from zcode list 2>&1); rc=$?
[ $rc -eq 0 ] && { PASS=$((PASS+1)); echo "  ✓ import-from zcode list（空 HOME 优雅）"; } || { FAIL=$((FAIL+1)); FAILED_NAMES+=("zcode 空 HOME"); echo "  ✗ rc=$rc: $out"; }

echo "── 12. 硬删除（级联）──────────────────────────"
check "hard delete" "hard deleted" $CH --db "$DB" delete "$ID2" --hard
check_fail "show 已硬删的会话应失败" $CH --db "$DB" show "$ID2"

echo "── 13. Daemon JSON-RPC（14 方法）──────────────"
rpc() { echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":$2}" | $CH --db "$DB" daemon 2>/dev/null; }
declare -a METHODS=(
  "system.getInfo|{}"
  "workspace.list|{}"
  "conversation.list|{}"
  "conversation.get|{\"id\":\"$ID\"}"
  "message.list|{\"conversation_id\":\"$ID\"}"
  "event.list|{\"conversation_id\":\"$ID\"}"
  "search.query|{\"query\":\"WorkManager\"}"
  "search.query|{\"query\":\"provider:generic WorkManager\",\"engine\":\"tantivy\"}"
  "search.query|{\"query\":\"WorkManager\",\"engine\":\"fts5\"}"
  "knowledge.extract|{\"conversation_id\":\"$ID\"}"
  "conversation.similar|{\"conversation_id\":\"$ID\"}"
)
for m in "${METHODS[@]}"; do
  name="${m%%|*}"; params="${m#*|}"
  out=$(rpc "$name" "$params")
  if echo "$out" | grep -q '"result"'; then PASS=$((PASS+1)); echo "  ✓ daemon $name"
  else FAIL=$((FAIL+1)); FAILED_NAMES+=("daemon $name"); echo "  ✗ daemon $name: $(echo "$out" | head -c 200)"; fi
done
# knowledge.save 两步流程：extract → save → get 校验闭环
KRESULT=$(rpc "knowledge.extract" "{\"conversation_id\":\"$ID\"}" | python3 -c 'import json,sys; print(json.dumps(json.load(sys.stdin)["result"]))')
if [ -n "$KRESULT" ]; then
  out=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"knowledge.save\",\"params\":{\"conversation_id\":\"$ID\",\"result\":$KRESULT}}" | $CH --db "$DB" daemon 2>/dev/null)
  echo "$out" | grep -q '"saved":true' && { PASS=$((PASS+1)); echo "  ✓ daemon knowledge.save（两步）"; } || { FAIL=$((FAIL+1)); FAILED_NAMES+=("daemon knowledge.save"); echo "  ✗ daemon knowledge.save: $(echo "$out" | head -c 160)"; }
  rpc "knowledge.get" "{\"conversation_id\":\"$ID\"}" | grep -q '"result"' && { PASS=$((PASS+1)); echo "  ✓ daemon knowledge.get"; } || { FAIL=$((FAIL+1)); FAILED_NAMES+=("daemon knowledge.get"); }
else
  FAIL=$((FAIL+1)); FAILED_NAMES+=("daemon knowledge.extract 结果为空")
fi

# conversation.delete + restore 走一遍（会改变状态，放最后）
rpc "conversation.delete" "{\"id\":\"$ID\"}" | grep -q '"result"' && { PASS=$((PASS+1)); echo "  ✓ daemon conversation.delete"; } || { FAIL=$((FAIL+1)); FAILED_NAMES+=("daemon delete"); }
rpc "conversation.restore" "{\"id\":\"$ID\"}" | grep -q '"result"' && { PASS=$((PASS+1)); echo "  ✓ daemon conversation.restore"; } || { FAIL=$((FAIL+1)); FAILED_NAMES+=("daemon restore"); }
rpc "provider.sync" "{\"path\":\"$ROOT/fixtures/markdown/tauri-background.md\",\"workspace_name\":\"daemon-ws\"}" | grep -q '"result"' && { PASS=$((PASS+1)); echo "  ✓ daemon provider.sync"; } || { FAIL=$((FAIL+1)); FAILED_NAMES+=("daemon provider.sync"); }
# 错误路径：未知方法
out=$(rpc "no.such.method" "{}")
echo "$out" | grep -q -- '-32601' && { PASS=$((PASS+1)); echo "  ✓ daemon 未知方法报 -32601"; } || { FAIL=$((FAIL+1)); FAILED_NAMES+=("daemon 未知方法"); }

echo "────────────────────────────────────────────────"
echo "CLI E2E：$PASS 通过 / $FAIL 失败"
if [ $FAIL -gt 0 ]; then printf '失败项：%s\n' "${FAILED_NAMES[*]}"; exit 1; fi
rm -rf "$TMP"
