//! 底部终端面板（v1.3.0）：portable-pty 起真实 shell（$SHELL），stdout 由
//! 后台线程持续读取并 base64 编码后经 `terminal-output` 事件回传前端
//! （xterm.js 渲染）；`terminal-exit` 通知进程退出。单实例：面板关闭仅隐藏
//! 不杀会话，重开复用；`terminal_kill` 显式重启。
use std::io::{Read, Write};
use std::sync::Mutex;

use base64::Engine as _;
use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tauri::{AppHandle, Emitter};

use super::io_err;

/// 唯一终端会话（writer = stdin、master 用于 resize、child 用于 kill/wait）。
struct TermSession {
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send>,
    master: Box<dyn MasterPty + Send>,
}

static SESSION: Mutex<Option<TermSession>> = Mutex::new(None);

/// 平台默认 shell：unix 取 $SHELL（兜底 /bin/zsh），Windows 用 PowerShell。
fn default_shell() -> String {
    if cfg!(windows) {
        "powershell.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
    }
}

/// 启动（或复用已存活的）终端会话。
#[tauri::command]
pub(crate) async fn terminal_spawn(app: AppHandle, cols: u16, rows: u16) -> Result<(), String> {
    let mut guard = SESSION.lock().map_err(|e| io_err(e))?;
    if guard.is_some() {
        return Ok(()); // 面板重开：复用存活 shell，保留现场
    }
    let pty = NativePtySystem::default();
    let pair = pty
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| io_err(e))?;
    let mut cmd = CommandBuilder::new(default_shell());
    if let Ok(home) = std::env::var("HOME") {
        cmd.cwd(home);
    }
    let child = pair.slave.spawn_command(cmd).map_err(|e| io_err(e))?;
    // reader/writer 都从 master 取（SlavePty 只负责 spawn）；顺序无碍
    let mut reader = pair.master.try_clone_reader().map_err(|e| io_err(e))?;
    let writer = pair.master.take_writer().map_err(|e| io_err(e))?;
    // slave fd 此后不再需要（子进程持有自己的副本），drop 不会断开 pty
    drop(pair.slave);

    let emitter = app.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let payload = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    if emitter.emit("terminal-output", payload).is_err() {
                        break;
                    }
                }
            }
        }
        // 会话结束（exit / kill / master drop）：通知前端展示「已退出」
        let _ = emitter.emit("terminal-exit", ());
    });

    *guard = Some(TermSession {
        writer,
        child,
        master: pair.master,
    });
    Ok(())
}

/// 向终端 stdin 写入按键数据（xterm.js onData，UTF-8 文本）。
#[tauri::command]
pub(crate) async fn terminal_write(data: String) -> Result<(), String> {
    let mut guard = SESSION.lock().map_err(|e| io_err(e))?;
    if let Some(s) = guard.as_mut() {
        s.writer
            .write_all(data.as_bytes())
            .and_then(|_| s.writer.flush())
            .map_err(|e| io_err(e))?;
    }
    Ok(())
}

/// 按视口尺寸调整 pty（前端 fit 后回调）。
#[tauri::command]
pub(crate) async fn terminal_resize(cols: u16, rows: u16) -> Result<(), String> {
    let guard = SESSION.lock().map_err(|e| io_err(e))?;
    if let Some(s) = guard.as_ref() {
        s.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| io_err(e))?;
    }
    Ok(())
}

/// 终止会话（前端「重启」用）：kill + wait 回收，drop master 触发读线程退出。
#[tauri::command]
pub(crate) async fn terminal_kill() -> Result<(), String> {
    let mut guard = SESSION.lock().map_err(|e| io_err(e))?;
    if let Some(mut s) = guard.take() {
        let _ = s.child.kill();
        let _ = s.child.wait();
    }
    Ok(())
}
