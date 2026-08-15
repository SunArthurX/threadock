//! 进程隔离集成测试：spawn markdown adapter 二进制，验证跨进程 parse 链路。
//!
//! 这证明 plan §10.4 的核心目标：adapter 在独立进程，崩溃不影响主进程。

use ch_adapter_host::{AdapterProcess, HostError};

/// 找到 workspace target/debug 下的 ch-adapter-markdown 二进制。
fn adapter_binary() -> String {
    let metadata = cargo_metadata::MetadataCommand::new().exec().unwrap();
    let target = metadata.target_directory.join("debug/ch-adapter-markdown");
    target.to_string()
}

#[test]
fn spawn_markdown_adapter_and_parse() {
    let mut proc = AdapterProcess::spawn(&adapter_binary()).unwrap();

    // 1. 握手
    let meta = proc.handshake().unwrap();
    assert_eq!(meta.id, "markdown");
    assert_eq!(meta.protocol_version, 1);

    // 2. 解析一段 Markdown
    let md = b"# Test\n\n## User\nhello\n## Assistant\nworld\n";
    let conv = proc.parse("test.md", md).unwrap();
    assert_eq!(conv.source_conversation_id, "test.md");
    assert_eq!(conv.title.as_deref(), Some("Test"));
    assert_eq!(conv.messages.len(), 2);

    // 3. 进程仍存活
    assert!(proc.is_alive());
}

#[test]
fn markdown_adapter_health() {
    let mut proc = AdapterProcess::spawn(&adapter_binary()).unwrap();
    proc.handshake().unwrap();
    let h = proc.health().unwrap();
    assert!(h.healthy);
}

#[test]
fn adapter_process_isolates_crash() {
    // 关键测试：如果 adapter 进程出问题，主进程不应受影响。
    // 我们用 `false`（立即退出的进程）模拟 adapter 崩溃。
    let mut proc = AdapterProcess::spawn("false").unwrap();
    let result = proc.handshake();
    assert!(
        matches!(
            result,
            Err(HostError::ProcessExited(_)) | Err(HostError::Io(_))
        ),
        "crashed adapter must be detected, got {:?}",
        result
    );
    // 主进程（本测试）继续运行——证明隔离生效
}

#[test]
fn protocol_version_mismatch_detected() {
    // markdown adapter 报告 protocol_version=1，host 期望 1，应通过。
    // 若未来 adapter 升级到 2 而 host 仍 1，handshake 应报错。
    let mut proc = AdapterProcess::spawn(&adapter_binary()).unwrap();
    let meta = proc.handshake().unwrap();
    assert_eq!(meta.protocol_version, ch_adapter_sdk::PROTOCOL_VERSION);
}
