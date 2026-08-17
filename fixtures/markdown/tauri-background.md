# Tauri Android background task migration

## User
We need to move the sync job off the main thread on Android. What did we decide last time?

## Assistant
Use WorkManager for deferrable background work. Avoid Foreground Services unless the task is user-visible; Android 14+ enforces strict foreground service types.

## Command
cargo tauri android init

## Diff
src-tauri/src/lib.rs: added `run_background_task` registering a WorkManager worker via the tauri plugin.

## Tool
bash: listed src-tauri/gen/android to confirm the generated gradle project layout.

## Assistant
The worker compiles and schedules with KEEP policy. Next step is exponential backoff for network failures.

## Error
Build failed: minSdkVersion 24 required by the plugin, project pins 21.
