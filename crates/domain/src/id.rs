//! ID 生成与校验工具。
//!
//! 当前实现基于 UUID v4 + 前缀（如 `ws_`、`conv_`），对应 plan §12 主键风格。
//! 未来可平滑切换到 ULID 以获得时间有序性，只要此前缀约定不变即可。

use crate::error::{DomainError, Result};
use uuid::Uuid;

/// 生成带前缀的 UUID v4 ID。
#[must_use] 
pub fn generate(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

/// 校验 ID 是否符合 `<prefix>_<hex32>` 格式。
pub fn validate(id: &str, expected_prefix: &str) -> Result<()> {
    let rest = id
        .strip_prefix(expected_prefix)
        .ok_or_else(|| DomainError::InvalidId(format!("missing prefix {expected_prefix}")))?;

    let rest = rest
        .strip_prefix('_')
        .ok_or_else(|| DomainError::InvalidId("missing underscore after prefix".into()))?;

    Uuid::parse_str(rest)
        .map_err(|_| DomainError::InvalidId(format!("invalid uuid segment: {rest}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_has_prefix() {
        let id = generate("ws");
        assert!(id.starts_with("ws_"));
        assert!(validate(&id, "ws").is_ok());
    }

    #[test]
    fn validate_rejects_wrong_prefix() {
        let id = generate("ws");
        assert!(validate(&id, "conv").is_err());
    }

    #[test]
    fn validate_rejects_garbage() {
        assert!(validate("not-an-id", "ws").is_err());
        assert!(validate("ws_", "ws").is_err());
        assert!(validate("ws_notahex!", "ws").is_err());
    }

    #[test]
    fn generated_ids_are_unique() {
        let a = generate("msg");
        let b = generate("msg");
        assert_ne!(a, b);
    }
}
