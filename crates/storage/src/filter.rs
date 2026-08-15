//! 会话过滤条件，对应 plan §6.4「按时间、来源、Workspace、状态和标签筛选」。

use ch_domain::Provider;

/// 会话列表过滤条件。
///
/// 所有字段可选；None 表示不过滤该维度。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConversationFilter {
    pub provider: Option<Provider>,
    pub workspace_id: Option<String>,
    /// Some(true) 只看收藏；Some(false) 只看未收藏；None 不限。
    pub favorite: Option<bool>,
    /// Some(true) 只看已归档；Some(false) 只看未归档；None 不限。
    pub archived: Option<bool>,
}

impl ConversationFilter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn with_provider(mut self, p: Provider) -> Self {
        self.provider = Some(p);
        self
    }
    #[must_use]
    pub fn with_workspace(mut self, id: impl Into<String>) -> Self {
        self.workspace_id = Some(id.into());
        self
    }
    #[must_use]
    pub fn favorites_only(mut self) -> Self {
        self.favorite = Some(true);
        self
    }
    #[must_use]
    pub fn unarchived_only(mut self) -> Self {
        self.archived = Some(false);
        self
    }
    #[must_use]
    pub fn archived_only(mut self) -> Self {
        self.archived = Some(true);
        self
    }
}
