//! LLM 提取全链路旅程测试：真实 DaemonState + 本地 mock OpenAI 兼容服务。
//!
//! 与 `llm_cmd.rs` 的单元级测试互补，这里按真人用户旅程串联：
//! 配置保存（API Key 密封）→ 连接测试 → 导入会话 → engine="llm" 提取 →
//! 解析映射回真实消息 id；并断言发往端点的请求契约（Bearer 头、编号转录、
//! json_mode）。全程走 127.0.0.1 本地 mock，无外网依赖。

#![cfg(test)]

use crate::commands::*;
use ch_daemon::{DaemonState, DaemonStateConfig};
use std::io::{Read, Write as _};
use std::sync::{Arc, Mutex};

/// 捕获到的一次请求（用于断言 Authorization / 请求体契约）。
#[derive(Debug, Clone)]
struct Captured {
    path: String,
    authorization: Option<String>,
    body: serde_json::Value,
}

/// 一次性 mock OpenAI 服务：按队列依次应答，记录收到的每个请求。
/// 线程在队列耗尽后退出；残留 accept 线程随测试进程结束而终止。
struct MockLlm {
    addr: std::net::SocketAddr,
    captured: Arc<Mutex<Vec<Captured>>>,
}

impl MockLlm {
    /// `responses`：每个元素 `(状态行, 响应体)`，应答一个连接。
    fn start(responses: Vec<(String, String)>) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("mock addr");
        let captured: Arc<Mutex<Vec<Captured>>> = Arc::new(Mutex::new(Vec::new()));
        let cap = Arc::clone(&captured);
        std::thread::spawn(move || {
            for (status, body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let raw = read_request(&mut stream);
                let (head, rest) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
                let path = head
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .split(' ')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                let authorization = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Authorization: Bearer ").map(str::to_string));
                let parsed = serde_json::from_str(rest).unwrap_or(serde_json::Value::Null);
                cap.lock().expect("mutex poisoned").push(Captured {
                    path,
                    authorization,
                    body: parsed,
                });
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        Self { addr, captured }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn captured(&self) -> std::sync::MutexGuard<'_, Vec<Captured>> {
        self.captured.lock().expect("mutex poisoned")
    }
}

/// 读取一个完整 HTTP 请求（头 + Content-Length 体）——与 ch-llm client 测试同款。
fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let headers_end;
    loop {
        let n = stream.read(&mut chunk).expect("read request");
        assert!(n != 0, "client closed before sending full request");
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            headers_end = pos;
            break;
        }
    }
    let headers = String::from_utf8_lossy(&buf[..headers_end]).into_owned();
    let content_len: usize = headers
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().expect("content-length value"))
        })
        .unwrap_or(0);
    while buf.len() < headers_end + 4 + content_len {
        let n = stream.read(&mut chunk).expect("read body");
        assert!(n != 0, "client closed mid-body");
        buf.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// mock 模型返回的提取结果（source 引用转录编号）。
fn extraction_content() -> String {
    serde_json::json!({
        "summary": "讨论 Tauri 后台任务方案，结论是用 WorkManager",
        "decisions": [{"decision": "采用 WorkManager", "reason": "官方推荐", "source": [2]}],
        "todos": [{"text": "补充后台任务测试", "source": [1]}],
        "errors": [],
        "commands": ["cargo test"],
        "files": [{"path": "src/main.rs", "source": [1]}]
    })
    .to_string()
}

/// OpenAI 兼容的 chat/completions 成功响应体。
fn openai_body(content: &str) -> String {
    serde_json::json!({
        "model": "mock-1",
        "choices": [{"message": {"role": "assistant", "content": content}}]
    })
    .to_string()
}

/// mock app + 临时目录真实后端。
fn setup() -> (tauri::App<tauri::test::MockRuntime>, tempfile::TempDir) {
    let app = tauri::test::mock_app();
    let dir = tempfile::TempDir::new().expect("tempdir");
    use tauri::Manager as _;
    let state = DaemonState::open(DaemonStateConfig {
        data_dir: dir.path().to_path_buf(),
        ..Default::default()
    })
    .expect("open state");
    app.manage(state);
    (app, dir)
}

/// 导入一个最小 markdown 会话，返回会话 id。
fn import_sample(state: &tauri::State<'_, DaemonState>, dir: &std::path::Path) -> String {
    let md = dir.join("s.md");
    std::fs::write(
        &md,
        "# 测试会话\n\n## User\n\n你好\n\n## Assistant\n\n决定用 WorkManager 处理后台任务\n",
    )
    .expect("file I/O failed");
    let imported = tauri::async_runtime::block_on(import_file(
        state.clone(),
        md.to_string_lossy().into_owned(),
        None,
    ))
    .expect("import sample conversation");
    imported.conversation_id
}

/// 旅程：配置（密封 Key）→ 连接测试 → AI 提取 → 结果映射 + 请求契约。
#[test]
fn journey_llm_extraction_full_chain() {
    let mock = MockLlm::start(vec![
        ("200 OK".into(), openai_body("ok")),
        ("200 OK".into(), openai_body(&extraction_content())),
    ]);
    let (app, dir) = setup();
    use tauri::Manager as _;
    let state = app.state::<DaemonState>();

    // 1. 保存配置：本地端点 + API Key（走密封存储）
    let view = tauri::async_runtime::block_on(llm_config_set(
        state.clone(),
        LlmConfigInput {
            enabled: true,
            base_url: mock.base_url(),
            model: "mock-1".into(),
            api_key: Some("sk-journey-key-123456".into()),
            ..Default::default()
        },
    ))
    .expect("set llm config");
    assert!(view.enabled);
    assert!(view.has_api_key);
    assert_eq!(view.api_key_masked.as_deref(), Some("sk-***3456"));
    assert!(view.is_local, "127.0.0.1 端点应识别为本地");
    assert!(!view.api_key_broken);

    // 2. 连接测试：发往 mock，返回延迟与模型名
    let probe = tauri::async_runtime::block_on(llm_test_connection(state.clone()))
        .expect("test connection");
    assert_eq!(probe["ok"], serde_json::json!(true));
    assert_eq!(probe["model"], serde_json::json!("mock-1"));
    {
        let cap = mock.captured();
        let c0 = &cap[0];
        assert_eq!(c0.path, "/chat/completions");
        assert_eq!(
            c0.authorization.as_deref(),
            Some("sk-journey-key-123456"),
            "解密后的 Key 必须随 Bearer 头发送（密封→解密链路贯通）"
        );
        assert!(
            c0.body.get("response_format").is_none(),
            "连接测试不带 json_mode"
        );
        assert_eq!(c0.body["model"], "mock-1");
    }

    // 3. AI 引擎提取
    let conv_id = import_sample(&state, dir.path());
    let k = tauri::async_runtime::block_on(extract_knowledge(
        state.clone(),
        conv_id.clone(),
        Some("llm".into()),
    ))
    .expect("llm extraction");
    assert_eq!(k.extractor, "llm:mock-1@prompt-v1");
    assert!(k.summary.contains("WorkManager"), "摘要应来自模型输出");
    assert_eq!(k.decisions.len(), 1);
    assert_eq!(k.decisions[0].decision, "采用 WorkManager");
    assert_eq!(k.decisions[0].reason.as_deref(), Some("官方推荐"));
    assert_eq!(k.commands, vec!["cargo test".to_string()]);

    // 4. source 编号必须映射回库里的真实消息 id
    {
        let repo = state.read_repo.lock().expect("mutex poisoned");
        let ids: Vec<String> = repo
            .list_messages(&conv_id)
            .expect("SQL execution failed")
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(k.decisions[0].source_message_ids.len(), 1);
        assert!(
            ids.contains(&k.decisions[0].source_message_ids[0]),
            "决策 source[2] 应映射到真实 assistant 消息"
        );
        assert!(
            ids.contains(&k.todos[0].source_message_ids[0]),
            "TODO source[1] 应映射到真实 user 消息"
        );
        assert_eq!(k.files[0].source_message_ids.len(), 1);
    }

    // 5. 发往端点的请求契约：Bearer + json_mode + 编号转录
    {
        let cap = mock.captured();
        let c1 = &cap[1];
        assert_eq!(c1.authorization.as_deref(), Some("sk-journey-key-123456"));
        assert_eq!(c1.body["response_format"]["type"], "json_object");
        assert_eq!(c1.body["model"], "mock-1");
        let user_msg = c1.body["messages"][1]["content"]
            .as_str()
            .unwrap_or_default();
        assert!(
            user_msg.contains("[1] user: 你好"),
            "编号转录 user：{user_msg}"
        );
        assert!(
            user_msg.contains("[2] assistant: 决定用 WorkManager"),
            "编号转录 assistant：{user_msg}"
        );
        assert!(
            user_msg.contains("测试会话"),
            "会话标题应进入转录：{user_msg}"
        );
    }
}

/// 旅程：服务端 401 → 错误透传给前端（且 401 不触发降级重试）。
#[test]
fn journey_llm_extraction_http_error_surfaces() {
    let mock = MockLlm::start(vec![(
        "401 Unauthorized".into(),
        r#"{"error":"invalid api key"}"#.into(),
    )]);
    let (app, dir) = setup();
    use tauri::Manager as _;
    let state = app.state::<DaemonState>();

    tauri::async_runtime::block_on(llm_config_set(
        state.clone(),
        LlmConfigInput {
            enabled: true,
            base_url: mock.base_url(),
            model: "mock-1".into(),
            ..Default::default()
        },
    ))
    .expect("set llm config");

    let conv_id = import_sample(&state, dir.path());
    let err = tauri::async_runtime::block_on(extract_knowledge(
        state.clone(),
        conv_id,
        Some("llm".into()),
    ))
    .expect_err("401 must fail");
    assert!(err.contains("401"), "错误应含状态码：{err}");
    assert_eq!(
        mock.captured().len(),
        1,
        "401 不应触发 response_format 降级重试"
    );
}

/// 旅程：模型返回非 JSON → 解析错误（不 panic、不落脏数据）。
#[test]
fn journey_llm_extraction_garbage_reply_is_parse_error() {
    let mock = MockLlm::start(vec![(
        "200 OK".into(),
        openai_body("抱歉，我无法完成这个任务。"),
    )]);
    let (app, dir) = setup();
    use tauri::Manager as _;
    let state = app.state::<DaemonState>();

    tauri::async_runtime::block_on(llm_config_set(
        state.clone(),
        LlmConfigInput {
            enabled: true,
            base_url: mock.base_url(),
            model: "mock-1".into(),
            ..Default::default()
        },
    ))
    .expect("set llm config");

    let conv_id = import_sample(&state, dir.path());
    let err = tauri::async_runtime::block_on(extract_knowledge(
        state.clone(),
        conv_id,
        Some("llm".into()),
    ))
    .expect_err("garbage must fail");
    assert!(
        err.contains("解析") || err.contains("JSON"),
        "应为解析错误：{err}"
    );
}
