//! LLM 提取配置域（plan §13.5 / §14.3）：API Key 密封存储 + 连接测试 + LLM 引擎提取。
//!
//! 安全要点：
//! - 前端只拿 [`LlmConfigView`]（masked 密钥提示，无明文/密文）。
//! - `api_key_sealed` 密文存 `app_settings.llm_config`；主密钥为数据目录下
//!   0600 密钥文件（见 [`ch_llm::SecretVault`]），数据库泄露不泄密。
//! - 加密/解密/网络都在 `run_blocking` 中执行，不占 tokio worker。

use super::*;
use ch_daemon::DaemonState;
use ch_llm::{LlmConfig, SecretVault};

/// app_settings 中的 LLM 配置键。
const LLM_CONFIG_KEY: &str = "llm_config";

/// 前端可见视图：永远不含密钥（明文或密文）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LlmConfigView {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub timeout_secs: u64,
    pub max_input_chars: usize,
    pub has_api_key: bool,
    /// 打码提示（如 `sk-***1234`）；密文损坏时为 None。
    pub api_key_masked: Option<String>,
    pub is_local: bool,
    /// 密文存在但解不开（如更换设备后主密钥不同）：提示重新录入。
    pub api_key_broken: bool,
}

/// 配置写入入参：`api_key` 提供则密封替换；`clear_api_key` 优先清除。
#[derive(Debug, Default, Clone, serde::Deserialize)]
pub struct LlmConfigInput {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub max_input_chars: Option<usize>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

/// 读取 LLM 配置（无则默认：关闭、空端点）。
pub(crate) fn load_llm_config(state: &DaemonState) -> Result<LlmConfig, String> {
    let repo = state.read_repo.lock().map_err(|e| storage_err(e))?;
    match repo
        .get_setting(LLM_CONFIG_KEY)
        .map_err(|e| storage_err(e))?
    {
        Some(json) => serde_json::from_str(&json).map_err(|e| {
            AppError::invalid("LLM 配置损坏")
                .with_detail(e.to_string())
                .to_string()
        }),
        None => Ok(LlmConfig::default()),
    }
}

/// 解密 API Key（调用期内存态；错误提示不含密钥材料）。
pub(crate) fn decrypt_api_key(
    state: &DaemonState,
    cfg: &LlmConfig,
) -> Result<Option<String>, String> {
    match &cfg.api_key_sealed {
        Some(sealed) => {
            let vault = SecretVault::open(&state.data_dir).map_err(|e| e.to_string())?;
            vault
                .open_sealed(sealed)
                .map(Some)
                .map_err(|_| "API Key 解密失败（可能更换过设备）：请到设置重新录入".to_string())
        }
        None => Ok(None),
    }
}

/// 视图化：masked/本地判定/破损标记。解密只为打码，明文即刻丢弃。
fn view_of(state: &DaemonState, cfg: &LlmConfig) -> LlmConfigView {
    let (masked, broken) = match &cfg.api_key_sealed {
        Some(sealed) => match SecretVault::open(&state.data_dir) {
            Ok(vault) => match vault.open_sealed(sealed) {
                Ok(key) => (Some(ch_llm::mask_key(&key)), false),
                Err(_) => (None, true),
            },
            Err(_) => (None, true),
        },
        None => (None, false),
    };
    LlmConfigView {
        enabled: cfg.enabled,
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        timeout_secs: cfg.timeout_secs,
        max_input_chars: cfg.max_input_chars,
        has_api_key: cfg.api_key_sealed.is_some(),
        api_key_masked: masked,
        is_local: cfg.is_local_endpoint(),
        api_key_broken: broken,
    }
}

/// 读取 LLM 配置视图（无密钥材料）。
#[tauri::command]
pub(crate) async fn llm_config_get(
    state: tauri::State<'_, DaemonState>,
) -> Result<LlmConfigView, String> {
    run_blocking(|| {
        let cfg = load_llm_config(&state)?;
        Ok(view_of(&state, &cfg))
    })
}

/// 保存 LLM 配置：非敏感字段直接覆盖；API Key 密封替换/清除/保持。
#[tauri::command]
pub(crate) async fn llm_config_set(
    state: tauri::State<'_, DaemonState>,
    input: LlmConfigInput,
) -> Result<LlmConfigView, String> {
    run_blocking(|| {
        let mut cfg = load_llm_config(&state)?;
        cfg.enabled = input.enabled;
        cfg.base_url = input.base_url;
        cfg.model = input.model;
        if let Some(t) = input.timeout_secs {
            cfg.timeout_secs = t;
        }
        if let Some(m) = input.max_input_chars {
            cfg.max_input_chars = m;
        }
        if input.clear_api_key {
            cfg.api_key_sealed = None;
        } else if let Some(key) = input
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            let vault = SecretVault::open(&state.data_dir).map_err(|e| e.to_string())?;
            cfg.api_key_sealed = Some(vault.seal(key).map_err(|e| e.to_string())?);
        }
        cfg.validate()
            .map_err(|e| AppError::invalid(e.to_string()).to_string())?;
        let json = serde_json::to_string(&cfg).map_err(|e| io_err(e))?;
        {
            let repo = state.repo.lock().map_err(|e| storage_err(e))?;
            repo.set_setting(LLM_CONFIG_KEY, &json)
                .map_err(|e| storage_err(e))?;
        }
        Ok(view_of(&state, &cfg))
    })
}

/// 连接测试：最小 chat 请求，返回延迟与实际模型名（快速失败，超时压到 ≤30s）。
#[tauri::command]
pub(crate) async fn llm_test_connection(
    state: tauri::State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    run_blocking(|| {
        let cfg = load_llm_config(&state)?;
        if cfg.base_url.trim().is_empty() || cfg.model.trim().is_empty() {
            return Err(AppError::invalid("请先填写并保存 base_url 与 model").to_string());
        }
        let mut probe = cfg.clone();
        probe.timeout_secs = probe.timeout_secs.clamp(1, 30);
        let api_key = decrypt_api_key(&state, &cfg)?;
        let chat = ch_llm::HttpChat::new(&probe, api_key);
        let started = std::time::Instant::now();
        let reply = ch_llm::Chat::chat(
            &chat,
            &ch_llm::ChatRequest {
                system: "You are a connectivity probe.".to_string(),
                user: "Reply with exactly: ok".to_string(),
                // 1024 而非 8：思考型模型思考 token 计入额度，小预算会只出
                // reasoning 无正文（GLM glm-5+ 为始终思考模型，无法关闭）
                max_tokens: 1024,
                json_mode: false,
            },
        )
        .map_err(|e| match e.detail() {
            Some(d) if !d.is_empty() => format!("{e}（{d}）"),
            _ => e.to_string(),
        })?;
        Ok(serde_json::json!({
            "ok": true,
            "latency_ms": started.elapsed().as_millis() as u64,
            "model": reply.model,
        }))
    })
}

/// LLM 引擎知识提取（conversations::extract_knowledge 的 engine="llm" 路径）。
/// 未启用/配置不完整给出可操作中文引导。
pub(crate) fn extract_with_llm(
    state: &DaemonState,
    input: &ch_knowledge::ExtractionInput,
) -> Result<ch_knowledge::ExtractionResult, String> {
    let cfg = load_llm_config(state)?;
    if !cfg.enabled {
        return Err("LLM 提取未启用：请在 设置 → AI 提取（大模型）中开启并配置".to_string());
    }
    if !cfg.is_ready(cfg.api_key_sealed.is_some()) {
        return Err("LLM 配置不完整：云端端点需要 API Key（设置 → AI 提取）".to_string());
    }
    let api_key = decrypt_api_key(state, &cfg)?;
    // 提取是长任务：思考型模型（GLM glm-5+ 实测最长会话 50s）+ 长 JSON 输出，
    // 默认 60s 极易超时——下限提到 180s（用户配置更大则尊重，上限 300 由 clamp 保证）
    let mut long_run = cfg.clone();
    long_run.timeout_secs = cfg.timeout_secs.max(180);
    let chat = std::sync::Arc::new(ch_llm::HttpChat::new(&long_run, api_key));
    let extractor = ch_knowledge::LlmExtractor::new(chat, cfg.model.clone(), cfg.max_input_chars);
    extractor.extract(input).map_err(|e| match e.detail() {
        Some(d) if !d.is_empty() => format!("{e}（{d}）"),
        _ => e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager as _;

    fn harness() -> (tauri::App<tauri::test::MockRuntime>, DaemonStateGuard) {
        let app = tauri::test::mock_app();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let state = ch_daemon::DaemonState::open(ch_daemon::DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        })
        .expect("open state");
        app.manage(state);
        (app, DaemonStateGuard { _dir: dir })
    }

    /// 占住 tempdir 生命周期。
    struct DaemonStateGuard {
        _dir: tempfile::TempDir,
    }

    fn input_of(v: &LlmConfigView) -> LlmConfigInput {
        LlmConfigInput {
            enabled: v.enabled,
            base_url: v.base_url.clone(),
            model: v.model.clone(),
            timeout_secs: Some(v.timeout_secs),
            max_input_chars: Some(v.max_input_chars),
            api_key: None,
            clear_api_key: false,
        }
    }

    #[test]
    fn config_get_defaults_disabled() {
        let (app, _guard) = harness();
        let state = app.state::<DaemonState>();
        let v = tauri::async_runtime::block_on(llm_config_get(state.clone()))
            .expect("get default config");
        assert!(!v.enabled);
        assert!(!v.has_api_key);
        assert!(!v.is_local);
    }

    #[test]
    fn config_set_seals_key_and_view_masks() {
        let (app, _guard) = harness();
        let state = app.state::<DaemonState>();
        let v = tauri::async_runtime::block_on(llm_config_set(
            state.clone(),
            LlmConfigInput {
                enabled: true,
                base_url: "https://api.openai.com/v1".into(),
                model: "gpt-4o-mini".into(),
                api_key: Some("sk-SECRET-12345678".into()),
                ..Default::default()
            },
        ))
        .expect("set config");
        assert!(v.enabled);
        assert!(v.has_api_key);
        assert_eq!(v.api_key_masked.as_deref(), Some("sk-***5678"));
        assert!(!v.api_key_broken);

        // 落库值：密文含 v1. 前缀，绝无明文
        let repo = state.read_repo.lock().expect("mutex poisoned");
        let json = repo
            .get_setting(LLM_CONFIG_KEY)
            .expect("SQL execution failed")
            .expect("config saved");
        assert!(json.contains("v1."), "密文应带版本前缀");
        assert!(!json.contains("SECRET"), "明文不得落库：{json}");

        // 再次 set 不带 key：保持已存密文
        drop(repo);
        let keep = tauri::async_runtime::block_on(llm_config_set(state.clone(), input_of(&v)))
            .expect("keep key");
        assert!(keep.has_api_key, "未提供 key 时应保持现有密文");

        // 清除
        let cleared = tauri::async_runtime::block_on(llm_config_set(
            state.clone(),
            LlmConfigInput {
                clear_api_key: true,
                ..input_of(&v)
            },
        ))
        .expect("clear key");
        assert!(!cleared.has_api_key);
        assert!(cleared.api_key_masked.is_none());
    }

    #[test]
    fn config_set_rejects_remote_plain_http() {
        let (app, _guard) = harness();
        let state = app.state::<DaemonState>();
        let err = tauri::async_runtime::block_on(llm_config_set(
            state.clone(),
            LlmConfigInput {
                enabled: true,
                base_url: "http://api.example.com/v1".into(),
                model: "m".into(),
                ..Default::default()
            },
        ))
        .expect_err("remote http must be rejected");
        assert!(err.contains("https"), "提示应说明需 https：{err}");
    }

    #[test]
    fn extract_llm_disabled_gives_guidance() {
        let (app, _guard) = harness();
        let state = app.state::<DaemonState>();
        let input = ch_knowledge::ExtractionInput {
            title: None,
            messages: vec![],
            events: vec![],
        };
        let err = extract_with_llm(&state, &input).expect_err("disabled");
        assert!(err.contains("设置"), "应引导到设置：{err}");
    }

    #[test]
    fn stored_knowledge_roundtrip() {
        // 弹窗秒开链路：手动提取统一落库 → knowledge_get_stored 读回同结果
        let (app, _guard) = harness();
        let state = app.state::<DaemonState>();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let md = dir.path().join("s.md");
        std::fs::write(
            &md,
            "# 测试会话\n\n## User\n\n需要修复登录\n\n## Assistant\n\n登录已修复\n",
        )
        .expect("write failed");
        let imported = tauri::async_runtime::block_on(import_file(
            state.clone(),
            md.to_string_lossy().into_owned(),
            None,
        ))
        .expect("import");

        // 无存档 → None
        assert!(tauri::async_runtime::block_on(knowledge_get_stored(
            state.clone(),
            imported.conversation_id.clone(),
        ))
        .expect("query")
        .is_none());

        // 规则提取 → 落库 → 存档可读回且一致
        let k = tauri::async_runtime::block_on(extract_knowledge_with(
            &state.clone(),
            None,
            imported.conversation_id.clone(),
            None,
        ))
        .expect("extract");
        assert_eq!(k.extractor, ch_knowledge::RULE_EXTRACTOR);
        let stored = tauri::async_runtime::block_on(knowledge_get_stored(
            state.clone(),
            imported.conversation_id.clone(),
        ))
        .expect("query 2")
        .expect("saved after extract");
        assert_eq!(stored.extractor, ch_knowledge::RULE_EXTRACTOR);
        assert_eq!(stored.version, 1);
        assert!(stored.extracted_at_ms > 0);
        assert_eq!(stored.result, k, "存档必须与提取结果一致");
    }

    #[test]
    fn extract_knowledge_defaults_to_rule_engine() {
        let (app, _guard) = harness();
        let state = app.state::<DaemonState>();
        // 导入一个最小 markdown 会话
        let dir = tempfile::TempDir::new().expect("tempdir");
        let md = dir.path().join("s.md");
        std::fs::write(
            &md,
            "# 测试会话\n\n## User\n\n你好\n\n## Assistant\n\n世界\n",
        )
        .expect("file I/O failed");
        let imported = tauri::async_runtime::block_on(import_file(
            state.clone(),
            md.to_string_lossy().into_owned(),
            None,
        ))
        .expect("import");

        // engine=None → 规则引擎
        let k = tauri::async_runtime::block_on(extract_knowledge_with(
            &state.clone(),
            None,
            imported.conversation_id.clone(),
            None,
        ))
        .expect("rule extract");
        assert_eq!(k.extractor, ch_knowledge::RULE_EXTRACTOR);

        // engine=llm 未启用 → 引导错误（不触发网络），但失败运行仍留痕
        let err = tauri::async_runtime::block_on(extract_knowledge_with(
            &state.clone(),
            None,
            imported.conversation_id.clone(),
            Some("llm".into()),
        ))
        .expect_err("llm disabled");
        assert!(err.contains("LLM"), "{err}");
        let runs = state
            .read_repo
            .lock()
            .expect("mutex poisoned")
            .list_llm_runs(&imported.conversation_id, 5)
            .expect("list runs");
        assert_eq!(runs.len(), 1, "失败也要留痕：{runs:?}");
        assert_eq!(runs[0].status, "failed");
        assert!(runs[0].error.as_deref().is_some_and(|e| e.contains("LLM")));
    }

    /// 真实配置端到端（在**副本库**上）：先用已存（误配的 Anthropic）端点复现报错，
    /// 再改 OpenAI 兼容端点实测连通。密钥仅内存解密用于调用，绝不打印。
    /// cargo test --lib real_glm_end_to_end -- --ignored --nocapture
    #[test]
    #[ignore = "读本机真实配置与密钥（副本），并真实调用所配端点"]
    fn real_glm_end_to_end() {
        let app_dir = std::path::PathBuf::from(std::env::var("HOME").expect("no HOME"))
            .join("Library/Application Support/com.threadock.desktop");
        assert!(app_dir.join("threadock.db").exists(), "本机无真实 app 数据");
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        std::fs::copy(
            app_dir.join("threadock.db"),
            dir.path().join("threadock.db"),
        )
        .expect("copy db failed");
        let keys_src = app_dir.join("keys/llm-master.key");
        if keys_src.exists() {
            std::fs::create_dir_all(dir.path().join("keys")).expect("mkdir keys failed");
            std::fs::copy(&keys_src, dir.path().join("keys/llm-master.key"))
                .expect("copy master key failed");
        }
        let state = ch_daemon::DaemonState::open(ch_daemon::DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        })
        .expect("state open");
        let app = tauri::test::mock_app();
        app.manage(state);
        let state = app.state::<DaemonState>();

        // 1) 已存配置探测：成功直接报告；失败则必须是可懂原因（不再是 missing field）
        let saved = tauri::async_runtime::block_on(llm_config_get(state.clone())).expect("get");
        println!(
            "已存配置：base_url={} model={} has_key={}",
            saved.base_url, saved.model, saved.has_api_key
        );
        if !saved.base_url.is_empty() && !saved.model.is_empty() {
            match tauri::async_runtime::block_on(llm_test_connection(state.clone())) {
                Ok(r) => println!(
                    "✓ 已存端点连通：model={} latency={}ms",
                    r.get("model")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("?"),
                    r.get("latency_ms")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                ),
                Err(e) => {
                    println!("已存端点探测：{e}");
                    assert!(!e.contains("missing field"), "不得再抛无信息 schema 错误");
                }
            }
        }

        // 2) 副本上改为 OpenAI 兼容端点（保留原模型与已存密文），实测连通
        let fixed = tauri::async_runtime::block_on(llm_config_set(
            state.clone(),
            LlmConfigInput {
                enabled: true,
                base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
                model: saved.model.clone(),
                ..Default::default()
            },
        ))
        .expect("set fixed config");
        assert_eq!(fixed.has_api_key, saved.has_api_key, "密文保持不变");
        // 按量端点（paas/v4）探测：账户余额状态与代码正确性无关，只报告不判定
        match tauri::async_runtime::block_on(llm_test_connection(state.clone())) {
            Ok(r) => println!(
                "✓ paas/v4 连接成功：model={} latency={}ms",
                r.get("model")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?"),
                r.get("latency_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
            ),
            Err(e) => {
                println!("✗ paas/v4 探测：{e}");
                assert!(!e.contains("missing field"), "不得再抛无信息 schema 错误");
            }
        }
    }

    /// 真实 AI 提取端到端（副本库）：用已存配置对消息最多的会话跑一次 LLM 提取，
    /// 捕获实际错误（超时/截断/解析）。密钥仅内存解密，绝不打印。
    /// cargo test --lib real_glm_extract -- --ignored --nocapture
    /// xref 探针：弹窗挂载即查的跨会话引用（12 关键词全库 FS）最坏耗时。
    /// cargo test --lib probe_xref -- --ignored --nocapture
    #[test]
    #[ignore = "读本机真实库副本"]
    fn probe_xref() {
        use std::time::Instant;
        let app_dir = std::path::PathBuf::from(std::env::var("HOME").expect("no HOME"))
            .join("Library/Application Support/com.threadock.desktop");
        assert!(app_dir.join("threadock.db").exists(), "无真实数据");
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::copy(
            app_dir.join("threadock.db"),
            dir.path().join("threadock.db"),
        )
        .expect("copy");
        let state = ch_daemon::DaemonState::open(ch_daemon::DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        })
        .expect("open");
        let app = tauri::test::mock_app();
        app.manage(state);
        let state = app.state::<DaemonState>();
        let convs = {
            let repo = state.read_repo.lock().expect("mutex");
            repo.list_conversations(None).expect("convs")
        };
        let keywords: Vec<super::KnowledgeXrefKeyword> = (0..12)
            .map(|i| super::KnowledgeXrefKeyword {
                text: format!("test{i}"),
                kind: "file".into(),
            })
            .collect();
        let mut worst = std::time::Duration::ZERO;
        for c in convs.iter().take(200) {
            let t = Instant::now();
            let _ = tauri::async_runtime::block_on(super::knowledge_xref(
                state.clone(),
                c.id.clone(),
                keywords.clone(),
            ));
            worst = worst.max(t.elapsed());
        }
        println!("xref（12 关键词）最坏 {:?}", worst);
    }

    /// 秒开链路耗时探针（副本库）：knowledge_get_stored + llm_extract_status
    /// 在全部会话上的最坏耗时——弹窗打开前的两次读。
    /// cargo test --lib probe_open_latency -- --ignored --nocapture
    #[test]
    #[ignore = "读本机真实库副本"]
    fn probe_open_latency() {
        use std::time::Instant;
        let app_dir = std::path::PathBuf::from(std::env::var("HOME").expect("no HOME"))
            .join("Library/Application Support/com.threadock.desktop");
        assert!(app_dir.join("threadock.db").exists(), "无真实数据");
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::copy(
            app_dir.join("threadock.db"),
            dir.path().join("threadock.db"),
        )
        .expect("copy");
        let state = ch_daemon::DaemonState::open(ch_daemon::DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        })
        .expect("open");
        let app = tauri::test::mock_app();
        app.manage(state);
        let state = app.state::<DaemonState>();

        let convs = {
            let repo = state.read_repo.lock().expect("mutex");
            repo.list_conversations(None).expect("convs")
        };
        let mut worst_stored = std::time::Duration::ZERO;
        let mut worst_status = std::time::Duration::ZERO;
        let mut with_stored = 0usize;
        let mut deser_fail = 0usize;
        for c in &convs {
            let t = Instant::now();
            let r =
                tauri::async_runtime::block_on(knowledge_get_stored(state.clone(), c.id.clone()));
            worst_stored = worst_stored.max(t.elapsed());
            match &r {
                Ok(Some(sk)) => {
                    with_stored += 1;
                    // 内容非空校验：至少一类有条目（防「存档在但内容空」的展示问题）
                    let n = sk.result.decisions.len()
                        + sk.result.todos.len()
                        + sk.result.errors.len()
                        + sk.result.commands.len()
                        + sk.result.files.len()
                        + if sk.result.summary.is_empty() { 0 } else { 1 };
                    if n == 0 {
                        println!(
                            "  空存档：{}（extractor={}）",
                            c.effective_title(),
                            sk.extractor
                        );
                    }
                }
                Err(e) => {
                    deser_fail += 1;
                    if deser_fail <= 3 {
                        println!("  反序列化失败：{} → {e}", c.effective_title());
                    }
                }
                _ => {}
            }
            let t = Instant::now();
            let _ = tauri::async_runtime::block_on(llm_extract_status(state.clone(), c.id.clone()));
            worst_status = worst_status.max(t.elapsed());
        }
        println!("反序列化失败 {} 个", deser_fail);
        println!(
            "会话 {} 个（有存档 {} · 反序列化失败 {}）· stored 最坏 {:?} · status 最坏 {:?}",
            convs.len(),
            with_stored,
            deser_fail,
            worst_stored,
            worst_status
        );
    }

    #[test]
    #[ignore = "读本机真实配置与密钥（副本），并真实调用所配端点"]
    fn real_glm_extract() {
        let app_dir = std::path::PathBuf::from(std::env::var("HOME").expect("no HOME"))
            .join("Library/Application Support/com.threadock.desktop");
        assert!(app_dir.join("threadock.db").exists(), "本机无真实 app 数据");
        let dir = tempfile::TempDir::new().expect("tempdir creation failed");
        std::fs::copy(
            app_dir.join("threadock.db"),
            dir.path().join("threadock.db"),
        )
        .expect("copy db failed");
        let keys_src = app_dir.join("keys/llm-master.key");
        if keys_src.exists() {
            std::fs::create_dir_all(dir.path().join("keys")).expect("mkdir keys failed");
            std::fs::copy(&keys_src, dir.path().join("keys/llm-master.key"))
                .expect("copy master key failed");
        }
        let state = ch_daemon::DaemonState::open(ch_daemon::DaemonStateConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        })
        .expect("state open");
        let app = tauri::test::mock_app();
        app.manage(state);
        let state = app.state::<DaemonState>();

        // 挑正文最长的会话（最难场景：输入最长）
        let (conv_id, n_msgs, total_chars, title, msgs, events) = {
            let repo = state.read_repo.lock().expect("mutex poisoned");
            let convs = repo.list_conversations(None).expect("list convs");
            let mut best: Option<(usize, String)> = None;
            for c in &convs {
                let n = repo
                    .list_messages(&c.id)
                    .map(|m| {
                        m.iter()
                            .filter_map(|x| x.content_text.as_ref())
                            .map(|t| t.chars().count())
                            .sum::<usize>()
                    })
                    .unwrap_or(0);
                if n > best.as_ref().map_or(0, |(bn, _)| *bn) {
                    best = Some((n, c.id.clone()));
                }
            }
            let (chars, id) = best.expect("no conversations");
            let conv = repo.get_conversation(&id).expect("get conv").expect("conv");
            let msgs = repo.list_messages(&id).expect("list msgs");
            let events = repo.list_events(&id).unwrap_or_default();
            (
                id,
                msgs.len(),
                chars,
                conv.effective_title().to_string(),
                msgs,
                events,
            )
        };
        println!("目标会话：{title}（{n_msgs} 条消息，正文 {total_chars} 字符）");
        let input = ch_knowledge::ExtractionInput {
            title: Some(title),
            messages: msgs,
            events,
        };
        let started = std::time::Instant::now();
        let outcome = super::llm_cmd::extract_with_llm(&state, &input);
        let elapsed = started.elapsed().as_secs();
        match &outcome {
            Ok(r) => println!(
                "✓ AI 提取成功（{elapsed}s）：summary {} 字 · decisions {} · todos {} · errors {} · commands {} · files {}",
                r.summary.chars().count(),
                r.decisions.len(),
                r.todos.len(),
                r.errors.len(),
                r.commands.len(),
                r.files.len(),
            ),
            Err(e) => println!("✗ AI 提取失败（{elapsed}s）：{e}"),
        }
        if let Ok(r) = outcome {
            // 落库断言：模拟 extract_knowledge 的 llm 分支保存行为
            let json = serde_json::to_string(&r).expect("serialize");
            {
                let repo = state.repo.lock().expect("mutex poisoned");
                let _ = repo.save_knowledge(&conv_id, &r.extractor, &json);
                let rec = repo.get_knowledge(&conv_id).expect("query").expect("saved");
                println!(
                    "✓ 已落库：extractor={} version={}",
                    rec.extractor, rec.version
                );
                assert!(
                    rec.extractor.starts_with("llm:"),
                    "AI 结果必须以 llm: 版本落库，实际 {}",
                    rec.extractor
                );
            }
        }
    }
}
