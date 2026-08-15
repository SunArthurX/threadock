//! stdio JSON-RPC 服务器循环 + 方法分派（plan §16.1）。

use crate::protocol::{JsonRpcRequest, JsonRpcResponse, PROTOCOL_VERSION};
use crate::state::DaemonState;
use ch_domain::{Conversation, Message, Workspace};
use ch_normalization::{normalize, RawConversation};
use std::io::{BufRead, Write};
use std::path::Path;

/// 在 stdio 上运行协议循环，直到 stdin EOF。
pub fn serve_stdio<R: BufRead, W: Write>(state: &DaemonState, mut stdin: R, stdout: &mut W) {
    let mut line = String::new();
    loop {
        line.clear();
        let n = match stdin.read_line(&mut line) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp = dispatch(state, trimmed);
        let json = serde_json::to_string(&resp).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialize failed"}}"#
                .to_string()
        });
        let _ = writeln!(stdout, "{json}");
        let _ = stdout.flush();
    }
}

fn dispatch(state: &DaemonState, line: &str) -> JsonRpcResponse {
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

    let result = match req.method.as_str() {
        "system.getInfo" => handle_get_info(state),
        "workspace.list" => handle_list_workspaces(state),
        "conversation.list" => handle_list_conversations(state, req.params),
        "conversation.get" => handle_get_conversation(state, req.params),
        "message.list" => handle_list_messages(state, req.params),
        "event.list" => handle_list_events(state, req.params),
        "conversation.delete" => handle_delete(state, req.params),
        "conversation.restore" => handle_restore(state, req.params),
        "conversation.similar" => handle_similar(state, req.params),
        "knowledge.extract" => handle_extract(state, req.params),
        "knowledge.save" => handle_save_knowledge(state, req.params),
        "knowledge.get" => handle_get_knowledge(state, req.params),
        "search.query" => handle_search(state, req.params),
        "provider.sync" => handle_sync(state, req.params),
        other => return JsonRpcResponse::err(id, -32601, format!("method not found: {other}")),
    };

    match result {
        Ok(v) => JsonRpcResponse::ok(id, v),
        Err(msg) => JsonRpcResponse::err(id, -32000, msg),
    }
}

// ── DTO（serde 序列化给客户端）────────────────────────────────────────────

#[derive(serde::Serialize)]
struct InfoDto {
    protocol_version: u32,
    data_dir: String,
    conversation_count: i64,
}

// ── 方法实现 ──────────────────────────────────────────────────────────────

fn handle_get_info(state: &DaemonState) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let count = repo.count_conversations().map_err(|e| e.to_string())?;
    serde_json::to_value(InfoDto {
        protocol_version: PROTOCOL_VERSION,
        data_dir: state.data_dir.to_string_lossy().into_owned(),
        conversation_count: count,
    })
    .map_err(|e| e.to_string())
}

fn handle_list_workspaces(state: &DaemonState) -> Result<serde_json::Value, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let ws = repo.list_workspaces().map_err(|e| e.to_string())?;
    serde_json::to_value(&ws).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct ListConversationsParams {
    #[serde(default)]
    workspace_id: Option<String>,
}

fn handle_list_conversations(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: ListConversationsParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let convs = repo
        .list_conversations(p.workspace_id.as_deref())
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&convs).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct GetConversationParams {
    id: String,
}

fn handle_get_conversation(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: GetConversationParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let conv: Conversation = repo
        .get_conversation(&p.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("conversation not found: {}", p.id))?;
    serde_json::to_value(&conv).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct DeleteParams {
    id: String,
    #[serde(default)]
    hard: Option<bool>,
}

/// 删除会话（plan §11.4）。hard=true 物理删除，否则软删除。
fn handle_delete(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: DeleteParams = serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    if p.hard.unwrap_or(false) {
        repo.hard_delete_conversation(&p.id)
            .map_err(|e| e.to_string())?;
        serde_json::to_value(serde_json::json!({"id": p.id, "deleted": true, "hard": true}))
            .map_err(|e| e.to_string())
    } else {
        repo.soft_delete_conversation(&p.id)
            .map_err(|e| e.to_string())?;
        serde_json::to_value(serde_json::json!({"id": p.id, "deleted": true, "hard": false}))
            .map_err(|e| e.to_string())
    }
}

/// 恢复软删除的会话。
fn handle_restore(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: DeleteParams = serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.restore_conversation(&p.id)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(serde_json::json!({"id": p.id, "restored": true}))
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct SimilarParams {
    conversation_id: String,
    #[serde(default = "default_similar_limit")]
    limit: usize,
}

fn default_similar_limit() -> usize {
    10
}

/// 相似会话推荐（plan §6.7）。
fn handle_similar(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: SimilarParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let target_conv = repo
        .get_conversation(&p.conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("conversation not found: {}", p.conversation_id))?;
    let target_msgs = repo
        .list_messages(&p.conversation_id)
        .map_err(|e| e.to_string())?;
    let target = ch_knowledge::conversation_text(
        &p.conversation_id,
        Some(target_conv.effective_title()),
        &target_msgs,
    );

    let all = repo.list_conversations(None).map_err(|e| e.to_string())?;
    let mut candidates = Vec::new();
    for c in &all {
        if c.id == p.conversation_id {
            continue;
        }
        let msgs = repo.list_messages(&c.id).map_err(|e| e.to_string())?;
        candidates.push(ch_knowledge::conversation_text(
            &c.id,
            Some(c.effective_title()),
            &msgs,
        ));
    }

    let hits = ch_knowledge::find_similar(&target, &candidates, p.limit);
    serde_json::to_value(
        hits.into_iter()
            .map(|h| {
                serde_json::json!({
                    "conversation_id": h.conversation_id,
                    "score": h.score,
                })
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct ListMessagesParams {
    conversation_id: String,
}

fn handle_list_messages(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: ListMessagesParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let msgs: Vec<Message> = repo
        .list_messages(&p.conversation_id)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&msgs).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct ListEventsParams {
    conversation_id: String,
}

fn handle_list_events(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: ListEventsParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let events = repo
        .list_events(&p.conversation_id)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&events).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct ExtractParams {
    conversation_id: String,
}

/// 知识提取（plan §13.5）：摘要/决策/TODO/错误/命令/涉及文件。
fn handle_extract(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: ExtractParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let conv = repo
        .get_conversation(&p.conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("conversation not found: {}", p.conversation_id))?;
    let messages = repo
        .list_messages(&p.conversation_id)
        .map_err(|e| e.to_string())?;
    let events = repo
        .list_events(&p.conversation_id)
        .map_err(|e| e.to_string())?;
    let input = ch_knowledge::ExtractionInput {
        title: Some(conv.effective_title().to_string()),
        messages,
        events,
    };
    let result = ch_knowledge::RuleExtractor::new().extract(&input);
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct SaveKnowledgeParams {
    conversation_id: String,
    result: ch_knowledge::ExtractionResult,
}

/// 保存知识提取结果（plan §13.5 持久化 + 版本）。
fn handle_save_knowledge(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: SaveKnowledgeParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let json = serde_json::to_string(&p.result).map_err(|e| e.to_string())?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let id = repo
        .save_knowledge(&p.conversation_id, &p.result.extractor, &json)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(serde_json::json!({"id": id, "saved": true})).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct GetKnowledgeParams {
    conversation_id: String,
}

/// 读取已保存的知识提取结果（当前版本）。
fn handle_get_knowledge(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: GetKnowledgeParams =
        serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let rec = repo
        .get_knowledge(&p.conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no saved knowledge for conversation {}", p.conversation_id))?;
    serde_json::to_value(serde_json::json!({
        "id": rec.id,
        "version": rec.version,
        "extractor": rec.extractor,
        "result": serde_json::from_str::<serde_json::Value>(&rec.result_json)
            .map_err(|e| e.to_string())?,
    }))
    .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct SearchParams {
    query: String,
    #[serde(default)]
    engine: Option<String>,
}

fn handle_search(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: SearchParams = serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;

    let engine = p.engine.as_deref().unwrap_or("tantivy");
    match engine {
        "tantivy" => {
            let idx = state.search_index.lock().map_err(|e| e.to_string())?;
            let q = ch_search::SearchQuery::new(&p.query);
            let hits = idx.search(&q).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(&hits).map_err(|e| e.to_string())?)
        }
        "fts5" => {
            let repo = state.repo.lock().map_err(|e| e.to_string())?;
            let q = ch_storage::SearchQuery::new(&p.query);
            let hits = repo.search(&q).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(&hits).map_err(|e| e.to_string())?)
        }
        other => Err(format!("unknown search engine: {other}")),
    }
}

#[derive(serde::Deserialize)]
struct SyncParams {
    /// 文件路径（绝对路径）。
    path: String,
    #[serde(default)]
    workspace_name: Option<String>,
}

/// 导入一个文件（plan §8.3 完整流水线）：归档 raw → 解析 → 标准化 → 入库 → 索引。
fn handle_sync(
    state: &DaemonState,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let p: SyncParams = serde_json::from_value(params).map_err(|e| format!("bad params: {e}"))?;
    let path = Path::new(&p.path);

    // 1. Raw 归档
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let raw_store = state.raw_store.lock().map_err(|e| e.to_string())?;
    let raw_payload = raw_store.put(&bytes).map_err(|e| e.to_string())?;
    drop(raw_store);

    // 2. 解析（按扩展名）
    let raw = parse_by_extension(path)?;
    let normalized = normalize(raw).map_err(|e| e.to_string())?;

    // 3. 入库
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.upsert_provider(normalized.conversation.provider)
        .map_err(|e| e.to_string())?;
    let workspace_id = resolve_workspace(&repo, p.workspace_name.as_deref(), path)?;
    let mut conv = normalized.conversation.clone();
    conv.workspace_id = workspace_id.clone();
    let raw_hash = raw_payload.hash.clone();
    conv.raw_payload_id = Some(raw_hash.clone());
    let conversation_id = repo.upsert_conversation(&conv).map_err(|e| e.to_string())?;
    for m in &normalized.messages {
        let mut m = m.clone();
        m.conversation_id = conversation_id.clone();
        repo.upsert_message(&m).map_err(|e| e.to_string())?;
    }
    for e in &normalized.events {
        let mut e = e.clone();
        e.conversation_id = conversation_id.clone();
        repo.upsert_event(&e).map_err(|e| e.to_string())?;
    }
    let conv_title = conv.effective_title().to_string();
    let provider = conv.provider;
    drop(repo);

    // 4. 索引（Tantivy）
    let idx = state.search_index.lock().map_err(|e| e.to_string())?;
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let messages = repo
        .list_messages(&conversation_id)
        .map_err(|e| e.to_string())?;
    drop(repo);
    let mut writer = idx.writer(15_000_000).map_err(|e| e.to_string())?;
    for m in &messages {
        let im = ch_search::index::IndexableMessage {
            message_id: m.id.clone(),
            conversation_id: conversation_id.clone(),
            provider,
            workspace_id: workspace_id.clone(),
            role: m.role,
            title: Some(conv_title.clone()),
            body: m.content_text.clone(),
        };
        idx.index_message(&mut writer, &im)
            .map_err(|e| e.to_string())?;
    }
    idx.commit(writer).map_err(|e| e.to_string())?;

    #[derive(serde::Serialize)]
    struct SyncResult {
        conversation_id: String,
        workspace_id: Option<String>,
        messages: usize,
        events: usize,
        raw_payload_id: String,
    }
    serde_json::to_value(SyncResult {
        conversation_id,
        workspace_id,
        messages: normalized.messages.len(),
        events: normalized.events.len(),
        raw_payload_id: raw_hash,
    })
    .map_err(|e| e.to_string())
}

// ── 辅助 ──────────────────────────────────────────────────────────────────

fn parse_by_extension(path: &Path) -> Result<RawConversation, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jsonl" | "ndjson" => ch_adapter_jsonl::parse_file(path).map_err(|e| e.to_string()),
        _ => ch_adapter_markdown::parse_file(path).map_err(|e| e.to_string()),
    }
}

fn resolve_workspace(
    repo: &ch_storage::Repository,
    name: Option<&str>,
    path: &Path,
) -> Result<Option<String>, String> {
    let Some(name) = name else {
        return Ok(None);
    };
    let parent_path = path.parent().map(|p| p.to_string_lossy().into_owned());
    let mut candidate = ch_identity_resolver::SourceWorkspaceCandidate::new(name);
    candidate.canonical_path = parent_path;

    let known: Vec<_> = repo
        .list_workspaces()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|ws| ch_identity_resolver::IdentityKey {
            workspace_id: ws.id,
            display_name: ws.display_name,
            manifest_id: None,
            git_remote: ws.git_remote,
            git_common_dir: ws.git_common_dir,
            canonical_path: ws.canonical_path,
            filesystem_id: None,
        })
        .collect();

    let resolution = ch_identity_resolver::resolve(&candidate, &known);
    match resolution {
        ch_identity_resolver::Resolution::AutoMerge(m) => Ok(Some(m.workspace_id)),
        ch_identity_resolver::Resolution::NeedsConfirmation {
            candidate: Some(m), ..
        } => Ok(Some(m.workspace_id)),
        _ => {
            let mut ws = Workspace::new(name);
            ws.canonical_path = path.parent().map(|p| p.to_string_lossy().into_owned());
            let id = repo.upsert_workspace(&ws).map_err(|e| e.to_string())?;
            Ok(Some(id))
        }
    }
}

// （Provider/Role 的 Display 已在各自类型上实现，无需额外 import）

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DaemonState;
    use ch_domain::{Conversation, Message, Provider, Role, Workspace};
    use std::io::Cursor;

    fn run(state: &DaemonState, input: &str) -> String {
        let stdin = Cursor::new(input);
        let mut stdout = Vec::new();
        serve_stdio(state, stdin, &mut stdout);
        String::from_utf8(stdout).expect("unexpected None")
    }

    fn send(state: &DaemonState, method: &str, params: serde_json::Value) -> JsonRpcResponse {
        let line = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        })
        .to_string();
        let out = run(state, &line);
        serde_json::from_str(out.trim()).expect("parse failed")
    }

    fn seeded_state() -> DaemonState {
        let state = DaemonState::open_in_memory().expect("unexpected None");
        let repo = state.repo.lock().expect("mutex poisoned");
        repo.upsert_provider(Provider::Generic).expect("upsert failed");
        let mut c = Conversation::new(Provider::Generic, "daemon-src");
        c.title = Some("Daemon 测试".into());
        let cid = repo.upsert_conversation(&c).expect("upsert failed");
        let mut m = Message::new(&cid, Role::User, 1);
        m.content_text = Some("搜索关键词 tauri".into());
        repo.upsert_message(&m).expect("upsert failed");

        // 索引到 Tantivy
        let idx = state.search_index.lock().expect("mutex poisoned");
        let mut writer = idx.writer(15_000_000).expect("file I/O failed");
        idx.index_message(
            &mut writer,
            &ch_search::index::IndexableMessage {
                message_id: m.id.clone(),
                conversation_id: cid.clone(),
                provider: Provider::Generic,
                workspace_id: None,
                role: Role::User,
                title: Some("Daemon 测试".into()),
                body: Some("搜索关键词 tauri".into()),
            },
        )
        .expect("unexpected None");
        idx.commit(writer).expect("file I/O failed");
        drop(idx);
        drop(repo);
        state
    }

    #[test]
    fn system_get_info() {
        let state = DaemonState::open_in_memory().expect("unexpected None");
        let resp = send(&state, "system.getInfo", serde_json::json!({}));
        assert!(resp.result.is_some());
        let info = resp.result.expect("unexpected None");
        assert_eq!(info["protocol_version"], PROTOCOL_VERSION);
    }

    #[test]
    fn workspace_list() {
        let state = DaemonState::open_in_memory().expect("unexpected None");
        let repo = state.repo.lock().expect("mutex poisoned");
        repo.upsert_workspace(&Workspace::new("ws-daemon")).expect("upsert failed");
        drop(repo);
        let resp = send(&state, "workspace.list", serde_json::json!({}));
        let result = resp.result.expect("unexpected None");
        let arr = result.as_array().expect("unexpected None");
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn conversation_list() {
        let state = seeded_state();
        let resp = send(&state, "conversation.list", serde_json::json!({}));
        let result = resp.result.expect("unexpected None");
        let arr = result.as_array().expect("unexpected None");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["title"], "Daemon 测试");
    }

    #[test]
    fn conversation_get() {
        let state = seeded_state();
        let repo = state.repo.lock().expect("mutex poisoned");
        let cid = repo.list_conversations(None).expect("unexpected None")[0].id.clone();
        drop(repo);
        let resp = send(&state, "conversation.get", serde_json::json!({"id": cid}));
        let result = resp.result.expect("unexpected None");
        assert_eq!(result["title"], "Daemon 测试");
    }

    #[test]
    fn conversation_get_not_found() {
        let state = seeded_state();
        let resp = send(
            &state,
            "conversation.get",
            serde_json::json!({"id": "nope"}),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn message_list() {
        let state = seeded_state();
        let repo = state.repo.lock().expect("mutex poisoned");
        let cid = repo.list_conversations(None).expect("unexpected None")[0].id.clone();
        drop(repo);
        let resp = send(
            &state,
            "message.list",
            serde_json::json!({"conversation_id": cid}),
        );
        let result = resp.result.expect("unexpected None");
        let arr = result.as_array().expect("unexpected None");
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn search_tantivy() {
        let state = seeded_state();
        let resp = send(
            &state,
            "search.query",
            serde_json::json!({"query": "tauri", "engine": "tantivy"}),
        );
        let result = resp.result.expect("unexpected None");
        let arr = result.as_array().expect("unexpected None");
        assert!(!arr.is_empty());
    }

    #[test]
    fn event_list() {
        let state = seeded_state();
        // 给 seeded 会话加一个事件
        let repo = state.repo.lock().expect("mutex poisoned");
        let cid = repo.list_conversations(None).expect("unexpected None")[0].id.clone();
        let mut e = ch_domain::Event::new(&cid, ch_domain::EventType::CommandStarted, 1);
        e.summary = Some("cargo build".into());
        repo.upsert_event(&e).expect("upsert failed");
        drop(repo);

        let resp = send(
            &state,
            "event.list",
            serde_json::json!({"conversation_id": cid}),
        );
        let result = resp.result.expect("unexpected None");
        let arr = result.as_array().expect("unexpected None");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["event_type"], "command_started");
        assert_eq!(arr[0]["summary"], "cargo build");
    }

    #[test]
    fn knowledge_extract() {
        let state = seeded_state();
        // 给会话补一条含 TODO/决策的消息
        let repo = state.repo.lock().expect("mutex poisoned");
        let cid = repo.list_conversations(None).expect("unexpected None")[0].id.clone();
        let mut m = ch_domain::Message::new(&cid, ch_domain::Role::Assistant, 2);
        m.content_text = Some("决定用 WorkManager\nTODO 处理 Android 14 限制".into());
        repo.upsert_message(&m).expect("upsert failed");
        drop(repo);

        let resp = send(
            &state,
            "knowledge.extract",
            serde_json::json!({"conversation_id": cid}),
        );
        let result = resp.result.expect("unexpected None");
        assert!(result["extractor"].as_str().expect("unexpected None").contains("rule"));
        assert!(result["decisions"]
            .as_array()
            .expect("unexpected None")
            .iter()
            .any(|d| { d["decision"].as_str().unwrap_or("").contains("WorkManager") }));
        assert!(result["todos"]
            .as_array()
            .expect("unexpected None")
            .iter()
            .any(|t| { t["text"].as_str().unwrap_or("").contains("Android 14") }));
    }

    #[test]
    fn knowledge_save_and_get() {
        let state = seeded_state();
        let repo = state.repo.lock().expect("mutex poisoned");
        let cid = repo.list_conversations(None).expect("unexpected None")[0].id.clone();
        drop(repo);

        // 先提取
        let extract_resp = send(
            &state,
            "knowledge.extract",
            serde_json::json!({"conversation_id": cid}),
        );
        let extracted = extract_resp.result.expect("unexpected None");

        // 保存
        let save_resp = send(
            &state,
            "knowledge.save",
            serde_json::json!({"conversation_id": cid, "result": extracted}),
        );
        assert!(save_resp.result.expect("unexpected None")["saved"].as_bool().expect("unexpected None"));

        // 读取
        let get_resp = send(
            &state,
            "knowledge.get",
            serde_json::json!({"conversation_id": cid}),
        );
        let got = get_resp.result.expect("unexpected None");
        assert_eq!(got["version"], 1);
        assert!(got["extractor"].as_str().expect("unexpected None").contains("rule"));
    }

    #[test]
    fn knowledge_get_not_found_errors() {
        let state = seeded_state();
        let resp = send(
            &state,
            "knowledge.get",
            serde_json::json!({"conversation_id": "nope"}),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn conversation_soft_delete_and_restore() {
        let state = seeded_state();
        let repo = state.repo.lock().expect("mutex poisoned");
        let cid = repo.list_conversations(None).expect("unexpected None")[0].id.clone();
        drop(repo);

        // 软删除
        let resp = send(
            &state,
            "conversation.delete",
            serde_json::json!({"id": cid}),
        );
        let result = resp.result.expect("unexpected None");
        assert_eq!(result["hard"], false);

        // 仍存在但 status=deleted
        let get_resp = send(&state, "conversation.get", serde_json::json!({"id": cid}));
        let conv = get_resp.result.expect("unexpected None");
        assert_eq!(conv["source_status"], "deleted");

        // 恢复
        let restore_resp = send(
            &state,
            "conversation.restore",
            serde_json::json!({"id": cid}),
        );
        assert_eq!(restore_resp.result.expect("unexpected None")["restored"], true);
    }

    #[test]
    fn conversation_hard_delete() {
        let state = seeded_state();
        let repo = state.repo.lock().expect("mutex poisoned");
        let cid = repo.list_conversations(None).expect("unexpected None")[0].id.clone();
        drop(repo);

        let resp = send(
            &state,
            "conversation.delete",
            serde_json::json!({"id": cid, "hard": true}),
        );
        assert_eq!(resp.result.expect("unexpected None")["hard"], true);

        // 再查应报错
        let get_resp = send(&state, "conversation.get", serde_json::json!({"id": cid}));
        assert!(get_resp.error.is_some());
    }

    #[test]
    fn conversation_similar() {
        let state = seeded_state();
        // seeded 已有一条「搜索关键词 tauri」的会话
        // 加一条内容相似的
        let repo = state.repo.lock().expect("mutex poisoned");
        let target_id = repo.list_conversations(None).expect("unexpected None")[0].id.clone();
        let mut c2 = ch_domain::Conversation::new(Provider::Generic, "similar-src");
        c2.title = Some("Tauri 相关".into());
        let cid2 = repo.upsert_conversation(&c2).expect("upsert failed");
        let mut m = ch_domain::Message::new(&cid2, ch_domain::Role::User, 1);
        m.content_text = Some("tauri android 后台任务 搜索".into());
        repo.upsert_message(&m).expect("upsert failed");
        // 加一条无关的
        let c3 = ch_domain::Conversation::new(Provider::Generic, "unrelated");
        let cid3 = repo.upsert_conversation(&c3).expect("upsert failed");
        let mut m3 = ch_domain::Message::new(&cid3, ch_domain::Role::User, 1);
        m3.content_text = Some("python pandas 数据分析".into());
        repo.upsert_message(&m3).expect("upsert failed");
        drop(repo);

        let resp = send(
            &state,
            "conversation.similar",
            serde_json::json!({"conversation_id": target_id}),
        );
        let result = resp.result.expect("unexpected None");
        let arr = result.as_array().expect("unexpected None");
        assert!(!arr.is_empty(), "should find similar conversations");
        // 相似的应排在前面
        assert_eq!(arr[0]["conversation_id"].as_str().expect("unexpected None"), cid2);
    }

    #[test]
    fn search_unknown_engine_errors() {
        let state = seeded_state();
        let resp = send(
            &state,
            "search.query",
            serde_json::json!({"query": "x", "engine": "bogus"}),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn unknown_method_errors() {
        let state = DaemonState::open_in_memory().expect("unexpected None");
        let resp = send(&state, "nope", serde_json::json!({}));
        assert_eq!(resp.error.expect("unexpected None").code, -32601);
    }

    #[test]
    fn invalid_json_errors() {
        let state = DaemonState::open_in_memory().expect("unexpected None");
        let out = run(&state, "not json");
        let resp: JsonRpcResponse = serde_json::from_str(out.trim()).expect("parse failed");
        assert_eq!(resp.error.expect("unexpected None").code, -32700);
    }

    #[test]
    fn eof_terminates_cleanly() {
        let state = DaemonState::open_in_memory().expect("unexpected None");
        let input = format!(
            "{}\n{}\n",
            r#"{"jsonrpc":"2.0","id":1,"method":"system.getInfo","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"workspace.list","params":{}}"#
        );
        let out = run(&state, &input);
        assert_eq!(out.trim().lines().count(), 2);
    }

    #[test]
    fn provider_sync_imports_file() {
        let state = seeded_state();
        let tmp = tempfile::NamedTempFile::with_suffix(".md").expect("tempdir creation failed");
        std::fs::write(tmp.path(), "## User\nhello sync\n## Assistant\nworld\n").expect("file I/O failed");
        let path_str = tmp.path().to_string_lossy().into_owned();

        let resp = send(
            &state,
            "provider.sync",
            serde_json::json!({"path": path_str, "workspace_name": "sync-ws"}),
        );
        let result = resp.result.expect("unexpected None");
        assert!(result["conversation_id"]
            .as_str()
            .expect("unexpected None")
            .starts_with("conv_"));
        assert_eq!(result["messages"], 2);
        assert!(result["raw_payload_id"].as_str().expect("unexpected None").len() == 64);

        // 导入后能搜到
        let repo = state.repo.lock().expect("mutex poisoned");
        assert_eq!(repo.count_conversations().expect("unexpected None"), 2); // seeded 1 + new 1
    }
}
