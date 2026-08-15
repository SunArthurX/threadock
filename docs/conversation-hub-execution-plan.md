# Conversation Hub 落地执行计划

> **配套文档**：[`ai-ide-conversation-hub-enterprise-plan.md`](./ai-ide-conversation-hub-enterprise-plan.md)（v1.0，2026-08-02）
> **本文档定位**：对原方案的**解读 + 拆解 + 可勾选的落地执行清单**。不重复原方案全文，只提炼「要做什么、按什么顺序、谁来做、卡点在哪」。
> **编制日期**：2026-08-02
> **维护方式**：随项目推进逐条勾选 `[ ]` → `[x]`；新增动作追加到对应阶段末尾。

---

## 0. 如何使用本文档

1. **先看第 1 章「一页读懂」**：3 分钟掌握产品本质与关键决策。
2. **再看第 2 章「解读」**：理解方案在每一层的取舍与权衡，知道「为什么这么定」。
3. **进入第 3 章「落地清单」**：按 Phase 0→6 顺序执行，每条都是独立可验收的动作。
4. **执行前先查第 4 章「前置决策」**：这里有 6 个必须先拍板的问题，否则后面返工。
5. **风险持续跟进**：用第 7 章的风险登记表，每周 review。

**勾选约定**：`[ ]` 待办 / `[x]` 完成 / `[~]` 部分完成 / `[-]` 已裁剪（附理由）。

---

## 1. 一页读懂 Conversation Hub

### 1.1 一句话定义

> **跨 AI IDE 的统一会话、任务和知识管理平台**——把 Codex / Cursor / Claude Code / ZCode / MiniMax Code / OpenCode 等工具里的 Project、Conversation、Tool Call、Command、Diff、Artifact 全部标准化、归档、检索。

**它不是**：不是新 IDE、不是代码启动器、不是 Agent 运行时、第一版不做协同编辑。

### 1.2 核心产品结构

```
Unified Workspace：my-web-app（示例项目名，可替换为任意仓库）
├── Codex        每小时优化 / 分析三端架构
├── Cursor       修复 TS 类型 / 重构窗口管理
├── ZCode        Android 端适配
└── MiniMax Code 自动生成集成测试
```

### 1.3 八个不可妥协的关键决策（来自附录 C）

| # | 决策 | 结论 | 一旦违反的后果 |
|---|---|---|---|
| 1 | 主体架构 | **Local-first** | 改成云端为主 → 隐私合规崩盘 |
| 2 | 桌面框架 | **Tauri 2**（非 Electron） | 改 Electron → 内存/包体翻倍，且偏离 Rust 栈 |
| 3 | 核心语言 | **Rust** | 换 Node/Go → Adapter 进程隔离和性能目标难保证 |
| 4 | 主数据 | **SQLite（WAL）** | 换 Postgres 嵌入式 → 运维复杂度爆炸 |
| 5 | 搜索 | **Tantivy**（FTS5 降级） | 仅用 FTS5 → 中文检索质量不达标 |
| 6 | Adapter 隔离 | **独立进程** | 改成同进程插件 → 一个 Adapter 崩溃拖垮全局 |
| 7 | 原始数据 | **Raw + Normalized 双存** | 只存标准化 → 第三方改 Schema 后无法回溯 |
| 8 | 第三方数据 | **只读、不改源库** | 直接写源数据库 → 损坏用户 Cursor/Codex |

> **执行铁律**：以上 8 条是地基。任何 PR 试图松动它们都必须走 ADR 评审。

### 1.4 规模与节奏（精简视角）

| 维度 | 企业 GA 目标 | MVP（12 周）只需 |
|---|---|---|
| 团队 | 8~10 人（精简 4~5 人延长到 9~12 月） | 3 来源 + 本地功能，团队可 4 人 |
| 来源 | 6+ 工具 + 20+ Adapter | Codex / Cursor / Claude Code + 通用导入 |
| 数据量 | 10 万会话 / 500 万消息 / 20GB 原始 | 10k 会话基准通过 |
| 检索 | 100k P95 < 300ms | 同（性能基线不放松） |
| 企业 | SSO/审计/保留/加密同步 | 全部推迟到 Phase 5 |

**关键洞察**：性能基线（搜索 300ms、导入 500 msg/s）从 MVP 就要达标，**不能后期优化**——这是最容易踩的坑。

---

## 2. 方案解读（为什么这么定）

### 2.1 产品定位的精妙之处

方案把**「Workspace」作为一级组织单元**，而不是「来源」或「时间」。这是关键差异化：

- 痛点：同一个项目（如某个 Git 仓库）在 Codex/Cursor/ZCode 各开了一堆会话，用户心智里是**一个项目**，不是四个工具的历史。
- 所以领域模型里 `WORKSPACE ||--o{ SOURCE_WORKSPACE : maps`——一个统一 Workspace 映射多个来源 Workspace。

**落地含义**：Workspace 合并算法（§4.3）是产品的灵魂，必须从 Phase 0 就开始打磨，置信度阈值和「人工确认」交互要在 MVP 验收。

### 2.2 架构的三层信任边界

```
受信任核心（UI / Daemon / DB）  ←  Daemon 单点写 DB
        ↓ 调用
受限执行区（各 Adapter 进程）   ←  隔离、配额、白名单
        ↓ 只读
外部边界（第三方 App / 可选云）  ←  默认无网络
```

**为什么这么分**：第三方工具的私有数据库是不稳定 API，Schema 频繁变。把解析放进隔离进程后：
- Adapter 崩溃 ≠ 主程序崩溃（SLO: 隔离率 100%）。
- 恶意/被篡改 Adapter 摸不到主库（Daemon 是唯一写者）。
- 未知 Schema 时可降级而不污染数据（Schema 指纹机制）。

**落地含义**：Adapter Host 是 Phase 1 的硬骨头，**不能为了快把它做成同进程库**。

### 2.3 双存储：Raw + Normalized

```
第三方数据 → 保存原始 Raw（BLAKE3 内容寻址 + zstd + 可选加密）
          → 标准化 Normalized（SQLite 表）
```

**为什么两份都存**：
1. 第三方改了 Schema，旧 Adapter 解析不了时，原始数据还在，未来升级解析器可重新标准化（无需再访问第三方 App）。
2. 审计/取证需要原始证据。
3. 成本可接受：zstd 压缩 + 内容寻址天然去重。

**落地含义**：Raw Store 在 Phase 1 必须就位，且**先写 Raw、再写 Normalized、最后提交游标**——这个顺序保证幂等和可恢复。

### 2.4 同步设计的核心：幂等 + 游标

```
检测 → 快照 → 解析 → 校验 → 写 Raw → 标准化 → 去重 → SQLite 事务 → 更新索引 → 提交游标
```

- **幂等键**：`provider + installation + source_conversation_id + source_message_id`；来源无稳定 ID 时用 BLAKE3 内容 Hash（**禁止只按文本去重**——同消息可在不同会话合法出现）。
- **游标**：只有事务提交后才更新 `sync_cursors`。崩溃恢复时从上次游标重跑，因幂等不会产生重复。
- **删除语义**：来源删了 → 标记 `source_status=deleted`，**不立即物理删**本地副本（保留审计），物理删只在用户/合规触发时发生。

**落地含义**：这套游标+幂等是 Phase 2 MVP 的隐含前置，Phase 1 做基础平台时就要把表结构留好。

### 2.5 搜索：为什么 Tantivy + FTS5 双轨

- **Tantivy**：Rust 原生全文检索，中文分词可插拔，BM25 + 高亮，满足「中英文混合 + 代码标识符」要求。
- **FTS5**：SQLite 内置，作为 **MVP 降级方案**和索引异常兜底。

排序公式分两阶段：
- 初期：`BM25 + 标题/Workspace/时效/收藏/精确标识符加权`。
- 语义检索上线后：Hybrid `0.65×BM25 + 0.25×向量 + 0.10×元数据`，权重**必须可配、可 A/B，不写死**。

**落地含义**：MVP 阶段可先用 FTS5 跑通链路，但 Tantivy 接入要在 Phase 2 完成，不能拖。

### 2.6 企业化的「可选」哲学

方案反复强调：**企业能力可选，不破坏个人版简洁性**。三种部署模式渐进：

| 模式 | 数据位置 | 适合 |
|---|---|---|
| A 个人本地版 | 全本机 | 个人/开源 |
| B 企业托管本地版 | 内容本机 + 策略/审计元数据上云 | 源码外发敏感企业 |
| C 团队同步版 | 加密同步到云 | 需跨设备/团队复用 |

**关键约束**：云端**不是 MVP 前置依赖**；同步默认仅元数据，正文需显式启用；管理员**不能直接读未同步的本机内容**。

**落地含义**：Phase 5 之前所有企业功能都用 Feature Flag 关闭，个人版安装即用、无账号。

### 2.7 风险全景里最该盯的三条

1. **第三方 Schema 高频变化**（概率高×影响高）→ Schema 指纹 + 版本化 Adapter + Golden Fixture + 降级导入。这是 Adapter 体系存在的根本理由。
2. **会话含密钥/敏感源码**（概率高×影响高）→ 本地优先 + 脱敏 + 加密 + 目录排除 + 默认禁云同步。
3. **团队范围过大**（概率高×影响高）→ MVP 严格限定 3 来源 + 本地功能。**警惕功能蔓延**。

---

## 3. 分阶段落地执行清单

> 与原方案 §23 路线图对齐，每阶段含：**目标 / 入口条件 / 任务清单 / 退出标准 / 验收证据**。
> 阶段间是 Gate（§27），不达标不进下一阶段。

### Phase 0：产品与技术验证（Week 1~2，2 周）

**目标**：固化边界、验证最关键数据源、验证进程模型、完成法律预评审。

**入口条件**：团队到位（至少 PM + Tech Lead + 1 Rust + 1 Adapter）、Monorepo 仓库就绪。

#### 任务

- [ ] **P0-1 立项与基线**
  - [ ] 建立 Monorepo（结构见原方案 §22）
  - [ ] 建立 ADR 模板，写下 ADR-001 ~ ADR-010 的占位（清单见原方案 §22.2）
  - [ ] 固化统一术语（原方案 §4.1）为 `docs/glossary.md`
  - [ ] 领域模型 v0.1 落到 `crates/domain`（实体 + 关系，参考 §4.2 ER 图）
  - [ ] 明确 MVP 范围：Codex / Cursor / Claude Code + 通用导入，**其余拒绝**
  - [ ] PRD v0.1 冻结到 MVP 范围

- [ ] **P0-2 数据源调查**
  - [ ] Codex app-server 能力梳理（Thread/Turn/事件流）→ 记录字段覆盖率
  - [ ] Cursor Markdown 导出 + 本地 SQLite 快照 PoC
  - [ ] Claude Code Session/Resume PoC
  - [ ] ZCode、MiniMax 数据可用性**调查**（不实现，仅评估风险等级）
  - [ ] 形成「来源兼容矩阵 v0.1」（参考 §10.6）

- [ ] **P0-3 协议与模型定义**
  - [ ] Adapter Protocol v0.1（trait 见 §10.2）
  - [ ] 统一模型 v0.1（表结构草案，参考 §12.1）
  - [ ] 事件类型枚举（§12.2 的 19 种 event_type）

- [ ] **P0-4 风险与合规**
  - [ ] 威胁模型 STRIDE 评审（§14.8 表格逐条过）
  - [ ] 第三方条款初审（Codex/Cursor/Claude 的 ToS、导出授权）
  - [ ] 数据分类策略草案（Public/Internal/Confidential/Restricted，§14.1）

#### 退出标准（Gate 0）

- [ ] Codex、Cursor **各导入 ≥100 条 Conversation** 成功
- [ ] 原始数据与标准化数据**同时保存**可验证
- [ ] Adapter 崩溃**不影响**主进程（手动 kill 验证）
- [ ] 法律/条款风险清单完成，无阻断性红线
- [ ] PRD 冻结到 MVP 范围

**验收证据**：PoC 录屏 / Fixture 样本 / 风险登记表初版 / ADR-001~005 通过评审。

---

### Phase 1：基础平台（Week 3~6，4 周）

**目标**：搭好桌面壳 + Daemon + DB + Adapter Host，跑通通用 Markdown/JSONL Adapter 端到端链路。

**入口条件**：Phase 0 退出标准全部满足。

#### 任务

- [ ] **P1-1 桌面与进程骨架**
  - [ ] Tauri 2 Desktop 壳 + React + TS + Vite（§9.2 技术栈）
  - [ ] Rust Daemon（tokio + axum + tracing，§9.3 依赖清单）
  - [ ] Daemon 生命周期管理（启动/单例/退出）
  - [ ] Tauri Capability 最小化配置（§14.2，前端无直接文件系统权限）

- [ ] **P1-2 本地 IPC**
  - [ ] macOS/Linux：Unix Domain Socket；Windows：Named Pipe
  - [ ] Socket 文件权限仅当前用户（§14.4）
  - [ ] JSON-RPC 2.0 Envelope（含协议版本 + Client ID）
  - [ ] 每次安装生成本地认证 Token
  - [ ] 禁止监听 `0.0.0.0`，局域网需显式启用 + TLS

- [ ] **P1-3 存储层**
  - [ ] SQLite Migration 框架（顺序版本、可重入、启动前备份，§12.4）
  - [ ] WAL 模式 + 4 项 PRAGMA（§9.4）
  - [ ] 核心表全部建好（providers / installations / workspaces / source_workspaces / conversations / turns / messages / events / artifacts / sync_cursors / audit_logs，§12.1）
  - [ ] Raw Store：BLAKE3 内容寻址 + zstd + 可选 XChaCha20-Poly1305（§9.6）
  - [ ] 数据目录结构落地（`db/ raw/ index/ backups/ adapters/ logs/`）

- [ ] **P1-4 Adapter Host**
  - [ ] 独立进程 + JSON-RPC over stdio（§10.4）
  - [ ] 启动超时 / 单次调用超时 / 心跳
  - [ ] 内存与 CPU 配额限制
  - [ ] 文件访问白名单 + 默认禁网
  - [ ] 崩溃重启上限 + 签名/Hash 校验 + 版本回滚

- [ ] **P1-5 端到端链路（用通用 Adapter 验证）**
  - [ ] Markdown Adapter（最简，用于打通流水线）
  - [ ] JSONL Adapter
  - [ ] 链路：Source → 快照 → 解析 → Schema 校验 → 写 Raw → 标准化 → 去重 → SQLite 事务 → 索引 → 游标 → 通知 UI
  - [ ] 结构化日志 + Correlation ID（字段见 §19.1，**默认不记正文/源码/Token**）

- [ ] **P1-6 基础设置与诊断**
  - [ ] 设置页（来源管理、隐私开关、数据路径）
  - [ ] 诊断包生成（§19.4，**不含正文**）
  - [ ] 数据库完整性检查 + 在线备份

#### 退出标准

- [ ] UI 稳定连接 Daemon
- [ ] 主数据库可备份**且可恢复**（实测）
- [ ] Adapter 可独立安装/启用/禁用
- [ ] 导入操作**幂等**（重复导入不产生重复）
- [ ] **10 万条 Message 导入基准通过**（≥500 msg/s）

**验收证据**：基准测试报告 / 备份恢复演练记录 / Adapter kill 测试录屏。

---

### Phase 2：核心 MVP（Week 7~12，6 周）

**目标**：三个核心来源可用 + Workspace 合并 + 搜索 + 导出。**这是面向首批真实用户的版本。**

**入口条件**：Phase 1 退出标准满足；Fixture 采集计划就绪。

#### 任务

- [ ] **P2-1 三个核心 Adapter**
  - [ ] Codex Adapter（app-server 优先，导出/Session 降级，§10.5）
  - [ ] Cursor Adapter（Markdown 导出 + 授权 SQLite 快照；Background Agent 远程历史**不抓取**）
  - [ ] Claude Code Adapter（CLI 会话列表/Resume + IDE Session History + 用户导出）
  - [ ] 每个 Adapter 采集字段对标 §6.2 清单（标题/Project/时间/模型/消息/工具调用/命令/文件操作/Diff/审批/耗时/Token/状态/原始 ID/Schema 版本）

- [ ] **P2-2 Workspace 合并**
  - [ ] 实现 7 级合并优先级（§4.3：手动 > Manifest ID > Git Remote > Common Dir > 绝对路径 > inode > 名称相似度）
  - [ ] 记录 `match_method / match_confidence / matched_at / matched_by / manual_override`
  - [ ] 低置信度需用户确认交互
  - [ ] 支持手动合并/拆分/重命名 + 别名/标签/收藏/归档
  - [ ] 多 Worktree 支持

- [ ] **P2-3 Conversation 浏览**
  - [ ] 列表筛选（时间/来源/Workspace/状态/标签）
  - [ ] 详情：Message / Command / Tool Call / Diff / Artifact
  - [ ] 折叠低价值事件
  - [ ] 原始视图 ↔ 统一视图切换
  - [ ] 复制/导出/收藏/备注
  - [ ] **一键打开来源应用**
  - [ ] 来源支持时恢复原会话
  - [ ] **完整度提示**（完整/部分/有限，§17.3）——禁止让用户误以为都能完整恢复

- [ ] **P2-4 搜索**
  - [ ] Tantivy 索引接入（字段见 §13.1）
  - [ ] 中文分词器可插拔 + N-gram 兜底
  - [ ] 文件路径/命令专用 tokenizer
  - [ ] 查询语法（§13.2：`provider:` `workspace:` `type:` `file:` `status:` `after:` `before:` `model:`）
  - [ ] 结果高亮 + 命中片段
  - [ ] 保存搜索条件
  - [ ] 排序公式 v1（§13.4 BM25 + 加权）
  - [ ] FTS5 降级路径就绪

- [ ] **P2-5 同步与去重**
  - [ ] 同步状态机（§11.1：Disabled/Discovering/Ready/Syncing/Partial/Error）
  - [ ] 增量同步 16 步流程（§11.2）
  - [ ] 幂等键 + 内容 Hash 兜底（§11.3，**禁止纯文本去重**）
  - [ ] 删除语义（§11.4：标记 deleted，不立即物理删）
  - [ ] 冲突处理（§11.5 表格 6 种场景）
  - [ ] 自动/手动/定时同步

- [ ] **P2-6 导出与备份**
  - [ ] 单条会话导出 Markdown
  - [ ] Workspace 批量导出
  - [ ] 原始 JSON/JSONL 导出
  - [ ] 可选是否含命令/Diff/Artifact
  - [ ] 导出前敏感信息扫描 + 脱敏
  - [ ] 本地加密备份 + 导入恢复
  - [ ] 数据可移植性验证（导出→重导入一致）

#### MVP 验收（Gate 1，Week 12）

- [ ] 三个核心来源可导入
- [ ] 统一 Project 分组准确率 **≥95%**（低置信度要求确认）
- [ ] **100k Conversation 搜索 P95 < 300ms**
- [ ] 同步可中断恢复
- [ ] 导出与重导入一致
- [ ] **无 P0/P1 安全缺陷**
- [ ] 无数据重复
- [ ] 可备份恢复
- [ ] Adapter 崩溃隔离

**验收证据**：性能基准报告 / 95% 合并准确率统计 / 安全扫描报告 / 10+ 真实脱敏 Fixture。

---

### Phase 3：Adapter 平台化（Week 13~18，6 周）

**目标**：新来源可**不改核心代码**接入；高风险来源 Best Effort 不损坏数据。

**入口条件**：MVP 通过 Gate 1。

#### 任务

- [ ] **P3-1 新增 Adapter**
  - [ ] ZCode Adapter（官方能力优先，无稳定 API 标记 Best Effort，§10.5）
  - [ ] MiniMax Code Adapter（同上限制）
  - [ ] OpenCode Adapter（JSON/JSONL Session + CLI Wrapper）

- [ ] **P3-2 Adapter 平台机制**
  - [ ] Adapter Manifest 规范（§10.3：id/version/protocolVersion/entrypoint/platforms/permissions/capabilities）
  - [ ] Adapter 签名 + Hash 校验落地
  - [ ] Schema 指纹（§11.6：application_version/storage_format/schema_tables/schema_columns/schema_hash/adapter_parser_version）
  - [ ] 未知 Schema 自动降级（停止解析 + 保存诊断 + 提示升级 + 允许手动导入）
  - [ ] Adapter 权限可视化（用户可查看文件/网络/进程权限）
  - [ ] Adapter 独立更新 + 回滚

- [ ] **P3-3 兼容性测试体系**
  - [ ] Golden Fixture Test Kit（每个 Adapter 必备，§20.2 的 13 类用例）
  - [ ] 多版本 Fixture
  - [ ] 来源版本兼容矩阵
  - [ ] Crash/Timeout 隔离测试
  - [ ] 数据损坏/字段缺失/未知字段/重复/数据库被占用/运行中写入 用例

#### 退出标准（Gate 2 前置）

- [ ] 新 Adapter **不改核心代码**即可接入
- [ ] ZCode/MiniMax 未知 Schema 时**不损坏数据**
- [ ] Adapter 更新可独立回滚
- [ ] 第三方 Adapter 权限可视化
- [ ] Crash-free Session 达标
- [ ] **30 名真实用户连续使用两周**（Private Beta）

---

### Phase 4：知识化与移动准备（Week 19~22，4 周）

**目标**：AI 摘要/决策提取；Android 可安全浏览桌面数据。

#### 任务

- [ ] **P4-1 AI 知识提取**（§13.5，全部默认关闭/显式启用）
  - [ ] 自动摘要
  - [ ] 技术决策提取（含来源引用）
  - [ ] TODO / 错误与解决方案 / 关键命令 / 涉及文件提取
  - [ ] 相关 Conversation 推荐 + 重复聚类
  - [ ] 可选本地模型或云模型（**不自动上传完整源码**）
  - [ ] 记录模型与 Prompt 版本
  - [ ] 人工编辑后保留版本，**不覆盖原始对话**
  - [ ] AI 生成内容与原始数据**严格视觉区分**

- [ ] **P4-2 本地 API 与移动**
  - [ ] 本地 HTTP/WebSocket API（事件订阅 §16.3）
  - [ ] Android 浏览 PoC（Tauri 2 Mobile + React）
  - [ ] 二维码安全配对 + 双向认证 + TLS
  - [ ] 局域网服务**默认关闭**，会话可撤销
  - [ ] 通知机制（同步完成等）

- [ ] **P4-3 可复用包抽取**（§18.3）
  - [ ] domain-types / api-client / query-language / markdown-components / search-components / shared-ui
  - [ ] 明确**不复用**：本地 Adapter / 桌面进程管理 / 第三方 DB 访问 / 桌面路径权限

#### 退出标准（Gate 2，Week 20）

- [ ] AI 提取结果有来源引用
- [ ] **关闭 AI 能力后产品完整可用**
- [ ] Android 可安全搜索和查看桌面数据
- [ ] 局域网服务默认关闭

---

### Phase 5：企业强化（Week 23~26，4 周）

**目标**：策略、审计、SSO、加密同步 PoC。**仍保持「企业可选」。**

#### 任务

- [ ] **P5-1 身份与权限**
  - [ ] OIDC SSO（§14.7）
  - [ ] RBAC
  - [ ] 组织策略签名 + 验证

- [ ] **P5-2 治理**
  - [ ] Adapter Allowlist
  - [ ] 强制最低客户端版本
  - [ ] 禁止个人云模型 / 指定允许的模型供应商
  - [ ] 数据保留策略（含 Dry Run）
  - [ ] 法律保留（优先于普通删除）
  - [ ] 导出策略 + 审计导出

- [ ] **P5-3 审计与同步**
  - [ ] 审计日志（§12.1 audit_logs 表，普通用户不可改）
  - [ ] 企业同步 PoC（§15.3：默认仅同步元数据，正文需显式启用）
  - [ ] 客户端加密 / 服务端可检索模式可选
  - [ ] 敏感 Workspace 强制禁同步
  - [ ] KMS 集成
  - [ ] 数据驻留区域

- [ ] **P5-4 管理控制台**
  - [ ] 管理控制台 Web（策略下发 / 审计查询 / 远程擦除企业同步副本）
  - [ ] 企业安装包 + 配置模板

#### 退出标准（Gate 3，Week 26）

- [ ] 策略可签名验证
- [ ] **管理员不能直接读取本机未同步内容**
- [ ] 所有导出行为可审计
- [ ] 数据删除可验证
- [ ] SSO + 租户隔离测试通过

---

### Phase 6：GA（Week 27~28，2 周）

**目标**：第三方审计闭环 + 灾难恢复 + 发布。

#### 任务

- [ ] **P6-1 验证**
  - [ ] 第三方安全审计（关闭高危问题）
  - [ ] 性能回归测试（§20.4：Small/Medium/Large/Stress 四档数据集）
  - [ ] 灾难恢复演练
  - [ ] 升级/回滚演练

- [ ] **P6-2 发布与运维**
  - [ ] CI/CD 全流程（§21：lint→unit→contract→build→security→package→sign→E2E→canary→stable）
  - [ ] macOS 签名公证 + Windows Code Signing + Updater 签名
  - [ ] SBOM + 依赖漏洞扫描 + License Scan
  - [ ] 发布通道（dev/nightly/alpha/beta/stable/enterprise-lts）
  - [ ] 自动回滚开关
  - [ ] 运维 Runbook + 值班流程

- [ ] **P6-3 文档**
  - [ ] 隐私政策 + 企业 SLA
  - [ ] 用户指南 + 管理员指南
  - [ ] 数据保留策略文档 + 灾难恢复方案

#### 退出标准（Gate 4，Week 28）

- [ ] 第三方审计关闭高危
- [ ] 灾难恢复演练通过
- [ ] 性能容量测试通过
- [ ] 发布与回滚演练通过
- [ ] 隐私/法律文档完成

---

## 4. 执行前必须先拍板的前置决策

> 以下 6 个问题**必须在 Phase 0 结束前有答案**，否则 Phase 1 会返工。用 AskUserQuestion 的精神逐条确认。

| # | 决策项 | 默认建议 | 不决定的后果 |
|---|---|---|---|
| D1 | **团队规模与节奏** | 精简 4~5 人 → 接受 9~12 个月 GA；还是 8~10 人 → 28 周 | 决定 Phase 划分是否需要拉长 |
| D2 | **首发平台** | macOS + Windows 同步？还是 macOS 优先？ | 影响 Adapter Host 跨平台抽象的优先级 |
| D3 | **AI 提取的模型策略** | 本地模型优先（隐私）？还是允许云模型？ | 影响 Phase 0 的法律评审范围 |
| D4 | **企业版是否本期做** | 个人版先行，企业推迟到 Phase 5 | 决定是否现在就引入 OIDC/KMS 依赖 |
| D5 | **Android 是否本期做** | 默认推迟到 Phase 4 PoC | 决定是否现在抽取共享包 |
| D6 | **开源 or 闭源** | 影响 License Scan、SBOM、签名密钥管理 | 影响整个 CI/CD 与发布体系 |

---

## 5. 与原方案的章节映射

| 本计划章节 | 对应原方案章节 | 关系 |
|---|---|---|
| §1 一页读懂 | §1 执行摘要 + 附录 C | 浓缩 |
| §2.1 定位 | §2~3 | 解读 |
| §2.2 信任边界 | §8.4 | 解读 |
| §2.3 双存储 | §9.6 + §12.3 | 解读 |
| §2.4 同步 | §11 | 解读 |
| §2.5 搜索 | §13 | 解读 |
| §2.6 企业 | §14~15 | 解读 |
| §2.7 风险 | §26 | 提炼 Top3 |
| §3 Phase 0~6 | §23 + §27 | 拆解为可勾选清单 |
| §4 前置决策 | （原方案未明确，本文补充） | **新增** |
| §6 DoD | 附录 A + §28 | 落地化 |
| §7 风险登记 | §26 | 转为跟进表 |

---

## 6. 完成定义（DoD）——每条任务算「完成」的统一标准

> 改编自原方案附录 A。**任何一条任务勾选 `[x]` 前，以下全部满足：**

- [ ] 代码完成且通过 `cargo fmt + clippy + test` / `tsc + lint + test`
- [ ] Unit Test 完成
- [ ] 涉及协议/Adapter 的有 Contract Test
- [ ] 文档完成（含 ADR 若涉及决策）
- [ ] 日志字段 + 错误码完成（错误码见附录 B）
- [ ] 安全评审完成（尤其涉及文件/网络/密钥）
- [ ] 性能基准无明显回退
- [ ] 涉及 schema 变更的 Migration 已验证
- [ ] 用户可理解失败原因（错误信息可读）
- [ ] Feature Flag 可关闭
- [ ] 可回滚
- [ ] **不泄漏 Conversation 正文**（日志/诊断包/遥测）

---

## 7. 风险登记与跟进表

> 每周项目同步会 review。状态：🟡监控 / 🔴发生 / 🟢已缓解。

| ID | 风险 | P | I | 缓解措施 | 负责人 | 状态 | 备注 |
|---|---|---|---|---|---|---|---|
| R1 | 第三方 Schema 高频变化 | 高 | 高 | Schema 指纹 + 版本化 Adapter + Golden Fixture + 降级导入 | Adapter Lead | 🟡 | Adapter 体系核心动机 |
| R2 | 历史仅存云端 | 高 | 高 | 官方 API 优先 + 明确完整度 + 禁止非法抓取 | Adapter Lead | 🟡 | Cursor Background Agent |
| R3 | 第三方条款限制 | 中 | 高 | 法律评审 + 用户授权 + 只读 + 公开导出优先 | PM + 法务 | 🟡 | Phase 0 必须初审 |
| R4 | 会话含密钥/敏感源码 | 高 | 高 | 本地优先 + 脱敏 + 加密 + 目录排除 | Security | 🟡 | 默认禁云同步 |
| R5 | Adapter 恶意/被篡改 | 中 | 高 | 签名 + 进程隔离 + 权限白名单 | Security | 🟡 | |
| R6 | 数据量大搜索变慢 | 中 | 中 | Tantivy + 分页 + 异步索引 + 性能基准 | Rust Lead | 🟡 | MVP 即需达标 |
| R7 | SQLite 损坏 | 低 | 高 | WAL + 完整性检查 + 备份 + 恢复 | Rust Lead | 🟡 | |
| R8 | 搜索索引损坏 | 中 | 低 | 索引可重建 | Rust Lead | 🟢 | 设计即兜底 |
| R9 | 自动 Workspace 合并错误 | 中 | 中 | 置信度 + 人工确认 + 可拆分 | Adapter Lead | 🟡 | MVP 验收 95% |
| R10 | 原始数据体积快增 | 高 | 中 | 压缩 + 保留策略 + 按 Artifact 清理 | Rust Lead | 🟡 | |
| R11 | 企业同步引发合规 | 中 | 高 | 默认关闭 + 数据分级 + 区域 + KMS | Security | 🟡 | Phase 5 |
| R12 | UI 展示恶意 Markdown | 中 | 高 | HTML Sanitization + 禁脚本 | Frontend Lead | 🟡 | |
| R13 | 自动更新供应链攻击 | 低 | 高 | 签名 + KMS/HSM + 回滚 + SBOM | Security | 🟡 | |
| R14 | **团队范围过大** | 高 | 高 | MVP 严格限 3 来源 + 本地功能 | PM | 🔴 | **最需警惕** |

---

## 8. 30 天快速启动（Day 级颗粒度）

> 来自原方案 §29，落地为日清单。假设 Week 1~4 团队 4 人（PM/Tech Lead/Rust/Adapter）。

### Week 1：确认与 PoC

- [ ] **Day 1~2**：建 Monorepo + ADR 模板 + 术语固化 + 领域模型 v0.1 + MVP 范围锁定
- [ ] **Day 3~5**：Codex app-server PoC / Cursor Markdown+SQLite PoC / Claude Session PoC → 字段覆盖率 → 兼容矩阵 v0.1

### Week 2：基础架构

- [ ] Tauri Desktop 壳 + Rust Daemon
- [ ] UDS/Named Pipe 抽象 + JSON-RPC Envelope
- [ ] SQLite Migration + Raw Store
- [ ] Adapter Host Hello/Health

### Week 3：首个端到端链路

- [ ] Markdown Adapter
- [ ] Source → Raw → Normalize → SQLite 全链路
- [ ] Workspace + Conversation 列表 + 详情页
- [ ] 首次导入跑通
- [ ] 结构化日志 + Correlation ID

### Week 4：核心来源

- [ ] Codex/Cursor/Claude Code Adapter alpha
- [ ] 内容 Hash + 幂等
- [ ] Tantivy 基础索引
- [ ] **10k Conversation 性能基准**
- [ ] 第一次威胁模型评审

### 30 天验收

- [ ] 可导入 ≥3 来源
- [ ] 可按 Workspace 展示
- [ ] 可查看 Message
- [ ] 可全文搜索
- [ ] 重复同步不产生重复
- [ ] Adapter 崩溃不影响 UI
- [ ] 数据库可备份恢复
- [ ] **≥10 个真实脱敏 Fixture**

---

## 9. 下一步建议

1. **立刻做**：召集 PM + Tech Lead，对 §4 的 6 个前置决策逐条拍板，结论写进 ADR。
2. **本周做**：启动 Phase 0 的 P0-1（Monorepo + ADR + 术语）和 P0-2（数据源调查），并行法务条款初审。
3. **本里程碑做**：Phase 0 退出标准全部达成后再进 Phase 1，**不要跳 Gate**。
4. **持续做**：每周 review §7 风险表，R14（范围蔓延）每次产品评审都要复查。

---

> **文档结束**。后续修订：在对应阶段末尾追加新动作，风险表持续更新，完成的任务勾选 `[x]` 并附验收证据链接。
