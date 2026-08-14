# Conversation Hub

> 跨 AI IDE 的统一会话归档、检索与知识提取——把 Codex / Cursor / Claude Code / ZCode / MiniMax Code / OpenCode 等工具里的会话、工具调用、命令、Diff、Artifact 统一收集、标准化、全文检索、知识化。
>
> 配套方案：[`ai-ide-conversation-hub-enterprise-plan.md`](./ai-ide-conversation-hub-enterprise-plan.md)
> 落地清单：[`conversation-hub-execution-plan.md`](./conversation-hub-execution-plan.md)

## 当前状态：Phase 0/1 + 核心能力扩展

一个可运行的 Rust 工程，已实现端到端数据闭环 + 桌面 GUI + 常驻服务。

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
        │  ├ Repository │     SQLite WAL（V3 schema，12 表）
        │  ├ SearchIndex│     Tantivy（N-gram 中文 + BM25）
        │  └ RawStore   │     BLAKE3 内容寻址 + zstd
        └───────────────┘
                ▼
        ┌───────────────┐
        │ Adapter 进程  │  独立子进程（plan §10.4 隔离）
        │ markdown/jsonl│  stdio JSON-RPC
        └───────────────┘
```

### 能力清单（已实现）

| 能力 | 实现位置 | 对应 plan |
|---|---|---|
| 统一领域模型（6 来源 + 19 事件类型 + 7 级合并） | `crates/domain` + `crates/identity-resolver` | §4, §12 |
| SQLite V3（12 表 + WAL + Migration + FTS5） | `crates/storage` | §9.4, §12 |
| Tantivy 全文检索（N-gram 中文 + BM25 + 高亮） | `crates/search` | §9.5, §13 |
| 双引擎搜索（Tantivy 主 + FTS5 降级） | `crates/storage` + `crates/search` | §13 |
| 标准化流水线（BLAKE3 hash + 幂等 + 完整度评分） | `crates/normalization` | §11, §17.3 |
| Raw Store（内容寻址 + zstd 压缩） | `crates/raw-store` | §9.6, §2.3 |
| Workspace 自动合并（7 级优先级 + 置信度） | `crates/identity-resolver` | §4.3 |
| 收藏 / 标签 / 归档 / 软删除 / 硬删除 | `crates/storage` | §6.3/§6.4/§11.4 |
| 多维过滤（provider/workspace/favorite/archived） | `crates/storage` | §6.4 |
| Markdown + JSONL Adapter（中英文） | `crates/adapter-markdown` + `adapter-jsonl` | §10.5 |
| Adapter 进程隔离（stdio JSON-RPC + 崩溃检测） | `crates/adapter-sdk` + `adapter-host` | §10.4 |
| 导出（Markdown/JSON + 敏感信息脱敏 + 批量） | `crates/export` | §6.6, §14.6 |
| 加密备份/恢复（XChaCha20-Poly1305 + zstd） | `crates/backup` | §6.6, §14.3 |
| 知识提取（摘要/决策/TODO/错误/命令/文件） | `crates/knowledge` | §13.5 |
| 知识提取持久化（版本管理 + 不覆盖原始） | `crates/storage` V3 | §13.5 |
| Daemon 常驻服务（JSON-RPC over stdio） | `crates/daemon` | §8.2, §16 |
| Tauri 桌面 GUI（三栏 + 搜索 + 知识面板） | `apps/desktop` | §9.1, §17 |

### 尚未实现（后续阶段）

- Codex / Cursor / Claude Code 真实 Adapter（Phase 2，需调研各来源 API）
- Tantivy 可插拔中文分词器（当前用 N-gram 兜底）
- AI 提取走真实 LLM（当前规则引擎，接口已留好）
- 企业能力：SSO / 审计 / 保留策略 / 加密同步（Phase 5）

## 快速开始

### 构建

```bash
# CLI + Daemon + Adapter 二进制
cargo build --release --manifest-path Cargo.toml -p ch-cli -p ch-daemon -p ch-adapter-markdown

# Tauri 桌面应用
cd apps/desktop && npm install && npm run build
cd src-tauri && cargo build --release
```

### CLI 使用

```bash
CH=./target/release/ch

# 导入会话（md/jsonl 自动识别）
$CH --db ./hub.db import examples/tauri-android.md --workspace my-app
$CH --db ./hub.db import examples/rust-errors.md  --workspace cli-tool

# 列出会话（支持过滤）
$CH --db ./hub.db list
$CH --db ./hub.db list --favorite
$CH --db ./hub.db list --provider codex

# 全文搜索（FTS5 或 Tantivy）
$CH --db ./hub.db search 后台任务
$CH --db ./hub.db search-tantivy WorkManager

# 收藏 / 标签 / 归档 / 删除
$CH --db ./hub.db favorite <id>
$CH --db ./hub.db tag <id> rust
$CH --db ./hub.db archive <id>
$CH --db ./hub.db delete <id>          # 软删除（可恢复）
$CH --db ./hub.db delete <id> --hard   # 硬删除（永久）
$CH --db ./hub.db undelete <id>

# 知识提取（plan §13.5）
$CH --db ./hub.db knowledge <id>          # 提取并显示
$CH --db ./hub.db knowledge <id> --save   # 提取并持久化（版本管理）
$CH --db ./hub.db knowledge <id> --show   # 显示已保存的提取结果

# 导出（含脱敏）
$CH --db ./hub.db export markdown <id> out.md
$CH --db ./hub.db export workspace <ws-id> out-dir/

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

方法清单（plan §16.1）：`system.getInfo` / `workspace.list` / `conversation.list` / `conversation.get` / `conversation.delete` / `message.list` / `event.list` / `search.query` / `knowledge.extract` / `knowledge.save` / `knowledge.get` / `provider.sync`

## 运行测试

```bash
cargo test --workspace --manifest-path Cargo.toml   # 全部单元测试（263 个）
cargo clippy --workspace --all-targets              # 代码质量检查（0 warning）
```

## 项目结构

```
threadock/
├── crates/
│   ├── domain/              统一领域模型（无存储依赖）
│   ├── storage/             SQLite V3 + Migration + FTS5 + 过滤 + 删除
│   ├── raw-store/           内容寻址原始数据（BLAKE3 + zstd）
│   ├── search/              Tantivy 全文检索（N-gram 中文 + BM25）
│   ├── identity-resolver/   Workspace 身份解析（7 级合并优先级）
│   ├── normalization/       标准化流水线（hash + 幂等 + 完整度）
│   ├── export/              导出（Markdown/JSON + 脱敏）
│   ├── backup/              加密备份/恢复（XChaCha20-Poly1305）
│   ├── knowledge/           知识提取（规则引擎，plan §13.5）
│   ├── adapter-sdk/         Adapter trait + stdio JSON-RPC 协议
│   ├── adapter-host/        进程隔离（spawn + 崩溃检测）
│   ├── adapter-markdown/    Markdown Adapter（独立进程二进制）
│   ├── adapter-jsonl/       JSONL Adapter
│   ├── daemon/              常驻服务（JSON-RPC over stdio）
│   └── cli/                 `ch` 命令行（import/list/search/export/...）
├── apps/desktop/            Tauri 2 桌面应用（React + TS）
│   ├── src/                 前端（三栏 + 搜索 + 知识面板）
│   └── src-tauri/           Rust 后端（嵌入 DaemonState）
├── examples/                示例会话 Markdown
└── *.md                     方案与落地计划文档
```

## 关键设计决策

详见 [`conversation-hub-execution-plan.md` §1.3](./conversation-hub-execution-plan.md)。plan §1.3 的八条「不可妥协关键决策」全部已实现：

| 决策 | 状态 |
|---|---|
| Local-first | ✅ 数据默认留本机 |
| SQLite WAL | ✅ V3 schema |
| Tauri 桌面 | ✅ React + TS |
| Rust 核心 | ✅ |
| Tantivy 搜索 | ✅ N-gram 中文 |
| Adapter 进程隔离 | ✅ stdio JSON-RPC |
| Raw + Normalized 双存储 | ✅ |
| 第三方只读 | ✅ |


## 测试

```bash
lsof -nP -i :1420 -sTCP:LISTEN -t | xargs kill -9
cd apps/desktop
npx tauri dev
```

## License

MIT OR Apache-2.0
