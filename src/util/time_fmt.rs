use std::time::{SystemTime, UNIX_EPOCH};

/// 本地时区偏移（小时）。本机自用，固定 UTC+8。
/// 若跨时区使用需改此处。
const TZ_OFFSET_HOURS: i64 = 8;

/// 返回当前本地时间的 "HH:MM:SS.mmm" 字符串。
pub fn now_local_ts() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock moved backwards");
    format_ts(dur.as_secs() as i64, dur.subsec_millis())
}

/// 根据自 epoch 的秒数与毫秒数，格式化为本地 "HH:MM:SS.mmm"。
pub fn format_ts(secs_since_epoch: i64, millis: u32) -> String {
    let total_secs = secs_since_epoch + TZ_OFFSET_HOURS * 3600;
    let h = (total_secs / 3600) % 24;
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_known_utc_midnight() {
        // UTC 00:00:00.000 → 本地 +8 = 08:00:00.000
        assert_eq!(format_ts(0, 0), "08:00:00.000");
    }

    #[test]
    fn test_format_known_utc_noon() {
        // UTC 12:00:00.000 → 本地 +8 = 20:00:00.000
        assert_eq!(format_ts(12 * 3600, 0), "20:00:00.000");
    }

    #[test]
    fn test_format_with_millis() {
        // UTC 00:00:00.123 → 08:00:00.123
        assert_eq!(format_ts(0, 123), "08:00:00.123");
    }

    #[test]
    fn test_format_day_wraparound() {
        // UTC 16:00:00.000 → +8 = 24:00 → wrap → 00:00:00.000
        assert_eq!(format_ts(16 * 3600, 0), "00:00:00.000");
    }

    #[test]
    fn test_now_local_ts_format() {
        let ts = now_local_ts();
        // 格式 HH:MM:SS.mmm，长度 12
        assert_eq!(ts.len(), 12);
        assert_eq!(ts.as_bytes()[2], b':');
        assert_eq!(ts.as_bytes()[5], b':');
        assert_eq!(ts.as_bytes()[8], b'.');
    }
}
