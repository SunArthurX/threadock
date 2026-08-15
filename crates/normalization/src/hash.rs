//! 内容哈希，对应 plan §11.3。
//!
//! 关键约束（来自 plan）：
//! - 禁止仅按文本内容去重——同一消息可在不同会话合法出现。
//! - 因此 message hash 必须绑定 provider + conversation + role 等上下文，
//!   而 conversation hash 绑定 provider + source_id + 全部消息摘要。
//!
//! 算法：BLAKE3（plan §9.6）。

use ch_domain::{Conversation, Message, Provider, Role};

/// 计算单条消息的内容 hash。
///
/// 输入包括：provider、conversation 占位（用 conversation 的 source id）、
/// role、文本内容、结构化内容。**不包含** sequence_number，
/// 因为同一条消息在不同导入轮次中序号可能漂移，但内容相同应得同 hash。
pub fn content_hash_for_message(
    provider: Provider,
    conversation_source_id: &str,
    role: Role,
    text: &str,
    content_json: Option<&serde_json::Value>,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ch-msg-v1\n");
    hasher.update(provider.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(conversation_source_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(role.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(text.as_bytes());
    hasher.update(b"\n");
    if let Some(json) = content_json {
        // 用 JSON 的规范序列化保证字段顺序无关
        let canonical = canonical_json(json);
        hasher.update(canonical.as_bytes());
    }
    let hash = hasher.finalize();
    hash.to_hex().to_string()
}

/// 计算 conversation 的内容 hash。
///
/// 由 provider + source_conversation_id + 所有消息 hash 的拼接构成，
/// 这样任何一条消息变化都会改变 conversation 的 hash。
pub fn content_hash_for_conversation(
    provider: Provider,
    source_conversation_id: &str,
    message_hashes: &[&str],
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"ch-conv-v1\n");
    hasher.update(provider.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(source_conversation_id.as_bytes());
    hasher.update(b"\n");
    for h in message_hashes {
        hasher.update(h.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize().to_hex().to_string()
}

/// 给现有 Message 计算 hash 的便捷方法。
pub fn hash_message(m: &Message, provider: Provider, conversation_source_id: &str) -> String {
    content_hash_for_message(
        provider,
        conversation_source_id,
        m.role,
        m.content_text.as_deref().unwrap_or(""),
        m.content_json.as_ref(),
    )
}

/// 给现有 Conversation 计算 hash 的便捷方法。
pub fn hash_conversation(c: &Conversation, message_hashes: &[&str]) -> String {
    content_hash_for_conversation(c.provider, &c.source_conversation_id, message_hashes)
}

/// JSON 规范序列化：键排序、无空格、UTF-8。保证相同语义得到相同字节。
fn canonical_json(v: &serde_json::Value) -> String {
    // serde_json 的 to_string 默认无空格；键顺序在 serde_json::Value(BTreeMap) 下有序。
    // 为稳定，先转回 BTreeMap 风格再序列化。
    let canon = canonicalize_value(v);
    serde_json::to_string(&canon).unwrap_or_default()
}

fn canonicalize_value(v: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonicalize_value(&map[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_value).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_message_same_hash() {
        let h1 = content_hash_for_message(Provider::Codex, "src-1", Role::User, "hello", None);
        let h2 = content_hash_for_message(Provider::Codex, "src-1", Role::User, "hello", None);
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_provider_different_hash() {
        // plan §11.3：禁止纯文本去重，不同 provider 同文本必须不同 hash
        let h1 = content_hash_for_message(Provider::Codex, "src-1", Role::User, "hello", None);
        let h2 = content_hash_for_message(Provider::Cursor, "src-1", Role::User, "hello", None);
        assert_ne!(h1, h2, "different provider must produce different hash");
    }

    #[test]
    fn different_conversation_same_text_different_hash() {
        // 同 provider 同文本，但不同 conversation → 不同 hash
        let h1 = content_hash_for_message(Provider::Codex, "conv-a", Role::User, "hello", None);
        let h2 = content_hash_for_message(Provider::Codex, "conv-b", Role::User, "hello", None);
        assert_ne!(h1, h2);
    }

    #[test]
    fn different_role_different_hash() {
        let h1 = content_hash_for_message(Provider::Codex, "c", Role::User, "x", None);
        let h2 = content_hash_for_message(Provider::Codex, "c", Role::Assistant, "x", None);
        assert_ne!(h1, h2);
    }

    #[test]
    fn json_key_order_irrelevant() {
        let a = serde_json::json!({"b": 1, "a": 2});
        let b = serde_json::json!({"a": 2, "b": 1});
        let h1 = content_hash_for_message(Provider::Codex, "c", Role::User, "x", Some(&a));
        let h2 = content_hash_for_message(Provider::Codex, "c", Role::User, "x", Some(&b));
        assert_eq!(h1, h2, "JSON key order must not affect hash");
    }

    #[test]
    fn conversation_hash_changes_with_messages() {
        let base =
            |hashes: &[&str]| content_hash_for_conversation(Provider::Codex, "conv-1", hashes);
        let h1 = base(&["aaa", "bbb"]);
        let h2 = base(&["aaa", "bbb"]);
        let h3 = base(&["aaa", "ccc"]);
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn hashes_are_hex_64() {
        // BLAKE3 → 32 字节 → 64 hex 字符
        let h = content_hash_for_message(Provider::Codex, "c", Role::User, "x", None);
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
