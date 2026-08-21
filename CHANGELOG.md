# Changelog

## [1.2.0] - 2026-08-21

13 轮 UI/UX 打磨（默认对话 tab 等）+ AI 知识提取（大模型引擎）+ API Key 本地加密存储 + 四家来源事件采集修复 + 应用图标重做。

### Fixed
- **Codex 执行事件「获取不了」**（用户实例：西游记立绘 /goal 会话）：新版
  Codex 把 shell/生图/看图/计划更新全封装为 `custom_tool_call` 的 JS 工具桥
  （命令在 `input` 字段的 `tools.xxx({...})` 里，`arguments` 恒空）——新增
  `js_bridge` 模块解析 JS（括号配平 + 无引号键扫描 + 转义还原），命令/生图/
  看图/补丁/计划/目标映射为可读事件（`printf 'Prompt entries…`、`生成图片…`、
  `查看图片 …`）；`*_output` 按 `call_id` 配对合并输出（截断 4KB）；
  `wait` 轮询与 `get_goal` 降噪。真实会话验证：683 条噪音/空事件 →
  474 条可读事件
- **ZCode 事件采集为零（schema 漂移）**：真实 part 类型已变为 `tool`
  （`state.{status,input,output,time}`），Adapter 仍按旧 `tool_use`/`command`
  匹配——新增 `tool` 处理（Bash→命令、Read/Write/Edit→文件事件，输出并入
  payload）。真实库验证：0 → 465 条事件；旧类型保留兼容
- **MiniMax 事件采集为零**：`tool_calls[]`（含 `tool_call_args` 与
  `tool_call_result_data`）完全未解析——现映射为命令/文件事件并带输出。
  真实库验证：0 → 747 条事件
- **Claude Code 事件摘要无内容**：`Tool: Bash`/`Tool result` → 命令本身、
  `读取/写入/修改 文件名`、结果内容预览
- **事件→消息归属错乱**：消息/事件序号是两条独立流（曾按序号跨流比较），
  改为按时间戳归属；Claude Code/ZCode 事件补父消息时间戳；无时间戳事件
  归最后一条消息（近似）
- 测试：js_bridge 9 项矩阵 + 四家 Adapter 新 schema 用例 + 命令层集成旅程
  （解析→入库→详情→提取）+ 归属回归（独立序号流不再错挂）；前端 377 /
  桌面 33 / Adapter 43 项全过

### Added
- **恢复会话直接在终端打开**：详情页「⏯ 恢复命令」升级为「⏯ 恢复会话」——
  点击直接在系统终端新窗口执行恢复命令（macOS Terminal via osascript /
  Windows cmd / Linux gnome-terminal·konsole·xterm 逐个尝试）；终端打开
  失败自动回退为复制；右键仍可复制命令文本。命令在后端按会话来源构造，
  前端不传自由文本（避免任意命令执行面）。真实冒烟：osascript 打开
  Terminal 执行测试命令通过

### Fixed
- **重置数据后概览/成本需手动同步**：`reset_range` 会删除指标表
  （usage_records/tool_call_records），但重置后的自动重导只恢复会话、
  从不触发 `ops_sync`（且 30 分钟节流会拦住常规调用）→ 概览/成本一直
  空/旧直到手动点。现在重置后自动链式执行：重导会话 → 强制重算指标
  （force 绕过节流）→ 刷新预算条/红点，全程无需手动点击

### Added
- **会话详情「回到顶部」浮动按钮**：与现有「滚到底部」↓ 对称——向下滚动
  超过 400px 出现 ↑，点击平滑回顶；顶部/底部附近自动隐藏对应按钮
- **执行事件挂到对应消息下 + 详情展开**：事件按 `sequence_number` 归属到
  「序号 ≤ 事件序号的最大消息」，以紧凑行挂在消息气泡下（≤4 条，超出折叠
  「还有 N 条」）；点击事件行展开详情（完整摘要 / 状态 / 起止时间与耗时 /
  payload JSON 美化展示）。早于首条消息的事件显示为顶部「会话前置事件」组；
  底部平铺事件列表移除（上一轮的平铺分页方案被本设计取代）。
  `EventDto` 扩展 `status` / `completed_at_ms` / `payload_json`（超 8KB 截断）
- **消息内联本机图片**：消息里引用的本机图片（Markdown 图片语法 /
  Unix·Windows 绝对路径 / `file://` 链接，http 远程图除外）若仍在原位置，
  直接在对话流中展示；已移动/删除显示灰色占位。新命令 `read_image_file`
  （扩展名白名单 png/jpg/jpeg/gif/webp/bmp/svg/ico → MIME、单图 20MB 上限、
  不存在返回 None）；前端模块级缓存（同路径只读一次）、每消息限 6 张；
  纯函数 `extractLocalImagePaths` 12 项矩阵测试 + 组件三态测试
- **AI 提取（大模型引擎，可选，默认关闭）**：知识提取支持切换 LLM 引擎，
  输出与规则引擎同构（摘要/决策/TODO/错误/命令/文件 + 消息级来源引用），
  `extractor` 记录 `llm:{model}@prompt-v1`（模型 + Prompt 版本，plan §13.5）
- **OpenAI 兼容端点配置**：云端（OpenAI / DeepSeek / GLM…）与本地推理
  （Ollama / LM Studio / llama.cpp server）同一套配置；GUI 提供 4 个预设；
  本地端点自动识别并标记「本地」（允许 http），云端强制 https
- **API Key 本地加密存储**：XChaCha20-Poly1305 AEAD（随机 nonce + 固定 AAD
  + `v1` 版本前缀）；主密钥为应用数据目录下 **0600 权限密钥文件**
  （`keys/llm-master.key`，跨平台统一，不依赖 OS 钥匙串——用户决策，
  避免 macOS/Windows/Linux 钥匙串可用性差异）；明文永不落盘/不出现在
  日志与错误信息（plan §14.3）
- **新 crate `ch-llm`**：`LlmConfig`（校验/钳制/本地端点判定）+ `SecretVault`
  （密封保险库，主密钥 Zeroize）+ `HttpChat`（ureq+rustls；`response_format`
  被 400/422 拒绝时降级重试；401/429/5xx 分类）
- **`LlmExtractor`**（`ch-knowledge`）：编号转录（截断上限）+ 严格 JSON
  schema prompt + 宽松解析（剥围栏/条目裁剪/source 编号映射回真实消息 id）
- **GUI**：设置页「AI 提取（大模型）」区（开关/预设/Key 密码框 masked
  回显/测试连接/密钥破损提示）；知识弹窗 ⚙规则/✨AI 引擎切换 + 模型徽标
- **新 Tauri 命令**：`llm_config_get` / `llm_config_set` / `llm_test_connection`；
  `extract_knowledge` 增加 `engine` 参数（None/rule 默认规则引擎）

### Security
- API Key 视图契约：前端只接收 `has_api_key` + `api_key_masked`
  （`sk-***1234`），明文/密文均不回传
- 数据库单独泄露不解密（主密钥不在数据库中）；跨设备迁移检测（密文解不
  开 → 提示重新录入，不静默失败）
- 传输输入截断上限 `max_input_chars`（默认 48,000 字符，上限 200,000）

### Changed
- **应用图标重做**：Apple Blue 渐变（#0071E3 → #4F8CFF，135°）+ 缩放 logo；
  v2 源图去圆角交由各平台自加 superellipse / circular mask，logo 60% /
  底面 84% 下调 Dock 视觉占比；全套平台图标重生成（icns/ico/ios/android/store）
- **dev 模式 Dock 图标**：`npx tauri dev` 裸二进制无 bundle icns，启动时
  `NSApplication` 运行时直设应用图标（debug 构建 + 设置结果日志），
  发布包继续用内置 icns 不受影响
- 清理 11 个无引用旧版 `icon-NxN.png` 图标；Dock 预览图归档至
  `docs/optimization-rounds/`

## [1.1.1] - 2026-08-19

依赖安全修复轮（Dependabot 5 项告警清零）。

### Security
- **vite ^5.4.0 → ^6.4.3**：修复 fs.deny 绕过（high）、launch-editor UNC
  路径泄漏、优化依赖 .map 路径穿越三条公告（均无 5.x 补丁线）
- **esbuild 0.21.5 → 0.25.12 / nanoid → 3.3.18**：随 vite 6 升级 +
  `npm audit fix`；apps/desktop `npm audit` 0 漏洞
- **glib 告警（unsoundness）**：tauri 2.11.5 直接依赖 gtk ^0.18，当前
  生态无升级路径；受影响 API（`VariantStrIter` 迭代器）未被 tauri 栈
  使用且 glib 仅 Linux GUI 目标编译——已在 GitHub 按「受影响代码未使用」
  处置并留说明，待 tauri 升级 gtk-rs 后重评

### Fixed
- **vite 6 下 5 个测试套件加载失败**：vite 6 收紧根外文件访问，构建期
  `?raw` 导入工作区 Cargo.toml（版本号派生）被 Denied——vite/vitest
  配置增加 `server.fs.allow` 放行仓库根（恢复 vite 5 行为）

## [1.1.0] - 2026-08-19

搜索体验闭环：结果保留 + 按主对话分组 + 跨子对话命中步进 + 正文精准匹配。

### Added
- **搜索结果按主对话分组**（GUI 左栏）：新命令 `search_grouped` 复用双引擎
  （Tantivy→FTS5 降级统一抽为 `engine_search`）在命令层按主对话聚合，子任务
  命中经 `source_parent_id` 回溯折叠到主对话之下；左栏搜索模式呈现主对话树
  （root 行显示总命中数，子对话命中缩进折叠），与普通列表父子心智一致
- **跨子对话命中步进**（GUI 右栏）：新命令 `search_tree_hits` 返回某主对话及
  其全部子任务内的命中（按「主对话 → 子任务时间序 → 消息序号」阅读顺序）；
  详情区顶部步进条 `🎯 关键词 N/M ↑↓` 支持按钮与全局 ↑/↓ 键跳转，跨会话
  自动切换详情并高亮滚动；关键词经 `searchPreset` 预填详情页 ⌘F 页内搜索
- storage 批量查询 `conversations_by_ids` / `conversations_by_source_ids` /
  `message_order_by_ids`（分组与排序一次取齐，避免 N+1）

### Fixed
- **点击搜索结果后左栏结果被清空**：`jumpToSearchResult` 不再
  `setSearchResults(null)`，退出搜索统一走 Esc / 顶栏「清除」
- **命中步进条随内容滚出视野**：步进条移出 ScrollArea 钉在右栏顶部
  （此前 sticky 在 WKWebView 自绘滚动条场景下失效）
- **标题命中导致全文搜索噪音**：FTS5/Tantivy/prompt_reuse 三处统一只匹配
  消息正文（body-only）——此前标题含关键词的会话每条消息都算命中且 snippet
  无高亮；标题检索走列表既有「搜索标题…」入口

### Changed
- 搜索角色筛选（全部/仅用户/仅助手）由前端内存过滤升级为服务端重查；
  移除旧 SearchPanel 的「复制全部命中」（消息级平铺列表已由分组视图取代）

## [1.0.1] - 2026-08-17

v1.0.0 后的全功能测试轮（CLI 真人 E2E 59 项 + Tauri 命令层 3 旅程 + 浏览器 GUI 真人模拟）发现并修复：

### Fixed
- **daemon 忽略 `--db` 文件名**：`ch --db hub.db daemon` 实际打开的是 `<dir>/threadock.db`
  （空库）——CLI E2E 发现；`DaemonStateConfig` 新增 `db_path` 透传，默认行为不变
- **`import-from zcode` 在 `~/.zcode` 缺失时**报 sqlite 原始错误：list/import 加存在性
  守卫，对齐 claude-code 的友好提示行为
- **前端对后端 null 返回零容错**：`ops_by_provider` 等返回 null 时概览/成本页直接崩
  （ErrorBoundary 兜底但视图报废）——OpsView 18 处 + ActivityView 3 处 + KnowledgeView
  2 处归一化 `?? []`；恶劣后端（全部命令返回 null）下 8 视图全部正常渲染空态
- **GUI 版本号漂移**：前端 `APP_VERSION="0.1.0"` / `CORE_VERSION="0.2.0"` 硬编码从未
  随版本更新（更新日志弹窗显示 v0.1.0）——改为构建期从 `package.json` /
  workspace `Cargo.toml` 派生，新增 round5 契约测试锁定「三者一致」；补 1.0.0 更新
  日志条目
- **知识库页在 IPC 异常时永远「加载中」**：null 归一为空 KB 走空态

### Tests
- 新增 `scripts/e2e_cli.sh`：13 组场景 × 59 断言（真实二进制 + 临时库；导入幂等/
  双引擎搜索语法/知识/脱敏/导出/备份恢复/硬删级联/daemon 14 方法含错误路径）
- 新增 Tauri 命令层 E2E（`src/e2e_journeys.rs`）：mock app + 真实后端 3 条用户旅程
  （会话生命周期/Workspace 治理/保存搜索），直接调 GUI 实际 invoke 的命令函数
- 浏览器 GUI 真人模拟：IPC mock 注入驱动真实前端——8 视图巡检、搜索语法、保存搜索
  下拉、原始视图切换、恢复命令复制、Workspace 重命名/置信度徽标、恶劣后端压力

## [1.0.0] - 2026-08-17

首个正式版本。以执行计划 Phase 2（MVP，Gate 1）为验收基线，P0 发布工程 +
P1 MVP 功能缺口 + P2 验收证据全部闭环。

### Added
- **搜索查询语法**（plan §13.2）：`provider:` `workspace:` `type:` `role:` `status:`
  `file:` `model:` `after:` `before:` 前缀，FTS5 全量 SQL 过滤 + Tantivy 索引内过滤
  （含纯过滤 AllQuery、多 workspace OR），双引擎三集成层（GUI/daemon/CLI）一致生效
- **保存搜索**：V14 `saved_searches` 表，搜索框 ☆ 保存 → 下拉一键执行/删除
- **Workspace 治理**（plan §4.3/P2-2）：手动合并（并入 + 治理审计 + 映射升 manual/1.0）、
  拆分（会话多选 → 移到新 Workspace）、重命名、来源映射置信度列表；
  设置页新增「Workspace 管理」分区（<0.8 低置信度警示徽标）
- **原始视图 ↔ 统一视图**（plan P2-3）：会话详情一键查看 Raw Store 未标准化归档
- **一键打开来源应用 / 恢复命令**（plan P2-3）：GUI 来源拉起应用；
  claude-code / codex 生成 `claude --resume` / `codex resume` 命令并复制
- **jieba 可插拔中文分词器**（plan §13.1）：`--features jieba` 启用（自实现
  tantivy TokenStream 适配，词典全局单例），默认 N-gram 兜底；CI 独立 job 验证
- **Release 流水线**：推 v* tag → 三平台 Tauri 安装包 + CLI 四平台二进制 + SHA256SUMS
- **用户文档**：`docs/user-guide.md`（11 章用户指南）+ `docs/privacy.md`（隐私声明）
- **Golden Fixture Kit**（plan §20.2）：`fixtures/` 脱敏样本集 + adapter golden tests
- **Gate 1 大规模基准**：10 万会话 / 50 万消息 FTS5 P95 = **50.9ms**（红线 300ms），
  报告留档 `docs/benchmark-report-v1.0.0.md`

### Fixed
- **resolver 误并缺陷**：候选与已知项都带结构化标识（remote/path/fsid）却全未命中时，
  纯名称相同不再静默 AutoMerge，降级 NeedsConfirmation（合并准确率基准化时发现）
- 版本三处不一致（workspace/Tauri/CHANGELOG）自 0.4.0 起统一

### Tests
- workspace 421（+42：query_syntax 18 / 语法集成 8 / saved-search 与 workspace 治理 6 /
  golden 4 / 大规模基准 1 / jieba 1 等）+ Tauri 13 + 前端 344，全绿
- ESLint 0 error / 0 warning（57 个历史 warning 清零：40+ 真修复 + 10 处带理由豁免）
- 合并准确率：11 例标注样本 100%（错误 AutoMerge = 0）

### 已知限制（发布说明）
- 安装包**未签名**（macOS 右键打开 / Windows SmartScreen「仍要运行」；SHA256 校验）
- 自动更新（Tauri Updater）未配置，规划 1.1
- OpenCode Adapter、Daemon UDS IPC、Adapter 进程配额移至 1.1（v1.0 范围裁剪决策）

## [0.4.0] - 2026-08-17

> 版本治理：此前 workspace（0.2.0）/ Tauri（0.1.0）/ CHANGELOG（0.3.0）三处版本不一致，自本版起统一。

### Added（0.3.0 后 26 轮产品迭代补记）
- **Command Palette**：全局命令面板 + 快捷键
- **首次启动引导**（OnboardingTour）+ 启动更新日志（ChangelogModal）
- **活动页大改版**：GitHub 风格热力图（7×N 布局 + hover tooltip + 自适应）、4 张统计卡、工具 Top 10、24h 时间分布
- **会话详情增强**：页内搜索、多选批量操作、新鲜度标识、Toast undo、右键菜单、代码块语法高亮
- **知识库增强**：跨会话知识引用、知识筛选导出、周对比视图
- **私人笔记**：会话级 user-only markdown 笔记（V13 conversation_notes 表，不参与搜索/导出/统计）
- **配置导入导出** + 批量加标签 + 自定义标题（空 = 恢复原始）
- **Prompt 复用 / 会话续做**：write_clipboard 自定义 Tauri command（绕开 macOS WKWebView 剪贴板限制）
- **UI 体系**：自定义 ScrollArea 组件（WKWebView 滚动条兼容）、可拖拽 resizer、暗色/亮色主题协调、全页 max-width 布局
- **底部状态栏** + 排序置顶 + 用户偏好持久化

### Fixed
- 项目页「查看会话」SQL JOIN 列名错（sqlite no such column）
- 重置时间范围改为用户自选 + 修 import_state 残留导致「重置 = 数据丢失」
- ScrollArea 滚轮失灵、热力图 cell 变形/黑屏回归、activity_stats tuple 序列化
- 第 26 轮 4 区并发体检：32 findings → 0

### Engineering
- CI：修复三类历史失败 + clippy 1.97+ 新增 lint + rustfmt 版本要求
- 安全：启用 Dependabot + CodeQL；修 supply-chain advisory 3 个（2 完成 + 1 个上游 blocked：RUSTSEC-2026-0253 lru，等 tantivy 升级，CI 已容忍）
- 测试增长：workspace 379 + Tauri 13 + 前端 344（全绿）；ESLint 0 error

## [0.3.0] - 2026-08-15

### Governance Closed-Loop (治理闭环，三批次)
- **Budget alerting**: notify_on_exceed now enforced (settings toggle + topbar global budget bar + over-limit toast with per-month dedup); month-end cost/token projection (daily-run-rate extrapolation)
- **Audit disposition**: findings persisted fingerprints (V11 audit_finding_states), ignore / false-positive whitelist, per-conversation rescan, policy rule enable/disable switches (was delete-only)
- **Notification system**: in-app toasts (budget/audit/weekly/retention) — none existed before
- **Conversation governance**: favorite star + tag editing (backend APIs existed, UI was zero), archive, soft-delete + recycle-bin view with restore, hard delete with typed「删除」confirmation
- **Hard delete now cascades**: raw blob + tantivy documents + governance log (previously leaked both)
- **Timeline fixed (M15)**: messages+events merged by timestamp (was unsorted concat), event timestamps shown (EventDto.created_at_ms added), cap 100 → 2000
- **Data lifecycle**: storage dashboard (db/raw/index), orphan-blob GC with 1h race protection, retention policy (auto-archive >N days at startup), search-index rebuild
- **Weekly report automation**: auto-generated when >7 days stale into app_data/reports/ + manual button
- **Governance audit trail**: audit_logs table enabled — every sensitive action (reset/GC/delete/policy/disposition) recorded and shown in Settings
- **Dashboard**: per-card visibility toggles (persisted), daily cache-hit trend chart, automation watch-list (read-only principle preserved)

### Fixed
- **Search delete was silently broken**: "raw" tokenizer was actually SimpleTokenizer — underscore IDs were tokenized so delete_term never matched; switched to RawTokenizer + realistic-ID regression test + rebuild-index migration command for existing indexes
- MSRV job/toolchain: lockfile requires 1.88 (declared 1.75 was fictional)

### Tests
- 371 workspace (was 361) + 7 tauri (was 4... now 7) + 34 frontend (was 24)

## [0.2.1] - 2026-08-15

### Security
- **Search snippet XSS fixed (stored)**: Tantivy & FTS5 highlight paths both HTML-escape output; CSP enabled in tauri.conf.json (was null)
- **Backup KDF upgraded to Argon2id**: random 16-byte salt per backup (v2 format); v1 backups still restorable; zip-slip path validation + length-field bounds (no panic on crafted files)
- **Redaction coverage extended**: content_json now redacted in JSON exports; custom rules applied on GUI & batch export paths (was CLI-single-only); new patterns: sk-proj-/sk-ant- (hyphenated keys), glpat-, xox[baprs]-, AIza, bare JWT, PEM private key blocks
- **Daemon hardening**: 16MiB per-line request cap (capped reader), panic recovery via catch_unwind (handler crash no longer kills daemon)
- **Audit HTML report**: rule names now escaped (was raw injection point)

### Performance
- **Import throughput 1575 → 3903 msg/s (2.48x)**: synchronous=FULL → NORMAL (WAL-recommended), all 4 legacy per-message upsert paths unified onto single-transaction import_conversation_batch (Tauri import_file, daemon provider.sync, CLI import/import_raw)
- **N+1 eliminated on conversation list**: one GROUP BY for all child counts (was 1 query per conversation on every UI refresh)
- **RawStore dedup short-circuit**: existing content-hash objects skip zstd-9 recompression + rewrite + fsync
- **Audit scan O(n²) → O(n)**: OFFSET pagination replaced with keyset (WHERE m.id > last)
- **conversation.similar bounded**: 100 candidates x 20 messages cap (was: entire DB in memory)
- **Heavy commands off the async runtime**: auto_sync/ops_sync/audit/import wrapped in block_in_place; ops_sync collects (lock-free) before taking the write lock
- **daemon read path on read connection**: all read handlers use read_repo (was: write lock for everything)
- **Tantivy writer heap centralized** as ch_search::DEFAULT_WRITER_HEAP

### Fixed
- import_conversation_batch no longer overwrites conv.workspace_id with NULL when workspace_name is None
- IS_BUSY panic-safety: RAII BusyGuard (a panic no longer permanently blocks sync/reset)
- 7 silent error swallows now logged (import-state persistence failures caused invisible duplicate imports)
- MSRV violation: NonZero::get fn-ref (stable 1.79) replaced with 1.75-compatible call

### Changed
- Error codes actually classified now: 139 internal_err sites split into storage/search/import/io (was: everything Internal)
- open_db deduplicated into ch_adapter_sdk::open_readonly (3 copies removed)
- README updated: Threadock branding, V10 schema/22 tables, current crate tree, corrected doc paths
- Clippy clean under rust 1.95 pedantic (37 new-lint fixes across workspace)

### Refactored
- **storage/repository.rs split (3373 → 2082 lines)**: ops metrics into `repository/ops_queries.rs` (25 methods), audit/policy/budget into `repository/audit_repo.rs`, knowledge persistence into `repository/knowledge_repo.rs`, settings/redaction rules into `repository/settings.rs` — public re-export paths unchanged
- **auto_sync_inner table-driven**: 5 copy-pasted provider blocks replaced by `source_table()` descriptor table + one generic loop; JSON output keys byte-identical (frontend contract)
- **src-tauri/lib.rs split (1913 → 174 lines)**: commands organized into `commands/{conversations,import,ops,audit,export_cmd,dto}` with shared helpers in `commands/mod.rs`

### Engineering
- **Frontend testing from zero**: vitest + Testing Library (14 tests incl. snippet XSS regression), ESLint (typescript-eslint + react-hooks, 0 errors)
- **CI extended**: workspace tests on ubuntu/macos/windows matrix, MSRV job (1.88 — declared 1.75 was fictional, lockfile requires 1.88; verified locally), cargo-audit supply-chain job, frontend lint+test steps
- **Tauri command layer tests from zero**: source_table/auto_sync roundtrip (output-key contract), import_file_inner roundtrip + idempotency

### Tests
- 361 workspace tests (was 342) + 3 Tauri + 14 frontend: +XSS escape (tantivy/fts5), +Argon2 salt/zip-slip/truncated-backup rejection, +content_json redaction, +capped-line reader, +batch workspace preservation, +bulk child counts, +RawStore idempotency


## [0.2.0] - 2026-08-15

### Added
- **M1-M15 CodeAgentOps**: Full observability platform for AI coding agents
  - M1-M2: Metrics collection (4 agents, 44K+ request-level usage records)
  - M3: Governance dashboard with animated charts
  - M4: Security audit engine (sensitive info + dangerous commands)
  - M5: Policy, budget, and pricing model
  - M6: Asset inventory (skills/plugins across all agents)
  - M7: Cost attribution by project + cache hit rate analysis
  - M8: Automation/cron task governance
  - M9: Anomaly detection (error spikes, retry storms, context exceeded)
  - M10: Agent health scoring (stability 0-100)
  - M11: Latency P50/P95 percentile tracking
  - M12: Token waste detection (context accumulation patterns)
  - M13: Agent side-by-side benchmark comparison
  - M14: Weekly HTML report generation
  - M15: Conversation timeline view
- **5 AI IDE adapters**: ZCode, Claude Code, Cursor, MiniMax Code, Codex
- **Dual-connection SQLite**: WAL mode read/write separation for zero-lock UI
- **All 56 Tauri commands async**: Non-blocking IPC on tokio runtime
- **Collapsible left sidebar**: 5-tab navigation with icon-only collapse mode
- **Incremental import**: Stale detection via source update timestamps
- **10-minute auto-sync**: Background data refresh with cancel support
- **Dark/light theme**: CSS variable design system with persistence

### Performance
- WAL dual-connection: reads never blocked by writes (6ms concurrent queries)
- Chunked batch transactions: 2000 rows per commit
- Single tantivy commit per sync cycle
- Fire-and-forget ops_sync: zero blocking on page render
- Parallel codex collection: 8 threads, 8s → 1.8s

### Fixed
- providers.adapter_id NOT NULL constraint crash (silent import failures)
- MiniMax hidden session filtering (visibility vs archived distinction)
- ZCode subagent_child parent linkage (424 orphan children repaired)
- Audit UTF-8 char boundary panic (multibyte context slicing)
- Invalid Date in risk calls (OffsetDateTime serialization → ms)
- ops_overview parameter count mismatch (10 placeholders, 1 param)
- Import freshness via V10 import_state table

## [0.1.0] - 2026-08-02

### Initial Release
- Core conversation archival and search platform
- 4 AI IDE adapters with read-only snapshot principle
- Tantivy full-text search with N-gram Chinese tokenization
- BLAKE3 content-addressed raw storage with zstd compression
- Encrypted backup (XChaCha20-Poly1305)
- Redaction engine (7 builtin + custom regex rules)
