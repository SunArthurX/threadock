# Threadock

> 跨 AI IDE 的统一会话归档、检索、知识提取与治理平台——把 ZCode / Claude Code / Cursor / MiniMax Code / Codex 等工具里的会话、工具调用、命令、Diff、Artifact 统一收集、标准化、全文检索、知识化，并对用量/成本/安全做持续治理。
>
> 配套方案：[`docs/ai-ide-conversation-hub-enterprise-plan.md`](./docs/ai-ide-conversation-hub-enterprise-plan.md)
> 落地清单：[`docs/conversation-hub-execution-plan.md`](./docs/conversation-hub-execution-plan.md)
> 治理平台方案：[`docs/codeagent-ops-plan.md`](./docs/codeagent-ops-plan.md)
> Tauri API 文档：[`docs/api.md`](./docs/api.md)（自动生成）
> 用户指南：[`docs/user-guide.md`](./docs/user-guide.md) · 隐私声明：[`docs/privacy.md`](./docs/privacy.md)
> 性能基准：[`docs/benchmark-report-v1.0.0.md`](./docs/benchmark-report-v1.0.0.md)

## 当前状态：v1.0.0（2026-08-17）

首个正式版本。端到端数据闭环 + 桌面 GUI + 常驻服务 + CodeAgentOps 治理平台（M1–M15 全量），
执行计划 Phase 2（MVP / Gate 1）验收线全部达标（见 CHANGELOG 1.0.0 与性能基准报告）。

### 架构

```
┌─────────────┐   ┌─────────────┐   ┌──────────────┐
│  Tauri GUI  │   │  CLI (ch)   │   │ 外部客户端   │
└──────┬──────┘   └──────┬──────┘   └──────┬───────┘
       │ Tauri cmd       │ 子命令          │ JSON-RPC
       └────────┬────────┴─────────────────┘
                ▼
        ┌───────────────┐
        │  DaemonState  │  ← 单点写者（plan §8.2）
        │  ├ Repository │     SQLite WAL（V14 schema，25 表 + FTS5）
        │  ├ SearchIndex│     Tantivy（N-gram 中文 + BM25）
        │  └ RawStore   │     BLAKE3 内容寻址 + zstd
        └───────────────┘
                ▼
        ┌───────────────┐
        │ Adapter       │  通用 Adapter：独立子进程（stdio JSON-RPC）
        │ markdown/jsonl│  IDE Adapter：只读直读源库（plan §10.4/§10.5）
        └───────────────┘
```

### 能力清单（已实现）

会话中枢：

| 能力 | 实现位置 | 对应 plan |
|---|---|---|
| 统一领域模型（6 来源 + 19 事件类型） | `crates/domain` | §4, §12 |
| SQLite V14（25 表 + WAL + Migration + FTS5） | `crates/storage` | §9.4, §12 |
| Tantivy 全文检索（N-gram 中文 + BM25 + 高亮） | `crates/search` | §9.5, §13 |
| 双引擎搜索（Tantivy 主 + FTS5 降级） | `crates/storage` + `crates/search` | §13 |
| 标准化流水线（BLAKE3 hash + 幂等 + 完整度评分） | `crates/normalization` | §11, §17.3 |
| Raw Store（内容寻址 + zstd 压缩） | `crates/raw-store` | §9.6, §2.3 |
| Workspace 自动合并（7 级优先级 + 置信度） | `crates/identity-resolver` | §4.3 |
| 5 个 IDE Adapter（ZCode / Claude Code / Cursor / MiniMax / Codex，只读） | `crates/adapter-*` | §10.5 |
| 通用 Adapter 进程隔离（stdio JSON-RPC + 崩溃检测） | `crates/adapter-sdk` + `adapter-host` | §10.4 |
| 增量同步（import_state 新鲜度检测 + 10 分钟自动同步） | `apps/desktop` | §11.2 |
| 收藏 / 标签 / 归档 / 软删除 / 回收站 / 硬删除（级联清理） | `crates/storage` + GUI | §6.3/§6.4/§11.4 |
| **搜索查询语法**（`provider:` `workspace:` `type:` `status:` `file:` `model:` `after:` `before:`） | `ch_domain::query_syntax` + 双引擎 | §13.2 |
| **保存搜索**（V14 表，跨会话持久） | `crates/storage` + GUI | §13.2 |
| **Workspace 治理**：手动合并/拆分/重命名 + 置信度警示 | `crates/storage` + GUI | §4.3 / P2-2 |
| **原始视图 ↔ 统一视图切换**（Raw Store 只读展示） | GUI | P2-3 |
| **一键打开来源应用 / 恢复命令**（claude/codex resume） | GUI | P2-3 |
| **jieba 可插拔分词器**（`--features jieba`，默认 N-gram 兜底） | `crates/search` | §13.1 |
| 导出（Markdown/JSON/批量 + 敏感信息脱敏 + 自定义规则） | `crates/export` | §6.6, §14.6 |
| 加密备份/恢复（XChaCha20-Poly1305 + Argon2id） | `crates/backup` | §6.6, §14.3 |
| 知识提取（摘要/决策/TODO/错误/命令/文件 + 版本化持久化） | `crates/knowledge` + `crates/storage` | §13.5 |
| 相似会话推荐（有界候选集） | `crates/storage` | §6.7 |
| Daemon 常驻服务（JSON-RPC over stdio，14 个方法） | `crates/daemon` | §8.2, §16 |
| Tauri 桌面 GUI（101 个 Tauri 命令） | `apps/desktop` | §9.1, §17 |

CodeAgentOps 治理平台（M1–M15，见 `docs/codeagent-ops-plan.md`）：

| 能力 | 实现位置 |
|---|---|
| 用量/成本采集（请求级 usage，4+ Agent） | `crates/ops-metrics` |
| 治理仪表盘（成本/缓存命中/延迟 P50/P95 趋势图） | `apps/desktop` OpsView |
| 安全审计引擎（敏感信息 + 危险命令 + HTML 报告） | `crates/audit` |
| 策略 / 预算 / 定价模型 + 超限告警 + 月末预测 | `crates/storage` + GUI |
| 资产盘点（跨 Agent 的 skills / plugins） | `crates/ops-metrics` |
| 项目成本归因 + 缓存命中率分析 | `crates/ops-metrics` |
| 异常检测（错误尖峰 / 重试风暴 / 上下文超限） | `crates/ops-metrics` |
| Agent 健康评分（稳定性 0-100） | `crates/ops-metrics` |
| Token 浪费检测 + Agent 横向对比 | `crates/ops-metrics` |
| 周报自动生成（HTML，`app_data/reports/`） | `apps/desktop` |
| 数据生命周期（存储看板 / 孤儿 blob GC / 保留策略 / 索引重建） | `apps/desktop` SettingsView |
| 治理审计轨迹（audit_logs，敏感操作全记录） | `crates/storage` |

桌面 GUI 视图：概览 / 会话（时间线 + 右键菜单 + 批量操作）/ 知识（跨会话引用 + 筛选导出）/ 活动（GitHub 风格热力图）/ 项目 / 治理（成本·资产·安全·自动化）/ 设置；含 Command Palette、首次启动引导、暗色/亮色主题、私人笔记、启动更新日志。

### 尚未实现（1.1+ 路线）

- AI 提取走真实 LLM（当前规则引擎，接口已留好）
- OpenCode Adapter（第 6 来源）
- Daemon UDS/Named Pipe IPC + 本地认证 Token（当前仅 stdio）
- Adapter Host 资源配额（内存/CPU 限制、文件白名单、禁网）
- Tauri Updater 自动更新、安装包签名公证（待证书）
- Android 移动端浏览（Phase 4 PoC）
- 企业能力：SSO / RBAC / KMS / 加密同步（Phase 5）

## 快速开始

### 构建

```bash
# CLI + Daemon + Adapter 二进制
cargo build --release -p ch-cli -p ch-daemon -p ch-adapter-markdown

# Tauri 桌面应用
cd apps/desktop && npm install && npm run build
cd src-tauri && cargo build --release
```

### 安装（v1.0.0 起）

推 `v*` tag 后由 [Release 流水线](./.github/workflows/release.yml) 自动构建并发布：
macOS（dmg，arm64/x64）、Windows（nsis + msi）、Linux（appimage + deb）安装包，
以及四平台 CLI 二进制与 SHA256SUMS。
**安装包当前未签名**（证书待接入）：macOS 首次打开右键 → 打开；Windows SmartScreen 选「仍要运行」；
下载后请比对 SHA256SUMS。

### CLI 使用

```bash
CH=./target/release/ch

# 导入会话（md 自动识别）
$CH --db ./hub.db import docs/tauri-android.md --workspace my-app

# 从真实 IDE 数据导入（只读）
$CH --db ./hub.db import-from claude-code list
$CH --db ./hub.db import-from claude-code <session-id>
$CH --db ./hub.db import-from zcode list
$CH --db ./hub.db import-from zcode <session-id>

# 列出会话（支持过滤）
$CH --db ./hub.db list
$CH --db ./hub.db list --favorite
$CH --db ./hub.db list --provider codex

# 全文搜索（FTS5 或 Tantivy），支持查询语法前缀
$CH --db ./hub.db search 后台任务
$CH --db ./hub.db search-tantivy WorkManager
$CH --db ./hub.db search 'provider:codex 错误处理'
$CH --db ./hub.db search 'workspace:my-app after:2026-01-01 status:favorite tauri'

# 收藏 / 标签 / 归档 / 删除
$CH --db ./hub.db favorite <id>       # unfavorite / favorites
$CH --db ./hub.db tag <id> rust       # untag / tags
$CH --db ./hub.db archive <id>        # unarchive
$CH --db ./hub.db delete <id>          # 软删除（undelete 可恢复）
$CH --db ./hub.db delete <id> --hard   # 硬删除（永久，级联清理索引与 raw）

# 知识提取与相似推荐
$CH --db ./hub.db knowledge <id>          # 提取并显示
$CH --db ./hub.db knowledge <id> --save   # 提取并持久化（版本管理）
$CH --db ./hub.db knowledge <id> --show   # 显示已保存的提取结果
$CH --db ./hub.db similar <id>            # 相似会话

# 导出（含脱敏）
$CH --db ./hub.db export markdown <id> out.md
$CH --db ./hub.db export json <id> out.json
$CH --db ./hub.db export workspace <ws-id> out-dir/

# 自定义脱敏规则
$CH --db ./hub.db redaction-rule add my-key 'sk-foo-[0-9]+'
$CH --db ./hub.db redaction-rule list

# 加密备份 / 恢复
CH_BACKUP_PASSWORD="mypassword" $CH --db ./hub.db backup hub.chbak
CH_BACKUP_PASSWORD="mypassword" $CH --db ./hub.db restore hub.chbak restored/

# 启动 Daemon（stdio JSON-RPC 服务）
$CH --db ./data daemon

# 数据库完整性检查
$CH --db ./hub.db integrity
```

### Daemon JSON-RPC

Daemon 通过 stdio 接收 newline-delimited JSON-RPC 2.0：

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"system.getInfo","params":{}}' | $CH daemon
echo '{"jsonrpc":"2.0","id":2,"method":"search.query","params":{"query":"tauri","engine":"tantivy"}}' | $CH daemon
```

方法清单（plan §16.1）：`system.getInfo` / `workspace.list` / `conversation.list` / `conversation.get` / `conversation.delete` / `conversation.restore` / `conversation.similar` / `message.list` / `event.list` / `search.query` / `knowledge.extract` / `knowledge.save` / `knowledge.get` / `provider.sync`

## 运行测试

```bash
cargo test --workspace                                  # 421 个测试
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml   # 13 个测试
cd apps/desktop && npm test                             # 344 个前端测试
cd apps/desktop && npm run lint && npm run build        # ESLint（0 error / 0 warning）+ 构建
cargo clippy --workspace --all-targets                  # pedantic 级 0 warning
cargo test -p ch-search --features jieba                # jieba 分词器（22 个测试）
cargo test --release -p ch-benchmarks --test perf -- --ignored --nocapture  # 性能基准
cargo test --release -p ch-benchmarks --test perf large_scale -- --ignored --nocapture  # Gate 1 十万会话门禁
```

## 项目结构

```
threadock/
├── crates/
│   ├── domain/              统一领域模型（无存储依赖）
│   ├── storage/             SQLite V14 + Migration + FTS5 + 过滤 + 删除 + 治理表
│   ├── raw-store/           内容寻址原始数据（BLAKE3 + zstd）
│   ├── search/              Tantivy 全文检索（N-gram 中文 + BM25）
│   ├── identity-resolver/   Workspace 身份解析（7 级合并优先级）
│   ├── normalization/       标准化流水线（hash + 幂等 + 完整度）
│   ├── export/              导出（Markdown/JSON + 脱敏）
│   ├── backup/              加密备份/恢复（XChaCha20 + Argon2id）
│   ├── knowledge/           知识提取（规则引擎，plan §13.5）
│   ├── adapter-sdk/         Adapter trait + stdio JSON-RPC 协议
│   ├── adapter-host/        进程隔离（spawn + 超时 + 崩溃检测）
│   ├── adapter-markdown/    Markdown Adapter（独立进程二进制）
│   ├── adapter-jsonl/       JSONL Adapter
│   ├── adapter-claude-code/ Claude Code 会话 Adapter（~/.claude JSONL）
│   ├── adapter-zcode/       ZCode Adapter（SQLite 直读）
│   ├── adapter-cursor/      Cursor Adapter（state.vscdb）
│   ├── adapter-minimax/     MiniMax Code Adapter（runtime-state.sqlite）
│   ├── adapter-codex/       Codex Adapter（~/.codex/sessions JSONL）
│   ├── ops-metrics/         用量/成本/健康度指标采集（治理）
│   ├── audit/               安全审计（敏感信息 + 危险命令扫描）
│   ├── benchmarks/          性能基准（吞吐 / 搜索延迟 / 冷启动）
│   ├── daemon/              常驻服务（JSON-RPC over stdio）
│   └── cli/                 `ch` 命令行（import/list/search/export/...）
├── apps/desktop/            Tauri 2 桌面应用（React + TS）
│   ├── src/                 前端（概览/会话/知识/活动/项目/治理/设置）
│   └── src-tauri/           Rust 后端（101 个 Tauri 命令，嵌入 DaemonState）
├── docs/                    方案、执行计划、治理计划、API 文档
│   └── *.md                 示例会话（tauri-android.md / rust-errors.md）
└── .github/workflows/       CI（3 OS 测试矩阵 + MSRV + cargo-audit + CodeQL）
```

## 关键设计决策

详见 [`docs/conversation-hub-execution-plan.md` §1.3](./docs/conversation-hub-execution-plan.md)。plan §1.3 的八条「不可妥协关键决策」全部已实现：

| 决策 | 状态 |
|---|---|
| Local-first | ✅ 数据默认留本机 |
| SQLite WAL | ✅ V14 schema |
| Tauri 桌面 | ✅ React + TS |
| Rust 核心 | ✅ |
| Tantivy 搜索 | ✅ N-gram 中文 |
| Adapter 进程隔离 | ✅ 通用 Adapter 子进程；IDE Adapter 只读直读 |
| Raw + Normalized 双存储 | ✅ |
| 第三方只读 | ✅ |

## Roadmap：v1.0.0 完成情况

以执行计划 Phase 2（MVP，Gate 1）为验收基线。原差距清单（19 项）的处置结果：

### P0 发布工程 — 全部完成

| # | 事项 | 状态 |
|---|---|---|
| 1 | 版本号统一（三处不一致 → 单一版本流） | ✅ 0.4.0 起统一，1.0.0 收口 |
| 2 | CHANGELOG 补记 26 轮迭代 | ✅ [0.4.0] 条目 |
| 3 | Release 流水线（tag → 三平台安装包 + CLI + SHA256SUMS） | ✅ `.github/workflows/release.yml` |
| 4 | 签名与公证 | ⏸ 未签名发布（证书待接入，Release 说明已声明验证方式） |
| 5 | 用户文档（用户指南 + 隐私声明） | ✅ `docs/user-guide.md` + `docs/privacy.md` |
| 6 | Tauri Updater | ⏭ 移出 1.0，规划 1.1 |

### P1 MVP 功能缺口 — 全部完成（裁剪项除外）

| # | 事项 | 状态 |
|---|---|---|
| 7 | 搜索查询语法（8 个前缀，双引擎三集成层） | ✅ |
| 8 | Workspace 合并人工交互（合并/拆分/重命名 + 置信度警示） | ✅ |
| 9 | 保存搜索条件 | ✅ |
| 10 | 一键打开来源应用 / 恢复命令 | ✅ |
| 11 | 原始视图 ↔ 统一视图切换 | ✅ |
| 12 | jieba 可插拔分词器 | ✅（feature 门控 + CI 独立 job） |
| 13 | Daemon UDS IPC + 认证 Token | ⏭ 移出 1.0（1.1） |
| 14 | Adapter Host 配额 | ⏭ 移出 1.0（1.1） |
| 15 | OpenCode Adapter | ⏭ 移出 1.0（1.1） |

### P2 验收证据 — 全部完成

| # | 事项 | 状态 |
|---|---|---|
| 16 | 100k 会话搜索 P95 < 300ms 基准报告 | ✅ 实测 50.9ms，`docs/benchmark-report-v1.0.0.md` |
| 17 | Workspace 合并准确率 ≥95% 统计 | ✅ 11 例标注样本 100%，错误 AutoMerge = 0 |
| 18 | 真实脱敏 Fixture 集（Golden Fixture Kit） | ✅ `fixtures/` + 4 个 golden tests |
| 19 | ESLint 57 warning 清零 | ✅ 0 error / 0 warning |

明确不算 v1.0.0 缺口（plan 本就排在 Phase 4/5）：真实 LLM 提取、Android 端、SSO/RBAC/KMS/加密同步、语义向量检索 Hybrid。

## 开发模式（桌面端）

```bash
lsof -nP -i :1420 -sTCP:LISTEN -t | xargs kill -9   # 清理残留 dev server
cd apps/desktop
npx tauri dev
```

## License

MIT OR Apache-2.0
