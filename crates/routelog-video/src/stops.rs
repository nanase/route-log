//! 停車の候補を、動画の音と映像の動きから見分ける規則。
//!
//! このカメラの録音では、止まると音が大きくなる（停車中は -14 dB 前後、走ると -17 dB 前後）。
//! 映像の動きだけでは、暗いトンネルの中も止まって見えるので、音と合わせて決める。
//!
//! 測った値は 1 秒ごとに持ち、候補は読むたびにここの規則で出す。規則を変えても、
//! 動画を読み直さずに済む。

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// 測り方の版。測り方（どこを細かく測るかを含む）を変えたら上げ、前の測定を使わない。
pub const MEASURE_VERSION: u32 = 1;

/// 音がこれ以上の秒を、止まっているかもしれない秒とする（1 チャンネル目の RMS、dB）。
const AUDIO_MIN_DB: f32 = -15.3;
/// 0.1 秒ごとの差で、動いた画素の割合（%）がこれ未満の秒を、止まっている秒とする。
const STILL_PCT: f32 = 1.0;
/// これより短い止まりは、候補にしない。
const MIN_SECONDS: usize = 3;
/// 止まっている秒の切れ目が、これ以下ならつなぐ。
const MERGE_SECONDS: usize = 2;
/// 長い停車の中では、人や車が前を通って動きが大きくなり、候補が割れる。切れ目がこれ以下の長さで、
/// その間に走行の大きさの音が `JOIN_QUIET_SECONDS` より長く続かなければ、前後の候補をつなぐ。
const JOIN_GAP_SECONDS: usize = 60;
const JOIN_QUIET_SECONDS: usize = 6;

/// キーフレームの差で、動いた画素の割合（%）がこれ未満の秒の周りだけを、細かく測る。
/// キーフレームは 0.5 秒おきなので、止まりかけの遅い動きも大きく出る。緩めにしておく。
const COARSE_STILL_PCT: f32 = 5.0;
/// 細かく測る所の前後に足す秒。止まり始めと走り出しを窓に入れる。
const WINDOW_PAD_SECONDS: usize = 8;

/// 動画 1 本を 1 秒ごとに測った値。i 番目は、動画の i 秒目からの 1 秒である。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measured {
    /// 測り方の版。`MEASURE_VERSION` と違えば使わない
    pub version: u32,
    /// 1 チャンネル目の音の大きさ（RMS、dB）
    pub audio_db: Vec<f32>,
    /// キーフレーム（0.5 秒おき）の差で、動いた画素の割合（%）。その秒の中の小さいほう
    pub coarse_pct: Vec<f32>,
    /// 0.1 秒ごとの差で、動いた画素の割合（%）の平均。細かく測っていない秒は `None`
    pub fine_pct: Vec<Option<f32>>,
}

/// 停車の候補。動画の中の位置で、`[start_ms, end_ms)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// 細かく測る秒の範囲。音が大きく、キーフレームで見て止まっていそうな所の前後である。
pub fn windows(audio_db: &[f32], coarse_pct: &[f32]) -> Vec<Range<usize>> {
    let seconds = audio_db.len().min(coarse_pct.len());
    let hits =
        (0..seconds).filter(|&s| audio_db[s] >= AUDIO_MIN_DB && coarse_pct[s] < COARSE_STILL_PCT);
    let mut windows: Vec<Range<usize>> = Vec::new();
    // 1 秒だけの当たりは、細かく測っても候補にならないので測らない。
    for run in runs(hits).into_iter().filter(|run| run.len() >= 2) {
        let window = run.start.saturating_sub(WINDOW_PAD_SECONDS)
            ..(run.end + WINDOW_PAD_SECONDS).min(audio_db.len());
        match windows.last_mut() {
            Some(last) if window.start <= last.end => last.end = last.end.max(window.end),
            _ => windows.push(window),
        }
    }
    windows
}

/// 停車の候補。動画の中の位置の順。
pub fn candidates(measured: &Measured) -> Vec<Candidate> {
    let Measured {
        audio_db, fine_pct, ..
    } = measured;
    let seconds = audio_db.len().min(fine_pct.len());
    let still = (0..seconds)
        .filter(|&s| fine_pct[s].is_some_and(|pct| pct < STILL_PCT) && audio_db[s] >= AUDIO_MIN_DB);
    let mut joined: Vec<Range<usize>> = Vec::new();
    for run in runs(still)
        .into_iter()
        .filter(|run| run.len() >= MIN_SECONDS)
    {
        match joined.last_mut() {
            Some(last) if joins(audio_db, last.end..run.start) => last.end = run.end,
            _ => joined.push(run),
        }
    }
    joined
        .into_iter()
        .map(|run| Candidate {
            start_ms: run.start as i64 * 1000,
            end_ms: run.end as i64 * 1000,
        })
        .collect()
}

/// 秒の並びを、切れ目が `MERGE_SECONDS` 以下ならつないで、続いた範囲にする。
fn runs(seconds: impl Iterator<Item = usize>) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    for s in seconds {
        match runs.last_mut() {
            Some(last) if s <= last.end + MERGE_SECONDS => last.end = s + 1,
            _ => runs.push(s..s + 1),
        }
    }
    runs
}

/// 2 つの候補の切れ目 `gap` が、同じ停車の中か。
fn joins(audio_db: &[f32], gap: Range<usize>) -> bool {
    if gap.len() > JOIN_GAP_SECONDS {
        return false;
    }
    let mut quiet = 0;
    for s in gap {
        quiet = if audio_db[s] < AUDIO_MIN_DB {
            quiet + 1
        } else {
            0
        };
        if quiet > JOIN_QUIET_SECONDS {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const STOPPED_DB: f32 = -14.0;
    const DRIVING_DB: f32 = -17.5;

    /// 秒ごとの様子を 1 文字で書いて、測った値にする。
    ///
    /// - `S`：止まっている（音は停車の大きさ、動きはほぼ 0）
    /// - `D`：走っている（音は走行の大きさ、動きは大きい）
    /// - `T`：トンネル（音は走行の大きさ、動きはほぼ 0）
    /// - `P`：止まっているが、前を人が通る（音は停車の大きさ、動きは大きい）
    /// - `C`：止まりかけ（音は停車の大きさ、キーフレームでは動いて見え、0.1 秒ごとでは小さい）
    fn measured(script: &str) -> Measured {
        let (audio_db, coarse_pct, fine_pct) = script
            .chars()
            .map(|c| match c {
                'S' => (STOPPED_DB, 0.1, 0.1),
                'D' => (DRIVING_DB, 30.0, 20.0),
                'T' => (DRIVING_DB, 0.5, 0.5),
                'P' => (STOPPED_DB, 40.0, 30.0),
                'C' => (STOPPED_DB, 8.0, 0.5),
                _ => panic!("{c}"),
            })
            .fold(
                (vec![], vec![], vec![]),
                |(mut a, mut c, mut f), (x, y, z)| {
                    a.push(x);
                    c.push(y);
                    f.push(Some(z));
                    (a, c, f)
                },
            );
        Measured {
            version: MEASURE_VERSION,
            audio_db,
            coarse_pct,
            fine_pct,
        }
    }

    fn seconds(candidates: &[Candidate]) -> Vec<(i64, i64)> {
        candidates
            .iter()
            .map(|c| (c.start_ms / 1000, c.end_ms / 1000))
            .collect()
    }

    #[test]
    fn a_stop_with_loud_engine_and_no_motion_is_a_candidate() {
        let m = measured("DDDDDSSSSSSDDDD");
        assert_eq!(seconds(&candidates(&m)), [(5, 11)]);
    }

    #[test]
    fn a_short_stop_is_not_a_candidate() {
        let m = measured("DDDDSSDDDDSSSDDD");
        assert_eq!(
            seconds(&candidates(&m)),
            [(10, 13)],
            "2 秒は短く、3 秒は候補"
        );
    }

    #[test]
    fn a_tunnel_looks_still_but_sounds_like_driving() {
        let m = measured("DDDTTTTTTTTTTDDD");
        assert_eq!(candidates(&m), []);
    }

    #[test]
    fn a_short_break_in_the_stillness_is_bridged() {
        let m = measured("DDSSSSPPSSSSDD");
        assert_eq!(seconds(&candidates(&m)), [(2, 12)]);
    }

    #[test]
    fn people_passing_by_during_a_long_stop_do_not_split_it() {
        let m = measured("DDSSSSSPPPPPPPPPPSSSSSDD");
        assert_eq!(seconds(&candidates(&m)), [(2, 22)]);
    }

    #[test]
    fn two_stops_with_driving_between_stay_apart() {
        // 走った音が 7 秒続いた。
        let m = measured("DDSSSSSDDDDDDDSSSSSDD");
        assert_eq!(seconds(&candidates(&m)), [(2, 7), (14, 19)]);
        // 6 秒までは、同じ停車の中とみなす。
        let m = measured("DDSSSSSPDDDDDDPSSSSSDD");
        assert_eq!(seconds(&candidates(&m)), [(2, 20)]);
    }

    #[test]
    fn stops_far_apart_are_not_joined_even_if_it_stays_loud() {
        let gap = "P".repeat(61);
        let m = measured(&format!("SSSS{gap}SSSS"));
        assert_eq!(seconds(&candidates(&m)), [(0, 4), (65, 69)]);
        let gap = "P".repeat(60);
        let m = measured(&format!("SSSS{gap}SSSS"));
        assert_eq!(seconds(&candidates(&m)), [(0, 68)]);
    }

    #[test]
    fn seconds_not_measured_finely_are_not_still() {
        let mut m = measured("DDSSSSSSDD");
        for s in 2..5 {
            m.fine_pct[s] = None;
        }
        assert_eq!(seconds(&candidates(&m)), [(5, 8)]);
    }

    #[test]
    fn windows_surround_the_loud_still_seconds() {
        let driving = "D".repeat(20);
        let m = measured(&format!(
            "{driving}CCSSSS{driving}SSSS{driving}TTTTTT{driving}S{driving}"
        ));
        assert_eq!(
            windows(&m.audio_db, &m.coarse_pct),
            [22 - 8..26 + 8, 46 - 8..50 + 8],
            "止まりかけはキーフレームで動いて見える。トンネルと 1 秒だけの止まりは測らない"
        );
    }

    #[test]
    fn windows_close_together_are_merged_and_kept_inside_the_video() {
        let m = measured(&format!("SSSS{}SSSS", "D".repeat(10)));
        assert_eq!(windows(&m.audio_db, &m.coarse_pct), vec![0..18]);
    }
}
