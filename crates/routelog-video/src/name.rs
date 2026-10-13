use regex::{Captures, Regex, RegexBuilder};
use routelog_core::jst::{self, Civil};

use crate::Error;

/// 録画開始時刻を表す名前付きグループ。どれも必須。
const TIME_GROUPS: [&str; 6] = ["year", "month", "day", "hour", "minute", "second"];

/// DJI Action 4 のファイル名。`DJI_20261003050500_0004_D.MP4`。
const DJI_ACTION4: &str = r"^DJI_(?<year>\d{4})(?<month>\d{2})(?<day>\d{2})(?<hour>\d{2})(?<minute>\d{2})(?<second>\d{2})_(?<seq>\d+)_D\.MP4$";

/// ファイル名から読み取ったもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedName {
    /// 録画開始時刻。ファイル名の時刻を JST として読む。
    pub start_ms: i64,
    /// 通し番号。ファイル名に無ければ 0。
    pub seq: u32,
}

/// 動画のファイル名の形。機種が変わったら正規表現を差し替える。
#[derive(Debug, Clone)]
pub struct NamePattern {
    regex: Regex,
}

impl NamePattern {
    /// 名前付きグループ `year`・`month`・`day`・`hour`・`minute`・`second` を持つ
    /// 正規表現から作る。`seq` は任意。大文字と小文字は区別しない。
    pub fn new(pattern: &str) -> Result<Self, Error> {
        let regex = RegexBuilder::new(pattern).case_insensitive(true).build()?;
        for group in TIME_GROUPS {
            if !regex.capture_names().flatten().any(|name| name == group) {
                return Err(Error::MissingGroup(group));
            }
        }
        Ok(Self { regex })
    }

    pub fn dji_action4() -> Self {
        Self::new(DJI_ACTION4).expect("the built-in pattern is valid")
    }

    /// 形に合わないファイル名と、存在しない日時は `None`。
    pub fn parse(&self, file_name: &str) -> Option<ParsedName> {
        let captures = self.regex.captures(file_name)?;
        let number = |group: &str| number(&captures, group);
        let start_ms = jst::to_unix_ms(&Civil {
            year: number("year")?,
            month: number("month")?,
            day: number("day")?,
            hour: number("hour")?,
            minute: number("minute")?,
            second: number("second")?,
            millisecond: 0,
        })?;
        let seq = number("seq")
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0);
        Some(ParsedName { start_ms, seq })
    }
}

fn number(captures: &Captures, group: &str) -> Option<i64> {
    captures.name(group)?.as_str().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_start_time_as_jst() {
        let parsed = NamePattern::dji_action4()
            .parse("DJI_20261003050500_0004_D.MP4")
            .unwrap();
        assert_eq!(jst::iso(parsed.start_ms), "2026-10-03T05:05:00+09:00");
        assert_eq!(parsed.seq, 4);
    }

    #[test]
    fn the_extension_may_be_lower_case() {
        assert!(
            NamePattern::dji_action4()
                .parse("DJI_20261003050500_0004_D.mp4")
                .is_some()
        );
    }

    #[test]
    fn other_files_do_not_match() {
        let pattern = NamePattern::dji_action4();
        assert_eq!(pattern.parse("DJI_20261003050500_0005_D.JPG"), None);
        assert_eq!(pattern.parse("DJI_20261003050500_0004_D.MP4.bak"), None);
        assert_eq!(pattern.parse("notes.txt"), None);
    }

    #[test]
    fn a_date_that_does_not_exist_does_not_match() {
        assert_eq!(
            NamePattern::dji_action4().parse("DJI_20260230050500_0004_D.MP4"),
            None
        );
    }

    #[test]
    fn another_camera_can_bring_its_own_pattern() {
        let pattern = NamePattern::new(
            r"^(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})_(?<hour>\d{2})(?<minute>\d{2})(?<second>\d{2})\.mov$",
        )
        .unwrap();
        let parsed = pattern.parse("2026-10-03_050500.mov").unwrap();
        assert_eq!(jst::iso(parsed.start_ms), "2026-10-03T05:05:00+09:00");
        assert_eq!(parsed.seq, 0);
    }

    #[test]
    fn a_pattern_without_the_time_groups_is_refused() {
        assert!(matches!(
            NamePattern::new(r"^(?<year>\d{4})\.mp4$"),
            Err(Error::MissingGroup("month"))
        ));
    }
}
