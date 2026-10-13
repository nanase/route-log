//! 日本時間（JST）の暦と Unix ミリ秒の相互変換。
//!
//! JST は UTC+9 で固定なので、タイムゾーンのデータベースは要らない。

const OFFSET_MS: i64 = 9 * 3_600_000;
const DAY_MS: i64 = 86_400_000;

/// JST の年月日と時刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub millisecond: i64,
}

// 暦と通算日の変換は Howard Hinnant の days_from_civil / civil_from_days による。
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

/// JST の暦を Unix ミリ秒にする。存在しない日付や時刻は `None`。
pub fn to_unix_ms(civil: &Civil) -> Option<i64> {
    let Civil {
        year,
        month,
        day,
        hour,
        minute,
        second,
        millisecond,
    } = *civil;
    let in_range = (1..=12).contains(&month)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..60).contains(&second)
        && (0..1000).contains(&millisecond);
    if !in_range {
        return None;
    }
    let days = days_from_civil(year, month, day);
    // 2 月 30 日のような日付は、往復すると別の日になる。
    if civil_from_days(days) != (year, month, day) {
        return None;
    }
    let time_ms = ((hour * 60 + minute) * 60 + second) * 1000 + millisecond;
    Some(days * DAY_MS + time_ms - OFFSET_MS)
}

/// Unix ミリ秒を JST の暦にする。
pub fn from_unix_ms(unix_ms: i64) -> Civil {
    let local = unix_ms + OFFSET_MS;
    let (year, month, day) = civil_from_days(local.div_euclid(DAY_MS));
    let time_ms = local.rem_euclid(DAY_MS);
    Civil {
        year,
        month,
        day,
        hour: time_ms / 3_600_000,
        minute: time_ms / 60_000 % 60,
        second: time_ms / 1000 % 60,
        millisecond: time_ms % 1000,
    }
}

/// `YYYY-MM-DD`（JST）。
pub fn date(unix_ms: i64) -> String {
    let c = from_unix_ms(unix_ms);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

/// `YYYY-MM-DDThh:mm:ss+09:00`。ミリ秒は切り捨てる。
pub fn iso(unix_ms: i64) -> String {
    let c = from_unix_ms(unix_ms);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+09:00",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn civil(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> Civil {
        Civil {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond: 0,
        }
    }

    #[test]
    fn epoch_is_nine_in_the_morning() {
        assert_eq!(to_unix_ms(&civil(1970, 1, 1, 9, 0, 0)), Some(0));
        assert_eq!(from_unix_ms(0), civil(1970, 1, 1, 9, 0, 0));
    }

    #[test]
    fn known_instant() {
        // 2000-01-01T00:00:00Z は Unix 秒で 946684800。
        assert_eq!(
            to_unix_ms(&civil(2000, 1, 1, 9, 0, 0)),
            Some(946_684_800_000)
        );
        assert_eq!(iso(946_684_800_000), "2000-01-01T09:00:00+09:00");
    }

    #[test]
    fn date_changes_at_jst_midnight() {
        let midnight = to_unix_ms(&civil(2026, 10, 3, 0, 0, 0)).unwrap();
        assert_eq!(date(midnight - 1), "2026-10-02");
        assert_eq!(date(midnight), "2026-10-03");
    }

    #[test]
    fn round_trips_across_leap_days() {
        for c in [
            civil(2024, 2, 29, 23, 59, 59),
            civil(2026, 12, 31, 12, 34, 56),
            civil(1969, 7, 21, 11, 56, 0),
        ] {
            assert_eq!(from_unix_ms(to_unix_ms(&c).unwrap()), c);
        }
    }

    #[test]
    fn rejects_what_the_calendar_does_not_have() {
        assert_eq!(to_unix_ms(&civil(2026, 2, 30, 0, 0, 0)), None);
        assert_eq!(to_unix_ms(&civil(2026, 13, 1, 0, 0, 0)), None);
        assert_eq!(to_unix_ms(&civil(2026, 1, 1, 24, 0, 0)), None);
        assert_eq!(to_unix_ms(&civil(2025, 2, 29, 0, 0, 0)), None);
    }
}
