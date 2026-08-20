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
                max_tokens: 8,
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
    let chat = std::sync::Arc::new(ch_llm::HttpChat::new(&cfg, api_key));
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
        let k = tauri::async_runtime::block_on(extract_knowledge(
            state.clone(),
            imported.conversation_id.clone(),
            None,
        ))
        .expect("rule extract");
        assert_eq!(k.extractor, "rule-v1");

        // engine=llm 未启用 → 引导错误（不触发网络）
        let err = tauri::async_runtime::block_on(extract_knowledge(
            state.clone(),
            imported.conversation_id.clone(),
            Some("llm".into()),
        ))
        .expect_err("llm disabled");
        assert!(err.contains("LLM"), "{err}");
    }
}
