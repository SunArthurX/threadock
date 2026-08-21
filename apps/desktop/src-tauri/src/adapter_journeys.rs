//! Adapter 修复轮集成旅程：Codex JS 工具桥会话走完整命令层链路——
//! 解析 → 标准化 → 入库 → 详情（事件挂到对应消息 + payload 带输出）→
//! 规则知识提取消费事件。与 e2e/llm 旅程互补：这里验证的是
//! 「修好的 Adapter 数据进库后在 GUI 后端可见什么」。

#![cfg(test)]

use ch_daemon::{DaemonState, DaemonStateConfig};

/// 合成一个新 schema 的 Codex 会话文件（JS 工具桥 + 输出 + wait 噪音）。
fn make_codex_session(dir: &std::path::Path) -> std::path::PathBuf {
    let f = dir.join("rollout-2026-08-20T10-00-00-integration.jsonl");
    let lines = [
        r#"{"timestamp":"2026-08-20T10:00:00.000Z","type":"session_meta","payload":{"id":"it-codex-1","cwd":"/tmp/proj"}}"#,
        r#"{"timestamp":"2026-08-20T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"审计图片素材库"}]}}"#,
        // JS 工具桥：exec + 输出配对
        r#"{"timestamp":"2026-08-20T10:00:03.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"exec","call_id":"cA","input":"const r=await tools.exec_command({cmd:\"printf 'Prompt entries: '; rg -c '.' prompts.md\",\"workdir\":\"/tmp/proj\"});text(r.output);"}}"#,
        r#"{"timestamp":"2026-08-20T10:00:05.000Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"cA","output":[{"type":"input_text","text":"Prompt entries: 520"}]}}"#,
        // wait 噪音（应被滤除）
        r#"{"timestamp":"2026-08-20T10:00:06.000Z","type":"response_item","payload":{"type":"function_call","name":"wait","arguments":"{}","call_id":"cW"}}"#,
        // 助手总结消息
        r#"{"timestamp":"2026-08-20T10:00:08.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"审计完成：520 条 prompt 全部就绪。"}]}}"#,
    ];
    std::fs::write(&f, lines.join("\n")).expect("file I/O failed");
    f
}

/// 旅程：JS 桥会话 → 入库 → 详情事件可读 + 挂在消息下 → 提取正常。
#[test]
fn journey_codex_js_bridge_import_to_detail() {
    let app = tauri::test::mock_app();
    let dir = tempfile::TempDir::new().expect("tempdir");
    use tauri::Manager as _;
    let state = DaemonState::open(DaemonStateConfig {
        data_dir: dir.path().to_path_buf(),
        ..Default::default()
    })
    .expect("open state");
    app.manage(state);
    let state = app.state::<DaemonState>();

    // 1. 解析（修复后的 Adapter）
    let session_file = make_codex_session(dir.path());
    let raw = ch_adapter_codex::parse_session(&session_file).expect("parse codex session");
    assert_eq!(raw.events.len(), 1, "wait 噪音滤除后应只剩 exec 事件");

    // 2. 入库（命令层同款标准化 + 批量导入 + 索引提交）
    let dto =
        crate::commands::import_raw_to_state(&state, raw, Some("Codex"), None).expect("import");
    let conv_id = dto.conversation_id.clone();

    // 3. 详情：事件类型、摘要、payload 输出全链路可见
    let detail = tauri::async_runtime::block_on(crate::commands::get_conversation_detail(
        state.clone(),
        conv_id.clone(),
    ))
    .expect("detail");
    assert_eq!(detail.events.len(), 1);
    let ev = &detail.events[0];
    assert_eq!(
        ev.event_type, "command_completed",
        "输出配对后应为 Completed"
    );
    assert!(
        ev.summary
            .as_deref()
            .unwrap()
            .starts_with("printf 'Prompt entries"),
        "摘要应是命令本身：{:?}",
        ev.summary
    );
    assert!(
        ev.payload_json
            .as_deref()
            .unwrap()
            .contains("Prompt entries: 520"),
        "payload 应含配对输出"
    );

    // 4. 消息归属：user 消息（seq 较小）应挂该事件（GUI eventGrouping 同规则）
    let user_msg = detail
        .messages
        .iter()
        .find(|m| m.role == "user")
        .expect("user message");
    let assistant_msg = detail
        .messages
        .iter()
        .find(|m| m.role == "assistant")
        .expect("assistant message");
    // 事件时间在 user 之后、assistant 之前 → GUI 归属规则会挂到 user 名下
    //（归属按时间戳：序号是消息/事件两条独立流，不作依据）
    let ev_at = ev.created_at_ms.expect("event timestamp");
    assert!(ev_at > user_msg.created_at_ms.expect("user ts"));
    assert!(ev_at < assistant_msg.created_at_ms.expect("assistant ts"));

    // 5. 规则知识提取仍消费事件（命令进 commands 列表）
    let k = tauri::async_runtime::block_on(crate::commands::extract_knowledge(
        state.clone(),
        conv_id,
        None,
    ))
    .expect("rule extract");
    assert!(
        k.commands.iter().any(|c| c.contains("printf")),
        "命令事件应进入知识提取的 commands：{:?}",
        k.commands
    );
}
