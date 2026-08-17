//! Workspace 身份解析器，对应 plan §4.3 七级合并优先级。
//!
//! ## 用法
//!
//! 1. 每个来源 workspace 构造为 [`SourceWorkspaceCandidate`]，携带它已知的
//!    各类标识（manifest id / git remote / path / inode / name）。
//! 2. 调用 [`resolve`] 把候选与一组「已知统一 workspace」对比，返回
//!    [`Resolution`]：要么归并到某个已有 workspace，要么建议新建。
//!
//! ## 置信度阈值
//!
//! [`AUTO_CONFIRM_THRESHOLD`] = 0.75。高于此值自动归并；低于此值（名称相似度等）
//! 降级为 [`Resolution::NeedsConfirmation`]，必须由用户确认（plan §4.3、§3 Epic 3）。

use crate::normalize::{canonicalize_git_remote, canonicalize_path, name_similarity};
use ch_domain::MatchMethod;
use std::path::PathBuf;

/// 自动归并的置信度阈值。低于此值的匹配需用户确认。
pub const AUTO_CONFIRM_THRESHOLD: f64 = 0.75;

/// 一个来源 workspace 的候选标识集合。
///
/// 不是所有字段都必须有——来源不同，能提供的标识也不同。
/// 字段越全，匹配越精确。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceWorkspaceCandidate {
    /// 显示名（来源记录的 workspace 名）。
    pub display_name: String,
    /// 统一 Project Manifest ID（如果来源提供）。
    pub manifest_id: Option<String>,
    /// Git Remote URL（原始，未规范化）。
    pub git_remote: Option<String>,
    /// Git Common Directory（worktree 共享的 .git 目录）。
    pub git_common_dir: Option<String>,
    /// 本地绝对路径（原始，未规范化）。
    pub canonical_path: Option<String>,
    /// 文件系统对象 `ID（inode/st_dev` 组合），用于跨 worktree 识别。
    pub filesystem_id: Option<String>,
}

impl SourceWorkspaceCandidate {
    pub fn new(display_name: impl Into<String>) -> Self {
        Self {
            display_name: display_name.into(),
            ..Default::default()
        }
    }
}

/// 一个已知统一 workspace 提供给 resolver 比对的标识。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IdentityKey {
    pub workspace_id: String,
    pub display_name: String,
    pub manifest_id: Option<String>,
    pub git_remote: Option<String>,
    pub git_common_dir: Option<String>,
    pub canonical_path: Option<String>,
    pub filesystem_id: Option<String>,
}

impl IdentityKey {
    pub fn new(workspace_id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            display_name: display_name.into(),
            ..Default::default()
        }
    }
}

/// 单次匹配的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub workspace_id: String,
    pub method: MatchMethod,
    pub confidence: f64,
}

/// 解析结果。
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// 高置信度自动归并（confidence >= `AUTO_CONFIRM_THRESHOLD`）。
    AutoMerge(Match),
    /// 低置信度候选（名称相似度等），需用户确认。
    /// 携带最佳候选，若无任何候选则为 None（此时等价于 `CreateNew`）。
    NeedsConfirmation {
        candidate: Option<Match>,
        confidence: f64,
    },
    /// 无任何匹配，建议新建统一 workspace。
    CreateNew,
}

impl Resolution {
    #[must_use]
    pub fn is_auto_merge(&self) -> bool {
        matches!(self, Resolution::AutoMerge(_))
    }
    #[must_use]
    pub fn needs_confirmation(&self) -> bool {
        matches!(self, Resolution::NeedsConfirmation { .. })
    }
    #[must_use]
    pub fn is_create_new(&self) -> bool {
        matches!(self, Resolution::CreateNew)
    }
    /// 归并到的 `workspace_id（AutoMerge` 或 `NeedsConfirmation` 的候选）；否则 None。
    #[must_use]
    pub fn matched_workspace_id(&self) -> Option<&str> {
        match self {
            Resolution::AutoMerge(m)
            | Resolution::NeedsConfirmation {
                candidate: Some(m), ..
            } => Some(&m.workspace_id),
            _ => None,
        }
    }
}

/// 把候选与一组已知统一 workspace 比对，返回解析结果。
///
/// 算法：按 plan §4.3 七级优先级逐级尝试，取**最高优先级**（而非最高分数）的命中。
/// 优先级顺序：Manual > `ManifestId` > `GitRemote` > `GitCommonDir` > `CanonicalPath` > `FilesystemId` > `NameSimilarity`。
///
/// 注意：Manual 由调用方在更上层处理（用户已显式指定 `workspace_id`），
/// 本函数不处理 Manual，从 `ManifestId` 开始。
#[must_use]
pub fn resolve(candidate: &SourceWorkspaceCandidate, known: &[IdentityKey]) -> Resolution {
    // 各级独立计算命中，最后按优先级选最高
    let mut best: Option<Match> = None;
    // 记录是否出现过名称相似度候选（用于 NeedsConfirmation）
    let mut name_candidate: Option<Match> = None;
    // 名称候选对应的已知 key 是否带结构化标识（路径/remote/fsid…）。
    // 双方都有结构化标识却在 L2-L6 全未命中 = 证据冲突，
    // 此时即使名称完全相同也不允许静默自动合并（防「同名不同项目」误并）。
    let mut name_candidate_structural = false;
    let candidate_structural = candidate.manifest_id.is_some()
        || candidate.git_remote.is_some()
        || candidate.git_common_dir.is_some()
        || candidate.canonical_path.is_some()
        || candidate.filesystem_id.is_some();

    for key in known {
        // Level 2: ManifestId（1.0）—— 最高非手动级，命中即返回
        if let (Some(cmid), Some(kmid)) = (&candidate.manifest_id, &key.manifest_id) {
            if cmid == kmid {
                return Resolution::AutoMerge(Match {
                    workspace_id: key.workspace_id.clone(),
                    method: MatchMethod::ManifestId,
                    confidence: 1.0,
                });
            }
        }

        // Level 3: GitRemote（0.95）
        if let (Some(crm), Some(krm)) = (&candidate.git_remote, &key.git_remote) {
            let cn = canonicalize_git_remote(crm);
            let kn = canonicalize_git_remote(krm);
            if !cn.is_empty() && cn == kn {
                let m = Match {
                    workspace_id: key.workspace_id.clone(),
                    method: MatchMethod::GitRemote,
                    confidence: 0.95,
                };
                best = Some(pick_stronger(best, m));
                continue;
            }
        }

        // Level 4: GitCommonDir（0.9）
        if let (Some(cg), Some(kg)) = (&candidate.git_common_dir, &key.git_common_dir) {
            let cn = canonicalize_path(cg);
            let kn = canonicalize_path(kg);
            if !cn.is_empty() && cn == kn {
                let m = Match {
                    workspace_id: key.workspace_id.clone(),
                    method: MatchMethod::GitCommonDir,
                    confidence: 0.9,
                };
                best = Some(pick_stronger(best, m));
                continue;
            }
        }

        // Level 5: CanonicalPath（0.85）
        if let (Some(cp), Some(kp)) = (&candidate.canonical_path, &key.canonical_path) {
            let cn = canonicalize_path(cp);
            let kn = canonicalize_path(kp);
            if !cn.is_empty() && cn == kn {
                let m = Match {
                    workspace_id: key.workspace_id.clone(),
                    method: MatchMethod::CanonicalPath,
                    confidence: 0.85,
                };
                best = Some(pick_stronger(best, m));
                continue;
            }
        }

        // Level 6: FilesystemId（0.8）
        if let (Some(cf), Some(kf)) = (&candidate.filesystem_id, &key.filesystem_id) {
            if cf == kf {
                let m = Match {
                    workspace_id: key.workspace_id.clone(),
                    method: MatchMethod::FilesystemId,
                    confidence: 0.8,
                };
                best = Some(pick_stronger(best, m));
                continue;
            }
        }

        // Level 7: NameSimilarity（≤0.7）—— 不参与 best，单列作候选
        let sim = name_similarity(&candidate.display_name, &key.display_name);
        if sim > 0.0 {
            let m = Match {
                workspace_id: key.workspace_id.clone(),
                method: MatchMethod::NameSimilarity,
                confidence: sim,
            };
            let key_structural = key.manifest_id.is_some()
                || key.git_remote.is_some()
                || key.git_common_dir.is_some()
                || key.canonical_path.is_some()
                || key.filesystem_id.is_some();
            let stronger =
                pick_stronger(name_candidate.clone(), m.clone()).workspace_id == m.workspace_id;
            if stronger {
                name_candidate_structural = key_structural;
            }
            name_candidate = Some(pick_stronger(name_candidate, m));
        }
    }

    match best {
        // 有高优先级命中（confidence >= 阈值）→ 自动归并
        Some(m) if m.confidence >= AUTO_CONFIRM_THRESHOLD => Resolution::AutoMerge(m),
        // 有高优先级命中但低于阈值（理论上不会发生，因为 L2-L6 都 ≥0.8），
        // 仍按需确认处理
        Some(m) => {
            let confidence = m.confidence;
            Resolution::NeedsConfirmation {
                candidate: Some(m),
                confidence,
            }
        }
        // 无高优先级命中，但有名称候选
        None => match name_candidate {
            // 名称完全相同（confidence 1.0）→ 允许自动归并，
            // 除非双方都带结构化标识却全未命中（同名不同项目的冲突证据）
            Some(m)
                if m.confidence >= AUTO_CONFIRM_THRESHOLD
                    && !(candidate_structural && name_candidate_structural) =>
            {
                Resolution::AutoMerge(m)
            }
            // 名称相似但未达阈值 / 结构化证据冲突 → 需用户确认
            Some(m) => {
                let confidence = m.confidence;
                Resolution::NeedsConfirmation {
                    candidate: Some(m),
                    confidence,
                }
            }
            None => Resolution::CreateNew,
        },
    }
}

/// 取优先级更高（method 排序更靠前）的 match；同 method 取更高 confidence。
fn pick_stronger(a: Option<Match>, b: Match) -> Match {
    match a {
        None => b,
        Some(prev) => {
            if method_priority(b.method) < method_priority(prev.method)
                || (method_priority(b.method) == method_priority(prev.method)
                    && b.confidence > prev.confidence)
            {
                b
            } else {
                prev
            }
        }
    }
}

/// method 优先级数字（越小越优先）。对齐 plan §4.3 顺序。
fn method_priority(m: MatchMethod) -> u8 {
    match m {
        MatchMethod::Manual => 0,
        MatchMethod::ManifestId => 1,
        MatchMethod::GitRemote => 2,
        MatchMethod::GitCommonDir => 3,
        MatchMethod::CanonicalPath => 4,
        MatchMethod::FilesystemId => 5,
        MatchMethod::NameSimilarity => 6,
    }
}

/// 便利：从候选生成新建统一 workspace 时用的显示名。
#[must_use]
pub fn display_name_for_new(candidate: &SourceWorkspaceCandidate) -> String {
    // 优先用目录名，其次原名
    if let Some(p) = &candidate.canonical_path {
        let canon = canonicalize_path(p);
        if let Some(last) = canon.split('/').next_back() {
            if !last.is_empty() {
                return last.to_string();
            }
        }
    }
    candidate.display_name.clone()
}

/// 计算用于入库记录的 PathBuf（规范化后）。供 `source_workspaces` 表的 `raw_path` 用。
#[must_use]
pub fn canonical_path_buf(raw: &str) -> PathBuf {
    PathBuf::from(canonicalize_path(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::MatchMethod;

    fn known(id: &str, name: &str) -> IdentityKey {
        IdentityKey::new(id, name)
    }

    // ── Level 2: ManifestId ──────────────────────────────────────────────

    #[test]
    fn manifest_id_match_auto_merges() {
        let mut c = SourceWorkspaceCandidate::new("any");
        c.manifest_id = Some("manifest-xyz".into());
        let mut k = known("ws_1", "different-name");
        k.manifest_id = Some("manifest-xyz".into());
        let r = resolve(&c, &[k]);
        assert!(r.is_auto_merge());
        let Resolution::AutoMerge(m) = r else {
            unreachable!()
        };
        assert_eq!(m.workspace_id, "ws_1");
        assert_eq!(m.method, MatchMethod::ManifestId);
        assert!((m.confidence - 1.0).abs() < 1e-9);
    }

    // ── Level 3: GitRemote ───────────────────────────────────────────────

    #[test]
    fn git_remote_match_ssh_vs_https() {
        // 同一仓库的 SSH 与 HTTPS 形式应归并
        let mut c = SourceWorkspaceCandidate::new("repo-a");
        c.git_remote = Some("git@github.com:org/repo.git".into());
        let mut k = known("ws_2", "repo-b");
        k.git_remote = Some("https://github.com/org/repo".into());
        let r = resolve(&c, &[k]);
        assert!(r.is_auto_merge());
        if let Resolution::AutoMerge(m) = r {
            assert_eq!(m.method, MatchMethod::GitRemote);
            assert!(m.confidence >= 0.9);
        }
    }

    #[test]
    fn git_remote_different_repo_no_merge() {
        let mut c = SourceWorkspaceCandidate::new("a");
        c.git_remote = Some("git@github.com:org/repo-a.git".into());
        let mut k = known("ws", "b");
        k.git_remote = Some("https://github.com/org/repo-b".into());
        let r = resolve(&c, &[k]);
        // 不同仓库，名称也不同 → CreateNew
        assert!(r.is_create_new() || r.needs_confirmation());
    }

    // ── Level 4: GitCommonDir ────────────────────────────────────────────

    #[test]
    fn git_common_dir_match() {
        let mut c = SourceWorkspaceCandidate::new("wt1");
        c.git_common_dir = Some("/proj/.git".into());
        let mut k = known("ws_3", "main");
        k.git_common_dir = Some("/proj/.git/worktrees/wt1".into());
        // common_dir 不同，这里测完全相等
        k.git_common_dir = Some("/proj/.git".into());
        let r = resolve(&c, &[k]);
        assert!(r.is_auto_merge());
        if let Resolution::AutoMerge(m) = r {
            assert_eq!(m.method, MatchMethod::GitCommonDir);
        }
    }

    // ── Level 5: CanonicalPath ───────────────────────────────────────────

    #[test]
    fn canonical_path_match_ignores_trailing_slash() {
        let mut c = SourceWorkspaceCandidate::new("foo");
        c.canonical_path = Some("/home/u/foo".into());
        let mut k = known("ws_4", "foo");
        k.canonical_path = Some("/home/u/foo/".into());
        let r = resolve(&c, &[k]);
        assert!(r.is_auto_merge());
    }

    #[test]
    fn canonical_path_resolves_dotdot() {
        let mut c = SourceWorkspaceCandidate::new("foo");
        c.canonical_path = Some("/home/u/bar/../foo".into());
        let mut k = known("ws_5", "foo");
        k.canonical_path = Some("/home/u/foo".into());
        let r = resolve(&c, &[k]);
        assert!(r.is_auto_merge());
    }

    // ── Level 6: FilesystemId ────────────────────────────────────────────

    #[test]
    fn filesystem_id_match() {
        let mut c = SourceWorkspaceCandidate::new("x");
        c.filesystem_id = Some("inode-123:dev-45".into());
        let mut k = known("ws_6", "y");
        k.filesystem_id = Some("inode-123:dev-45".into());
        let r = resolve(&c, &[k]);
        assert!(r.is_auto_merge());
        if let Resolution::AutoMerge(m) = r {
            assert_eq!(m.method, MatchMethod::FilesystemId);
        }
    }

    // ── Level 7: NameSimilarity（需确认）────────────────────────────────

    #[test]
    fn name_similarity_below_threshold_needs_confirmation() {
        // 名称包含关系 → 0.7 < 0.75 → 需确认
        let c = SourceWorkspaceCandidate::new("my-web-app");
        let k = known("ws_7", "web-app");
        let r = resolve(&c, &[k]);
        assert!(
            r.needs_confirmation(),
            "0.7 similarity must need confirmation"
        );
        if let Resolution::NeedsConfirmation { candidate, .. } = r {
            assert!(candidate.is_some());
            if let Some(m) = candidate {
                assert_eq!(m.method, MatchMethod::NameSimilarity);
            }
        }
    }

    #[test]
    fn identical_name_auto_merges() {
        // 名称完全相同 → similarity 1.0 >= 阈值 → 自动归并
        let c = SourceWorkspaceCandidate::new("same-proj");
        let k = known("ws_8", "same-proj");
        let r = resolve(&c, &[k]);
        assert!(r.is_auto_merge(), "identical name should auto-merge");
    }

    // ── CreateNew ────────────────────────────────────────────────────────

    #[test]
    fn no_overlap_creates_new() {
        let c = SourceWorkspaceCandidate::new("unique-thing");
        let knowns = [
            known("ws_a", "completely-different"),
            known("ws_b", "another-one"),
        ];
        let r = resolve(&c, &knowns);
        assert!(r.is_create_new());
    }

    // ── 优先级 ────────────────────────────────────────────────────────────

    #[test]
    fn higher_priority_wins_over_lower() {
        // 候选同时命中 path(0.85) 和 name(1.0)；path 优先级更高
        let mut c = SourceWorkspaceCandidate::new("proj");
        c.canonical_path = Some("/home/u/proj".into());
        let mut k1 = known("ws_path", "proj");
        k1.canonical_path = Some("/home/u/proj".into());
        let k2 = known("ws_name", "proj"); // 名称也匹配
        let r = resolve(&c, &[k1, k2]);
        assert!(r.is_auto_merge());
        if let Resolution::AutoMerge(m) = r {
            // path 优先级高于 name
            assert_eq!(m.workspace_id, "ws_path");
            assert_eq!(m.method, MatchMethod::CanonicalPath);
        }
    }

    #[test]
    fn manifest_beats_git_remote() {
        // ManifestId 命中应优先于 GitRemote
        let mut c = SourceWorkspaceCandidate::new("x");
        c.manifest_id = Some("m1".into());
        c.git_remote = Some("git@github.com:o/r1.git".into());
        let mut k_manifest = known("ws_m", "different");
        k_manifest.manifest_id = Some("m1".into());
        let mut k_remote = known("ws_r", "yet-another");
        k_remote.git_remote = Some("https://github.com/o/r1".into());
        let r = resolve(&c, &[k_remote, k_manifest]);
        if let Resolution::AutoMerge(m) = r {
            assert_eq!(m.workspace_id, "ws_m");
            assert_eq!(m.method, MatchMethod::ManifestId);
        } else {
            panic!("should auto-merge");
        }
    }

    // ── 辅助 ──────────────────────────────────────────────────────────────

    #[test]
    fn display_name_for_new_prefers_dirname() {
        let mut c = SourceWorkspaceCandidate::new("原名");
        c.canonical_path = Some("/home/u/my-project".into());
        assert_eq!(display_name_for_new(&c), "my-project");
    }

    #[test]
    fn display_name_for_new_fallbacks_to_raw_name() {
        let c = SourceWorkspaceCandidate::new("just-a-name");
        assert_eq!(display_name_for_new(&c), "just-a-name");
    }

    #[test]
    fn matched_workspace_id_helpers() {
        let m = Match {
            workspace_id: "ws_x".into(),
            method: MatchMethod::GitRemote,
            confidence: 0.95,
        };
        let auto = Resolution::AutoMerge(m.clone());
        assert_eq!(auto.matched_workspace_id(), Some("ws_x"));

        let nc = Resolution::NeedsConfirmation {
            candidate: Some(m.clone()),
            confidence: 0.7,
        };
        assert_eq!(nc.matched_workspace_id(), Some("ws_x"));

        let new = Resolution::CreateNew;
        assert_eq!(new.matched_workspace_id(), None);
    }
}
