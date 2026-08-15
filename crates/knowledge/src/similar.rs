//! 相似会话推荐，对应 plan §6.7「相关 Conversation 推荐 + 重复问题聚类」。
//!
//! ## 算法
//!
//! 用轻量词频 Jaccard 相似度（不依赖 Tantivy 内部 API，纯算法可充分测试）：
//! 1. 把每条会话的消息文本拼接，提取关键词集合（去停用词、去标点、小写化）。
//! 2. 对候选会话集合，计算与目标会话的 Jaccard 相似度 = |交集| / |并集|。
//! 3. 返回 Top-N（排除自身、score=0）。
//!
//! ## 中英文
//!
//! 中文按 2-gram 切词，英文按空格/标点切词，都小写化。
//! 停用词过滤常见无意义词。

use ch_domain::Message;
use std::collections::HashSet;

/// 一个待比较的会话摘要（id + 拼接文本）。
#[derive(Debug, Clone)]
pub struct ConversationText {
    pub id: String,
    pub title: Option<String>,
    pub text: String,
}

/// 相似度命中。
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarHit {
    pub conversation_id: String,
    pub score: f64,
}

/// 从消息列表构造 `ConversationText`。
pub fn conversation_text(id: &str, title: Option<&str>, messages: &[Message]) -> ConversationText {
    let mut text = String::new();
    for m in messages {
        if let Some(t) = &m.content_text {
            text.push_str(t);
            text.push(' ');
        }
    }
    ConversationText {
        id: id.to_string(),
        title: title.map(String::from),
        text,
    }
}

/// 找出与 target 最相似的 N 条会话。
///
/// - `target`：目标会话文本。
/// - `candidates`：候选池（不含 target 本身）。
/// - `limit`：返回条数。
#[must_use]
pub fn find_similar(
    target: &ConversationText,
    candidates: &[ConversationText],
    limit: usize,
) -> Vec<SimilarHit> {
    let target_keywords = keywords(&target.text);
    if target_keywords.is_empty() {
        return Vec::new();
    }

    let mut hits: Vec<SimilarHit> = candidates
        .iter()
        .filter(|c| c.id != target.id)
        .map(|c| {
            let kw = keywords(&c.text);
            let score = jaccard(&target_keywords, &kw);
            SimilarHit {
                conversation_id: c.id.clone(),
                score,
            }
        })
        .filter(|h| h.score > 0.0)
        .collect();

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(limit);
    hits
}

/// 计算两个集合的 Jaccard 相似度。
fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// 提取关键词集合（中英文混合）。
fn keywords(text: &str) -> HashSet<String> {
    let lower = text.to_lowercase();
    let mut kws = HashSet::new();

    // 英文：按非字母数字分割
    for token in lower.split(|c: char| !c.is_alphanumeric()) {
        let t = token.trim();
        if t.len() >= 3 && !is_stopword(t) {
            kws.insert(t.to_string());
        }
    }

    // 中文：2-gram
    let chars: Vec<char> = lower.chars().filter(|c| c.is_alphabetic()).collect();
    for i in 0..chars.len().saturating_sub(1) {
        let gram: String = chars[i..i + 2].iter().collect();
        if !is_stopword(&gram) {
            kws.insert(gram);
        }
    }

    kws
}

/// 常见停用词（避免无意义词拉高相似度）。
fn is_stopword(s: &str) -> bool {
    const STOP: &[&str] = &[
        "the", "and", "for", "are", "but", "not", "you", "all", "can", "her", "was", "one", "our",
        "out", "has", "have", "from", "this", "that", "with", "they", "will", "each", "which",
        "their", "what", "about", "would", "there", "been", "more", "than", "very", "your", "into",
        "them", "then", "这些", "我们", "一个", "可以", "什么", "怎么", "使用", "进行", "这个",
        "那个",
    ];
    STOP.contains(&s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ch_domain::{Message, Role};

    fn msg(text: &str) -> Message {
        let mut m = Message::new("c", Role::User, 1);
        m.content_text = Some(text.into());
        m
    }

    fn conv_text(id: &str, text: &str) -> ConversationText {
        ConversationText {
            id: id.into(),
            title: None,
            text: text.into(),
        }
    }

    #[test]
    fn identical_text_has_high_similarity() {
        let target = conv_text("t", "Tauri Android 后台任务 WorkManager");
        let candidate = conv_text("c1", "Tauri Android 后台任务 WorkManager");
        let hits = find_similar(&target, &[candidate], 5);
        assert_eq!(hits.len(), 1);
        assert!((hits[0].score - 1.0).abs() < 0.01);
    }

    #[test]
    fn related_conversations_ranked_higher() {
        let target = conv_text("t", "如何用 Tauri 做 Android 后台任务");
        let related = conv_text("c1", "Tauri Android 后台任务用 WorkManager 实现");
        let unrelated = conv_text("c2", "Python 数据分析 pandas numpy");
        let hits = find_similar(&target, &[unrelated, related], 5);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].conversation_id, "c1");
        // 无关会话应排后面或被过滤
        assert!(hits
            .iter()
            .all(|h| h.conversation_id == "c1" || h.score < hits[0].score));
    }

    #[test]
    fn completely_different_returns_empty() {
        let target = conv_text("t", "Rust 错误处理 thiserror anyhow");
        let candidate = conv_text("c1", "JavaScript 前端 React 组件");
        let hits = find_similar(&target, &[candidate], 5);
        assert!(hits.is_empty());
    }

    #[test]
    fn excludes_self() {
        let target = conv_text("t", "Tauri Android");
        let self_candidate = conv_text("t", "Tauri Android");
        let hits = find_similar(&target, &[self_candidate], 5);
        assert!(hits.is_empty(), "should exclude self by id");
    }

    #[test]
    fn limit_respected() {
        let target = conv_text("t", "Tauri Android 后台任务");
        let candidates: Vec<_> = (0..10)
            .map(|i| conv_text(&format!("c{i}"), "Tauri Android 后台任务 WorkManager"))
            .collect();
        let hits = find_similar(&target, &candidates, 3);
        assert!(hits.len() <= 3);
    }

    #[test]
    fn empty_target_returns_empty() {
        let target = conv_text("t", "");
        let candidate = conv_text("c1", "something");
        let hits = find_similar(&target, &[candidate], 5);
        assert!(hits.is_empty());
    }

    #[test]
    fn keywords_extracts_chinese_bigrams() {
        let kws = keywords("后台任务 实现");
        assert!(kws.contains("后台"));
        assert!(kws.contains("台任"));
        assert!(kws.contains("任务"));
    }

    #[test]
    fn keywords_extracts_english_tokens() {
        let kws = keywords("cargo build release");
        assert!(kws.contains("cargo"));
        assert!(kws.contains("build"));
        assert!(kws.contains("release"));
    }

    #[test]
    fn keywords_filters_stopwords() {
        let kws = keywords("the cargo and build");
        // the/and 是停用词
        assert!(!kws.contains("the"));
        assert!(!kws.contains("and"));
        assert!(kws.contains("cargo"));
    }

    #[test]
    fn conversation_text_from_messages() {
        let msgs = vec![msg("你好"), msg("你好啊")];
        let ct = conversation_text("c1", Some("测试"), &msgs);
        assert_eq!(ct.id, "c1");
        assert_eq!(ct.title.as_deref(), Some("测试"));
        assert!(ct.text.contains("你好"));
    }

    #[test]
    fn jaccard_zero_for_disjoint() {
        let a: HashSet<String> = ["a", "b"].iter().map(|s| (*s).to_string()).collect();
        let b: HashSet<String> = ["c", "d"].iter().map(|s| (*s).to_string()).collect();
        assert!((jaccard(&a, &b) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn jaccard_half_overlap() {
        let a: HashSet<String> = ["a", "b"].iter().map(|s| (*s).to_string()).collect();
        let b: HashSet<String> = ["b", "c"].iter().map(|s| (*s).to_string()).collect();
        // 交集 {b}=1，并集 {a,b,c}=3
        assert!((jaccard(&a, &b) - 1.0 / 3.0).abs() < 0.01);
    }
}
