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

/// 返回当前本地时间的 "YYYYMMDD_HHMMSS" 字符串（文件名友好，无冒号/点）。
/// 用于存盘文件默认名。
pub fn now_local_compact() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock moved backwards");
    format_compact(dur.as_secs() as i64)
}

/// 根据自 epoch 的秒数与毫秒数，格式化为本地 "HH:MM:SS.mmm"。
pub fn format_ts(secs_since_epoch: i64, millis: u32) -> String {
    let total_secs = secs_since_epoch + TZ_OFFSET_HOURS * 3600;
    let h = (total_secs / 3600) % 24;
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, millis)
}

/// 根据自 epoch 的秒数，格式化为本地 "YYYYMMDD_HHMMSS"（文件名友好）。
/// 基于 epoch 秒换算，不依赖系统日历；用 365.2425 天/年的平均年长近似
/// 算出年月日——对本工具用例（文件名）足够精确。
pub fn format_compact(secs_since_epoch: i64) -> String {
    let total_secs = secs_since_epoch + TZ_OFFSET_HOURS * 3600;
    let days = total_secs.div_euclid(86400);
    let rem = total_secs.rem_euclid(86400);
    let h = rem / 3600;
    let m = (rem / 60) % 60;
    let s = rem % 60;
    let (y, mo, d) = days_to_ymd(days);
    format!("{:04}{:02}{:02}_{:02}{:02}{:02}", y, mo, d, h, m, s)
}

/// 自 1970-01-01 起的天数 → (年, 月, 日)。
/// 使用 civil_from_days 算法（Howard Hinnant），格里高利历，正向有效。
fn days_to_ymd(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
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

    #[test]
    fn test_format_compact_epoch() {
        // 1970-01-01 00:00:00 UTC → 本地 +8 = 1970-01-01 08:00:00
        assert_eq!(format_compact(0), "19700101_080000");
    }

    #[test]
    fn test_format_compact_is_filename_friendly() {
        let s = format_compact(0);
        // 无冒号、无点、无空格等 Windows 文件名非法字符
        for c in s.chars() {
            assert!(
                c.is_ascii_digit() || c == '_',
                "文件名含非法字符: {}",
                c
            );
        }
        assert_eq!(s.len(), 15); // YYYYMMDD_HHMMSS
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        // 1970-01-01 是 epoch 第 0 天
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_year_rollover() {
        // 1970 非闰年（365 天）：day 365 = 1971-01-01
        assert_eq!(days_to_ymd(365), (1971, 1, 1));
        assert_eq!(days_to_ymd(366), (1971, 1, 2));
    }

    #[test]
    fn test_days_to_ymd_leap_day() {
        // 1972 是闰年。1970-01-01 到 1972-01-01 = 365+365 = 730 天
        assert_eq!(days_to_ymd(730), (1972, 1, 1));
        // 1972-02-29 存在：730 + 31(jan) + 28(feb前28天) = 789 → 1972-02-29
        assert_eq!(days_to_ymd(789), (1972, 2, 29));
        // day 790 → 1972-03-01
        assert_eq!(days_to_ymd(790), (1972, 3, 1));
    }

    #[test]
    fn test_days_to_ymd_non_leap_century() {
        // 2100 非闰年（能被100整除但不能被400整除）。
        // 1970-01-01 到 2100-01-01 的天数：130 年，其中闰年数 = 32（含 2000，不含 2100）
        // = 130*365 + 32 = 47482
        assert_eq!(days_to_ymd(47482), (2100, 1, 1));
        // 2100-02-28 → 2100-03-01 之间没有 29
        // day 47482 + 31 + 27 = 47540 → 2100-02-28
        assert_eq!(days_to_ymd(47540), (2100, 2, 28));
        // day 47541 → 2100-03-01（跳过 29）
        assert_eq!(days_to_ymd(47541), (2100, 3, 1));
    }

    #[test]
    fn test_format_compact_hours_from_secs() {
        // 验证时分秒换算：epoch + 1 小时 = 本地 09:00:00
        assert_eq!(format_compact(3600), "19700101_090000");
        // epoch + 16 小时 = 本地次日 00:00:00 → 日期翻到 1970-01-02
        assert_eq!(format_compact(16 * 3600), "19700102_000000");
    }
}
