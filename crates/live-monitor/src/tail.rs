//! JSONL 尾部读取：只取文件末尾若干字节，供实时活动摘要解析（不整读大文件）。

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// 读取文件尾部最多 `max_bytes` 字节。
///
/// 返回 `(是否读到整个文件, 完整行列表)`：
/// - 从文件中间起读时丢弃首个残行；
/// - 末尾没有换行符的半行也丢弃（写入方可能正在追加，宁缺勿错）。
pub fn tail_lines(path: &Path, max_bytes: u64) -> std::io::Result<(bool, Vec<String>)> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    f.seek(SeekFrom::Start(start))?;
    let mut raw = Vec::new();
    f.read_to_end(&mut raw)?;
    let buf = String::from_utf8_lossy(&raw);
    let full = start == 0;
    let ends_with_newline = buf.ends_with('\n');
    let mut lines: Vec<String> = buf.lines().map(String::from).collect();
    if !full {
        // 首行可能是被截断的残行
        lines.remove(0);
    }
    if !ends_with_newline {
        lines.pop();
    }
    Ok((full, lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_all_lines_when_file_fits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("a.jsonl");
        std::fs::write(&p, "{\"a\":1}\n{\"a\":2}\n").expect("write");
        let (full, lines) = tail_lines(&p, 1024).expect("tail");
        assert!(full);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn drops_partial_head_and_tail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("b.jsonl");
        // 30 字节，窗口 10 字节 → 从中间起读，首尾都可能残缺
        let content = "aaaaaaaaaa\nbbbbbbbbbb\ncccccccccc\n";
        std::fs::write(&p, content).expect("write");
        let (full, lines) = tail_lines(&p, 10).expect("tail");
        assert!(!full);
        for l in &lines {
            assert_eq!(l.len(), 10, "只应有完整行: {l:?}");
        }
    }

    #[test]
    fn missing_file_is_io_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("nope.jsonl");
        assert!(tail_lines(&p, 1024).is_err());
    }
}
