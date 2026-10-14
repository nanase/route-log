//! 通過時刻の推定。
//!
//! 時刻を確定したイベントを基準にして、その間を距離で線形に補間する。停車していた間は、
//! 時刻が進んでも位置が変わらないものとして、補間から除く。

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

/// 停車の場所を地図で決めたあと、経路をこれより遠くへ変えたら、決めた場所を捨てる（m）。
pub const STOP_KEEP_M: f64 = 50.0;

/// 停車の間に確定した時刻のうち、停車の場所からこれ以内のものは、停車の場所にいたものとみなす（m）。
pub const STOP_NEAR_M: f64 = 30.0;

/// 位置と時刻の組として確定したイベントか。場所を地図で決めた停車も含む。
fn is_knot(event: &Event) -> bool {
    event.status == EventStatus::Confirmed && event.time_ms.is_some() && !event.detail.by_time
}

/// 時刻推定の基準と、それを決めたイベントの ID。
struct Settled<'a> {
    knots: Vec<(Knot, &'a str)>,
    /// 停車の間に確定した時刻のうち、停車の場所と位置が合わないので、基準にしなかったもの
    aside: Vec<&'a str>,
}

/// 確定したイベントから、時刻推定の基準を作る。
///
/// 停車は、場所が分かれば、着いた時刻と出た時刻の 2 つの基準になる。場所は、地図で決めてあれば
/// そこ、無ければ停車の間に確定した最初のイベントの位置にする。停車の間に確定した他のイベントは、
/// 基準にしない。停車の間は進んでいないので、違う位置の基準があると、停車の終わりで位置が飛ぶ。
fn settle(events: &[Event]) -> Settled<'_> {
    let spans = stops(events);
    let stay = |event: &Event| event.detail.stay_ms.unwrap_or(0) > 0;
    let mut plain: Vec<(Knot, &str)> = events
        .iter()
        .filter(|event| is_knot(event) && !(event.kind == EventKind::Stop && stay(event)))
        .filter_map(|event| {
            let knot = Knot {
                route_m: event.route_m,
                time_ms: event.time_ms?,
            };
            Some((knot, event.id.as_str()))
        })
        .collect();
    plain.sort_by_key(|(knot, _)| knot.time_ms);
    let mut knots = Vec::new();
    let mut aside = Vec::new();
    for span in &spans {
        let within = |time_ms: i64| span.start_ms <= time_ms && time_ms <= span.end_ms;
        let pinned = events
            .iter()
            .filter(|event| event.kind == EventKind::Stop && is_knot(event) && stay(event))
            .filter(|event| event.time_ms.is_some_and(within))
            .min_by_key(|event| event.time_ms)
            .map(|event| (event.route_m, event.id.as_str()));
        let (inside, outside): (Vec<_>, Vec<_>) = plain
            .into_iter()
            .partition(|(knot, _)| within(knot.time_ms));
        plain = outside;
        let first = inside.first().map(|(knot, id)| (knot.route_m, *id));
        let Some((place_m, owner)) = pinned.or(first) else {
            plain.extend(inside);
            continue;
        };
        for (knot, id) in inside {
            if (knot.route_m - place_m).abs() > STOP_NEAR_M {
                aside.push(id);
            }
        }
        for time_ms in [span.start_ms, span.end_ms] {
            let knot = Knot {
                route_m: place_m,
                time_ms,
            };
            knots.push((knot, owner));
        }
    }
    knots.extend(plain);
    knots.sort_by(|(a, _), (b, _)| {
        a.route_m
            .total_cmp(&b.route_m)
            .then(a.time_ms.cmp(&b.time_ms))
    });
    Settled { knots, aside }
}

/// 時刻推定の基準を、位置の順に並べる。
pub fn knots(events: &[Event]) -> Vec<Knot> {
    settle(events)
        .knots
        .into_iter()
        .map(|(knot, _)| knot)
        .collect()
}

/// 停車していた時間。その間は、時刻が進んでも位置が変わらない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// 停車のイベントを、始まりの順に並べる。重なるものは 1 つにまとめる。
pub fn stops(events: &[Event]) -> Vec<Stop> {
    let mut found: Vec<Stop> = events
        .iter()
        .filter(|event| event.kind == EventKind::Stop)
        .filter_map(|event| {
            let start_ms = event.time_ms?;
            let stay_ms = event.detail.stay_ms.filter(|stay| *stay > 0)?;
            Some(Stop {
                start_ms,
                end_ms: start_ms + stay_ms,
            })
        })
        .collect();
    found.sort_by_key(|stop| stop.start_ms);
    let mut merged: Vec<Stop> = Vec::new();
    for stop in found {
        match merged.last_mut() {
            Some(last) if stop.start_ms <= last.end_ms => {
                last.end_ms = last.end_ms.max(stop.end_ms)
            }
            _ => merged.push(stop),
        }
    }
    merged
}

/// その時刻までに、走っていた時間。時刻から、それまでの停車の長さを引いたもの。
fn moving_ms(stops: &[Stop], time_ms: i64) -> i64 {
    let stopped: i64 = stops
        .iter()
        .map(|stop| (time_ms.min(stop.end_ms) - stop.start_ms).max(0))
        .sum();
    time_ms - stopped
}

/// 走っていた時間が `moving` になる時刻。停車の始まりにちょうど当たるなら、始まりの時刻にする。
fn clock_ms(stops: &[Stop], moving: i64) -> i64 {
    let mut stopped = 0;
    for stop in stops {
        if moving <= stop.start_ms - stopped {
            break;
        }
        stopped += stop.end_ms - stop.start_ms;
    }
    moving + stopped
}

/// その位置を通る時刻。最初の基準より前と、最後の基準より後は推定しない。
///
/// 基準の間は、走っていた時間に比例して進んだものとする。`stops` は `stops` が返した並びを渡す。
pub fn time_at(knots: &[Knot], stops: &[Stop], route_m: f64) -> Option<i64> {
    if let Some(exact) = knots.iter().find(|k| k.route_m == route_m) {
        return Some(exact.time_ms);
    }
    let pair = knots
        .windows(2)
        .find(|pair| pair[0].route_m < route_m && route_m < pair[1].route_m)?;
    let t = (route_m - pair[0].route_m) / (pair[1].route_m - pair[0].route_m);
    let (from, to) = (
        moving_ms(stops, pair[0].time_ms),
        moving_ms(stops, pair[1].time_ms),
    );
    // 2 つの基準が同じ停車の中にあると、走っていた時間が無い。手前の基準の時刻にしておく。
    if from == to {
        return Some(pair[0].time_ms);
    }
    Some(clock_ms(
        stops,
        from + ((to - from) as f64 * t).round() as i64,
    ))
}

/// その時刻にいる位置。最初の基準より前と、最後の基準より後は分からない。
///
/// 停車の間と、同じ位置にある 2 つの基準の間は、同じ位置を返す。
pub fn position_at(knots: &[Knot], stops: &[Stop], time_ms: i64) -> Option<f64> {
    let mut by_time = knots.to_vec();
    by_time.sort_by_key(|k| k.time_ms);
    if let Some(exact) = by_time.iter().find(|k| k.time_ms == time_ms) {
        return Some(exact.route_m);
    }
    let pair = by_time
        .windows(2)
        .find(|pair| pair[0].time_ms < time_ms && time_ms < pair[1].time_ms)?;
    let (from, to) = (
        moving_ms(stops, pair[0].time_ms),
        moving_ms(stops, pair[1].time_ms),
    );
    if from == to {
        return Some(pair[0].route_m);
    }
    let t = (moving_ms(stops, time_ms) - from) as f64 / (to - from) as f64;
    Some(pair[0].route_m + (pair[1].route_m - pair[0].route_m) * t)
}

/// 時刻だけが分かっているイベントを置く位置。
///
/// 基準の間なら、その時刻にいる位置。最初の基準より前なら最初の基準の位置、最後の基準より後なら
/// 最後の基準の位置にしておく。どこにいたかは分からないが、一覧では位置の順に並べるので、時刻の
/// 近い基準のそばに出す。経路の始点に置くと、時刻の離れたイベントの間に紛れる。基準が無ければ `None`。
fn place_at(knots: &[Knot], stops: &[Stop], time_ms: i64) -> Option<f64> {
    position_at(knots, stops, time_ms).or_else(|| {
        let first = knots.iter().min_by_key(|k| k.time_ms)?;
        let last = knots.iter().max_by_key(|k| k.time_ms)?;
        Some(if time_ms < first.time_ms {
            first.route_m
        } else {
            last.route_m
        })
    })
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

/// 場所を地図で決めた停車を、いまの経路の上に置き直す。経路が遠くへ変わっていたら、決めた場所を
/// 捨てて、前後の確定した時刻から位置を求める形に戻す。
fn place_stops(events: &mut [Event], line: &[geo::LonLat]) {
    for event in events.iter_mut().filter(|e| e.kind == EventKind::Stop) {
        let Some(at) = event.detail.at else {
            event.detail.by_time = true;
            continue;
        };
        match geo::nearest_m(line, at, event.route_m) {
            Some((route_m, off)) if off <= STOP_KEEP_M => {
                event.route_m = route_m;
                event.detail.by_time = false;
            }
            _ => {
                event.detail.at = None;
                event.detail.by_time = true;
            }
        }
    }
}

/// イベントの時刻と位置を、いまの経路・確定した時刻・時計の補正値に合わせて計算し直す。
///
/// 1. 経路の両端に出発と到着を置く。経由点のイベントと、場所を決めた停車を、いまの経路の上に置く
/// 2. 動画で確定した時刻を、カメラの時刻と時計の補正値から求め直す
/// 3. 時刻だけが分かっているイベントの位置を、時刻から求める。場所を決めていない停車も、ここで置く
/// 4. 確定していないイベントの時刻を推定する。停車していた間は、進んでいないものとする
///
/// 時刻の合わない、確定済みのイベントの ID を返す。位置が先なのに時刻が後になっているものと、
/// 停車の間に確定したのに、停車の場所と位置が合わないものである。
pub fn refresh(doc: &mut RideDocument) -> Vec<String> {
    let length_m = doc.length_m();
    let distances = doc.waypoint_distances();
    let line = doc.line();
    let clock_offset_ms = doc.ride.clock_offset_ms;
    let events = &mut doc.events;
    ensure_endpoints(events, length_m);
    place_waypoints(events, &doc.waypoints, &distances);
    place_stops(events, &line);

    for event in events.iter_mut() {
        if let (EventStatus::Confirmed, Some(camera_ms)) = (event.status, event.detail.camera_ms) {
            event.time_ms = Some(camera_ms + clock_offset_ms);
        }
    }

    let settled = settle(events);
    let knots: Vec<Knot> = settled.knots.iter().map(|(knot, _)| *knot).collect();
    let mut conflicts: Vec<String> = Vec::new();
    for pair in settled.knots.windows(2) {
        if pair[0].0.time_ms > pair[1].0.time_ms {
            for (_, id) in pair {
                if !conflicts.iter().any(|c| c == id) {
                    conflicts.push(id.to_string());
                }
            }
        }
    }
    conflicts.extend(settled.aside.iter().map(|id| id.to_string()));
    let stops = stops(events);
    for event in events.iter_mut() {
        if event.detail.by_time {
            if let Some(route_m) = event.time_ms.and_then(|t| place_at(&knots, &stops, t)) {
                event.route_m = route_m;
            }
        } else if event.status != EventStatus::Confirmed {
            event.time_ms = time_at(&knots, &stops, event.route_m);
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
        assert_eq!(time_at(&knots, &[], 500.0), Some(50_000));
        assert_eq!(time_at(&knots, &[], 2000.0), Some(300_000));
        assert_eq!(time_at(&knots, &[], 1000.0), Some(100_000));
    }

    #[test]
    fn nothing_is_estimated_outside_the_knots() {
        let knots = [knot(1000.0, 100), knot(3000.0, 500)];
        assert_eq!(time_at(&knots, &[], 999.0), None);
        assert_eq!(time_at(&knots, &[], 3001.0), None);
        assert_eq!(time_at(&[knot(1000.0, 100)], &[], 1000.0), Some(100_000));
        assert_eq!(time_at(&[knot(1000.0, 100)], &[], 1001.0), None);
        assert_eq!(time_at(&[], &[], 0.0), None);
    }

    #[test]
    fn position_is_the_inverse_of_time() {
        let knots = [knot(0.0, 0), knot(1000.0, 100), knot(3000.0, 500)];
        assert_eq!(position_at(&knots, &[], 50_000), Some(500.0));
        assert_eq!(position_at(&knots, &[], 300_000), Some(2000.0));
        assert_eq!(position_at(&knots, &[], -1), None);
        assert_eq!(position_at(&knots, &[], 500_001), None);
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
        assert_eq!(position_at(&knots, &[], 250_000), Some(1000.0));
        assert_eq!(position_at(&knots, &[], 450_000), Some(1500.0));
        assert_eq!(time_at(&knots, &[], 1500.0), Some(450_000));
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

    fn stop(id: &str, start_s: i64, stay_s: i64) -> Event {
        let mut event = event(id, EventKind::Stop, 0.0, Some(start_s));
        event.hidden = true;
        event.detail.by_time = true;
        event.detail.stay_ms = Some(stay_s * 1000);
        event
    }

    #[test]
    fn time_does_not_move_the_position_during_a_stop() {
        // 0 秒に 0 m、1000 秒に 4000 m。その間の 200 秒から 800 秒まで停車していた。
        // 走っていたのは 400 秒なので、1 秒に 10 m 進む。
        let knots = [knot(0.0, 0), knot(4000.0, 1000)];
        let stops = [Stop {
            start_ms: 200_000,
            end_ms: 800_000,
        }];
        assert_eq!(position_at(&knots, &stops, 100_000), Some(1000.0));
        assert_eq!(position_at(&knots, &stops, 200_000), Some(2000.0));
        assert_eq!(position_at(&knots, &stops, 500_000), Some(2000.0));
        assert_eq!(position_at(&knots, &stops, 800_000), Some(2000.0));
        assert_eq!(position_at(&knots, &stops, 900_000), Some(3000.0));

        assert_eq!(time_at(&knots, &stops, 1000.0), Some(100_000));
        // 停車した場所そのものは、着いた時刻にする。
        assert_eq!(time_at(&knots, &stops, 2000.0), Some(200_000));
        assert_eq!(time_at(&knots, &stops, 3000.0), Some(900_000));
    }

    #[test]
    fn stops_that_overlap_count_once() {
        let events = [stop("a", 100, 200), stop("b", 250, 150), stop("c", 900, 0)];
        assert_eq!(
            stops(&events),
            [Stop {
                start_ms: 100_000,
                end_ms: 400_000
            }]
        );
    }

    #[test]
    fn a_stop_event_shifts_the_estimates_and_is_placed_where_it_happened() {
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(0)),
                event("x", EventKind::Tunnel, 1000.0, None),
                event("y", EventKind::Bridge, 3000.0, None),
                event("a", EventKind::Arrival, 4000.0, Some(1000)),
                stop("s", 200, 600),
            ],
        );
        refresh(&mut doc);
        let found: Vec<_> = doc
            .events
            .iter()
            .map(|e| {
                (
                    e.id.as_str(),
                    e.route_m.round(),
                    e.time_ms.map(|t| t / 1000),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("d", 0.0, Some(0)),
                ("x", 1000.0, Some(100)),
                ("s", 2000.0, Some(200)),
                ("y", 3000.0, Some(900)),
                ("a", 4000.0, Some(1000))
            ]
        );
    }

    #[test]
    fn an_event_known_by_its_time_outside_the_knots_sits_by_the_nearest_knot() {
        // 確定してあるのは、出発と 1000 m のトンネルだけ。その後の停車と、出発より前のメモは、
        // どこにいたかが分からない。経路の始点ではなく、時刻の近い基準の位置に出す。
        let mut memo = event("memo", EventKind::Custom, 0.0, Some(50));
        memo.detail.by_time = true;
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(100)),
                event("x", EventKind::Tunnel, 1000.0, Some(200)),
                event("y", EventKind::Bridge, 3000.0, None),
                stop("s", 500, 600),
                memo,
            ],
        );
        refresh(&mut doc);
        let order: Vec<_> = doc
            .events
            .iter()
            .map(|e| (e.id.as_str(), e.route_m.round()))
            .collect();
        assert_eq!(
            order,
            [
                ("d", 0.0),
                ("memo", 0.0),
                ("x", 1000.0),
                ("s", 1000.0),
                ("y", 3000.0),
                ("", 4000.0)
            ]
        );
    }

    #[test]
    fn two_knots_inside_one_stop_do_not_break_the_estimate() {
        let knots = [knot(0.0, 300), knot(1000.0, 400)];
        let stops = [Stop {
            start_ms: 200_000,
            end_ms: 800_000,
        }];
        assert_eq!(time_at(&knots, &stops, 500.0), Some(300_000));
        assert_eq!(position_at(&knots, &stops, 350_000), Some(0.0));
    }

    /// 赤道の上を東へ `m` 進んだ点。
    fn east(m: f64) -> geo::LonLat {
        [m / 111_195.08, 0.0]
    }

    fn found(doc: &RideDocument) -> Vec<(&str, f64, Option<i64>)> {
        doc.events
            .iter()
            .map(|e| {
                (
                    e.id.as_str(),
                    e.route_m.round(),
                    e.time_ms.map(|t| t / 1000),
                )
            })
            .collect()
    }

    #[test]
    fn a_stop_placed_on_the_map_holds_the_position_there() {
        // 200 秒から 800 秒まで、1000 m の所に止まっていた。場所を決めなければ、走っていた時間の
        // 比から 2000 m の所に置かれる。
        let mut pinned = stop("s", 200, 600);
        pinned.detail.by_time = false;
        pinned.detail.at = Some(east(1000.0));
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(0)),
                event("x", EventKind::Tunnel, 500.0, None),
                event("y", EventKind::Bridge, 3000.0, None),
                event("a", EventKind::Arrival, 4000.0, Some(1000)),
                pinned,
            ],
        );
        assert_eq!(refresh(&mut doc), Vec::<String>::new());
        assert_eq!(
            found(&doc),
            [
                ("d", 0.0, Some(0)),
                ("x", 500.0, Some(100)),
                ("s", 1000.0, Some(200)),
                ("y", 3000.0, Some(933)),
                ("a", 4000.0, Some(1000))
            ]
        );
        let (knots, stops) = (knots(&doc.events), stops(&doc.events));
        let at = |time_ms| position_at(&knots, &stops, time_ms).map(f64::round);
        assert_eq!((at(800_000), at(900_000)), (Some(1000.0), Some(2500.0)));
    }

    #[test]
    fn a_stop_placed_on_the_map_goes_back_to_an_estimate_when_the_route_moves_away() {
        let mut pinned = stop("s", 200, 600);
        pinned.detail.by_time = false;
        pinned.detail.at = Some([east(1000.0)[0], 100.0 / 111_195.08]);
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(0)),
                event("a", EventKind::Arrival, 4000.0, Some(1000)),
                pinned,
            ],
        );
        refresh(&mut doc);
        let stop = doc.events.iter().find(|e| e.id == "s").unwrap();
        assert_eq!(
            (stop.detail.at, stop.detail.by_time, stop.route_m.round()),
            (None, true, 2000.0)
        );
    }

    #[test]
    fn a_confirmed_time_inside_a_stop_fixes_where_the_stop_was() {
        // 停車の間に、2000 m の所の時刻を確定した。そこが停車の場所になる。2010 m の所も停車の場所と
        // みなす。2100 m の所は、停車の間は進んでいないので合わない。基準にせず、知らせる。
        let mut doc = document(
            4000.0,
            0,
            vec![
                event("d", EventKind::Departure, 0.0, Some(0)),
                event("w", EventKind::PrefBoundary, 1500.0, Some(300)),
                event("near", EventKind::Custom, 1510.0, Some(500)),
                event("far", EventKind::Tunnel, 1600.0, Some(800)),
                event("y", EventKind::Bridge, 2500.0, None),
                event("a", EventKind::Arrival, 4000.0, Some(1000)),
                stop("s", 200, 600),
            ],
        );
        assert_eq!(refresh(&mut doc), ["far"]);
        let (knots, stops) = (knots(&doc.events), stops(&doc.events));
        let at = |time_ms| position_at(&knots, &stops, time_ms).map(f64::round);
        // 停車の終わりで飛ばない。1500 m から走り出す。
        assert_eq!(
            (at(799_999), at(800_000), at(900_000)),
            (Some(1500.0), Some(1500.0), Some(2750.0))
        );
        let stop = doc.events.iter().find(|e| e.id == "s").unwrap();
        assert_eq!(stop.route_m.round(), 1500.0);
        assert_eq!(times(&doc)[5], ("y", Some(880)));
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
