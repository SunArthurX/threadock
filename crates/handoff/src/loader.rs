//! 从 Repository 装载 handoff 输入（CLI / daemon / 桌面端三端共用）。
//!
//! 批量装载默认跳过软删除与子任务会话（`source_parent_id` 非空）：
//! 子任务是主会话派生的子代理线程，单独交接只会产生噪音；按 id 点名时不跳过。

use crate::{HandoffError, HandoffInput, HandoffResult};
use ch_domain::{Conversation, Event, Message, Workspace};
use ch_storage::{ConversationFilter, Repository};

/// 拥有全部数据的 handoff 输入（借用视图经 [`OwnedHandoffInput::as_input`] 获取）。
#[derive(Debug, Clone)]
pub struct OwnedHandoffInput {
    pub conversation: Conversation,
    pub workspace: Option<Workspace>,
    pub messages: Vec<Message>,
    pub events: Vec<Event>,
}

impl OwnedHandoffInput {
    /// 借用视图。
    #[must_use]
    pub fn as_input(&self) -> HandoffInput<'_> {
        HandoffInput {
            conversation: &self.conversation,
            workspace: self.workspace.as_ref(),
            messages: &self.messages,
            events: &self.events,
        }
    }
}

fn storage_err(e: &ch_storage::StorageError) -> HandoffError {
    HandoffError::Storage(e.to_string())
}

/// 读取用户自定义脱敏规则（enabled 的；与导出路径同一来源，plan §14.6）。
///
/// 读取失败时降级为空（脱敏缺失不阻断交接——与桌面导出的容错口径一致）。
#[must_use]
pub fn load_redaction_rules(repo: &Repository) -> Vec<ch_export::CustomRule> {
    repo.list_redaction_rules()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.enabled)
        .map(|r| ch_export::CustomRule::new(r.name, r.pattern))
        .collect()
}

/// 装载一条会话的消息、事件与工作区。
pub fn load_conversation(
    repo: &Repository,
    conversation: Conversation,
) -> HandoffResult<OwnedHandoffInput> {
    let messages = repo
        .list_messages(&conversation.id)
        .map_err(|e| storage_err(&e))?;
    let events = repo
        .list_events(&conversation.id)
        .map_err(|e| storage_err(&e))?;
    let workspace = match &conversation.workspace_id {
        Some(ws) => repo.get_workspace(ws).map_err(|e| storage_err(&e))?,
        None => None,
    };
    Ok(OwnedHandoffInput {
        conversation,
        workspace,
        messages,
        events,
    })
}

/// 按内部 id 点名装载（找不到的 id 记入错误，不中断其余）。
pub fn load_by_ids(
    repo: &Repository,
    ids: &[String],
) -> HandoffResult<(Vec<OwnedHandoffInput>, Vec<String>)> {
    let mut found = Vec::with_capacity(ids.len());
    let mut missing = Vec::new();
    for id in ids {
        match repo.get_conversation(id).map_err(|e| storage_err(&e))? {
            Some(conv) => found.push(load_conversation(repo, conv)?),
            None => missing.push(id.clone()),
        }
    }
    Ok((found, missing))
}

/// 按过滤条件批量装载（排除软删除与子任务）。
pub fn load_filtered(
    repo: &Repository,
    filter: &ConversationFilter,
) -> HandoffResult<Vec<OwnedHandoffInput>> {
    let mut filter = filter.clone();
    if filter.deleted.is_none() {
        filter.deleted = Some(false);
    }
    let convs = repo
        .list_conversations_filtered(&filter)
        .map_err(|e| storage_err(&e))?;
    let mut out = Vec::with_capacity(convs.len());
    for conv in convs {
        if conv.source_parent_id.is_some() {
            continue;
        }
        out.push(load_conversation(repo, conv)?);
    }
    Ok(out)
}

/// 借用视图列表（配合 [`crate::handoff_batch`]）。
#[must_use]
pub fn as_inputs(owned: &[OwnedHandoffInput]) -> Vec<HandoffInput<'_>> {
    owned.iter().map(OwnedHandoffInput::as_input).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{EventType, Provider, Role};

    fn seeded() -> (Repository, String, String) {
        let repo = Repository::open_in_memory().expect("in-memory repo failed");
        repo.upsert_provider(Provider::ZCode)
            .expect("upsert failed");
        let mut ws = Workspace::new("demo");
        ws.canonical_path = Some("/tmp/demo".into());
        let ws_id = repo.upsert_workspace(&ws).expect("upsert failed");

        let mut parent = Conversation::new(Provider::ZCode, "src-parent");
        parent.title = Some("主会话".into());
        parent.workspace_id = Some(ws_id);
        let parent_id = repo.upsert_conversation(&parent).expect("upsert failed");
        let mut m = Message::new(&parent_id, Role::User, 1);
        m.content_text = Some("hello".into());
        repo.upsert_message(&m).expect("upsert failed");
        let mut e = Event::new(&parent_id, EventType::CommandCompleted, 1);
        e.summary = Some("ls".into());
        repo.upsert_event(&e).expect("upsert failed");

        let mut child = Conversation::new(Provider::ZCode, "src-child");
        child.source_parent_id = Some("src-parent".into());
        let child_id = repo.upsert_conversation(&child).expect("upsert failed");
        let mut cm = Message::new(&child_id, Role::User, 1);
        cm.content_text = Some("child".into());
        repo.upsert_message(&cm).expect("upsert failed");

        (repo, parent_id, child_id)
    }

    #[test]
    fn load_by_ids_reports_missing() {
        let (repo, parent_id, _child) = seeded();
        let (found, missing) =
            load_by_ids(&repo, &[parent_id.clone(), "conv_nope".into()]).expect("load failed");
        assert_eq!(found.len(), 1);
        assert_eq!(missing, vec!["conv_nope".to_string()]);
        let input = found[0].as_input();
        assert_eq!(input.messages.len(), 1);
        assert_eq!(input.events.len(), 1);
        assert_eq!(input.workspace_path(), Some("/tmp/demo"));
    }

    #[test]
    fn load_filtered_skips_children_and_deleted() {
        let (repo, parent_id, child_id) = seeded();
        let all = load_filtered(&repo, &ConversationFilter::new()).expect("load failed");
        assert_eq!(all.len(), 1, "子任务会话应被跳过");
        assert_eq!(all[0].conversation.id, parent_id);
        assert_ne!(all[0].conversation.id, child_id);

        repo.soft_delete_conversation(&parent_id)
            .expect("soft delete failed");
        let after = load_filtered(&repo, &ConversationFilter::new()).expect("load failed");
        assert!(after.is_empty(), "软删除会话默认不交接");
    }
}
