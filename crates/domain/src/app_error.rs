//! 统一应用错误：结构化错误码 + 用户友好消息，替代裸 `e.to_string()`。
//!
//! 前端收到 `{code, message, detail}` JSON，可按 code 分类处理。

use serde::Serialize;

/// 统一错误码（前端可按此分发处理逻辑）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 数据库操作失败
    Storage,
    /// 全文检索失败
    Search,
    /// 数据导入/解析失败
    Import,
    /// 审计扫描失败
    Audit,
    /// 文件 I/O 失败
    Io,
    /// 资源不存在
    NotFound,
    /// 并发冲突（同步中/重置中）
    Busy,
    /// 参数无效
    Invalid,
    /// 内部错误（不应发生）
    Internal,
}

/// 统一错误结构：前端序列化为 `{code, message, detail}`。
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: ErrorCode,
    /// 用户友好中文消息。
    pub message: String,
    /// 底层错误详情（可选，调试用）。
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn storage(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Storage, msg)
    }
    pub fn search(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Search, msg)
    }
    pub fn import(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Import, msg)
    }
    pub fn audit(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Audit, msg)
    }
    pub fn io(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Io, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, msg)
    }
    pub fn busy(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Busy, msg)
    }
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Invalid, msg)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)?;
        if let Some(d) = &self.detail {
            write!(f, " ({d})")?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

// ── From 自动转换（让 `?` 运算符直接工作）───────────────────────────

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::io("文件读写失败").with_detail(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::import("数据解析失败").with_detail(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_serialize_snake_case() {
        let e = AppError::storage("test");
        let json = serde_json::to_string(&e).expect("unexpected None");
        assert!(json.contains("\"storage\""), "code 应为 snake_case: {json}");
        assert!(json.contains("\"message\""));
    }

    #[test]
    fn display_includes_code_and_detail() {
        let e = AppError::not_found("会话不存在").with_detail("id=abc");
        let s = e.to_string();
        assert!(s.contains("NotFound"));
        assert!(s.contains("会话不存在"));
        assert!(s.contains("id=abc"));
    }
}
