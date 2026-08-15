# Rust 错误处理讨论

## User
thiserror 和 anyhow 怎么选？什么时候用哪个？

## Assistant
库的公共 API 用 thiserror，因为调用方需要 match 错误类型；应用层用 anyhow，因为只需要错误链和上下文。

## Command
cargo add thiserror anyhow

## Diff
Cargo.toml 新增 thiserror = "1.0" 和 anyhow = "1.0"
