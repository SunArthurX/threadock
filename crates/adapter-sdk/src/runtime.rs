//! Adapter 运行时：trait 定义 + stdio 协议循环。
//!
//! 具体 adapter（如 markdown）实现 [`ConversationAdapter`]，用 [`serve_stdio`] 启动。
//! adapter-host 通过 spawn 子进程 + 读写其 stdin/stdout 调用它。

use crate::protocol::{
    decode_bytes, AdapterMetadata, HealthResponse, HelloResponse, ParseRequest, ParseResponse,
};
use ch_normalization::RawConversation;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

/// Adapter 必须实现的接口（plan §10.2 的 MVP 子集）。
pub trait ConversationAdapter: Send + Sync {
    /// 返回元信息。
    fn metadata(&self) -> AdapterMetadata;

    /// 解析原始内容为 RawConversation。
    fn parse(&self, source_id: &str, content: &[u8]) -> Result<RawConversation, AdapterError>;

    /// 健康检查。
    fn health(&self) -> HealthResponse {
        HealthResponse {
            healthy: true,
            detail: None,
        }
    }
}

/// Adapter 错误。
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("{0}")]
    Parse(String),
}

/// JSON-RPC 2.0 信封。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    fn ok(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }
    fn err(id: serde_json::Value, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

/// 在 stdio 上运行协议循环，直到 stdin EOF。
///
/// 每个 adapter 二进制的 `main` 调用它：
///
/// ```ignore
/// let adapter = MyAdapter::new();
/// serve_stdio(&adapter, stdin, stdout);
/// ```
pub fn serve_stdio<A: ConversationAdapter, R: BufRead, W: Write>(
    adapter: &A,
    mut stdin: R,
    stdout: &mut W,
) {
    let mut line = String::new();
    loop {
        line.clear();
        let n = match stdin.read_line(&mut line) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp = handle_line(adapter, trimmed);
        let resp_json = serde_json::to_string(&resp).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialize failed"}}"#
                .to_string()
        });
        let _ = writeln!(stdout, "{resp_json}");
        let _ = stdout.flush();
    }
}

fn handle_line<A: ConversationAdapter>(adapter: &A, line: &str) -> JsonRpcResponse {
    let req: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return JsonRpcResponse::err(
                serde_json::Value::Null,
                -32700,
                format!("parse error: {e}"),
            )
        }
    };
    let id = req.id.unwrap_or(serde_json::Value::Null);

    match req.method.as_str() {
        "hello" => {
            let meta = adapter.metadata();
            let resp = HelloResponse { metadata: meta };
            let val = serde_json::to_value(&resp).unwrap_or_default();
            JsonRpcResponse::ok(id, val)
        }
        "parse" => {
            let preq: ParseRequest = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => return JsonRpcResponse::err(id, -32602, format!("bad params: {e}")),
            };
            let content = match decode_bytes(&preq.content_base64) {
                Ok(c) => c,
                Err(e) => return JsonRpcResponse::err(id, -32602, format!("bad base64: {e}")),
            };
            match adapter.parse(&preq.source_id, &content) {
                Ok(conv) => {
                    let presp = ParseResponse { conversation: conv };
                    let val = serde_json::to_value(&presp).unwrap_or_default();
                    JsonRpcResponse::ok(id, val)
                }
                Err(e) => JsonRpcResponse::err(id, -32000, e.to_string()),
            }
        }
        "health" => {
            let resp = adapter.health();
            let val = serde_json::to_value(&resp).unwrap_or_default();
            JsonRpcResponse::ok(id, val)
        }
        other => JsonRpcResponse::err(id, -32601, format!("method not found: {other}")),
    }
}

// 让协议版本常量在文档示例中可用
pub use crate::PROTOCOL_VERSION as _PROTOCOL_VERSION_REEXPORT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{encode_bytes, AdapterMetadata, HealthResponse, PROTOCOL_VERSION};
    use ch_domain::Provider;
    use ch_domain::Role;
    use ch_normalization::{RawConversation, RawMessage};
    use std::io::Cursor;

    struct FakeAdapter;
    impl ConversationAdapter for FakeAdapter {
        fn metadata(&self) -> AdapterMetadata {
            AdapterMetadata {
                id: "fake".into(),
                name: "Fake".into(),
                version: "0.1.0".into(),
                protocol_version: PROTOCOL_VERSION,
                provider: Provider::Generic,
            }
        }
        fn parse(&self, source_id: &str, content: &[u8]) -> Result<RawConversation, AdapterError> {
            let text = String::from_utf8_lossy(content).to_string();
            Ok(RawConversation {
                provider: Provider::Generic,
                source_conversation_id: source_id.to_string(),
                title: Some("fake".into()),
                model: None,
                started_at: None,
                messages: vec![RawMessage {
                    role: Role::User,
                    text: Some(text),
                    content_json: None,
                    source_message_id: None,
                    created_at: None,
                }],
                events: vec![],
                source_parent_id: None,
            })
        }
    }

    fn run(input: &str) -> String {
        let adapter = FakeAdapter;
        let stdin = Cursor::new(input);
        let mut stdout = Vec::new();
        serve_stdio(&adapter, stdin, &mut stdout);
        String::from_utf8(stdout).unwrap()
    }

    #[test]
    fn hello_returns_metadata() {
        let out = run(r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{}}"#);
        let resp: JsonRpcResponse = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(resp.id, serde_json::json!(1));
        let hello: HelloResponse = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(hello.metadata.id, "fake");
        assert_eq!(hello.metadata.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn parse_returns_conversation() {
        let content_b64 = encode_bytes(b"hello body");
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"parse","params":{{"source_id":"x.md","content_base64":"{content_b64}"}}}}"#
        );
        let out = run(&req);
        let resp: JsonRpcResponse = serde_json::from_str(out.trim()).unwrap();
        let presp: ParseResponse = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(presp.conversation.source_conversation_id, "x.md");
        assert_eq!(presp.conversation.messages.len(), 1);
    }

    #[test]
    fn health_returns_healthy() {
        let out = run(r#"{"jsonrpc":"2.0","id":3,"method":"health","params":{}}"#);
        let resp: JsonRpcResponse = serde_json::from_str(out.trim()).unwrap();
        let h: HealthResponse = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert!(h.healthy);
    }

    #[test]
    fn unknown_method_returns_error() {
        let out = run(r#"{"jsonrpc":"2.0","id":4,"method":"nope","params":{}}"#);
        let resp: JsonRpcResponse = serde_json::from_str(out.trim()).unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn invalid_json_returns_parse_error() {
        let out = run("not json");
        let resp: JsonRpcResponse = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(resp.error.unwrap().code, -32700);
    }

    #[test]
    fn eof_terminates_cleanly() {
        // 多行后 EOF 应正常退出
        let input = format!(
            "{}\n{}\n",
            r#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"health","params":{}}"#
        );
        let out = run(&input);
        let lines: Vec<&str> = out.trim().lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn empty_lines_ignored() {
        let input = "\n\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\",\"params\":{}}\n\n";
        let out = run(input);
        let lines: Vec<&str> = out.trim().lines().collect();
        assert_eq!(lines.len(), 1);
    }
}
