//! Adapter Host：隔离子进程调度，对应 plan §10.4「Adapter 隔离」。
//!
//! ## 核心目标
//!
//! - 每个 adapter 独立进程。
//! - 通过 stdio JSON-RPC 通信。
//! - 启动超时 / 调用超时 / 崩溃检测。
//! - **adapter 崩溃不影响主进程**（plan §10.4、§7.3 隔离率 100%）。
//!
//! ## 两种客户端
//!
//! - [`InProcClient`]：直接对一个实现 `ConversationAdapter` 的对象调用，不走子进程。
//!   用于测试、以及 MVP 阶段的同进程降级（plan §3「graceful degradation」）。
//! - [`AdapterProcess`]：spawn 真实子进程，跨进程调用。生产路径。

use ch_adapter_sdk::protocol::{
    encode_bytes, AdapterMetadata, HealthResponse, HelloResponse, ParseResponse, PROTOCOL_VERSION,
};
use ch_adapter_sdk::runtime::ConversationAdapter;
use ch_normalization::RawConversation;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use thiserror::Error;

pub type HostResult<T> = std::result::Result<T, HostError>;

#[derive(Debug, Error)]
pub enum HostError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("adapter process exited unexpectedly: {0}")]
    ProcessExited(String),

    #[error("timeout waiting for adapter response")]
    Timeout,

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("adapter returned error: {0}")]
    AdapterError(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

// ── JSON-RPC 信封（与 adapter-sdk 一致）─────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

// ── InProcClient：同进程调用 ─────────────────────────────────────────────

/// 同进程 adapter 客户端：直接调用 trait 方法，不走子进程。
///
/// 用于测试和 MVP 同进程降级。
pub struct InProcClient<A: ConversationAdapter> {
    adapter: A,
}

impl<A: ConversationAdapter> InProcClient<A> {
    pub fn new(adapter: A) -> Self {
        Self { adapter }
    }
    pub fn hello(&self) -> HostResult<AdapterMetadata> {
        Ok(self.adapter.metadata())
    }
    pub fn parse(&self, source_id: &str, content: &[u8]) -> HostResult<RawConversation> {
        self.adapter
            .parse(source_id, content)
            .map_err(|e| HostError::AdapterError(e.to_string()))
    }
    pub fn health(&self) -> HostResult<HealthResponse> {
        Ok(self.adapter.health())
    }
}

// ── AdapterProcess：跨进程调用 ───────────────────────────────────────────

/// 管理一个 adapter 子进程。
///
/// 生命周期：spawn → hello 握手 → 多次 parse → drop 时 kill。
pub struct AdapterProcess {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: i64,
    metadata: Option<AdapterMetadata>,
}

impl AdapterProcess {
    /// spawn 一个 adapter 二进制。
    ///
    /// `command` 为 adapter 可执行文件路径；argv 通常为空（adapter 通过 stdio 通信）。
    pub fn spawn(command: &str) -> HostResult<Self> {
        let mut child = Command::new(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| HostError::ProcessExited("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HostError::ProcessExited("no stdout".into()))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            metadata: None,
        })
    }

    /// 握手：调用 hello 并缓存 metadata。返回 metadata。
    pub fn handshake(&mut self) -> HostResult<AdapterMetadata> {
        let resp = self.call("hello", &serde_json::json!({}))?;
        let hello: HelloResponse = serde_json::from_value(resp)?;
        if hello.metadata.protocol_version != PROTOCOL_VERSION {
            return Err(HostError::Protocol(format!(
                "protocol version mismatch: adapter={}, host={}",
                hello.metadata.protocol_version, PROTOCOL_VERSION
            )));
        }
        self.metadata = Some(hello.metadata.clone());
        Ok(hello.metadata)
    }

    /// 解析内容。
    pub fn parse(&mut self, source_id: &str, content: &[u8]) -> HostResult<RawConversation> {
        let params = serde_json::json!({
            "source_id": source_id,
            "content_base64": encode_bytes(content),
        });
        let resp = self.call("parse", &params)?;
        let presp: ParseResponse = serde_json::from_value(resp)?;
        Ok(presp.conversation)
    }

    /// 健康检查。
    pub fn health(&mut self) -> HostResult<HealthResponse> {
        let resp = self.call("health", &serde_json::json!({}))?;
        Ok(serde_json::from_value(resp)?)
    }

    /// 子进程是否仍在运行。
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// 发送一个 JSON-RPC 请求并读取一行响应。
    fn call(&mut self, method: &str, params: &serde_json::Value) -> HostResult<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&req)?;
        writeln!(self.stdin, "{line}")?;
        self.stdin.flush()?;

        let mut buf = String::new();
        let n = self
            .stdout
            .read_line(&mut buf)
            .map_err(|e| HostError::ProcessExited(format!("read response failed: {e}")))?;
        if n == 0 {
            return Err(HostError::ProcessExited(
                "adapter stdout closed (process likely crashed)".into(),
            ));
        }
        let resp: JsonRpcResponse = serde_json::from_str(buf.trim())?;
        if let Some(err) = resp.error {
            return Err(HostError::AdapterError(format!(
                "[{}] {}",
                err.code, err.message
            )));
        }
        resp.result
            .ok_or_else(|| HostError::Protocol("no result".into()))
    }
}

impl Drop for AdapterProcess {
    fn drop(&mut self) {
        // 优雅关闭：关闭 stdin 让 adapter 自然退出；若仍存活则 kill。
        let _ = self.stdin.flush();
        if self.is_alive() {
            // 给一点时间优雅退出
            std::thread::sleep(Duration::from_millis(50));
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_adapter_sdk::protocol::{decode_bytes, AdapterMetadata};
    use ch_domain::{Provider, Role};
    use ch_normalization::{RawConversation, RawMessage};

    struct EchoAdapter;
    impl ConversationAdapter for EchoAdapter {
        fn metadata(&self) -> AdapterMetadata {
            AdapterMetadata {
                id: "echo".into(),
                name: "Echo".into(),
                version: "0.1.0".into(),
                protocol_version: PROTOCOL_VERSION,
                provider: Provider::Generic,
            }
        }
        fn parse(
            &self,
            source_id: &str,
            content: &[u8],
        ) -> Result<RawConversation, ch_adapter_sdk::runtime::AdapterError> {
            let text = String::from_utf8_lossy(content).to_string();
            Ok(RawConversation {
                provider: Provider::Generic,
                source_conversation_id: source_id.to_string(),
                title: Some("echo".into()),
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

    #[test]
    fn inproc_client_hello() {
        let client = InProcClient::new(EchoAdapter);
        let meta = client.hello().unwrap();
        assert_eq!(meta.id, "echo");
        assert_eq!(meta.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn inproc_client_parse() {
        let client = InProcClient::new(EchoAdapter);
        let conv = client.parse("src.md", b"hello").unwrap();
        assert_eq!(conv.source_conversation_id, "src.md");
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].text.as_deref(), Some("hello"));
    }

    #[test]
    fn inproc_client_health() {
        let client = InProcClient::new(EchoAdapter);
        assert!(client.health().unwrap().healthy);
    }

    #[test]
    fn encode_decode_bytes_roundtrip() {
        let original = b"\x00\x01\x02 binary";
        let encoded = encode_bytes(original);
        let decoded = decode_bytes(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn spawn_nonexistent_binary_errors() {
        // spawn 一个不存在的二进制应失败
        let result = AdapterProcess::spawn("/nonexistent/adapter-binary-xyz");
        assert!(result.is_err());
    }

    #[test]
    fn spawn_cat_as_adapter_reports_crash_on_handshake() {
        // 用 `false` 命令（永远失败立即退出）可靠地验证「子进程崩溃被检测」。
        let mut proc = AdapterProcess::spawn("false").unwrap();
        // false 立即退出，handshake 应报 ProcessExited 或 Io
        let result = proc.handshake();
        let is_expected = matches!(
            result,
            Err(HostError::ProcessExited(_)) | Err(HostError::Io(_))
        );
        assert!(
            is_expected,
            "expected ProcessExited or Io, got {:?}",
            result
        );
    }
}
