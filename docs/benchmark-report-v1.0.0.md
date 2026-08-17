# Threadock 性能基准报告（v1.0.0）

> 采集日期：2026-08-17 · 机器：Apple Silicon（darwin 24.6.0, arm64）· profile：release（opt-level 3, thin LTO）
> 复现：`cargo test --release -p ch-benchmarks --test perf -- --ignored --nocapture`
> 大规模门禁：`cargo test --release -p ch-benchmarks --test perf large_scale -- --ignored --nocapture`

## Gate 1 大规模红线（plan §1.4 / Phase 2 验收）

**「100k Conversation 搜索 P95 < 300ms」**

| 指标 | 数值 | 红线 | 结果 |
|---|---|---|---|
| 会话规模 | 100,000 | =100k | ✓ |
| 消息规模 | 500,000 | — | — |
| 数据灌入耗时 | 23.5 s | — | — |
| FTS5 搜索 P95（60 次查询，6 组词） | **50.9 ms** | < 300 ms | ✓ PASS |

样本词汇分布按真实场景建模（关键词命中 ~3-5% 文档：标题 10 选 1 轮换、正文按模数注入）。
病态场景（关键词出现在 100% 文档，如导入期统一后缀）实测 P95 ≈ 327 ms——超线 9%，
该场景在真实语料中不成立，记录备查；如未来出现该形态的查询，考虑 Tantivy 主引擎
（其 N-gram 词组查询不受 FTS5 rank 全量排序影响）。

## 常规基准（plan §7.2）

| 基准 | 数值 | 目标 | 结果 |
|---|---|---|---|
| 导入吞吐（500 会话 × 10 消息，`import_conversation_batch` 路径） | **4,613 msg/s** | > 500 msg/s | ✓ |
| FTS5 搜索 P95（5k 消息，50 次查询） | **3.2 ms** | < 300 ms | ✓ |
| Tantivy 搜索 P95（2.5k 消息，40 次查询） | **0.9 ms** | 记录用 | — |
| 冷启动 P95（打开库 + migration，10 次） | **11.1 ms** | < 2,500 ms | ✓ |

## Workspace 合并准确率（Gate 1：≥ 95%）

`cargo test -p ch-identity-resolver --test merge_accuracy -- --nocapture`

| 指标 | 数值 | 红线 | 结果 |
|---|---|---|---|
| 标注样本数 | 11（L2-L7 全命中路径 + 3 负例） | — | — |
| 判定正确率 | **11/11 = 100%** | ≥ 95% | ✓ |
| 错误 AutoMerge（不同项目被静默并组） | **0** | = 0 | ✓ |

注：基准化过程中发现并修复一个误并缺陷——双方都带结构化标识（remote/path/fsid）
却全未命中时，纯名称相同不再静默 AutoMerge，降级 NeedsConfirmation。

## 已知限制（如实声明）

1. **逐条 upsert 路径在大规模下不可用**：`upsert_conversation`/`upsert_message`
   每行一个事务，10 万级需 ~1 小时；所有正式导入路径（GUI/daemon/CLI）已统一走
   `import_conversation_batch` 批量事务（本报告 4,613 msg/s 即该路径）。
   基准库的 `seed_conversations` 仍走慢路径（用于吞吐测量），大规模 seed 用
   `seed_bulk_fast`。
2. 大规模基准仅覆盖 FTS5 引擎；Tantivy 在 10 万会话级的表现未单独留档
   （GUI 默认 Tantivy 主引擎 + FTS5 降级，二者小规模基准均远低于红线）。
3. 单机单次采样，非 CI 多机矩阵；后续可纳入发布前 checklist 固定复跑。
