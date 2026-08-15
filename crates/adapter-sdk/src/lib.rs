//! Adapter SDK：定义 Adapter trait 与 stdio JSON-RPC 协议，对应 plan §10.2/§10.4。
//!
//! ## 角色
//!
//! - **adapter-host**（主进程侧）：spawn adapter 子进程，通过 stdio 收发 JSON-RPC。
//! - **具体 adapter**（子进程侧）：实现 [`ConversationAdapter`]，用 [`serve_stdio`] 跑协议循环。
//! - **本 crate**：两者共享的 trait + 协议消息类型。
//!
//! ## 协议（plan §10.4「JSON-RPC over stdio」）
//!
//! 每行一个 JSON-RPC 2.0 消息（newline-delimited）。方法：
//! - `hello` → 返回 AdapterMetadata（启动握手）
//! - `parse` → 输入文件路径/内容，返回 `RawConversation`
//! - `health` → 健康检查
//!
//! 子进程崩溃被 host 检测到（stdout 关闭），不影响主进程（plan §10.4 隔离目标）。

pub mod protocol;
pub mod runtime;

pub use protocol::{
    AdapterMetadata, HealthRequest, HealthResponse, HelloRequest, HelloResponse, ParseRequest,
    ParseResponse, PROTOCOL_VERSION,
};
pub use runtime::{serve_stdio, ConversationAdapter};
