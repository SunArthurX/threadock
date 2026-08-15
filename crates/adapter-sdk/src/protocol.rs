//! JSON-RPC 2.0 协议消息，对应 plan §10.2 Adapter 统一接口。
//!
//! 消息用 newline-delimited JSON 在 stdio 上传输（plan §10.4）。

use ch_domain::Provider;
use ch_normalization::RawConversation;
use serde::{Deserialize, Serialize};

/// Adapter 协议版本（plan §16.4 API 版本策略）。
pub const PROTOCOL_VERSION: u32 = 1;

/// Adapter 元信息，对应 plan §10.3 Manifest 的子集。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub protocol_version: u32,
    pub provider: Provider,
}

// ── 请求/响应类型 ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloResponse {
    pub metadata: AdapterMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseRequest {
    /// 源标识（文件路径或 URL），用作 `source_conversation_id`。
    pub source_id: String,
    /// 原始内容字节（base64 编码，避免 JSON 里的二进制问题）。
    pub content_base64: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParseResponse {
    pub conversation: RawConversation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub detail: Option<String>,
}

// ── base64 编解码辅助 ────────────────────────────────────────────────────

use base64::{engine::general_purpose::STANDARD, Engine as _};

#[must_use]
pub fn encode_bytes(data: &[u8]) -> String {
    STANDARD.encode(data)
}

pub fn decode_bytes(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    STANDARD.decode(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip() {
        let m = AdapterMetadata {
            id: "markdown".into(),
            name: "Markdown".into(),
            version: "0.1.0".into(),
            protocol_version: PROTOCOL_VERSION,
            provider: Provider::Generic,
        };
        let json = serde_json::to_string(&m).expect("unexpected None");
        let back: AdapterMetadata = serde_json::from_str(&json).expect("parse failed");
        assert_eq!(m, back);
    }

    #[test]
    fn parse_request_with_base64() {
        let req = ParseRequest {
            source_id: "test.md".into(),
            content_base64: encode_bytes(b"## User\nhello"),
        };
        let json = serde_json::to_string(&req).expect("unexpected None");
        let back: ParseRequest = serde_json::from_str(&json).expect("parse failed");
        assert_eq!(back.source_id, "test.md");
        assert_eq!(
            decode_bytes(&back.content_base64).expect("unexpected None"),
            b"## User\nhello"
        );
    }

    #[test]
    fn encode_decode_roundtrip() {
        let original = b"binary\x00data\xff\xfe";
        let encoded = encode_bytes(original);
        let decoded = decode_bytes(&encoded).expect("unexpected None");
        assert_eq!(decoded, original);
    }
}
