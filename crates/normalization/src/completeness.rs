//! 导入完整度评分，对应 plan §17.3 与 §6.4「显示导入完整度和字段缺失」。
//!
//! 三档：
//! - **完整**（1.0）：含 Message + Tool Call + Diff + Command。
//! - **部分**（0.5~0.9）：含 Message + 至少一类执行事件。
//! - **有限**（<0.5）：仅文本消息，无任何执行事件。
//!
//! 禁止让用户误以为所有来源都能完整恢复（plan §17.3）。

/// 完整度档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completeness {
    /// 完整：Message + Tool + Diff + Command
    Full,
    /// 部分：Message + 至少一类事件
    Partial,
    /// 有限：仅文本
    Limited,
}

impl Completeness {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Completeness::Full => "完整",
            Completeness::Partial => "部分",
            Completeness::Limited => "有限",
        }
    }
}

/// 根据各字段是否存在计算 0.0~1.0 的完整度分数。
///
/// `has_messages` 通常恒为 true（否则不会入库）；其余维度按权重累加。
/// 5 个布尔维度即评分入参设计，非状态标志位滥用。
#[allow(clippy::fn_params_excessive_bools)]
#[must_use]
pub fn completeness_score(
    has_messages: bool,
    has_tool_calls: bool,
    has_diffs: bool,
    has_commands: bool,
    has_approvals: bool,
) -> f64 {
    if !has_messages {
        return 0.0;
    }
    let mut score: f64 = 0.4; // 消息本体占 40%
    if has_tool_calls {
        score += 0.2;
    }
    if has_diffs {
        score += 0.2;
    }
    if has_commands {
        score += 0.1;
    }
    if has_approvals {
        score += 0.1;
    }
    score.min(1.0)
}

/// 由分数映射到档位。
#[must_use]
pub fn grade(score: f64) -> Completeness {
    if score >= 0.9 {
        Completeness::Full
    } else if score >= 0.5 {
        Completeness::Partial
    } else {
        Completeness::Limited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_messages_is_zero() {
        assert!((completeness_score(false, false, false, false, false) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn messages_only_is_limited() {
        let s = completeness_score(true, false, false, false, false);
        assert!((s - 0.4).abs() < 1e-9);
        assert_eq!(grade(s), Completeness::Limited);
    }

    #[test]
    fn full_is_full() {
        let s = completeness_score(true, true, true, true, true);
        assert!((s - 1.0).abs() < 1e-9);
        assert_eq!(grade(s), Completeness::Full);
    }

    #[test]
    fn partial_in_between() {
        let s = completeness_score(true, true, false, false, false);
        assert!((s - 0.6).abs() < 1e-9);
        assert_eq!(grade(s), Completeness::Partial);
    }

    #[test]
    fn grade_boundaries() {
        assert_eq!(grade(0.89), Completeness::Partial);
        assert_eq!(grade(0.90), Completeness::Full);
        assert_eq!(grade(0.49), Completeness::Limited);
        assert_eq!(grade(0.50), Completeness::Partial);
    }

    #[test]
    fn labels_are_chinese() {
        assert_eq!(Completeness::Full.label(), "完整");
        assert_eq!(Completeness::Partial.label(), "部分");
        assert_eq!(Completeness::Limited.label(), "有限");
    }
}
