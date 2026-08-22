//! OpenAI 兼容 `chat/completions` 客户端（ureq + rustls，无 openssl 依赖）。
//!
//! - 传输抽象为 [`Chat`] trait：生产 [`HttpChat`]，测试用 mock（无外网）。
//! - `json_mode` 默认随请求发送 `response_format: {"type":"json_object"}`；
//!   部分 OpenAI 兼容服务（旧版本地推理等）不支持该字段并返回 4xx——
//!   此时**去掉该字段重试一次**再判失败。
//! - `base_url` 兼容两种填法：到版本根（`…/v4`，自动拼 `/chat/completions`）
//!   或完整端点（已含 `/chat/completions` 则原样使用，不重复拼接）。
//! - 响应体**先读原文再解析**：GLM 等网关常以 HTTP 200 + `{"error":{…}}`
//!   报告认证/模型名/余额错误，此时提取 code/message 给出真实原因，
//!   而不是抛 `missing field choices`。
//! - 错误分类见 [`LlmError`]：网络 / HTTP 状态码（401 认证、429 限流、
//!   5xx 服务端）/ 响应解析。错误信息不含凭据。

use std::time::Duration;

use serde::Deserialize;

use crate::{LlmConfig, LlmError};

/// 一次对话请求。
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
    pub json_mode: bool,
}

/// 一次对话回复。
#[derive(Debug, Clone)]
pub struct ChatReply {
    pub content: String,
    pub model: String,
}

/// 传输抽象：生产 ureq 实现，测试 mock。
pub trait Chat: Send + Sync {
    /// # Errors
    /// 网络/HTTP/解析错误，见 [`LlmError`]。
    fn chat(&self, req: &ChatRequest) -> Result<ChatReply, LlmError>;
}

/// 生产实现：OpenAI 兼容 HTTP 客户端。
pub struct HttpChat {
    base_url: String,
    model: String,
    api_key: Option<String>,
    agent: ureq::Agent,
}

impl HttpChat {
    /// 从配置构造（API Key 由调用方解密后注入，本结构不持久化）。
    #[must_use]
    pub fn new(config: &LlmConfig, api_key: Option<String>) -> Self {
        let timeout = Duration::from_secs(config.timeout_secs.clamp(1, 300));
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        Self {
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model.trim().to_string(),
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            agent,
        }
    }
}

impl Chat for HttpChat {
    fn chat(&self, req: &ChatRequest) -> Result<ChatReply, LlmError> {
        let url = chat_url(&self.base_url);
        let body = self.build_body(req);
        let mut result = self.post(&url, &body);
        if let Err(LlmError::HttpStatus {
            code: 400 | 422,
            detail,
        }) = &result
        {
            // 参数类 400/422 降级重试一次（见 fallback_body）
            if let Some(fallback) = fallback_body(detail, &body, req.json_mode) {
                result = self.post(&url, &fallback);
            }
        }
        let resp = result?;
        let raw = resp
            .into_string()
            .map_err(|e| LlmError::Parse(format!("响应读取失败：{e}")))?;
        parse_completion(&raw)
    }
}

impl HttpChat {
    fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": req.user},
            ],
            "max_tokens": req.max_tokens,
            "temperature": 0.2,
        });
        // 仅 json_mode 请求携带 response_format（连接探测等自由文本场景不带，
        // 严格服务端会因此强制 JSON 输出——全链路旅程测试发现的缺陷）
        if req.json_mode {
            body["response_format"] = serde_json::json!({"type": "json_object"});
        }
        // GLM 思考型模型：控制思考档位。思考 token 计入 max_tokens——
        // 连接探测（小预算）与严格 JSON 提取都会被思考吃光额度，返回空 content。
        // glm-4.5/4.6 可关闭（disabled）；glm-5+ 为始终思考模型（实测 1210：
        // 「该模型始终思考，不支持关闭思考；请使用 low、high 或 max」）→ 最小档 low
        if let Some(t) = self.thinking_param() {
            body["thinking"] = t;
        }
        body
    }

    /// GLM 端点（host 含 bigmodel）且模型为思考型时返回 thinking 参数值；
    /// 老模型（glm-4-flash 等）与其他厂商返回 None（不发，避免未知参数 400）。
    fn thinking_param(&self) -> Option<serde_json::Value> {
        let host = self
            .base_url
            .split("://")
            .nth(1)
            .unwrap_or_default()
            .split(['/', ':', '?', '#'])
            .next()
            .unwrap_or_default();
        if !host.contains("bigmodel") {
            return None;
        }
        let (major, minor) = glm_version(&self.model)?;
        if (major, minor) >= (5, 0) {
            Some(serde_json::json!({"type": "low"}))
        } else if (major, minor) >= (4, 5) {
            Some(serde_json::json!({"type": "disabled"}))
        } else {
            None
        }
    }

    fn post(&self, url: &str, body: &serde_json::Value) -> Result<ureq::Response, LlmError> {
        let mut request = self.agent.post(url);
        if let Some(key) = &self.api_key {
            request = request.set("Authorization", &format!("Bearer {key}"));
        }
        match request.send_json(body.clone()) {
            Ok(r) => Ok(r),
            Err(ureq::Error::Status(code, resp)) => {
                let detail = resp.into_string().unwrap_or_default();
                Err(LlmError::HttpStatus {
                    code,
                    detail: shorten(&detail, 300),
                })
            }
            Err(e) => Err(LlmError::Network(e.to_string())),
        }
    }
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// base_url → 请求端点：
/// - 已含 `/chat/completions`（用户粘贴完整端点）→ 原样使用；
/// - 已含 `/chatcompletion_v2`（MiniMax 原生端点）→ 原样使用；
/// - 其余（版本根，如 `…/v1`、`…/paas/v4`）→ 自动拼 `/chat/completions`。
fn chat_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/chat/completions") || base.contains("chatcompletion_v2") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

/// 解析响应体（原文在手，任何失败都能带上下文）。
fn parse_completion(raw: &str) -> Result<ChatReply, LlmError> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        LlmError::Parse(format!("响应不是 JSON：{e}（原文：{}）", shorten(raw, 120)))
    })?;
    // GLM 等网关以 HTTP 200 + 错误 JSON 报告认证/模型名/余额问题：
    // 先于 schema 解析提取，把真实原因交给用户（而不是 missing field `choices`）
    if !v.get("choices").is_some_and(serde_json::Value::is_array) {
        if let Some(msg) = server_error_message(&v) {
            return Err(LlmError::Parse(msg));
        }
    }
    let parsed: CompletionResponse = serde_json::from_value(v)
        .map_err(|e| LlmError::Parse(format!("{e}（响应原文：{}）", shorten(raw, 120))))?;
    parsed.into_reply()
}

/// 提取网关错误信息：OpenAI/GLM 风格 `{"error":{"code","message"}}`，
/// 或顶层 `{"code":…,"msg"/"message":…}` 形态。
fn server_error_message(v: &serde_json::Value) -> Option<String> {
    let extract = |err: &serde_json::Value| -> Option<String> {
        let message = err
            .get("message")
            .or_else(|| err.get("msg"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|m| !m.is_empty())?;
        let code = err
            .get("code")
            .and_then(|x| {
                x.as_str()
                    .map(str::to_string)
                    .or_else(|| x.as_i64().map(|i| i.to_string()))
            })
            .unwrap_or_default();
        Some(if code.is_empty() {
            format!("服务端错误：{message}")
        } else {
            format!("服务端错误 [{code}]：{message}")
        })
    };
    v.get("error").and_then(extract).or_else(|| extract(v))
}

/// 参数类 400/422 的降级重试体：按服务端报错定位要摘掉的参数——
/// thinking 被拒（如模型不支持所选档位）→ 摘 thinking；
/// response_format 被拒（旧版推理服务）→ 摘 response_format；
/// 都没点名且当前带 response_format（json_mode）→ 按旧策略摘 response_format。
/// 无可摘参数返回 None（不重试）。
fn fallback_body(
    detail: &str,
    body: &serde_json::Value,
    json_mode: bool,
) -> Option<serde_json::Value> {
    let d = detail.to_lowercase();
    let strip_thinking =
        body.get("thinking").is_some() && (d.contains("thinking") || d.contains("思考"));
    let strip_rf = body.get("response_format").is_some()
        && json_mode
        && (d.contains("response_format") || !strip_thinking);
    if !strip_thinking && !strip_rf {
        return None;
    }
    let mut fb = body.clone();
    if let Some(obj) = fb.as_object_mut() {
        if strip_thinking {
            obj.remove("thinking");
        }
        if strip_rf {
            obj.remove("response_format");
        }
    }
    Some(fb)
}

/// 模型名 → GLM 主次版本（非 glm-* 或无法解析返回 None）。
/// glm-5.3 → (5,3)、glm-4.6 → (4,6)、glm-4-flash → (4,0)、glm-4v-plus → (4,0)。
fn glm_version(model: &str) -> Option<(u32, u32)> {
    let lower = model.trim().to_lowercase();
    let rest = lower.strip_prefix("glm-")?;
    let mut parts = rest.split(['.', '-', 'v']);
    let major: u32 = parts.next().and_then(|p| p.parse().ok())?;
    if major == 0 {
        return None;
    }
    let minor: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    model: Option<String>,
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    /// OpenAI/GLM/MiniMax OpenAI 兼容形态。
    message: Option<ChoiceMessage>,
    /// MiniMax 原生 chatcompletion_v2 形态（`choices[].messages[]`）。
    #[serde(default)]
    messages: Option<Vec<ChoiceMessage>>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
    /// GLM 思考型模型的思考链（正文之外）——存在与否用于诊断。
    #[serde(default)]
    reasoning_content: Option<String>,
}

impl CompletionResponse {
    fn into_reply(self) -> Result<ChatReply, LlmError> {
        let mut saw_reasoning_only = false;
        let content = self
            .choices
            .iter()
            .filter_map(|c| {
                c.message
                    .as_ref()
                    .or(c.messages.as_deref().and_then(|m| m.first()))
            })
            .filter_map(|m| {
                if m.content
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|c| !c.is_empty())
                {
                    m.content.as_deref()
                } else {
                    if m.reasoning_content
                        .as_deref()
                        .is_some_and(|r| !r.trim().is_empty())
                    {
                        saw_reasoning_only = true;
                    }
                    None
                }
            })
            .map(str::trim)
            .find(|c| !c.is_empty());
        if let Some(c) = content {
            return Ok(ChatReply {
                content: c.to_string(),
                model: self.model.unwrap_or_else(|| "unknown".into()),
            });
        }
        let truncated = self
            .choices
            .iter()
            .any(|c| c.finish_reason.as_deref() == Some("length"));
        let reason = if truncated {
            "输出被 max_tokens 截断（思考型模型的思考也计入 token——GLM 建议在模型名不受限时由本应用自动关闭 thinking）"
        } else if saw_reasoning_only {
            "模型只返回了思考内容（reasoning）没有正文——请关闭 thinking 或换非思考模型"
        } else {
            "响应不含 choices[0].message.content"
        };
        Err(LlmError::Parse(reason.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write as _};
    use std::net::TcpListener;

    fn config(base_url: &str, timeout_secs: u64) -> LlmConfig {
        LlmConfig {
            enabled: true,
            base_url: base_url.into(),
            model: "test-model".into(),
            timeout_secs,
            max_input_chars: 1000,
            api_key_sealed: None,
        }
    }

    fn request_body(raw: &str) -> serde_json::Value {
        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default();
        serde_json::from_str(body).expect("request body json")
    }

    /// 读一个完整 HTTP 请求（头 + Content-Length 体），返回原文。
    fn read_request(stream: &mut std::net::TcpStream) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let headers_end;
        loop {
            let n = stream.read(&mut chunk).expect("read request");
            assert!(n != 0, "client closed before sending full request");
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
                headers_end = pos;
                break;
            }
        }
        let headers = String::from_utf8_lossy(&buf[..headers_end]).into_owned();
        let content_len: usize = headers
            .lines()
            .find_map(|l| {
                l.split(':')
                    .next()
                    .zip(l.split(':').nth(1))
                    .and_then(|(k, v)| {
                        (k.trim().eq_ignore_ascii_case("content-length"))
                            .then(|| v.trim().parse::<usize>().expect("content-length value"))
                    })
            })
            .unwrap_or(0);
        while buf.len() < headers_end + 4 + content_len {
            let n = stream.read(&mut chunk).expect("read body");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    fn write_response(stream: &mut std::net::TcpStream, status: &str, body: &str) {
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(resp.as_bytes()).expect("write response");
    }

    fn ok_body() -> String {
        r#"{"model":"test-model","choices":[{"message":{"role":"assistant","content":"{\"summary\":\"ok\"}"}}]}"#.into()
    }

    fn req() -> ChatRequest {
        ChatRequest {
            system: "sys".into(),
            user: "usr".into(),
            max_tokens: 64,
            json_mode: true,
        }
    }

    #[test]
    fn parses_success_response_and_sends_auth() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let raw = read_request(&mut stream);
            write_response(&mut stream, "200 OK", &ok_body());
            raw
        });
        let chat = HttpChat::new(
            &config(&format!("http://{addr}"), 5),
            Some("sk-secret".into()),
        );
        let reply = Chat::chat(&chat, &req()).expect("chat ok");
        assert_eq!(reply.model, "test-model");
        assert!(reply.content.contains("summary"));
        let raw = handle.join().expect("server thread");
        assert!(raw.contains("POST /chat/completions"), "{raw}");
        assert!(raw.contains("Authorization: Bearer sk-secret"), "{raw}");
        let body = request_body(&raw);
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(body["model"], "test-model");
    }

    #[test]
    fn json_mode_false_omits_response_format() {
        // 回归：json_mode=false（连接探测等自由文本场景）不得携带 response_format
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let raw = read_request(&mut stream);
            write_response(&mut stream, "200 OK", &ok_body());
            raw
        });
        let chat = HttpChat::new(&config(&format!("http://{addr}"), 5), None);
        let mut probe = req();
        probe.json_mode = false;
        let _ = Chat::chat(&chat, &probe).expect("chat ok");
        let raw = handle.join().expect("server thread");
        assert!(
            request_body(&raw).get("response_format").is_none(),
            "json_mode=false 不应带 response_format"
        );
    }

    #[test]
    fn http_error_classified() {
        for (status, code) in [("401 Unauthorized", 401u16), ("429 Too Many Requests", 429)] {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let addr = listener.local_addr().expect("addr");
            let body_status = status.to_string();
            let handle = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("accept");
                let _raw = read_request(&mut stream);
                write_response(&mut stream, &body_status, r#"{"error":"nope"}"#);
            });
            let chat = HttpChat::new(
                &config(&format!("http://{addr}"), 5),
                Some("sk-secret".into()),
            );
            let err = Chat::chat(&chat, &req()).expect_err("should fail");
            match err {
                LlmError::HttpStatus { code: c, .. } => assert_eq!(c, code),
                other => panic!("expect HttpStatus, got {other:?}"),
            }
            handle.join().expect("server thread");
        }
    }

    #[test]
    fn json_mode_falls_back_when_rejected() {
        // 第一枪 400（服务不支持 response_format）→ 客户端去掉该字段重试 → 200
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().expect("accept 1");
            let raw1 = read_request(&mut first);
            write_response(
                &mut first,
                "400 Bad Request",
                r#"{"error":"response_format unsupported"}"#,
            );
            let (mut second, _) = listener.accept().expect("accept 2");
            let raw2 = read_request(&mut second);
            write_response(&mut second, "200 OK", &ok_body());
            (raw1, raw2)
        });
        let chat = HttpChat::new(&config(&format!("http://{addr}"), 5), None);
        let reply = Chat::chat(&chat, &req()).expect("fallback should succeed");
        assert!(reply.content.contains("summary"));
        let (raw1, raw2) = handle.join().expect("server thread");
        assert!(request_body(&raw1).get("response_format").is_some());
        assert!(
            request_body(&raw2).get("response_format").is_none(),
            "重试请求必须去掉 response_format"
        );
    }

    #[test]
    fn empty_choices_is_parse_error() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _raw = read_request(&mut stream);
            write_response(&mut stream, "200 OK", r#"{"model":"m","choices":[]}"#);
        });
        let chat = HttpChat::new(&config(&format!("http://{addr}"), 5), None);
        let err = Chat::chat(&chat, &req()).expect_err("should fail");
        assert!(matches!(err, LlmError::Parse(_)), "{err:?}");
        handle.join().expect("server thread");
    }

    #[test]
    fn glm_200_error_body_surfaces_real_reason() {
        // 回归（2026-08 GLM 实测）：网关以 HTTP 200 + {"error":{...}} 报错，
        // 旧实现只报 missing field `choices`，真实原因（模型名/认证/余额）被吞
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _raw = read_request(&mut stream);
            write_response(
                &mut stream,
                "200 OK",
                r#"{"error":{"code":"1211","message":"模型不存在，请检查模型名称"}}"#,
            );
        });
        let chat = HttpChat::new(&config(&format!("http://{addr}"), 5), Some("k".into()));
        let err = Chat::chat(&chat, &req()).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("1211"), "应含错误码：{msg}");
        assert!(msg.contains("模型不存在"), "应含真实原因：{msg}");
        assert!(
            !msg.contains("missing field"),
            "不得再报 schema 错误：{msg}"
        );
        handle.join().expect("server thread");
    }

    #[test]
    fn non_json_200_body_includes_snippet() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _raw = read_request(&mut stream);
            write_response(&mut stream, "200 OK", "<html>gateway login page</html>");
        });
        let chat = HttpChat::new(&config(&format!("http://{addr}"), 5), None);
        let err = Chat::chat(&chat, &req()).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("响应不是 JSON"), "{msg}");
        assert!(
            msg.contains("gateway login page"),
            "应带原文片段辅助诊断：{msg}"
        );
        handle.join().expect("server thread");
    }

    #[test]
    fn full_endpoint_url_is_not_doubled() {
        // 用户把完整端点粘进 base_url（文档复制习惯）：不得拼出 …/chat/completions/chat/completions
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let raw = read_request(&mut stream);
            write_response(&mut stream, "200 OK", &ok_body());
            raw
        });
        let chat = HttpChat::new(
            &config(&format!("http://{addr}/api/paas/v4/chat/completions"), 5),
            None,
        );
        let _ = Chat::chat(&chat, &req()).expect("chat ok");
        let raw = handle.join().expect("server thread");
        let request_line = raw.lines().next().unwrap_or_default().to_string();
        assert_eq!(
            request_line, "POST /api/paas/v4/chat/completions HTTP/1.1",
            "{raw}"
        );
    }

    #[test]
    fn minimax_native_v2_endpoint_used_verbatim() {
        // MiniMax 原生端点 /v1/text/chatcompletion_v2：原样使用，不得再拼 /chat/completions（404 根因）
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let raw = read_request(&mut stream);
            write_response(&mut stream, "200 OK", &ok_body());
            raw
        });
        let chat = HttpChat::new(
            &config(&format!("http://{addr}/v1/text/chatcompletion_v2"), 5),
            None,
        );
        let _ = Chat::chat(&chat, &req()).expect("chat ok");
        let raw = handle.join().expect("server thread");
        let request_line = raw.lines().next().unwrap_or_default().to_string();
        assert_eq!(
            request_line, "POST /v1/text/chatcompletion_v2 HTTP/1.1",
            "{raw}"
        );
    }

    #[test]
    fn minimax_v2_messages_shape_parsed() {
        // MiniMax 原生 chatcompletion_v2 响应为 choices[].messages[]（复数）
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _raw = read_request(&mut stream);
            write_response(
                &mut stream,
                "200 OK",
                r#"{"model":"MiniMax-M3","choices":[{"finish_reason":"stop","messages":[{"role":"assistant","content":"{\"summary\":\"ok\"}"}]}]}"#,
            );
        });
        let chat = HttpChat::new(&config(&format!("http://{addr}"), 5), None);
        let reply = Chat::chat(&chat, &req()).expect("messages 形态应可解析");
        assert_eq!(reply.model, "MiniMax-M3");
        assert!(reply.content.contains("summary"));
        handle.join().expect("server thread");
    }

    #[test]
    fn glm_thinking_model_sent_disabled_for_bigmodel_host() {
        // glm-5.3 + bigmodel 端点：自动携带 thinking.type=disabled（思考会吃光 max_tokens 导致空 content）
        let mut cfg = config("https://open.bigmodel.cn/api/paas/v4", 5);
        // glm-5+ 为始终思考模型（1210）：不可 disabled，用最小档 low
        cfg.model = "glm-5.3".into();
        let body = HttpChat::new(&cfg, None).build_body(&req());
        assert_eq!(body["thinking"]["type"], "low", "{body}");
        // glm-4.5/4.6 可关闭
        cfg.model = "glm-4.6".into();
        assert_eq!(
            HttpChat::new(&cfg, None).build_body(&req())["thinking"]["type"],
            "disabled"
        );
    }

    #[test]
    fn thinking_param_not_sent_for_old_glm_or_other_hosts() {
        // glm-4-flash（非思考型）与其他厂商：不发 thinking，避免未知参数 400
        let mut old = config("http://bigmodel.test:1", 5);
        old.model = "glm-4-flash".into();
        let chat = HttpChat::new(&old, None);
        assert!(
            chat.build_body(&req()).get("thinking").is_none(),
            "glm-4-flash 不发 thinking"
        );
        let mut other = config("https://api.openai.com/v1", 5);
        other.model = "glm-5.3".into(); // 非 bigmodel 端点也不发
        assert!(
            HttpChat::new(&other, None)
                .build_body(&req())
                .get("thinking")
                .is_none(),
            "非 GLM 端点不发 thinking"
        );
    }

    #[test]
    fn glm_version_matrix() {
        for (m, expect) in [
            ("glm-5.3", Some((5, 3))),
            ("glm-4.6", Some((4, 6))),
            ("glm-4.5-air", Some((4, 5))),
            ("GLM-5", Some((5, 0))),
            ("glm-4-flash", Some((4, 0))),
            ("glm-4v-plus", Some((4, 0))),
            ("glm-4", Some((4, 0))),
            ("deepseek-chat", None),
            ("", None),
        ] {
            assert_eq!(glm_version(m), expect, "{m}");
        }
    }

    #[test]
    fn fallback_body_strips_only_the_rejected_param() {
        let with_both = serde_json::json!({
            "model": "glm-5.3",
            "response_format": {"type": "json_object"},
            "thinking": {"type": "low"},
        });
        // GLM 1210（思考不支持所选档位）：只摘 thinking，保留 response_format
        let fb = fallback_body(
            "该模型始终思考，不支持关闭思考；请使用 low、high 或 max。",
            &with_both,
            true,
        )
        .expect("应给出降级体");
        assert!(fb.get("thinking").is_none());
        assert!(fb.get("response_format").is_some(), "未点名的参数保留");
        // 点名 response_format：只摘它
        let fb2 = fallback_body("response_format unsupported", &with_both, true).expect("fb2");
        assert!(fb2.get("response_format").is_none());
        assert!(fb2.get("thinking").is_some());
        // 未点名 + json_mode → 旧策略摘 response_format
        let fb3 = fallback_body("bad request", &with_both, true).expect("fb3");
        assert!(fb3.get("response_format").is_none());
        assert!(fb3.get("thinking").is_some(), "未点名思考不得误摘");
        // 无可摘（探测请求且未点名）→ 不重试
        let probe = serde_json::json!({"model": "m"});
        assert!(fallback_body("bad request", &probe, false).is_none());
    }

    #[test]
    fn length_truncation_error_is_actionable() {
        // 回归：思考型模型耗尽 max_tokens → finish_reason=length + 空 content
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _raw = read_request(&mut stream);
            write_response(
                &mut stream,
                "200 OK",
                r#"{"model":"glm-5.3","choices":[{"finish_reason":"length","message":{"role":"assistant","content":"","reasoning_content":"让我想想…"}}]}"#,
            );
        });
        let chat = HttpChat::new(&config(&format!("http://{addr}"), 5), None);
        let err = Chat::chat(&chat, &req()).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("截断"), "应说明截断：{msg}");
        assert!(msg.contains("思考"), "应提示思考模型：{msg}");
        handle.join().expect("server thread");
    }

    #[test]
    fn reasoning_only_without_length_flag_reported() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _raw = read_request(&mut stream);
            write_response(
                &mut stream,
                "200 OK",
                r#"{"model":"m","choices":[{"finish_reason":"stop","message":{"role":"assistant","content":null,"reasoning_content":"只想不写"}}]}"#,
            );
        });
        let chat = HttpChat::new(&config(&format!("http://{addr}"), 5), None);
        let err = Chat::chat(&chat, &req()).expect_err("should fail");
        let msg = err.to_string();
        assert!(msg.contains("思考"), "应提示只有 reasoning：{msg}");
        handle.join().expect("server thread");
    }

    #[test]
    fn timeout_is_network_error() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("accept");
            std::thread::sleep(std::time::Duration::from_secs(3)); // 永不响应
        });
        let chat = HttpChat::new(&config(&format!("http://{addr}"), 1), None);
        let started = std::time::Instant::now();
        let err = Chat::chat(&chat, &req()).expect_err("should time out");
        assert!(matches!(err, LlmError::Network(_)), "{err:?}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "1s 超时应生效"
        );
        handle.join().expect("server thread");
    }
}
