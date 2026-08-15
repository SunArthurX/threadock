//! 时间在领域层（OffsetDateTime）与存储层（unix 毫秒 i64）之间的转换。
//!
//! 用 unix 毫秒而非秒，足以覆盖 1970±约 2900 万年的精度需求，
//! 且 `SQLite` 对 i64 排序/比较天然高效。

use ch_domain::Timestamp;
use time::OffsetDateTime;

/// 领域时间 → 存储毫秒。None 保持 None。
#[must_use]
pub fn to_millis(ts: Option<Timestamp>) -> Option<i64> {
    ts.map(|t| {
        let dur = t - OffsetDateTime::UNIX_EPOCH;
        // whole_milliseconds 返回 i128，对合理时间范围转 i64 安全
        dur.whole_milliseconds() as i64
    })
}

/// 存储毫秒 → 领域时间。
#[must_use]
pub fn from_millis(ms: Option<i64>) -> Option<Timestamp> {
    ms.and_then(|m| {
        let secs = m.div_euclid(1000);
        let nanos = (m.rem_euclid(1000)) as i32 * 1_000_000;
        OffsetDateTime::from_unix_timestamp(secs)
            .ok()
            .map(|t| t + time::Duration::nanoseconds(i64::from(nanos)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_roundtrip() {
        assert!(to_millis(None).is_none());
        assert!(from_millis(None).is_none());
    }

    #[test]
    fn known_instant_roundtrip() {
        // 2026-08-02T12:00:00Z = 1785230400000 ms
        let t = OffsetDateTime::from_unix_timestamp(1_785_230_400).expect("unexpected None");
        let ms = to_millis(Some(t)).expect("timestamp conversion failed");
        assert_eq!(ms, 1_785_230_400_000);
        let back = from_millis(Some(ms)).expect("timestamp conversion failed");
        assert_eq!(back, t);
    }

    #[test]
    fn subsecond_precision_preserved() {
        // 毫秒级精度：1.5 秒（1500 毫秒）能精确往返
        let t = OffsetDateTime::from_unix_timestamp_nanos(1_500_000_000).expect("unexpected None");
        let ms = to_millis(Some(t)).expect("timestamp conversion failed");
        assert_eq!(ms, 1500);
        let back = from_millis(Some(ms)).expect("timestamp conversion failed");
        assert_eq!(back, t);
    }

    #[test]
    fn sub_millisecond_precision_is_truncated() {
        // 500 纳秒 < 1 毫秒，会被截断到整毫秒——这是预期的存储精度
        let t = OffsetDateTime::from_unix_timestamp_nanos(1_000_500_000).expect("unexpected None");
        let ms = to_millis(Some(t)).expect("timestamp conversion failed");
        assert_eq!(ms, 1000);
    }

    #[test]
    fn negative_millis_handled() {
        // 1969 年（负时间戳）应能往返
        let t = OffsetDateTime::from_unix_timestamp(-1).expect("unexpected None");
        let ms = to_millis(Some(t)).expect("timestamp conversion failed");
        let back = from_millis(Some(ms)).expect("timestamp conversion failed");
        assert_eq!(back, t);
    }
}
