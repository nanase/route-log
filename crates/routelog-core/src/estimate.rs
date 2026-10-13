//! 通過時刻の推定。
//!
//! 時刻を確定したイベントを基準にして、その間を距離で線形に補間する。

use crate::event::{Event, EventDetail, EventKind, EventStatus};
use crate::geo;
use crate::ride::{RideDocument, Waypoint};

/// 経由点を動かしても、同じ経由点のイベントとみなす距離の上限（m）。
pub const WAYPOINT_KEEP_M: f64 = 30.0;

/// 時刻推定の基準。経路の上の位置と、そこを通った時刻の組。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Knot {
    pub route_m: f64,
    pub time_ms: i64,
}

fn is_knot(event: &Event) -> bool {
    event.status == EventStatus::Confirmed && event.time_ms.is_some() && !event.detail.by_time
}

/// 時刻を確定したイベントを、位置の順に並べる。
pub fn knots(events: &[Event]) -> Vec<Knot> {
    let mut knots: Vec<Knot> = events
        .iter()
        .filter(|event| is_knot(event))
        .filter_map(|event| {
            Some(Knot {
                route_m: event.route_m,
                time_ms: event.time_ms?,
            })
        })
        .collect();
    knots.sort_by(|a, b| {
        a.route_m
            .total_cmp(&b.route_m)
            .then(a.time_ms.cmp(&b.time_ms))
    });
    knots
}

/// その位置を通る時刻。最初の基準より前と、最後の基準より後は推定しない。
pub fn time_at(knots: &[Knot], route_m: f64) -> Option<i64> {
    if let Some(exact) = knots.iter().find(|k| k.route_m == route_m) {
        return Some(exact.time_ms);
    }
    let pair = knots
        .windows(2)
        .find(|pair| pair[0].route_m < route_m && route_m < pair[1].route_m)?;
    let t = (route_m - pair[0].route_m) / (pair[1].route_m - pair[0].route_m);
    Some(pair[0].time_ms + ((pair[1].time_ms - pair[0].time_ms) as f64 * t).round() as i64)
}

/// その時刻にいる位置。最初の基準より前と、最後の基準より後は分からない。
///
/// 同じ位置に基準が 2 つあれば、その間は止まっていたものとして同じ位置を返す。
pub fn position_at(knots: &[Knot], time_ms: i64) -> Option<f64> {
    let mut by_time = knots.to_vec();
    by_time.sort_by_key(|k| k.time_ms);
    if let Some(exact) = by_time.iter().find(|k| k.time_ms == time_ms) {
        return Some(exact.route_m);
    }
    let pair = by_time
        .windows(2)
        .find(|pair| pair[0].time_ms < time_ms && time_ms < pair[1].time_ms)?;
    let t = (time_ms - pair[0].time_ms) as f64 / (pair[1].time_ms - pair[0].time_ms) as f64;
    Some(pair[0].route_m + (pair[1].route_m - pair[0].route_m) * t)
}

fn endpoint(kind: EventKind, label: &str, route_m: f64) -> Event {
    Event {
        id: String::new(),
        kind,
        label: label.into(),
        route_m,
        time_ms: None,
        status: EventStatus::Estimated,
        hidden: false,
        detail: EventDetail::default(),
    }
}

/// 経路の両端に、出発と到着のイベントを 1 つずつ置く。経路が無ければ、確定していないものは消す。
fn ensure_endpoints(events: &mut Vec<Event>, length_m: f64) {
    let ends = [
        (EventKind::Departure, "出発", 0.0),
        (EventKind::Arrival, "到着", length_m),
    ];
    for (kind, label, route_m) in ends {
        if length_m <= 0.0 {
            events.retain(|e| e.kind != kind || e.status == EventStatus::Confirmed);
            continue;
        }
        match events.iter_mut().find(|e| e.kind == kind) {
            Some(event) => event.route_m = route_m,
            None => events.push(endpoint(kind, label, route_m)),
        }
    }
}

/// 経由点のイベントに付ける名前。番号は、地図の丸に書く番号と同じにする。
fn waypoint_label(index: usize) -> String {
    format!("経由点 {}", index + 1)
}

/// 経由点のイベントを、いまの経由点に合わせる。
///
/// イベントが覚えている位置にいちばん近い経由点を、そのイベントの経由点とみなし、位置と、
/// 人が直していない名前を、その経由点のものにする。経由点を消したか、遠くへ動かしたイベントは
/// 消す。通った場所が変わったので、確定した時刻も当てにならない。
fn place_waypoints(events: &mut Vec<Event>, waypoints: &[Waypoint], distances: &[f64]) {
    let mut taken = vec![false; waypoints.len()];
    events.retain_mut(|event| {
        if event.kind != EventKind::Waypoint {
            return true;
        }
        let nearest = event.detail.at.and_then(|at| {
            waypoints
                .iter()
                .enumerate()
                .filter(|(i, _)| !taken[*i])
                .map(|(i, w)| (i, geo::distance_m(at, [w.lon, w.lat])))
                .filter(|(_, distance)| *distance <= WAYPOINT_KEEP_M)
                .min_by(|a, b| a.1.total_cmp(&b.1))
        });
        let Some((i, _)) = nearest else {
            return false;
        };
        taken[i] = true;
        let name = waypoint_label(i);
        let renamed = !event.label.is_empty()
            && event.detail.extracted_label.as_deref() != Some(event.label.as_str());
        if !renamed {
            event.label = name.clone();
        }
        event.detail.extracted_label = Some(name);
        event.detail.at = Some([waypoints[i].lon, waypoints[i].lat]);
        event.route_m = distances[i];
        true
    });
}

/// イベントの時刻と位置を、いまの経路・確定した時刻・時計の補正値に合わせて計算し直す。
///
/// 1. 経路の両端に出発と到着を置く。経由点のイベントを、いまの経由点の位置に置く
/// 2. 動画で確定した時刻を、カメラの時刻と時計の補正値から求め直す
/// 3. 時刻だけが分かっているイベントの位置を、時刻から求める
/// 4. 確定していないイベントの時刻を推定する
///
/// 位置が先なのに時刻が後になっている、確定済みのイベントの ID を返す。
pub fn refresh(doc: &mut RideDocument) -> Vec<String> {
    let length_m = doc.length_m();
    let distances = doc.waypoint_distances();
    let clock_offset_ms = doc.ride.clock_offset_ms;
    let events = &mut doc.events;
    ensure_endpoints(events, length_m);
    place_waypoints(events, &doc.waypoints, &distances);

    for event in events.iter_mut() {
        if let (EventStatus::Confirmed, Some(camera_ms)) = (event.status, event.detail.camera_ms) {
            event.time_ms = Some(camera_ms + clock_offset_ms);
        }
    }

    let knots = knots(events);
    for event in events.iter_mut() {
        if event.detail.by_time {
            if let Some(route_m) = event.time_ms.and_then(|t| position_at(&knots, t)) {
                event.route_m = route_m;
            }
        } else if event.status != EventStatus::Confirmed {
            event.time_ms = time_at(&knots, event.route_m);
        }
    }
    // 同じ位置なら、出発を先頭に、到着を最後にする。
    let rank = |kind: EventKind| match kind {
        EventKind::Departure => 0,
        EventKind::Arrival => 2,
        _ => 1,
    };
    events.sort_by(|a, b| {
        a.route_m
            .total_cmp(&b.route_m)
            .then(rank(a.kind).cmp(&rank(b.kind)))
    });

    let confirmed: Vec<&Event> = events.iter().filter(|e| is_knot(e)).collect();
    let mut conflicts: Vec<String> = Vec::new();
    for pair in confirmed.windows(2) {
        if pair[0].time_ms > pair[1].time_ms {
            for event in pair {
                if !conflicts.contains(&event.id) {
                    conflicts.push(event.id.clone());
                }
            }
        }
    }
    conflicts
}

/// 推定した時刻を消す。推定は、経路と確定した時刻から `refresh` で求め直せる。
///
/// クラウドへ送る前に使う。時刻を 1 つ確定すると、まわりのイベントの推定がみな動く。推定まで
/// 送ると、確定のたびに、多くのイベントを書き換えさせることになる。
pub fn clear_estimates(doc: &mut RideDocument) {
    for event in &mut doc.events {
        if event.status == EventStatus::Estimated {
            event.time_ms = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ride::{Ride, Route, SegmentMode};

    fn knot(route_m: f64, time_s: i64) -> Knot {
        Knot {
            route_m,
            time_ms: time_s * 1000,
        }
    }

    #[test]
    fn time_is_interpolated_by_distance() {
        let knots = [knot(0.0, 0), knot(1000.0, 100), knot(3000.0, 500)];
        assert_eq!(time_at(&knots, 500.0), Some(50_000));
        assert_eq!(time_at(&knots, 2000.0), Some(300_000));
        assert_eq!(time_at(&knots, 1000.0), Some(100_000));
    }

    #[test]
    fn nothing_is_estimated_outside_the_knots() {
        let knots = [knot(1000.0, 100), knot(3000.0, 500)];
        assert_eq!(time_at(&knots, 999.0), None);
        assert_eq!(time_at(&knots, 3001.0), None);
        assert_eq!(time_at(&[knot(1000.0, 100)], 1000.0), Some(100_000));
        assert_eq!(time_at(&[knot(1000.0, 100)], 1001.0), None);
        assert_eq!(time_at(&[], 0.0), None);
    }

    #[test]
    fn position_is_the_inverse_of_time() {
        let knots = [knot(0.0, 0), knot(1000.0, 100), knot(3000.0, 500)];
        assert_eq!(position_at(&knots, 50_000), Some(500.0));
        assert_eq!(position_at(&knots, 300_000), Some(2000.0));
        assert_eq!(position_at(&knots, -1), None);
        assert_eq!(position_at(&knots, 500_001), None);
    }

    #[test]
    fn a_stop_keeps_the_position_while_time_passes() {
        // 1000 m の所に 100 秒から 400 秒まで止まっていた。
        let knots = [
            knot(0.0, 0),
            knot(1000.0, 100),
            knot(1000.0, 400),
            knot(2000.0, 500),
        ];
        assert_eq!(position_at(&knots, 250_000), Some(1000.0));
        assert_eq!(position_at(&knots, 450_000), Some(1500.0));
        assert_eq!(time_at(&knots, 1500.0), Some(450_000));
    }

    fn event(id: &str, kind: EventKind, route_m: f64, time_s: Option<i64>) -> Event {
        Event {
            id: id.into(),
            kind,
            label: id.into(),
            route_m,
            time_ms: time_s.map(|s| s * 1000),
            status: match time_s {
                Some(_) => EventStatus::Confirmed,
                None => EventStatus::Estimated,
            },
            hidden: false,
            detail: EventDetail::default(),
        }
    }

    /// 赤道の上を東へ `length_m` 進む経路。
    fn document(length_m: f64, clock_offset_ms: i64, events: Vec<Event>) -> RideDocument {
        let waypoint = |lon| Waypoint {
            lon,
            lat: 0.0,
            segment_mode: SegmentMode::Straight,
        };
        let waypoints = if length_m > 0.0 {
            vec![waypoint(0.0), waypoint(length_m / 111_195.08)]
        } else {
            vec![]
        };
        RideDocument {
            ride: Ride {
                id: "x".into(),
                title: "t".into(),
                ride_date: "2026-10-03".into(),
                clock_offset_ms,
                is_public: false,
                created_at: "a".into(),
                updated_at: "b".into(),
            },
            route: Route {
                segments: vec![vec![]; waypoints.len().saturating_sub(1)],
            },
            waypoints,
            events,
        }
    }

    fn times(doc: &RideDocument) -> Vec<(&str, Option<i64>)> {
        doc.events
            .iter()
            .map(|e| (e.id.as_str(), e.time_ms.map(|t| t / 1000)))
            .collect()
    }

    #[test]
    fn both_ends_of_the_route_get_an_event() {
        let mut doc = document(2000.0, 0, vec![]);
        refresh(&mut doc);
        let ends: Vec<_> = doc
            .events
            .iter()
            .map(|e| (e.kind, e.label.as_str(), e.route_m.round()))
            .collect();
        assert_eq!(
            ends,
            [
                (EventKind::Departure, "出発", 0.0),
                (EventKind::Arrival, "到着", 2000.0)
            ]
        );
        // もう一度計算しても増えない。経路が伸びれば、到着だけが動く。
        doc.waypoints[1].lon *= 2.0;
        refresh(&mut doc);
        assert_eq!(doc.events.len(), 2);
        assert_eq!(doc.events[1].route_m.round(), 4000.0);
    }

    #[test]
    fn at_the_same_place_the_departure_comes_first_and_the_arrival_last() {
        let mut doc = document(
            2000.0,
            0,
            vec![
                event("road", EventKind::Road, 0.0, None),
                event("station", EventKind::MichiNoEki, 2000.0, None),
            ],
        );
        refresh(&mut doc);
        // 到着は経路の長さの位置に置かれる。丸めの差で道の駅より手前に出ないよう、位置を揃える。
        let end = doc.length_m();
        doc.events
            .iter_mut()
            .find(|e| e.id == "station")
            .unwrap()
            .route_m = end;
        refresh(&mut doc);
        let kinds: Vec<_> = doc.events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [
                EventKind::Departure,
                EventKind::Road,
                EventKind::MichiNoEki,
                EventKind::Arrival
            ]
        );
    }

    #[test]
    fn without_a_route_the_unconfirmed_ends_go_away() {
        let mut doc = document(
            0.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(10)),
                event("a", EventKind::Arrival, 0.0, None),
            ],
        );
        refresh(&mut doc);
        assert_eq!(times(&doc), [("d", Some(10))]);
    }

    #[test]
    fn events_between_two_confirmed_ones_get_a_time() {
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(0)),
                event("x", EventKind::Tunnel, 1000.0, None),
                event("y", EventKind::Bridge, 3000.0, None),
                event("a", EventKind::Arrival, 4000.0, Some(400)),
            ],
        );
        assert_eq!(refresh(&mut doc), Vec::<String>::new());
        assert_eq!(
            times(&doc),
            [
                ("d", Some(0)),
                ("x", Some(100)),
                ("y", Some(300)),
                ("a", Some(400))
            ]
        );
    }

    #[test]
    fn confirming_one_in_the_middle_corrects_both_sides() {
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(0)),
                event("x", EventKind::Tunnel, 1000.0, None),
                event("m", EventKind::PrefBoundary, 2000.0, Some(300)),
                event("y", EventKind::Bridge, 3000.0, None),
                event("a", EventKind::Arrival, 4000.0, Some(400)),
            ],
        );
        refresh(&mut doc);
        assert_eq!(times(&doc)[1], ("x", Some(150)));
        assert_eq!(times(&doc)[3], ("y", Some(350)));
    }

    #[test]
    fn estimates_can_be_dropped_and_worked_out_again() {
        let mut memo = event("memo", EventKind::Custom, 0.0, Some(100));
        memo.detail.by_time = true;
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(0)),
                memo,
                event("x", EventKind::Tunnel, 3000.0, None),
                event("a", EventKind::Arrival, 4000.0, Some(400)),
            ],
        );
        refresh(&mut doc);
        let full = doc.clone();

        clear_estimates(&mut doc);
        // 確定した時刻は残る。推定だけが消える。
        assert_eq!(
            times(&doc),
            [
                ("d", Some(0)),
                ("memo", Some(100)),
                ("x", None),
                ("a", Some(400))
            ]
        );
        refresh(&mut doc);
        assert_eq!(doc, full);
    }

    #[test]
    fn an_estimate_that_lost_its_knots_is_cleared() {
        let mut stale = event("x", EventKind::Tunnel, 1000.0, None);
        stale.time_ms = Some(123_000);
        let mut doc = document(
            4000.0,
            0,
            vec![event("d", EventKind::Departure, 0.0, Some(0)), stale],
        );
        refresh(&mut doc);
        assert_eq!(times(&doc)[1], ("x", None));
    }

    #[test]
    fn a_time_confirmed_on_video_moves_with_the_clock_offset_but_a_typed_one_does_not() {
        let mut on_video = event("d", EventKind::Departure, 0.0, Some(0));
        on_video.detail.camera_ms = Some(0);
        let typed = event("a", EventKind::Arrival, 4000.0, Some(400));
        let mut doc = document(4000.0, 60_000, vec![on_video, typed]);
        refresh(&mut doc);
        assert_eq!(times(&doc), [("d", Some(60)), ("a", Some(400))]);
    }

    #[test]
    fn an_event_known_by_its_time_is_placed_on_the_route_and_is_not_a_knot() {
        let mut memo = event("memo", EventKind::Custom, 0.0, Some(100));
        memo.detail.by_time = true;
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(0)),
                memo,
                event("x", EventKind::Tunnel, 3000.0, None),
                event("a", EventKind::Arrival, 4000.0, Some(400)),
            ],
        );
        refresh(&mut doc);
        let memo = doc.events.iter().find(|e| e.id == "memo").unwrap();
        assert_eq!(memo.route_m.round(), 1000.0);
        // 手動のイベントを基準にしていれば、x は 100 秒と 400 秒の間で補間されて 300 秒より後になる。
        assert_eq!(times(&doc)[2], ("x", Some(300)));
    }

    #[test]
    fn a_hidden_event_with_a_confirmed_time_is_still_a_knot() {
        let mut silent = event("m", EventKind::Tunnel, 2000.0, Some(300));
        silent.hidden = true;
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(0)),
                event("x", EventKind::Bridge, 1000.0, None),
                silent,
                event("a", EventKind::Arrival, 4000.0, Some(400)),
            ],
        );
        refresh(&mut doc);
        assert_eq!(times(&doc)[1], ("x", Some(150)));
        assert_eq!(times(&doc)[2], ("m", Some(300)));
    }

    /// 赤道の上を東へ、1000 m ごとに経由点を置いた経路。
    fn with_waypoints(count: usize, events: Vec<Event>) -> RideDocument {
        let mut doc = document(0.0, 0, events);
        doc.waypoints = (0..count)
            .map(|i| Waypoint {
                lon: i as f64 * 1000.0 / 111_195.08,
                lat: 0.0,
                segment_mode: SegmentMode::Straight,
            })
            .collect();
        doc.route.segments = vec![vec![]; count.saturating_sub(1)];
        doc
    }

    fn at_waypoint(id: &str, doc: &RideDocument, index: usize, time_s: Option<i64>) -> Event {
        let mut event = event(id, EventKind::Waypoint, 0.0, time_s);
        event.label = String::new();
        event.hidden = true;
        event.detail.at = Some([doc.waypoints[index].lon, doc.waypoints[index].lat]);
        event
    }

    #[test]
    fn a_waypoint_event_sits_on_its_waypoint_and_can_be_a_knot() {
        let mut doc = with_waypoints(4, vec![]);
        let timed = at_waypoint("w", &doc, 2, Some(250));
        doc.events = vec![
            event("d", EventKind::Departure, 0.0, Some(0)),
            event("x", EventKind::Tunnel, 1000.0, None),
            timed,
        ];
        refresh(&mut doc);
        let waypoint = doc.events.iter().find(|e| e.id == "w").unwrap();
        assert_eq!(
            (waypoint.label.as_str(), waypoint.route_m.round()),
            ("経由点 3", 2000.0)
        );
        // 経由点の時刻が基準になり、手前のトンネルの推定が出る。
        assert_eq!(times(&doc)[1], ("x", Some(125)));
    }

    #[test]
    fn a_waypoint_event_follows_its_waypoint_and_goes_away_with_it() {
        let mut doc = with_waypoints(4, vec![]);
        let mut named = at_waypoint("n", &doc, 1, None);
        named.label = "役場前の交差点".into();
        doc.events = vec![named, at_waypoint("w", &doc, 2, Some(250))];
        refresh(&mut doc);

        // 手前に経由点を足すと番号がずれる。人が付けた名前は残り、付けていない名前は番号に合わせる。
        let first = doc.waypoints[0].clone();
        doc.waypoints.insert(
            1,
            Waypoint {
                lon: first.lon + 500.0 / 111_195.08,
                ..first
            },
        );
        doc.route.segments.push(vec![]);
        // 20 m だけ動かした経由点は、同じ経由点として扱う。
        doc.waypoints[3].lon += 20.0 / 111_195.08;
        refresh(&mut doc);
        let found: Vec<_> = doc
            .events
            .iter()
            .filter(|e| e.kind == EventKind::Waypoint)
            .map(|e| {
                (
                    e.id.as_str(),
                    e.label.as_str(),
                    e.route_m.round(),
                    e.time_ms,
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("n", "役場前の交差点", 1000.0, None),
                ("w", "経由点 4", 2020.0, Some(250_000))
            ]
        );

        // 経由点を消すと、そのイベントも消える。
        doc.waypoints.remove(3);
        doc.route.segments.pop();
        refresh(&mut doc);
        assert!(doc.events.iter().all(|e| e.id != "w"));
        assert!(doc.events.iter().any(|e| e.id == "n"));
    }

    #[test]
    fn confirmed_times_out_of_order_are_reported() {
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(100)),
                event("m", EventKind::PrefBoundary, 2000.0, Some(50)),
                event("a", EventKind::Arrival, 4000.0, Some(400)),
            ],
        );
        assert_eq!(refresh(&mut doc), ["d", "m"]);
    }
}
