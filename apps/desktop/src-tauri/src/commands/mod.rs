//! Tauri command 层：按域拆分的子模块与跨域共享设施。
//!
//! 共享：错误分类助手、BusyGuard 防重入、run_blocking（重活移出 runtime）、
//! 同步节流静态标志。各域命令在同名子模块，经 `pub(crate) use` 汇出到 lib.rs。

use ch_domain::app_error::AppError;

/// 将任何 Display 错误转为 AppError 字符串（替代裸 e.to_string()）。
/// 前端收到 `[code] message (detail)` 格式。
/// 分类辅助：按错误发生层给出结构化错误码（前端可按 code 分发），
/// 替代全部压成 Internal 的旧 internal_err。
pub(crate) fn storage_err(e: impl std::fmt::Display) -> String {
    AppError::storage("数据库操作失败")
        .with_detail(e.to_string())
        .to_string()
}

pub(crate) fn search_err(e: impl std::fmt::Display) -> String {
    AppError::search("搜索失败")
        .with_detail(e.to_string())
        .to_string()
}

pub(crate) fn import_err(e: impl std::fmt::Display) -> String {
    AppError::import("导入/解析失败")
        .with_detail(e.to_string())
        .to_string()
}

pub(crate) fn io_err(e: impl std::fmt::Display) -> String {
    AppError::io("文件读写失败")
        .with_detail(e.to_string())
        .to_string()
}

/// 全局防重入标志：避免重置/同步并发执行导致 UI 卡顿或数据竞争。
pub(crate) static IS_BUSY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 全局忙标志的 RAII guard：作用域结束（含 panic 展开）自动复位。
/// 旧实现 panic 后 IS_BUSY 永久为 true → 应用只能重启。
pub(crate) struct BusyGuard;

/// 取消同步标志：前端「取消更新」按钮设置，同步循环内定期检查提前退出。
pub(crate) static CANCEL_SYNC: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 把同步重活移出 tokio worker 线程。
///
/// Tauri async command 跑在 tokio runtime 上；内部的 SQLite/文件扫描/zstd
/// 是同步阻塞调用，直接执行会占住 worker 线程（多命令并发时饿死 runtime、
/// UI 卡死）。`block_in_place` 把当前 worker 换成阻塞池线程再执行。
pub(crate) fn run_blocking<T>(f: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(f)
}

/// ops_sync 上次完成时间（毫秒，进程内存缓存；真源为 app_settings.last_ops_sync_ms）。
pub(crate) static LAST_OPS_SYNC_MS: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

/// ops 同步节流窗口：30 分钟内不重复全量扫描（跨进程持久，重启后仍生效）。
pub(crate) const OPS_SYNC_THROTTLE_MS: i64 = 30 * 60 * 1000;

impl BusyGuard {
    fn acquire() -> Result<Self, String> {
        if IS_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::busy("同步中，请稍候…").to_string());
        }
        Ok(BusyGuard)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        IS_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

mod audit;
mod backup_cmd;
mod conversations;
mod dto;
mod export_cmd;
mod import;
mod insights;
mod llm_cmd;
mod maintenance;
mod ops;
mod search_cmd;
mod settings;
mod workspace_cmd;

pub(crate) use audit::*;
pub(crate) use backup_cmd::*;
pub(crate) use conversations::*;
pub(crate) use dto::*;
pub(crate) use export_cmd::*;
pub(crate) use import::*;
pub(crate) use insights::*;
pub(crate) use llm_cmd::*;
pub(crate) use maintenance::*;
pub(crate) use ops::*;
pub(crate) use search_cmd::*;
pub(crate) use settings::*;
pub(crate) use workspace_cmd::*;
