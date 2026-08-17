# Golden Fixture Kit（脱敏样本集，plan §20.2）

供 Adapter 解析回归测试使用（golden tests 断言领域事实，非逐字快照）。

## 内容

| 文件 | 覆盖 |
|---|---|
| `markdown/tauri-background.md` | User/Assistant/Command/Diff/Tool 混合段；英文标题与正文 |
| `markdown/rust-error-handling.md` | 中文会话；仅消息无事件 |
| `jsonl/opencode-style.jsonl` | meta 行 + 三种 role 消息 + 4 类事件 |
| `jsonl/minimal.jsonl` | 无 meta、只有消息的最小合法输入 |

## 脱敏原则

- 全部为**手工构造**的虚构会话，不含任何真实用户数据；
- 人名/仓库名/路径均为虚构（example.com / acme / dev）；
- 若未来从真实会话采集样本：先经 `crates/export` 脱敏管线（12 类内置规则 + 自定义），
  人工复核后才可入库，并在本表登记采集日期与脱敏操作。

## 使用

golden tests 位于 `crates/adapter-markdown/tests/golden.rs` 与
`crates/adapter-jsonl/tests/golden.rs`，通过 `CARGO_MANIFEST_DIR` 相对路径读取本目录：

```bash
cargo test -p ch-adapter-markdown --test golden
cargo test -p ch-adapter-jsonl --test golden
```

新增 Adapter 时：在对应 crate 加 `tests/golden.rs` 并在本目录补样本，
未知 Schema 的降级行为（不崩溃、不猜字段）必须有用例（plan §11.6）。
