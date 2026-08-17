//! Workspace 合并准确率基准（plan P2-2 / Gate 1 验收：统一 Project 分组准确率 ≥ 95%）。
//!
//! 标注样本集覆盖 plan §4.3 七级优先级的各命中路径与典型负例：
//! - L2 ManifestId、L3 GitRemote（含 https/ssh/.git 变体归一）、L4 GitCommonDir
//! - L5 CanonicalPath、L6 FilesystemId、L7 NameSimilarity（高分同义词）
//! - 负例：名字相近的不同项目、不同 remote、不同路径 —— 期望 CreateNew
//!
//! 运行：`cargo test -p ch-identity-resolver --test merge_accuracy -- --nocapture`
//! 输出逐例判定表与总准确率；断言 ≥ 0.95（Gate 1 红线）。

use ch_identity_resolver::{resolve, IdentityKey, Resolution, SourceWorkspaceCandidate};

struct Case {
    name: &'static str,
    candidate: SourceWorkspaceCandidate,
    /// 期望：Some(workspace_id) = 应归并到该 ws；None = 应新建。
    expect_merge_into: Option<&'static str>,
}

fn key(id: &str) -> IdentityKey {
    let mut k = IdentityKey::new(id, id);
    match id {
        // 项目 A：git remote 为主标识（https 与 ssh 写法应归一命中）
        "ws-alpha" => {
            k.git_remote = Some("https://github.com/acme/alpha.git".to_string());
            k.canonical_path = Some("/Users/dev/work/alpha".to_string());
        }
        // 项目 B：monorepo，靠 manifest id 锚定
        "ws-beta" => {
            k.manifest_id = Some("manifest-beta-42".to_string());
            k.display_name = "beta-monorepo".into();
        }
        // 项目 C：worktree 场景，共享 git common dir
        "ws-gamma" => {
            k.git_common_dir = Some("/Users/dev/work/gamma/.git".to_string());
            k.canonical_path = Some("/Users/dev/work/gamma".to_string());
        }
        // 项目 D：仅本地路径
        "ws-delta" => {
            k.canonical_path = Some("/Users/dev/projects/delta".to_string());
        }
        // 项目 E：仅名称（无任何结构化标识）
        "epsilon-app" => {
            k.display_name = "Epsilon App".into();
        }
        // 干扰项：与 A 同名不同 remote、与 D 相似路径
        "ws-zeta" => {
            k.git_remote = Some("git@github.com:other/alpha.git".to_string());
            k.canonical_path = Some("/Users/dev/projects/delta-v2".to_string());
        }
        _ => {}
    }
    k
}

/// 标注样本表（数据驱动，行数即样本数，不做人为拆分）
#[allow(clippy::too_many_lines)]
fn cases() -> Vec<Case> {
    let alpha_remote_ssh = {
        let mut c = SourceWorkspaceCandidate::new("alpha");
        c.git_remote = Some("git@github.com:acme/alpha.git".to_string());
        c
    };
    let alpha_remote_noext = {
        let mut c = SourceWorkspaceCandidate::new("alpha-mirror");
        c.git_remote = Some("https://github.com/acme/alpha".to_string());
        c
    };
    let beta_by_manifest = {
        let mut c = SourceWorkspaceCandidate::new("beta");
        c.manifest_id = Some("manifest-beta-42".to_string());
        c
    };
    let gamma_worktree = {
        let mut c = SourceWorkspaceCandidate::new("gamma-feature-x");
        // worktree：common dir 与主仓一致，路径不同
        c.git_common_dir = Some("/Users/dev/work/gamma/.git".to_string());
        c.canonical_path = Some("/Users/dev/work/gamma/.git/worktrees/feature-x".to_string());
        c
    };
    let delta_same_path = {
        let mut c = SourceWorkspaceCandidate::new("delta");
        c.canonical_path = Some("/Users/dev/projects/delta/".to_string());
        c
    };
    let delta_by_fs_id = {
        let mut c = SourceWorkspaceCandidate::new("delta-renamed-dir");
        c.filesystem_id = Some("inode=99123,dev=17".to_string());
        c
    };
    let epsilon_case_variant = SourceWorkspaceCandidate::new("epsilon app");
    let new_project_plain = SourceWorkspaceCandidate::new("brand-new-project");
    let similar_name_different_project = SourceWorkspaceCandidate::new("alpha-prime");
    let same_remote_family_different_repo = {
        let mut c = SourceWorkspaceCandidate::new("alpha");
        c.git_remote = Some("https://github.com/acme/alpha-lib".to_string());
        c
    };
    let similar_path_different_project = {
        let mut c = SourceWorkspaceCandidate::new("delta");
        c.canonical_path = Some("/Users/dev/projects/delta-v2/sub".to_string());
        c
    };

    vec![
        Case {
            name: "L3: ssh remote ↔ https remote（归一命中 alpha）",
            candidate: alpha_remote_ssh,
            expect_merge_into: Some::<&str>("ws-alpha"),
        },
        Case {
            name: "L3: 无 .git 后缀 remote（归一命中 alpha）",
            candidate: alpha_remote_noext,
            expect_merge_into: Some::<&str>("ws-alpha"),
        },
        Case {
            name: "L2: manifest id 精确命中 beta",
            candidate: beta_by_manifest,
            expect_merge_into: Some::<&str>("ws-beta"),
        },
        Case {
            name: "L4: worktree 共享 common dir 命中 gamma",
            candidate: gamma_worktree,
            expect_merge_into: Some::<&str>("ws-gamma"),
        },
        Case {
            name: "L5: 同路径（尾斜杠差异）命中 delta",
            candidate: delta_same_path,
            expect_merge_into: Some::<&str>("ws-delta"),
        },
        Case {
            name: "全新项目 → 新建",
            candidate: new_project_plain,
            expect_merge_into: None,
        },
        Case {
            name: "负例：相似名 alpha-prime ≠ alpha（不同项目）",
            candidate: similar_name_different_project,
            expect_merge_into: None,
        },
        Case {
            name: "负例：同组织不同仓库 alpha-lib ≠ alpha",
            candidate: same_remote_family_different_repo,
            expect_merge_into: None,
        },
    ]
    .into_iter()
    .chain([
        // L6 filesystem_id / L7 名称相似度：需要已知侧提供对应标识
        Case {
            name: "L6: filesystem_id 命中（已知侧同 fs id 的 delta）",
            candidate: delta_by_fs_id,
            expect_merge_into: Some::<&str>("ws-delta-fs"),
        },
        Case {
            name: "L7: 名称大小写变体命中 epsilon",
            candidate: epsilon_case_variant,
            expect_merge_into: Some::<&str>("epsilon-app"),
        },
        Case {
            name: "负例：delta-v2/sub 是 zeta 的路径不是 delta 的",
            candidate: similar_path_different_project,
            expect_merge_into: None,
        },
    ])
    .collect()
}

/// 已知 workspace 池：基础 6 个 + L6/L7 专用 2 个。
fn known_pool() -> Vec<IdentityKey> {
    let mut pool: Vec<IdentityKey> = [
        "ws-alpha",
        "ws-beta",
        "ws-gamma",
        "ws-delta",
        "epsilon-app",
        "ws-zeta",
    ]
    .iter()
    .map(|id| key(id))
    .collect();
    let mut delta_fs = IdentityKey::new("ws-delta-fs", "delta");
    delta_fs.filesystem_id = Some("inode=99123,dev=17".to_string());
    pool.push(delta_fs);
    pool.push(key("epsilon-app"));
    pool
}

#[test]
fn merge_accuracy_at_least_95_percent() {
    let pool = known_pool();
    let cases = cases();
    let total = cases.len();
    let mut correct = 0usize;
    let mut auto_merges = 0usize;
    let mut wrong_auto_merges = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for c in &cases {
        let r = resolve(&c.candidate, &pool);
        // 计分规则（Gate 1「自动分组准确率」语义）：
        // - 错误的 AutoMerge 是唯一硬失败（把不同项目静默并到一起）
        // - NeedsConfirmation 给出正确候选 = 正确（走人工确认交互，plan §4.3）
        // - 期望新建而 NeedsConfirmation/CreateNew = 正确（不误并）
        let pass = match (&r, c.expect_merge_into) {
            (Resolution::AutoMerge(m), Some(expect)) => m.workspace_id == expect,
            (Resolution::AutoMerge(_), None) | (Resolution::CreateNew, Some(_)) => false,
            (Resolution::NeedsConfirmation { candidate, .. }, Some(expect)) => {
                candidate.as_ref().is_some_and(|m| m.workspace_id == expect)
            }
            (Resolution::NeedsConfirmation { .. } | Resolution::CreateNew, None) => true,
        };
        if let Resolution::AutoMerge(m) = &r {
            auto_merges += 1;
            if !matches!((m, c.expect_merge_into), (m, Some(e)) if m.workspace_id == *e) {
                wrong_auto_merges += 1;
            }
        }
        if pass {
            correct += 1;
        } else {
            failures.push(format!(
                "  ✗ {}：期望 {:?}，实际 {r:?}",
                c.name, c.expect_merge_into
            ));
        }
    }
    let accuracy = correct as f64 / total as f64;
    println!("── Workspace 合并准确率（{total} 例）─────────────────");
    println!(
        "  判定正确 {correct}/{total} = {:.1}% · AutoMerge {auto_merges} 次（其中错误 {wrong_auto_merges} 次）",
        accuracy * 100.0
    );
    if !failures.is_empty() {
        println!("失败明细：");
        for f in &failures {
            println!("{f}");
        }
    }
    assert_eq!(
        wrong_auto_merges, 0,
        "存在错误的自动合并（不同项目被静默并组）"
    );
    assert!(
        accuracy >= 0.95,
        "合并准确率 {:.1}% 低于 Gate 1 红线 95%：\n{}",
        accuracy * 100.0,
        failures.join("\n")
    );
}
