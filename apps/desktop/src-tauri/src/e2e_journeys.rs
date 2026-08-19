//! Tauri 命令层端到端旅程测试（模拟真人 APP 的后端路径）。
//!
//! 用 `tauri::test` 的 mock app 承载真实 `DaemonState`（临时目录落盘：
//! SQLite + Tantivy + Raw Store 全真），直接调用 GUI 实际 invoke 的命令函数，
//! 按用户旅程串联：导入 → 浏览 → 搜索 → 治理 → 删除。
//!
//! 与 lib.rs 里针对 inner 函数的测试互补：这里验证的是「命令层参数/返回 DTO
//! 到状态层」的完整链路。

#![cfg(test)]

use crate::commands::*;
use ch_daemon::{DaemonState, DaemonStateConfig};

/// 一个 mock app + 指向临时目录的真实后端状态。
struct Harness {
    app: tauri::App<tauri::test::MockRuntime>,
    _dir: tempfile::TempDir,
}

fn harness() -> Harness {
    let dir = tempfile::TempDir::new().expect("tempdir");
    use tauri::Manager as _;
    let app = tauri::test::mock_app();
    let state = DaemonState::open(DaemonStateConfig {
        data_dir: dir.path().to_path_buf(),
        ..Default::default()
    })
    .expect("open state");
    app.manage(state);
    Harness { app, _dir: dir }
}

impl Harness {
    fn state(&self) -> tauri::State<'_, DaemonState> {
        use tauri::Manager as _;
        self.app.state::<DaemonState>()
    }
}

/// 把 fixture 拷到独立目录后返回新路径：同一源目录会被 resolver 按
/// CanonicalPath 正确判定为同一项目（这是产品行为，不是 bug），
/// 想要「两个项目」就必须给两个不同的目录。
fn fixture_in_dir(dir: &std::path::Path, name: &str) -> String {
    let src_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/markdown")
        .join(name);
    let dst = dir.join(name);
    std::fs::copy(&src_path, &dst).expect("copy fixture");
    dst.to_string_lossy().into_owned()
}

/// 旅程 1：会话生命周期（导入 → 详情 → 搜索 → 收藏/标签 → 知识 →
/// 原始视图 → 恢复命令 → 软删/恢复 → 硬删）。
#[test]
fn journey_conversation_lifecycle() {
    let h = harness();
    let state = h.state();

    // 导入（真实文件 → raw 归档 → 解析 → 批量入库 → 索引）
    let imported = tauri::async_runtime::block_on(import_file(
        state.clone(),
        fixture_in_dir(h._dir.path(), "tauri-background.md"),
        Some("e2e-app-a".into()),
    ))
    .expect("import");
    assert!(!imported.conversation_id.is_empty());
    assert!(imported.messages >= 3, "应有消息");

    // 列表 + 详情
    let list = tauri::async_runtime::block_on(list_conversations(
        state.clone(),
        None,
        None,
        None,
        None,
        None,
    ))
    .expect("list");
    assert_eq!(list.len(), 1);
    let detail = tauri::async_runtime::block_on(get_conversation_detail(
        state.clone(),
        imported.conversation_id.clone(),
    ))
    .expect("detail");
    assert!(!detail.messages.is_empty());
    assert!(!detail.completeness_label.is_empty(), "完整度标签应存在");

    // 搜索：纯文本 + 查询语法（provider: 前缀走 Tantivy 索引内过滤）
    let plain = tauri::async_runtime::block_on(search(state.clone(), "WorkManager".into(), None))
        .expect("search plain");
    assert!(!plain.is_empty(), "纯文本搜索应有命中");
    let syntax =
        tauri::async_runtime::block_on(search(state.clone(), "provider:generic sync".into(), None))
            .expect("search syntax");
    assert!(!syntax.is_empty(), "provider: 语法搜索应有命中");

    // 收藏 → status:favorite 语法（DB 后过滤路径）
    tauri::async_runtime::block_on(set_favorite(
        state.clone(),
        imported.conversation_id.clone(),
        true,
    ))
    .expect("favorite");
    let fav =
        tauri::async_runtime::block_on(search(state.clone(), "status:favorite sync".into(), None))
            .expect("search favorite");
    assert!(!fav.is_empty(), "status:favorite 应命中已收藏会话");

    // 标签
    tauri::async_runtime::block_on(add_tag(
        state.clone(),
        imported.conversation_id.clone(),
        "e2e-tag".into(),
    ))
    .expect("add tag");
    let detail2 = tauri::async_runtime::block_on(get_conversation_detail(
        state.clone(),
        imported.conversation_id.clone(),
    ))
    .expect("detail2");
    assert!(detail2.tags.contains(&"e2e-tag".to_string()));

    // 知识提取（规则引擎全链路）
    let k = tauri::async_runtime::block_on(extract_knowledge(
        state.clone(),
        imported.conversation_id.clone(),
    ))
    .expect("knowledge");
    assert!(!k.summary.is_empty(), "摘要应非空");

    // 原始视图：Raw Store 里应有未标准化归档
    let raw = tauri::async_runtime::block_on(conversation_raw(
        state.clone(),
        imported.conversation_id.clone(),
    ))
    .expect("raw");
    assert!(raw.is_some(), "导入路径必须落 raw 归档");
    assert!(raw.unwrap().contains("WorkManager"), "raw 内容应是原始文本");

    // 恢复命令：generic 来源无官方 resume → None
    let resume = tauri::async_runtime::block_on(resume_command(
        state.clone(),
        imported.conversation_id.clone(),
    ))
    .expect("resume");
    assert!(resume.is_none(), "generic 来源不支持恢复命令");

    // 软删：默认列表不可见（回收站语义）；详情仍可读（回收站需查看）
    tauri::async_runtime::block_on(delete_conversation(
        state.clone(),
        imported.conversation_id.clone(),
    ))
    .expect("soft delete");
    let default_list = tauri::async_runtime::block_on(list_conversations(
        state.clone(),
        None,
        None,
        None,
        None,
        None,
    ))
    .expect("list after delete");
    assert!(
        !default_list
            .iter()
            .any(|c| c.id == imported.conversation_id),
        "软删后默认列表不应出现"
    );
    let deleted_list = tauri::async_runtime::block_on(list_conversations(
        state.clone(),
        None,
        None,
        None,
        None,
        Some(true),
    ))
    .expect("list deleted");
    assert!(
        deleted_list
            .iter()
            .any(|c| c.id == imported.conversation_id),
        "include_deleted 列表应包含软删会话"
    );
    tauri::async_runtime::block_on(restore_conversation(
        state.clone(),
        imported.conversation_id.clone(),
    ))
    .expect("restore");
    let restored_list = tauri::async_runtime::block_on(list_conversations(
        state.clone(),
        None,
        None,
        None,
        None,
        None,
    ))
    .expect("list after restore");
    assert!(
        restored_list
            .iter()
            .any(|c| c.id == imported.conversation_id),
        "恢复后默认列表应重新出现"
    );

    // 硬删 → 详情失败
    tauri::async_runtime::block_on(hard_delete_conversation(
        state.clone(),
        imported.conversation_id.clone(),
    ))
    .expect("hard delete");
    assert!(
        tauri::async_runtime::block_on(get_conversation_detail(
            state.clone(),
            imported.conversation_id.clone()
        ))
        .is_err(),
        "硬删后详情应报错"
    );
}

/// 旅程 2：Workspace 治理（双项目导入 → 重命名 → 合并 → 拆分 → 治理审计）。
#[test]
fn journey_workspace_governance() {
    let h = harness();
    let state = h.state();

    let dir_a = h._dir.path().join("project-a");
    let dir_b = h._dir.path().join("project-b");
    std::fs::create_dir_all(&dir_a).expect("mkdir a");
    std::fs::create_dir_all(&dir_b).expect("mkdir b");
    let a = tauri::async_runtime::block_on(import_file(
        state.clone(),
        fixture_in_dir(&dir_a, "tauri-background.md"),
        Some("ws-alpha".into()),
    ))
    .expect("import a");
    let b = tauri::async_runtime::block_on(import_file(
        state.clone(),
        fixture_in_dir(&dir_b, "rust-error-handling.md"),
        Some("ws-beta".into()),
    ))
    .expect("import b");

    // 两个 workspace
    let wss = tauri::async_runtime::block_on(list_workspaces(state.clone())).expect("ws list");
    assert_eq!(wss.len(), 2, "应有 alpha/beta 两个 workspace");

    // 重命名 alpha
    let alpha_id = a.workspace_id.clone().expect("alpha ws id");
    let beta_id = b.workspace_id.clone().expect("beta ws id");
    tauri::async_runtime::block_on(workspace_rename(
        state.clone(),
        alpha_id.clone(),
        "alpha-renamed".into(),
    ))
    .expect("rename");
    let wss = tauri::async_runtime::block_on(list_workspaces(state.clone())).expect("ws list 2");
    assert!(wss.iter().any(|w| w.display_name == "alpha-renamed"));

    // 合并 beta → alpha（会话数迁移）
    let moved = tauri::async_runtime::block_on(workspace_merge(
        state.clone(),
        beta_id.clone(),
        alpha_id.clone(),
    ))
    .expect("merge");
    assert_eq!(moved, 1, "beta 的 1 条会话应迁入 alpha");
    let wss = tauri::async_runtime::block_on(list_workspaces(state.clone())).expect("ws list 3");
    assert_eq!(wss.len(), 1, "合并后只剩 1 个 workspace");
    let in_alpha = tauri::async_runtime::block_on(list_conversations(
        state.clone(),
        Some(alpha_id.clone()),
        None,
        None,
        None,
        None,
    ))
    .expect("list in alpha");
    assert_eq!(in_alpha.len(), 2, "两条会话都应在 alpha");

    // 拆分：把 rust 那条会话移到新 workspace
    let rust_conv = in_alpha
        .iter()
        .find(|c| c.title.as_deref().unwrap_or_default().contains("错误处理"))
        .expect("rust conv");
    let new_id = tauri::async_runtime::block_on(workspace_split(
        state.clone(),
        vec![rust_conv.id.clone()],
        "split-ws".into(),
    ))
    .expect("split");
    assert!(!new_id.is_empty());
    let in_new = tauri::async_runtime::block_on(list_conversations(
        state.clone(),
        Some(new_id.clone()),
        None,
        None,
        None,
        None,
    ))
    .expect("list in split");
    assert_eq!(in_new.len(), 1);

    // 来源映射置信度：通用文件导入不写 source_workspaces（该表由 IDE
    // 直读导入路径填充，见 auto_sync/source_table），这里验证命令契约即可
    let links =
        tauri::async_runtime::block_on(workspace_source_links(state.clone())).expect("links");
    for l in &links {
        assert!(!l.workspace_id.is_empty());
        assert!(!l.workspace_name.is_empty());
    }

    // 治理审计有 merge/split 记录
    let logs = tauri::async_runtime::block_on(governance_log_list(state.clone(), Some(20)))
        .expect("gov log");
    assert!(logs.iter().any(|l| l.action == "workspace.merge"));
    assert!(logs.iter().any(|l| l.action == "workspace.split"));
}

/// 旅程 3：保存搜索（GUI 搜索框 ☆ 保存 → 下拉执行 → 删除）。
#[test]
fn journey_saved_searches() {
    let h = harness();
    let state = h.state();

    let id1 = tauri::async_runtime::block_on(saved_search_upsert(
        state.clone(),
        "rust 查询".into(),
        "provider:generic thiserror".into(),
    ))
    .expect("save 1");
    let id2 = tauri::async_runtime::block_on(saved_search_upsert(
        state.clone(),
        "后台任务".into(),
        "WorkManager".into(),
    ))
    .expect("save 2");

    let list = tauri::async_runtime::block_on(saved_search_list(state.clone())).expect("list");
    assert_eq!(list.len(), 2);
    assert!(list
        .iter()
        .any(|s| s.id == id1 && s.query_text.contains("thiserror")));

    // 同名覆盖不新增
    let id1b = tauri::async_runtime::block_on(saved_search_upsert(
        state.clone(),
        "rust 查询".into(),
        "provider:generic anyhow".into(),
    ))
    .expect("upsert same name");
    assert_eq!(id1b, id1, "同名必须原地更新");
    let list = tauri::async_runtime::block_on(saved_search_list(state.clone())).expect("list 2");
    assert_eq!(list.len(), 2);

    // 删除
    tauri::async_runtime::block_on(saved_search_delete(state.clone(), id2.clone()))
        .expect("delete");
    let list = tauri::async_runtime::block_on(saved_search_list(state.clone())).expect("list 3");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id1);

    // 空名/空查询拒绝
    assert!(tauri::async_runtime::block_on(saved_search_upsert(
        state.clone(),
        "  ".into(),
        "q".into()
    ))
    .is_err());
    assert!(tauri::async_runtime::block_on(saved_search_upsert(
        state.clone(),
        "n".into(),
        " ".into()
    ))
    .is_err());
}

/// 旅程 4：搜索按主对话分组 + 会话树内命中步进。
/// 场景：主对话（导入 fixture）+ 直接写库造一个含关键词的子任务，
/// 验证子任务命中折叠到主对话 root 之下、树内命中按阅读顺序排列。
#[test]
fn journey_search_grouped_and_tree_hits() {
    let h = harness();
    let state = h.state();

    // 主对话：导入真实 fixture（含 WorkManager 关键词）
    let imported = tauri::async_runtime::block_on(import_file(
        state.clone(),
        fixture_in_dir(h._dir.path(), "tauri-background.md"),
        Some("e2e-search".into()),
    ))
    .expect("import");

    // 子任务：写连接直接造（父子靠 source_parent_id 关联）
    let (child_id, child_msg_id) = {
        let repo = state.repo.lock().expect("poisoned");
        let parent = repo
            .get_conversation(&imported.conversation_id)
            .expect("get parent")
            .expect("parent exists");
        let mut child = ch_domain::Conversation::new(ch_domain::Provider::Generic, "child-src-1");
        child.title = Some("子任务：后台任务".into());
        child.source_parent_id = Some(parent.source_conversation_id.clone());
        let cid = repo.upsert_conversation(&child).expect("upsert conv");
        let mut m = ch_domain::Message::new(&cid, ch_domain::Role::Assistant, 1);
        m.content_text = Some("WorkManager 在子任务里也被提到".into());
        let mid = repo.upsert_message(&m).expect("upsert msg");
        (cid, mid)
    };

    // 直写绕过了导入管线 → 给 Tantivy 补同一消息的文档（与导入路径同款三步），
    // 保证双引擎（Tantivy / FTS5 触发器）结果一致
    {
        let idx = state.search_index.lock().expect("poisoned");
        let mut writer = idx
            .writer(ch_search::index::DEFAULT_WRITER_HEAP)
            .expect("writer");
        idx.index_message(
            &mut writer,
            &ch_search::index::IndexableMessage {
                message_id: child_msg_id,
                conversation_id: child_id.clone(),
                provider: ch_domain::Provider::Generic,
                workspace_id: None,
                role: ch_domain::Role::Assistant,
                title: Some("子任务：后台任务".into()),
                body: Some("WorkManager 在子任务里也被提到".into()),
            },
        )
        .expect("index message");
        idx.commit(writer).expect("commit");
    }

    // search_grouped：子任务命中折叠到主对话 root 之下
    let groups =
        tauri::async_runtime::block_on(search_grouped(state.clone(), "WorkManager".into(), None))
            .expect("grouped");
    assert!(!groups.is_empty(), "应有分组命中");
    let child_group = groups
        .iter()
        .find(|g| g.conversation_id == child_id)
        .expect("子任务命中应有自己的聚合行");
    assert!(child_group.is_child, "子任务行应标记 is_child");
    assert_eq!(
        child_group.root_conversation_id, imported.conversation_id,
        "子任务命中应折叠到主对话 root"
    );
    assert!(child_group.hit_count >= 1);
    assert!(
        groups
            .iter()
            .any(|g| g.conversation_id == imported.conversation_id && !g.is_child),
        "主对话自身命中行应以自己为 root"
    );

    // search_tree_hits：主对话 + 子任务内的命中，主对话在前
    let hits = tauri::async_runtime::block_on(search_tree_hits(
        state.clone(),
        "WorkManager".into(),
        imported.conversation_id.clone(),
        None,
    ))
    .expect("tree hits");
    assert!(hits.len() >= 2, "主对话与子任务都应有命中");
    assert_eq!(
        hits[0].conversation_id, imported.conversation_id,
        "阅读顺序：主对话命中排最前"
    );
    let child_pos = hits
        .iter()
        .position(|x| x.conversation_id == child_id)
        .expect("子任务命中应在树内");
    assert!(child_pos > 0, "子任务命中应在主对话之后");

    // role 过滤透传：仅 user 时子任务（assistant）命中被排除
    let user_only = tauri::async_runtime::block_on(search_tree_hits(
        state.clone(),
        "WorkManager".into(),
        imported.conversation_id.clone(),
        Some("user".into()),
    ))
    .expect("tree hits role");
    assert!(
        user_only.iter().all(|x| x.role == "user"),
        "role=user 应只剩用户消息命中"
    );
    assert!(
        !user_only.iter().any(|x| x.conversation_id == child_id),
        "assistant 的子任务命中不应出现"
    );
}
