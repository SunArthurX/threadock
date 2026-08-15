//! Daemon 二进制入口：在 stdio 上运行 JSON-RPC 服务（plan §8.2）。
//!
//! 由 UI/CLI/其它客户端 spawn 后通过 stdin/stdout 通信。

use ch_daemon::{serve_stdio, DaemonState, DaemonStateConfig};
use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr) // 日志走 stderr，stdout 留给 JSON-RPC
        .init();

    // 数据目录：优先 CH_DATA_DIR 环境变量，否则 ./data
    let data_dir = std::env::var("CH_DATA_DIR").map_or_else(|_| PathBuf::from("./data"), PathBuf::from);

    let state = match DaemonState::open(DaemonStateConfig { data_dir }) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fatal: open daemon state: {e}");
            std::process::exit(1);
        }
    };

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve_stdio(
        &state,
        stdin.lock(),
        &mut std::io::BufWriter::new(stdout.lock()),
    );
}
