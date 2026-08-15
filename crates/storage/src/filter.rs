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
    /// Some(true) 只看已软删除；Some(false) 排除已软删除；None 不限。
    pub deleted: Option<bool>,
    /// 起始时间戳（毫秒，闭区间）。None 不限。
    pub started_after_ms: Option<i64>,
    /// 结束时间戳（毫秒，闭区间）。None 不限。
    pub started_before_ms: Option<i64>,
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
    #[must_use]
    pub fn deleted_only(mut self) -> Self {
        self.deleted = Some(true);
        self
    }
    #[must_use]
    pub fn exclude_deleted(mut self) -> Self {
        self.deleted = Some(false);
        self
    }
    /** 限定 started_at 在 [from, to] 闭区间（毫秒时间戳）。 */
    #[must_use]
    pub fn with_started_range_ms(mut self, from: i64, to: i64) -> Self {
        self.started_after_ms = Some(from);
        self.started_before_ms = Some(to);
        self
    }
}
