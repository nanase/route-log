//! 回。YouTube に上げる 1 本ぶんの範囲で、走行の経路の一部を、始まりと終わりの経由点で決める。
//!
//! 回は Resolve へ書き出すタイムラインの単位と、概略図の動画の範囲になる。前後の回で範囲が
//! 重なってもよい。

use serde::{Deserialize, Serialize};

use crate::estimate::{self, Stop};
use crate::ride::RideDocument;
use crate::timeline::Span;

/// 回の録画の長さの目安の上限（ミリ秒）。Resolve のソースのタイムラインを、これより長くしない。
///
/// 超えても止めはせず、画面で知らせる。画面側（`episodes.ts`）も同じ値を持つ。
pub const RECORDED_LIMIT_MS: i64 = 5 * 60 * 60 * 1000;

/// 回。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    /// UUIDv7。
    pub id: String,
    /// 回の名前。空なら、画面は並びの番号で呼ぶ。
    #[serde(default)]
    pub title: String,
    /// 始まりの経由点の ID。
    pub from: String,
    /// 終わりの経由点の ID。
    pub to: String,
}

/// 回の範囲を、いまの経路に当てたもの。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    /// 始まりの経由点の番号。
    pub from: usize,
    /// 終わりの経由点の番号。`from` より大きい。
    pub to: usize,
    /// 始まりの、経路の始点からの距離（m）。
    pub start_m: f64,
    /// 終わりの、経路の始点からの距離（m）。
    pub end_m: f64,
    /// 始まりの経由点を通った時刻。推定できなければ `None`。
    pub start_ms: Option<i64>,
    /// 終わりの経由点を通った時刻。推定できなければ `None`。
    pub end_ms: Option<i64>,
}

impl RideDocument {
    /// 回の範囲を、いまの経路と時刻に当てる。
    ///
    /// 端の経由点が無いか、始まりが終わりより前になければ `None`。回はそのまま残るので、画面は
    /// 範囲が正しくないことを知らせて、人に直してもらう。
    pub fn extent(&self, episode: &Episode) -> Option<Extent> {
        let index = |id: &str| {
            (!id.is_empty())
                .then(|| self.waypoints.iter().position(|w| w.id == id))
                .flatten()
        };
        let (from, to) = (index(&episode.from)?, index(&episode.to)?);
        if from >= to {
            return None;
        }
        let distances = self.waypoint_distances();
        let (knots, stops) = (estimate::knots(&self.events), estimate::stops(&self.events));
        let time = |route_m| estimate::time_at(&knots, &stops, route_m);
        Some(Extent {
            from,
            to,
            start_m: distances[from],
            end_m: distances[to],
            start_ms: time(distances[from]),
            end_ms: time(distances[to]),
        })
    }
}

/// `[start_ms, end_ms)` のうち、録画していて、停車していない時間（ミリ秒）。
///
/// `spans` はタイムラインの範囲で、走行から外した動画を含まない。重ならず、時刻の順に並ぶ。
/// `stops` は `stops` が返した並びを渡す。
pub fn recorded_ms(spans: &[Span], stops: &[Stop], start_ms: i64, end_ms: i64) -> i64 {
    let overlap = |from: i64, to: i64| (to.min(end_ms) - from.max(start_ms)).max(0);
    let recorded: i64 = spans.iter().map(|s| overlap(s.start_ms, s.end_ms)).sum();
    // 停車のうち、録画していた部分だけを引く。停車の多くは、録画を止めた間にある。
    let stopped: i64 = stops
        .iter()
        .flat_map(|stop| {
            spans.iter().map(move |span| {
                let (from, to) = (
                    stop.start_ms.max(span.start_ms),
                    stop.end_ms.min(span.end_ms),
                );
                if from < to { overlap(from, to) } else { 0 }
            })
        })
        .sum();
    recorded - stopped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, EventDetail, EventKind, EventStatus};
    use crate::ride::{Ride, Route, SegmentMode, Waypoint};

    /// 東へ 0.01 度ずつ並べた経由点。ID は `w0`、`w1`…。
    fn document(count: usize, events: Vec<Event>) -> RideDocument {
        RideDocument {
            ride: Ride {
                id: "r".into(),
                title: "t".into(),
                ride_date: "2026-10-03".into(),
                clock_offset_ms: 0,
                is_public: false,
                labels: Vec::new(),
                created_at: "a".into(),
                updated_at: "b".into(),
            },
            waypoints: (0..count)
                .map(|i| Waypoint {
                    id: format!("w{i}"),
                    lon: 138.0 + i as f64 * 0.01,
                    lat: 36.0,
                    segment_mode: SegmentMode::Straight,
                })
                .collect(),
            route: Route {
                segments: vec![Vec::new(); count.saturating_sub(1)],
            },
            events,
            episodes: Vec::new(),
        }
    }

    fn episode(from: &str, to: &str) -> Episode {
        Episode {
            id: "e".into(),
            title: String::new(),
            from: from.into(),
            to: to.into(),
        }
    }

    fn confirmed(route_m: f64, time_ms: i64) -> Event {
        Event {
            id: format!("c{route_m}"),
            kind: EventKind::Custom,
            label: String::new(),
            route_m,
            time_ms: Some(time_ms),
            status: EventStatus::Confirmed,
            hidden: false,
            detail: EventDetail::default(),
        }
    }

    #[test]
    fn the_extent_follows_the_waypoints_by_id() {
        let doc = document(4, Vec::new());
        let distances = doc.waypoint_distances();
        let extent = doc.extent(&episode("w1", "w3")).unwrap();
        assert_eq!((extent.from, extent.to), (1, 3));
        assert_eq!((extent.start_m, extent.end_m), (distances[1], distances[3]));
        assert_eq!((extent.start_ms, extent.end_ms), (None, None));

        // 手前に経由点を足しても、同じ経由点を指す。
        let mut moved = doc.clone();
        moved.waypoints.insert(0, moved.waypoints[0].clone());
        moved.waypoints[0].id = "new".into();
        moved.waypoints[0].lon = 137.99;
        moved.route.segments.push(Vec::new());
        let extent = moved.extent(&episode("w1", "w3")).unwrap();
        assert_eq!((extent.from, extent.to), (2, 4));
    }

    #[test]
    fn a_broken_range_has_no_extent() {
        let doc = document(3, Vec::new());
        assert_eq!(doc.extent(&episode("w2", "w1")), None);
        assert_eq!(doc.extent(&episode("w1", "w1")), None);
        assert_eq!(doc.extent(&episode("w0", "gone")), None);
        // ID の無い経由点を、空の ID で指したことにはしない。
        let mut blank = doc.clone();
        blank.waypoints[0].id = String::new();
        assert_eq!(blank.extent(&episode("", "w2")), None);
    }

    #[test]
    fn the_ends_take_the_estimated_times() {
        let doc = document(3, Vec::new());
        let distances = doc.waypoint_distances();
        let events = vec![confirmed(0.0, 0), confirmed(distances[2], 2_000_000)];
        let doc = document(3, events);
        let extent = doc.extent(&episode("w1", "w2")).unwrap();
        assert_eq!(extent.start_ms, Some(1_000_000));
        assert_eq!(extent.end_ms, Some(2_000_000));
    }

    fn span(start_ms: i64, end_ms: i64) -> Span {
        Span {
            file: 0,
            start_ms,
            end_ms,
        }
    }

    #[test]
    fn only_the_recorded_time_inside_counts() {
        let spans = [span(0, 100), span(200, 300)];
        assert_eq!(recorded_ms(&spans, &[], 0, 300), 200);
        assert_eq!(recorded_ms(&spans, &[], 50, 250), 100);
        assert_eq!(recorded_ms(&spans, &[], 100, 200), 0);
        assert_eq!(recorded_ms(&[], &[], 0, 300), 0);
    }

    #[test]
    fn stops_are_taken_out_only_where_recorded() {
        let spans = [span(0, 100), span(200, 300)];
        // 録画の間の 20、録画を止めた間の 100、録画をまたぐ 20 + 20。
        let stops = [
            Stop {
                start_ms: 10,
                end_ms: 30,
            },
            Stop {
                start_ms: 80,
                end_ms: 220,
            },
        ];
        assert_eq!(recorded_ms(&spans, &stops, 0, 300), 200 - 20 - 40);
        // 範囲の外の停車は引かない。
        assert_eq!(recorded_ms(&spans, &stops, 40, 300), 160 - 40);
    }
}
