# 设计：大模型知识提取（LLM Extraction）+ 本地加密配置

- 日期：2026-08-20
- 分支：`feat/llm-extraction`
- 状态：已实施（自主会话产出，决策依据如下）
- 对应需求：用户目标「支持大模型提取，大模型可配置在本地，但配置要加密，确保本地安全，新建分支开发」
- 对应 plan：`docs/ai-ide-conversation-hub-enterprise-plan.md` §13.5（AI 知识提取）、§14.3（本地数据保护/密钥管理）

## 1. 需求与自主决策记录

本设计在自主模式下完成，以下问题基于代码证据与 plan 要求作出决策：

| 问题 | 决策 | 依据 |
|---|---|---|
| 「大模型配置在本地」指本地推理还是本地保存配置？ | 两者都支持：统一 **OpenAI 兼容端点**配置（`base_url` + `model` + `api_key`），云端 API（OpenAI/DeepSeek/GLM…）与本地推理服务（Ollama / LM Studio / llama.cpp server，均暴露 OpenAI 兼容接口）同一套配置覆盖 | 一套客户端覆盖全部主流形态；Ollama 等本地服务不离开本机，天然满足隐私要求 |
| 加密对象是什么？ | **API Key**（唯一敏感项）以 XChaCha20-Poly1305 静态加密；非敏感项（开关/base_url/model/超时）明文存 `app_settings` | base_url/model 无保密价值且需要 GUI 回显；密钥绝不明文落盘 |
| 主密钥放哪？ | OS 安全存储优先（macOS Keychain / Windows Credential Manager / Linux Secret Service，经 `keyring` crate），不可用时回退应用数据目录下 **0600 权限随机密钥文件**并 tracing 告警 | plan §14.3「主密钥存入操作系统安全存储」；回退保证 headless Linux/CI 可用 |
| 默认引擎？ | 保持规则引擎为默认；LLM 引擎需在设置中**显式开启**（`enabled=false` 为初始态） | plan §13.5「默认本地关闭或显式启用」；避免隐式联网与费用 |
| 隐私边界？ | 发送内容截断上限 `max_input_chars`（默认 48,000 字符）；本地端点（127.0.0.1/localhost/[::1]）在 GUI 显示「本地」徽标；`docs/privacy.md` 补充数据流说明 | plan §13.5「不自动上传完整源码」；本地推理零外发 |
| 输出契约？ | `ExtractionResult` 结构不变；`extractor` 记录 `llm:{model}@prompt-v1`，来源引用映射回真实 `source_message_ids` | plan §13.5「记录使用的模型和 Prompt 版本」「生成结果有来源引用」；与规则引擎结果同构，前端零改动展示 |
| 后台批量提取（`knowledge_extract_all`）用哪个引擎？ | 保持规则引擎 | 确定性、离线、零成本；LLM 按需交互式触发，避免不可控 API 开销 |
| daemon JSON-RPC `knowledge.extract`？ | 保持规则引擎（本次不改） | 桌面 GUI 是本需求的主路径；daemon 扩展留待后续 |

## 2. 方案对比（3 选 1）

**方案 A：直接在 ch-knowledge 里加 `llm.rs` + 引入 reqwest。**
简单直接，但把 HTTP 客户端、密钥管理、配置持久化全部耦合进知识 crate；reqwest 依赖树重（hyper 全家桶），且密钥密封逻辑无法被其他域复用。否决。

**方案 B（选定）：新 crate `ch-llm`（客户端 + 配置 + 密钥密封）+ `ch-knowledge` 增 `LlmExtractor`（依赖注入 `Chat` trait）+ Tauri 命令层 + GUI。**
关注点分离：`ch-llm` 不依赖业务模型，可独立测试；`LlmExtractor` 通过 `Chat` trait 注入传输实现（生产用 ureq，测试用 mock），对齐仓库「小 crate + trait 边界」风格（如 Adapter SDK）；复用 workspace 已有加密依赖 `chacha20poly1305`（与 backup 同栈）。HTTP 用 `ureq 2`（rustls，无 openssl），轻量且同步模型与全仓一致。

**方案 C：Tauri 插件生态（tauri-plugin-stronghold / keyring 插件）。**
引入外部插件依赖与托管密钥库模型，与自研 crate 体系不一致； stronghold 重新发明密封格式且 CLI/daemon 无法复用。否决。

## 3. 架构与数据流

```
GUI 设置页（LLM 区）                GUI 知识弹窗（引擎切换 规则/AI）
   │ llm_config_set/get                  │ extract_knowledge(id, engine)
   │ llm_test_connection                 ▼
   ▼                              Tauri commands/conversations.rs
Tauri commands/llm_cmd.rs            ├─ engine="rule" → RuleExtractor（现状）
   │ 密封/解密 API Key                └─ engine="llm"  → ch-knowledge::LlmExtractor
   ▼                                        │ build prompt + parse JSON
ch-llm::SecretVault                        ▼
   ├─ 主密钥：OS keyring → 0600 key file   ch-llm::Chat（ureq, OpenAI 兼容 /chat/completions）
   └─ seal/open: XChaCha20-Poly1305             │ HTTPS（云）或 http://127.0.0.1（本地）
   ▼
app_settings("llm_config")：JSON{ enabled, base_url, model, timeout_secs,
                                   max_input_chars, api_key_sealed("v1.…") }
```

- 密文（`api_key_sealed`）存 SQLite `app_settings`；主密钥**不进数据库**——数据库被单独拷走/备份泄露时密文不可解。
- 前端永远收不到明文或密文密钥，只有 `api_key_masked`（`sk-***abcd` 形态）与 `has_api_key`。

## 4. ch-llm crate 设计

### 4.1 `secret.rs` — 密钥密封

- `SecretVault::open(data_dir: &Path)`：
  1. 尝试 OS keychain（service `threadock`，account `llm-master-key`）读取/生成 32B 主密钥；
  2. 不可用（无 dbus / 无钥匙串等）→ `<data_dir>/keys/llm-master.key`，随机 32B，Unix 0600，`tracing::warn` 说明回退；
  3. 主密钥 `Zeroize`（drop 时清零）。
- `seal(&str) -> String`：格式 `v1.` + base64(nonce[24] ‖ ciphertext‖tag[16])，nonce 每次随机（OsRng）。
- `open(&str) -> Result<String>`：校验版本前缀与 base64；AEAD 认证失败 → `LlmError::Decrypt`（错误信息不含密钥材料）。
- `mask_key(&str) -> String`：`sk-AbCd1234` → `sk-***d1234`（保留前缀与尾部各 4 字符，中间打码）。
- 版本字段 `v1` 为未来轮换预留（plan §14.3「密钥轮换必须有版本字段」）。

### 4.2 `config.rs` — 配置模型

```rust
pub struct LlmConfig {
    pub enabled: bool,            // 默认 false（显式启用）
    pub base_url: String,         // 如 https://api.openai.com/v1 或 http://127.0.0.1:11434/v1
    pub model: String,
    pub timeout_secs: u64,        // 默认 60，上限 300
    pub max_input_chars: usize,   // 默认 48_000，上限 200_000
    pub api_key_sealed: Option<String>, // 密文，绝不回传前端
}
```

- `validate()`：base_url 必须以 `http://` 或 `https://` 开头（http 仅允许本地端点，防降级明文外发）；model 非空；数值界限钳制。
- `is_local_endpoint()`：host 为 `localhost` / `127.0.0.0/8` / `::1` 时为真（GUI「本地」徽标 + 允许 http）。

### 4.3 `client.rs` — OpenAI 兼容客户端

- `trait Chat { fn chat(&self, req: ChatRequest) -> Result<ChatReply, LlmError> }`：
  - `ChatRequest { system, user, max_tokens, json_mode }`，`ChatReply { content, model }`。
  - 生产实现 `UreqChat`：POST `{base_url}/chat/completions`，Bearer 头，JSON 体；解析 `choices[0].message.content` 与 `model`。
  - `json_mode` 默认开（`response_format: {"type":"json_object"}`）；若服务端 4xx 拒绝，**去掉 response_format 重试一次**（兼容部分本地服务）。
- `LlmError`：`Network` / `HttpStatus{code}`（401 认证失败、429 限流、5xx 服务端）/ `Parse`（返回体不含有效内容）/ `Decrypt` / `Config`；Display 均为面向用户的中文短语，**绝不包含 api_key 或 Authorization 头**。
- Agent：连接池 + `timeout_connect/timeout_read`（来自 `timeout_secs`）。
- AAD 绑定：密封时以 `aad = "threadock.llm.api_key"` 固定串防密文挪用（跨域替换攻击面收敛）。

## 5. ch-knowledge `LlmExtractor` 设计（`src/llm.rs`）

- `LlmExtractor::new(chat: Arc<dyn Chat>, model_label: String)`。
- `extract(&ExtractionInput) -> Result<ExtractionResult, LlmError>`：
  1. **转录**：消息编号 `[1] user: …` / `[2] assistant: …`（`content_text` 为空跳过），按 `max_input_chars` 截断并标注截断说明；
  2. **System prompt（prompt-v1，常量）**：给出严格 JSON schema（summary/decisions/todos/errors/commands/files，source 用消息编号数组），要求不编造、无内容字段返回空数组；
  3. **调用** `Chat::chat`，`max_tokens` 2048、json_mode；
  4. **解析**：剥 ```` ```json ```` 围栏 → 首个 `{` 到最后一个 `}` 的平衡子串 → `serde_json` → 宽松映射（缺字段补默认、字符串数组容错、空串过滤、每类条目上限 50）；
  5. **来源映射**：编号(1-based) → 真实 `message.id`，越界忽略；无编号的条目 `source_message_ids` 为空；
  6. `extractor: format!("llm:{model_label}@prompt-v1")`。
- 不落盘、不改原始对话（plan §13.5「不覆盖原始对话」）。

## 6. Tauri 命令层（`commands/llm_cmd.rs` + `conversations.rs`）

| 命令 | 说明 |
|---|---|
| `llm_config_get` | 返回视图 DTO：`{enabled, base_url, model, timeout_secs, max_input_chars, has_api_key, api_key_masked, is_local}`；无密钥/密文 |
| `llm_config_set` | 保存非敏感字段；`api_key: Option<String>` 提供则密封替换；`clear_api_key: bool` 清除 |
| `llm_test_connection` | 发送最小 chat（max_tokens=8）返回 `{ok, latency_ms, model}`；错误分类中文提示 |
| `extract_knowledge`（改造） | 新增 `engine: Option<String>`（`"rule"` 默认 / `"llm"`）；llm 未启用/未配置 → 明确错误提示引导去设置 |

- 密封/解密在 `run_blocking` 中执行（Argon/网络不占 tokio worker）。
- 错误走 `AppError` 分类（`AppError::config`/新增分类沿用现有格式）。

## 7. GUI 设计

- **SettingsView 新增「AI 提取（大模型）」section**（模式对齐 BackupSection）：
  - 启用开关（默认关，开启时展示隐私提示：发送会话文本到所配端点，本地端点标记「不出本机」）；
  - base_url / model 输入 + 预设快捷按钮（OpenAI / DeepSeek / GLM / Ollama 本地）；
  - API Key 密码框（只写）+ masked 回显 + 清除按钮；
  - 「测试连接」按钮（结果 + 延迟）。
- **KnowledgeModal**：头部加引擎切换（⚙ 规则 / ✨ AI），切换即以新引擎重提；`extractor` 以 `llm:` 开头时显示模型徽标。
- App.tsx `extractKnowledge(engine)` 传递引擎；失败 toast（现有行为）。

## 8. 测试策略（TDD）

- `ch-llm`：
  - secret：roundtrip、篡改检测（翻转密文字节 → Decrypt 错）、错误版本前缀、mask、密钥文件 0600 权限（unix）、AAD 不匹配失败；
  - config：validate 各分支、本地端点判定（含 http 非本地拒绝）；
  - client：本地 `TcpListener` mock OpenAI 服务——成功解析、HTTP 401/429/500 分类、response_format 被拒后降级重试、超时路径；**无外网依赖**。
- `ch-knowledge`：mock `Chat` 返回固定 JSON / 带围栏 JSON / 垃圾文本 → 解析、裁剪、来源映射、extractor 标识；空 transcript 行为。
- `src-tauri` e2e（mock app + 临时目录真实后端）：config 往返、key masked 不泄露、extract_knowledge 默认 rule、llm 未启用报错文案。
- 全套走 `scripts/precheck.sh lint` 与 `test`（镜像 CI：fmt×2 + clippy pedantic -D warnings + cargo test 三平台矩阵本地 macOS + tsc + eslint + vitest）。

## 9. 安全清单（对照目标「一定要确保本地安全」）

1. API Key 永不明文落盘（DB/日志/localStorage/前端内存均无）。
2. 主密钥优先 OS 安全存储；回退文件 0600 且仅本用户可读。
3. AEAD 认证加密（XChaCha20-Poly1305，与 backup 同栈）+ 固定 AAD + 每次随机 nonce + 版本前缀。
4. 主密钥内存 Zeroize。
5. 日志与错误信息脱敏（masked 形态；LlmError 不携带凭据）。
6. LLM 默认关闭，显式启用；输入截断上限；本地端点可视化。
7. http 明文端点仅允许 localhost（防云端降级明文传输）。
8. 前端无文件系统/密钥访问权（沿用 Tauri 边界）。

## 10. 变更清单

| 位置 | 变更 |
|---|---|
| `crates/llm/`（新） | `Cargo.toml`、`src/{lib,config,secret,client}.rs` + 单元测试 |
| `crates/knowledge/` | `src/llm.rs`（LlmExtractor）+ `Cargo.toml` 依赖 `ch-llm` + 测试 |
| `apps/desktop/src-tauri/` | `commands/llm_cmd.rs`（新）、`conversations.rs`（engine 参数）、`lib.rs`（注册）、`e2e_journeys.rs`（适配） |
| `apps/desktop/src/` | `SettingsView.tsx`（LLM 区）、`KnowledgeModal.tsx`（引擎切换）、`App.tsx`、`types.ts` |
| `docs/` | `user-guide.md`、`privacy.md`、`README.md`、`CHANGELOG.md` |

## 11. 非目标（本次不做）

- 多 Provider 档案管理（多套密钥切换）。
- 流式输出 / 多轮对话式提取。
- daemon JSON-RPC 的 LLM 引擎。
- DEK/主密钥轮换 UI（版本字段已预留）。
