# Rust 错误处理选型讨论

## User
这个 CLI 工具的错误处理该用 thiserror 还是 anyhow？

## Assistant
库代码用 thiserror 定义具体错误类型，二进制入口用 anyhow 聚合。边界处用 From 转换。

## User
那 main 里怎么把错误友好地打出来？

## Assistant
main 返回 anyhow::Result，用 `{e:#}` 链式打印；退出码用 ExitCode::FAILURE。
