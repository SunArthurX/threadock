//! 治理采集耗时实测（临时诊断工具）
use std::time::Instant;

fn main() {
    let home = std::env::var("HOME").expect("write to String");
    println!("── collect_codex（114 文件流式扫描）──");
    let t = Instant::now();
    let (u, tc) = ch_ops_metrics::collect_codex(format!("{home}/.codex")).expect("write to String");
    println!("  {:?}  usage={} tools={}", t.elapsed(), u.len(), tc.len());

    println!("── collect_claude_code（54 文件）──");
    let t = Instant::now();
    let (u2, t2) =
        ch_ops_metrics::collect_claude_code(format!("{home}/.claude")).expect("write to String");
    println!("  {:?}  usage={} tools={}", t.elapsed(), u2.len(), t2.len());

    println!("── collect_zcode ──");
    let t = Instant::now();
    let (u3, t3) = ch_ops_metrics::collect_zcode(format!("{home}/.zcode/cli/db/db.sqlite"))
        .expect("write to String");
    println!("  {:?}  usage={} tools={}", t.elapsed(), u3.len(), t3.len());

    println!("── collect_minimax ──");
    let t = Instant::now();
    let u4 =
        ch_ops_metrics::collect_minimax(format!("{home}/.minimax/v2/sqlite/runtime-state.sqlite"))
            .expect("write to String");
    println!("  {:?}  usage={}", t.elapsed(), u4.len());
}
