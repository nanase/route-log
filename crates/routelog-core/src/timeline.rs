//! 複数の動画ファイルを、実時刻の 1 本の軸に並べる。
//!
//! 動画は分割され、録画を止めた間は何も無い。ファイル名の時刻と長さから、
//! 「この実時刻はどのファイルの何秒目か」とその逆を答える。

use serde::{Deserialize, Serialize};

use crate::jst;

/// 続けて録画したファイルの継ぎ目とみなす、時刻のずれの上限。
///
/// ファイル名の時刻は秒単位で、継ぎ目に最大 2 秒ほどの重なりや隙間が出る。
/// これより小さい隙間は録画停止として扱わない。
pub const SEAM_TOLERANCE_MS: i64 = 5_000;

/// 動画ファイル 1 本。時刻はカメラの時計のままで、補正は含まない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoFile {
    pub path: String,
    pub name: String,
    /// カメラが付ける通し番号。同じ時刻のファイルを並べるのに使う。
    pub seq: u32,
    pub start_ms: i64,
    pub duration_ms: i64,
    pub codec: String,
}

/// ファイル 1 本が受け持つ実時刻の範囲 `[start_ms, end_ms)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub file: usize,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// 録画を止めていた実時刻の範囲 `[start_ms, end_ms)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// 動画の中の位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub file: usize,
    pub offset_ms: i64,
}

/// 実時刻の軸に並べた動画ファイル。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Timeline {
    files: Vec<VideoFile>,
    clock_offset_ms: i64,
    spans: Vec<Span>,
    gaps: Vec<Gap>,
}

impl Timeline {
    /// `clock_offset_ms` はカメラの時計の補正値で、実時刻 = ファイル名の時刻 + 補正値。
    ///
    /// ファイルが重なるときは、後のファイルが始まった所で前のファイルを切る。
    pub fn new(mut files: Vec<VideoFile>, clock_offset_ms: i64) -> Self {
        files.sort_by_key(|f| (f.start_ms, f.seq));
        let mut spans = Vec::with_capacity(files.len());
        let mut gaps = Vec::new();
        for (i, file) in files.iter().enumerate() {
            let start_ms = file.start_ms + clock_offset_ms;
            let own_end_ms = start_ms + file.duration_ms;
            let end_ms = match files.get(i + 1) {
                Some(next) => {
                    let next_start_ms = next.start_ms + clock_offset_ms;
                    if next_start_ms <= own_end_ms + SEAM_TOLERANCE_MS {
                        next_start_ms
                    } else {
                        gaps.push(Gap {
                            start_ms: own_end_ms,
                            end_ms: next_start_ms,
                        });
                        own_end_ms
                    }
                }
                None => own_end_ms,
            };
            spans.push(Span {
                file: i,
                start_ms,
                end_ms,
            });
        }
        Self {
            files,
            clock_offset_ms,
            spans,
            gaps,
        }
    }

    pub fn files(&self) -> &[VideoFile] {
        &self.files
    }

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    pub fn gaps(&self) -> &[Gap] {
        &self.gaps
    }

    /// 実時刻を動画の位置にする。録画していない時刻は `None`。
    pub fn locate(&self, real_ms: i64) -> Option<Position> {
        let span = self
            .spans
            .iter()
            .find(|s| s.start_ms <= real_ms && real_ms < s.end_ms)?;
        // 継ぎ目の小さな隙間まで受け持つので、ファイルの長さを超えないようにする。
        let offset_ms = (real_ms - span.start_ms).min(self.files[span.file].duration_ms);
        Some(Position {
            file: span.file,
            offset_ms,
        })
    }

    /// 実時刻を動画の位置にする。録画していない時刻は、次に録画が始まる位置にする。
    ///
    /// 最後のファイルより後は `None`。
    pub fn locate_or_next(&self, real_ms: i64) -> Option<Position> {
        self.locate(real_ms).or_else(|| {
            let next = self
                .spans
                .iter()
                .find(|s| s.start_ms > real_ms && s.start_ms < s.end_ms)?;
            Some(Position {
                file: next.file,
                offset_ms: 0,
            })
        })
    }

    /// 動画の位置を実時刻にする。ファイルの外の位置は `None`。
    pub fn real_time(&self, position: Position) -> Option<i64> {
        let file = self.files.get(position.file)?;
        if !(0..=file.duration_ms).contains(&position.offset_ms) {
            return None;
        }
        Some(file.start_ms + self.clock_offset_ms + position.offset_ms)
    }
}

/// 1 日ぶんの動画ファイル。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RideVideos {
    /// 録画開始の日付。`YYYY-MM-DD`（JST）。
    pub date: String,
    pub files: Vec<VideoFile>,
}

/// 録画開始の日付（JST）ごとに分ける。日付の順、その中は開始時刻の順に並ぶ。
pub fn group_by_date(mut files: Vec<VideoFile>) -> Vec<RideVideos> {
    files.sort_by_key(|f| (f.start_ms, f.seq));
    let mut rides: Vec<RideVideos> = Vec::new();
    for file in files {
        let date = jst::date(file.start_ms);
        match rides.last_mut() {
            Some(ride) if ride.date == date => ride.files.push(file),
            _ => rides.push(RideVideos {
                date,
                files: vec![file],
            }),
        }
    }
    rides
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(seq: u32, start_s: i64, duration_s: i64) -> VideoFile {
        VideoFile {
            path: format!("{seq}.MP4"),
            name: format!("{seq}.MP4"),
            seq,
            start_ms: start_s * 1000,
            duration_ms: duration_s * 1000,
            codec: "hevc".into(),
        }
    }

    fn position(file: usize, offset_s: i64) -> Option<Position> {
        Some(Position {
            file,
            offset_ms: offset_s * 1000,
        })
    }

    #[test]
    fn converts_both_ways_inside_a_file() {
        let timeline = Timeline::new(vec![file(1, 100, 60)], 0);
        assert_eq!(timeline.locate(130_000), position(0, 30));
        assert_eq!(
            timeline.real_time(Position {
                file: 0,
                offset_ms: 30_000
            }),
            Some(130_000)
        );
    }

    #[test]
    fn a_recording_stop_is_a_gap() {
        let timeline = Timeline::new(vec![file(1, 100, 60), file(2, 400, 60)], 0);
        assert_eq!(
            timeline.gaps(),
            [Gap {
                start_ms: 160_000,
                end_ms: 400_000
            }]
        );
        assert_eq!(timeline.locate(200_000), None);
        assert_eq!(timeline.locate_or_next(200_000), position(1, 0));
        assert_eq!(
            timeline.locate(159_999),
            Some(Position {
                file: 0,
                offset_ms: 59_999
            })
        );
        assert_eq!(timeline.locate(160_000), None);
    }

    #[test]
    fn nothing_before_the_first_file_or_after_the_last() {
        let timeline = Timeline::new(vec![file(1, 100, 60)], 0);
        assert_eq!(timeline.locate(99_999), None);
        assert_eq!(timeline.locate_or_next(0), position(0, 0));
        assert_eq!(timeline.locate_or_next(160_000), None);
    }

    #[test]
    fn a_small_overlap_at_a_seam_goes_to_the_later_file() {
        // 前のファイルは 160 秒まであるが、次のファイルが 158 秒に始まる。
        let timeline = Timeline::new(vec![file(1, 100, 60), file(2, 158, 60)], 0);
        assert!(timeline.gaps().is_empty());
        assert_eq!(timeline.locate(157_000), position(0, 57));
        assert_eq!(timeline.locate(159_000), position(1, 1));
    }

    #[test]
    fn a_small_gap_at_a_seam_is_not_a_recording_stop() {
        // 前のファイルは 160 秒で終わり、次のファイルは 162 秒に始まる。
        let timeline = Timeline::new(vec![file(1, 100, 60), file(2, 162, 60)], 0);
        assert!(timeline.gaps().is_empty());
        // 隙間の中は、前のファイルの終わりを指す。
        assert_eq!(timeline.locate(161_000), position(0, 60));
        assert_eq!(timeline.locate(162_000), position(1, 0));
    }

    #[test]
    fn the_clock_offset_shifts_every_time() {
        let timeline = Timeline::new(vec![file(1, 100, 60), file(2, 400, 60)], -30_000);
        assert_eq!(
            timeline.spans()[0],
            Span {
                file: 0,
                start_ms: 70_000,
                end_ms: 130_000
            }
        );
        assert_eq!(
            timeline.gaps(),
            [Gap {
                start_ms: 130_000,
                end_ms: 370_000
            }]
        );
        assert_eq!(timeline.locate(100_000), position(0, 30));
        assert_eq!(
            timeline.real_time(Position {
                file: 1,
                offset_ms: 0
            }),
            Some(370_000)
        );
    }

    #[test]
    fn files_are_put_in_order() {
        let timeline = Timeline::new(vec![file(2, 400, 60), file(1, 100, 60)], 0);
        assert_eq!(timeline.files()[0].seq, 1);
        assert_eq!(timeline.locate(410_000), position(1, 10));
    }

    #[test]
    fn a_position_outside_the_file_has_no_time() {
        let timeline = Timeline::new(vec![file(1, 100, 60)], 0);
        assert_eq!(
            timeline.real_time(Position {
                file: 0,
                offset_ms: 60_001
            }),
            None
        );
        assert_eq!(
            timeline.real_time(Position {
                file: 0,
                offset_ms: -1
            }),
            None
        );
        assert_eq!(
            timeline.real_time(Position {
                file: 1,
                offset_ms: 0
            }),
            None
        );
    }

    #[test]
    fn groups_by_the_date_in_japan() {
        // 1970-01-01 の JST 23:59 と、翌日の JST 00:01。
        let before_midnight = (15 * 3600 - 60) as i64;
        let after_midnight = (15 * 3600 + 60) as i64;
        let rides = group_by_date(vec![
            file(2, after_midnight, 10),
            file(1, before_midnight, 10),
        ]);
        let dates: Vec<_> = rides
            .iter()
            .map(|r| (r.date.as_str(), r.files.len()))
            .collect();
        assert_eq!(dates, [("1970-01-01", 1), ("1970-01-02", 1)]);
    }
}
