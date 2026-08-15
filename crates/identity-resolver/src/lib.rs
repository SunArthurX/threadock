//! Workspace 身份解析，对应 plan §4.3「Workspace 合并规则」的七级优先级。
//!
//! ## 为什么这是产品灵魂
//!
//! 同一个代码项目会在 Codex/Cursor/ZCode 各开一堆会话。用户心智里是
//! **一个项目**，不是四个工具的历史。所以领域模型里
//! `WORKSPACE ||--o{ SOURCE_WORKSPACE : maps`——一个统一 Workspace
//! 映射多个来源 Workspace。
//!
//! 把多个来源的 workspace 正确归并到同一个统一 workspace，靠本 crate。
//!
//! ## 七级优先级（plan §4.3，从高到低）
//!
//! 1. **Manual**：用户手动绑定（1.0）
//! 2. **`ManifestId`**：统一 Project Manifest ID（1.0）
//! 3. **`GitRemote`**：规范化 Git Remote URL（0.95）
//! 4. **`GitCommonDir`**：Git Common Directory（0.9）
//! 5. **`CanonicalPath`**：规范化绝对路径（0.85）
//! 6. **`FilesystemId`**：文件系统对象 ID / inode（0.8）
//! 7. **`NameSimilarity`**：名称相似度（0.5，**低置信度，需用户确认**）
//!
//! 任何匹配都必须记录 `match_method / match_confidence / matched_at`（plan §4.3）。

pub mod normalize;
pub mod resolver;

pub use normalize::{canonicalize_git_remote, canonicalize_path, normalized_name};
pub use resolver::{
    resolve, IdentityKey, Resolution, SourceWorkspaceCandidate, AUTO_CONFIRM_THRESHOLD,
};
