//! OpenAI 兼容 `chat/completions` 客户端（ureq + rustls，无 openssl 依赖）。
//!
//! - 传输抽象为 [`Chat`] trait：生产 [`HttpChat`]，测试用 mock（无外网）。
//! - `json_mode` 默认随请求发送 `response_format: {"type":"json_object"}`；
//!   部分 OpenAI 兼容服务（旧版本地推理等）不支持该字段并返回 4xx——
//!   此时**去掉该字段重试一次**再判失败。
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
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.build_body(req);
        let mut result = self.post(&url, &body);
        if req.json_mode {
            if let Err(LlmError::HttpStatus {
                code: 400 | 422, ..
            }) = &result
            {
                // 降级重试：去掉 response_format（兼容不支持该字段的服务）。
                // 只对 400/422（参数类错误）重试；401/429 等重试无意义。
                let fallback = without_response_format(&body);
                result = self.post(&url, &fallback);
            }
        }
        let resp = result?;
        let parsed: CompletionResponse = resp
            .into_json()
            .map_err(|e| LlmError::Parse(e.to_string()))?;
        parsed.into_reply()
    }
}

impl HttpChat {
    fn build_body(&self, req: &ChatRequest) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": req.user},
            ],
            "max_tokens": req.max_tokens,
            "temperature": 0.2,
            "response_format": {"type": "json_object"},
        })
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

fn without_response_format(body: &serde_json::Value) -> serde_json::Value {
    let mut cloned = body.clone();
    if let Some(obj) = cloned.as_object_mut() {
        obj.remove("response_format");
    }
    cloned
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    model: Option<String>,
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Option<ChoiceMessage>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

impl CompletionResponse {
    fn into_reply(self) -> Result<ChatReply, LlmError> {
        let content = self
            .choices
            .iter()
            .filter_map(|c| c.message.as_ref())
            .filter_map(|m| m.content.as_deref())
            .map(str::trim)
            .find(|c| !c.is_empty());
        match content {
            Some(c) => Ok(ChatReply {
                content: c.to_string(),
                model: self.model.unwrap_or_else(|| "unknown".into()),
            }),
            None => Err(LlmError::Parse(
                "响应不含 choices[0].message.content（可能被 max_tokens 截断）".into(),
            )),
        }
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
