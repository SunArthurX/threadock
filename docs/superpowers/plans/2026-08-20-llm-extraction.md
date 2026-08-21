# LLM 知识提取（加密配置）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 知识提取支持大模型引擎（OpenAI 兼容端点，云端或本地推理服务），API Key 以 XChaCha20-Poly1305 静态加密存储，主密钥优先放 OS 钥匙串。

**Architecture:** 新 crate `ch-llm`（配置 + 密钥密封 + HTTP 客户端）→ `ch-knowledge::LlmExtractor`（prompt/解析，依赖注入 `Chat` trait）→ Tauri 命令层（`llm_config_get/set`、`llm_test_connection`、`extract_knowledge(engine)`）→ GUI（设置页 LLM 区 + 知识弹窗引擎切换）。设计文档：`docs/superpowers/specs/2026-08-20-llm-extraction-design.md`。

**Tech Stack:** Rust（ureq 2 + rustls、chacha20poly1305、zeroize、base64 0.22、keyring 3）、Tauri 2、React/TS。

## Global Constraints

- clippy 门禁：`-D warnings -W clippy::unwrap_used -W clippy::pedantic`（allowed: missing_errors_doc/missing_panics_doc/cast_precision_loss/cast_possible_wrap/cast_sign_loss/cast_possible_truncation/doc_markdown）——非测试代码不得 `unwrap()`
- fmt：`cargo fmt` 全过；前端 `tsc --noEmit` + `eslint` + `vitest` 全过
- 安全红线：API Key 明文不得出现在 DB/日志/错误信息/前端；`LlmError` 的 Display 不含凭据；非本地端点强制 https
- 默认关闭：`LlmConfig::default().enabled == false`
- 提交走 `scripts/precheck.sh`（pre-commit = lint；完成前跑 `precheck.sh test`）
- 分支：`feat/llm-extraction`

---

### Task 1: ch-llm crate 骨架 + 配置模型（config.rs）

**Files:**
- Create: `crates/llm/Cargo.toml`、`crates/llm/src/lib.rs`、`crates/llm/src/config.rs`
- Modify: `Cargo.toml`（workspace members 加 `crates/llm`）

**Interfaces（Produces）:**
```rust
// ch_llm
pub enum LlmError { KeyStore(String), Decrypt, Network(String), HttpStatus { code: u16, detail: String }, Parse(String), InvalidConfig(String) } // Clone+Error
pub struct LlmConfig { pub enabled: bool, pub base_url: String, pub model: String, pub timeout_secs: u64, pub max_input_chars: usize, pub api_key_sealed: Option<String> }
impl LlmConfig { pub fn validate(&mut self) -> Result<(), LlmError>; pub fn is_local_endpoint(&self) -> bool; pub fn is_ready(&self, has_api_key: bool) -> bool }
pub const DEFAULT_TIMEOUT_SECS: u64 = 60; pub const MAX_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_MAX_INPUT_CHARS: usize = 48_000; pub const MAX_INPUT_CHARS_LIMIT: usize = 200_000;
```

- [ ] **Step 1: 建 crate 与 Cargo.toml**（workspace 声明 + 依赖：thiserror/serde/serde_json/tracing/chacha20poly1305/zeroize/base64；target-specific keyring；ureq；dev-dep tempfile）
- [ ] **Step 2: lib.rs**：模块声明 + `LlmError`（`#[derive(Debug, Clone, Error)]`，Display 全中文、无凭据）
- [ ] **Step 3: 写 config.rs 失败测试**：defaults 值；`validate` trim+钳制（timeout 0→60、999→300；max_input 0→48000、999999→200000）；enabled+`http://example.com` → InvalidConfig；enabled+`http://127.0.0.1:11434/v1` → Ok；enabled+空 model → InvalidConfig；`is_local_endpoint`：localhost/127.x/[::1]/0.0.0.0 真、example.com 假；`is_ready` 矩阵（云无 key 假、云有 key 真、本地无 key 真、未启用假）
- [ ] **Step 4: 实现 config.rs**（host 解析手写：`[::1]` bracket 特判）
- [ ] **Step 5: `cargo test -p ch-llm` 通过；`cargo clippy -p ch-llm` 干净**
- [ ] **Step 6: Commit** `feat(llm): ch-llm crate 骨架与配置模型`

### Task 2: 密钥密封（secret.rs）

**Files:**
- Create: `crates/llm/src/secret.rs`（lib.rs 补导出）

**Interfaces（Produces）:**
```rust
pub struct SecretVault { /* master_key: Zeroizing<[u8;32]> */ pub source: MasterKeySource }
impl SecretVault {
    pub fn open(data_dir: &Path) -> Result<Self, LlmError>;          // keychain 优先；THREADOCK_NO_KEYCHAIN=1 或失败 → 文件
    pub fn with_file_key(data_dir: &Path) -> Result<Self, LlmError>; // 测试/hermetic
    pub fn seal(&self, plaintext: &str) -> Result<String, LlmError>; // "v1."+b64(nonce24‖ct+tag16)
    pub fn open_sealed(&self, sealed: &str) -> Result<String, LlmError>;
}
pub enum MasterKeySource { OsKeychain, KeyFile }
pub fn mask_key(key: &str) -> String; // "sk-AbCd1234"→"sk-***d1234"→取头3尾4
```

- [ ] **Step 1: 失败测试**（全部用 `with_file_key`+tempdir，防污染真钥匙串）：
  - roundtrip：seal→open_sealed == 原文；两次 seal 密文不同（随机 nonce）
  - 篡改：翻转密文末字节 / 改版本前缀 `v2.` / 坏 base64 / 缺 `.` → `Decrypt`
  - 跨 vault：vault A seal，vault B（不同 key 文件目录）open → `Decrypt`
  - mask_key：空串/短串(≤8 全打码保尾)/常规；trim
  - key 文件 0600 权限（unix cfg 测试）；重复 open 复用同一密钥（同目录两次 with_file_key 均可解开）
  - keychain 路径冒烟：`open(tempdir)` 成功即可（不断言 source；hermetic 环境走 KeyFile）
- [ ] **Step 2: 实现**：keychain（service `threadock`/account `llm-master-key`，base64 存取，NoEntry→生成写入）；文件兜底 `<data_dir>/keys/llm-master.key`（0600、损坏报 KeyStore）；AAD `b"threadock.llm.api_key"`；`XChaCha20Poly1305::generate_nonce(&mut OsRng)`
- [ ] **Step 3: 测试通过 + clippy 干净**
- [ ] **Step 4: Commit** `feat(llm): SecretVault 密钥密封（OS 钥匙串优先 + 0600 密钥文件兜底）`

### Task 3: OpenAI 兼容客户端（client.rs）

**Files:**
- Create: `crates/llm/src/client.rs`（lib.rs 补导出）

**Interfaces（Produces）:**
```rust
pub struct ChatRequest { pub system: String, pub user: String, pub max_tokens: u32, pub json_mode: bool } // Clone
pub struct ChatReply { pub content: String, pub model: String }                                        // Clone
pub trait Chat: Send + Sync { fn chat(&self, req: &ChatRequest) -> Result<ChatReply, LlmError>; }
pub struct HttpChat { /* base_url, model, api_key: Option<String>, agent: ureq::Agent */ }
impl HttpChat { pub fn new(config: &LlmConfig, api_key: Option<String>) -> Self; }
impl Chat for HttpChat { /* POST {base_url}/chat/completions；401/429/5xx → HttpStatus；json_mode 4xx → 去 response_format 重试一次 */ }
```

- [ ] **Step 1: 失败测试**（本地 `TcpListener` mock，读请求断言）：成功解析 `choices[0].message.content`+`model`；Authorization Bearer 头带 key；body 含 `response_format`；401 → `HttpStatus{code:401}`；json_mode 收到 400 后重试（第二次无 response_format，返回 200 成功）；响应缺 content → `Parse`；1s 超时 → `Network`
- [ ] **Step 2: 实现**（mock helper：读到 `\r\n\r\n` 后按 Content-Length 补读 body；Connection: close）
- [ ] **Step 3: 测试通过 + clippy 干净**
- [ ] **Step 4: Commit** `feat(llm): OpenAI 兼容 chat 客户端（ureq+rustls，response_format 降级重试）`

### Task 4: ch-knowledge LlmExtractor

**Files:**
- Create: `crates/knowledge/src/llm.rs`
- Modify: `crates/knowledge/src/lib.rs`（导出）、`crates/knowledge/Cargo.toml`（+ ch-llm）

**Interfaces:**
- Consumes: `ch_llm::{Chat, ChatRequest, ChatReply, LlmError}`（Task 3）
- Produces:
```rust
pub const PROMPT_VERSION: &str = "prompt-v1";
pub struct LlmExtractor { /* chat: Arc<dyn Chat>, model_label: String, max_input_chars: usize */ }
impl LlmExtractor {
    pub fn new(chat: Arc<dyn Chat>, model_label: String, max_input_chars: usize) -> Self;
    pub fn extract(&self, input: &ExtractionInput) -> Result<ExtractionResult, LlmError>; // extractor="llm:{model}@prompt-v1"
}
```

- [ ] **Step 1: 失败测试**（MockChat 固定回复）：全字段 JSON + source `[1,2]` → 映射回真实 message id；```json 围栏 + 前后杂文 → 仍解析；垃圾文本 → `Parse`；越界 source（`[99]`）忽略；超限裁剪（>50 条截断）；commands 容错字符串形态；空 transcript → Ok 空结果；extractor 标识；转录截断（小 max_input_chars 保留头部+标注）
- [ ] **Step 2: 实现**：`build_transcript`（编号 `[n] role: text`，title 头部，char 截断）；SYSTEM_PROMPT 常量（严格 JSON schema + source 编号规则 + 不编造）；`extract_json_object`（首 `{` 到末 `}`）；宽松映射 + 上限
- [ ] **Step 3: `cargo test -p ch-knowledge` 全过（含既有规则引擎测试）**
- [ ] **Step 4: Commit** `feat(knowledge): LlmExtractor——LLM 提取引擎（与规则引擎同构输出）`

### Task 5: Tauri 命令层

**Files:**
- Create: `apps/desktop/src-tauri/src/commands/llm_cmd.rs`
- Modify: `commands/mod.rs`（+ `mod llm_cmd; pub(crate) use llm_cmd::*;`）、`commands/conversations.rs`（extract_knowledge + engine）、`lib.rs`（注册 3 个新命令）、`e2e_journeys.rs`（extract_knowledge 调用补 `None`）

**Interfaces:**
- Consumes: Task 1–4 全部
- Produces（前端契约）：
```rust
pub struct LlmConfigView { enabled, base_url, model, timeout_secs, max_input_chars, has_api_key, api_key_masked: Option<String>, is_local, api_key_broken }
pub struct LlmConfigInput { enabled, base_url, model, timeout_secs: Option<u64>, max_input_chars: Option<usize>, api_key: Option<String>, clear_api_key: bool }
// 命令：llm_config_get / llm_config_set(input) / llm_test_connection → {ok, latency_ms, model}
pub(crate) fn extract_with_llm(state: &DaemonState, input: &ExtractionInput) -> Result<ExtractionResult, String>; // 未启用/缺 Key → 中文引导文案
// extract_knowledge(state, conversation_id, engine: Option<String>)：None/"rule"→规则引擎；"llm"→LLM
```
- 持久化：`app_settings` 键 `llm_config`（JSON）；vault 用 `state.data_dir`；crypto 走 `run_blocking`

- [ ] **Step 1: 失败测试**（`#[cfg(test)]` in llm_cmd.rs；`DaemonState::open_in_memory` + `THREADOCK_NO_KEYCHAIN=1`）：config 默认 get（disabled）；set+密封 → view 带 masked、`api_key_sealed` 密文含 `v1.` 且不含明文；DB 落盘值不含明文；clear_api_key；http 非本地 set → Err；`extract_with_llm` 未启用 → 引导文案；`extract_knowledge(engine=None)` 走规则引擎（导入 fixture 会话）
- [ ] **Step 2: 实现 llm_cmd.rs + 改 extract_knowledge + 注册**
- [ ] **Step 3: `cargo test -p threadock`（src-tauri）通过；clippy 干净**
- [ ] **Step 4: Commit** `feat(desktop): LLM 配置/测试/提取命令（API Key 密封落库，前端零密钥）`

### Task 6: GUI

**Files:**
- Modify: `apps/desktop/src/types.ts`（+ LlmConfigView）、`SettingsView.tsx`（+ LlmSection：开关/base_url/model/预设 4 个/API Key 密码框+masked 回显+清除/测试连接/本地徽标/隐私提示/破损提示）、`KnowledgeModal.tsx`（头部引擎段选 ⚙规则|✨AI → `onReextract(engine)`；`engine?` 可选默认 "rule"；`llm:` extractor 徽标）、`App.tsx`（`knowledgeEngine` state；`extractKnowledge(engine)` 传参 invoke；传 props）

**Interfaces:**
- Consumes: Task 5 命令契约
- Produces: `LlmSection()`（SettingsView 内部组件，模式同 BackupSection）；`onReextract: (engine: "rule" | "llm") => void`（KnowledgeModal，旧 `() => void` 调用方兼容）

- [ ] **Step 1: types + LlmSection + 设置页挂载**（section 放「治理」与「WorkspaceSection」之间；预设：OpenAI `https://api.openai.com/v1`/gpt-4o-mini、DeepSeek `https://api.deepseek.com/v1`/deepseek-chat、GLM `https://open.bigmodel.cn/api/paas/v4`/glm-4-flash、Ollama 本地 `http://127.0.0.1:11434/v1`/qwen2.5:7b）
- [ ] **Step 2: KnowledgeModal 引擎切换 + App.tsx 接线**（切换即以新引擎重提取；失败 toast 沿用）
- [ ] **Step 3: `npm run lint` + `npx tsc --noEmit` + `npx vitest run` 全过**（round6/round7 旧测试兼容靠可选 prop）
- [ ] **Step 4: Commit** `feat(gui): 设置页 AI 提取（大模型）配置区 + 知识弹窗规则/AI 引擎切换`

### Task 7: 文档

**Files:**
- Modify: `docs/user-guide.md`（新节「AI 提取（大模型）」：开启步骤、预设、Key 加密原理、本地 Ollama 零外发、THREADOCK_NO_KEYCHAIN）、`docs/privacy.md`（LLM 数据流：默认关、发送范围=当前会话转录、截断、本地端点不出机、Key 密封）、`README.md`（能力表「知识提取」行补 LLM 引擎）、`CHANGELOG.md`（Unreleased/Added）

- [ ] **Step 1: 四份文档更新；Commit** `docs: LLM 提取用户指南与隐私说明`

### Task 8: 全量验证与收尾

- [ ] **Step 1: `scripts/precheck.sh test`**（fmt×2 + clippy×2 + tsc + eslint + workspace 测试 + src-tauri 测试 + 前端 vitest + 构建）
- [ ] **Step 2: 人工核对安全清单**（设计文档 §9 的 8 条逐条对照代码）
- [ ] **Step 3: 最终 Commit（如有修复）+ 总结**
