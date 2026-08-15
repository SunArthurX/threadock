# 关于 Tauri Android 后台任务的讨论

## User
之前在哪个 Agent 里讨论过 Tauri Android 后台任务？我想找回来。

## Assistant
我在 Codex 里讨论过，关键结论是用 WorkManager 而不是 Foreground Service，因为后者在 Android 14+ 有严格限制。

## Command
cargo tauri android init

## Diff
src-tauri/src/lib.rs 新增了 `run_background_task` 函数，注册了 WorkManager。

## Command
cargo tauri android build --apk

## Tool
bash: 列出了 src-tauri/gen/android 目录
