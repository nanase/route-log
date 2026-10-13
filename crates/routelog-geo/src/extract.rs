//! 経路からイベントを抽出する。
//!
//! 経路を一定の間隔で区切り、その点ごとに「どの路線の上か」「橋やトンネルの上か」
//! 「どの市区町村の中か」を調べる。答えが変わった所がイベントになる。

use std::collections::BTreeSet;
use std::sync::OnceLock;

use routelog_core::geo::{self, LonLat};
use routelog_core::{Event, EventDetail, EventKind, EventStatus, RouteRef, route_name};
use rstar::primitives::{GeomWithData, Line, Rectangle};
use rstar::{PointDistance, RTree};

use crate::data::{Area, BBox, GeoData, Structure};
use crate::snap::{Graph, OnRoad, SnapOptions, usable};

/// 緯度 1 度の長さ（m）。
const M_PER_DEGREE: f64 = 111_195.08;

/// 抽出の閾値。どれも m。
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// 経路を区切る間隔。
    pub step_m: f64,
    /// 経路からこの距離までの道を、走っている道とみなす。
    pub road_m: f64,
    /// 経路と道の向きの差がこれ（度）より大きければ、交わるだけの道とみなす。
    pub road_angle_degrees: f64,
    /// これより短い路線の切り替わりは無視する。
    pub road_noise_m: f64,
    /// 路線の無い道をこれ以上走ったら、同じ路線に戻ってもイベントにする。
    pub road_again_m: f64,
    /// これより短い橋・トンネルは字幕にしない。
    pub structure_min_m: f64,
    /// 同じ名前の橋・トンネルの間がこれ以下なら、1 つにまとめる。
    pub structure_join_m: f64,
    /// これより短い市区町村の出入りは無視する。境に沿う道で境を何度もまたがないため。
    pub area_noise_m: f64,
    /// 経路からこの距離までの道の駅を、通過したとみなす。
    pub station_m: f64,
    /// 同じ道の駅から離れていた距離がこれ以下なら、同じ通過とみなす。
    pub station_apart_m: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            step_m: 10.0,
            road_m: 15.0,
            road_angle_degrees: 30.0,
            road_noise_m: 100.0,
            road_again_m: 1000.0,
            structure_min_m: 50.0,
            structure_join_m: 30.0,
            area_noise_m: 50.0,
            station_m: 100.0,
            station_apart_m: 500.0,
        }
    }
}

type Xy = [f64; 2];

/// 経度・緯度を、範囲の中心の緯度で平面（m）に写す。数十 km の範囲なら、距離の誤差は 1% ほど。
#[derive(Debug, Clone, Copy)]
struct Plane {
    m_per_lon_degree: f64,
}

impl Plane {
    fn at(latitude: f64) -> Self {
        Self {
            m_per_lon_degree: M_PER_DEGREE * latitude.to_radians().cos(),
        }
    }

    fn xy(self, point: LonLat) -> Xy {
        [point[0] * self.m_per_lon_degree, point[1] * M_PER_DEGREE]
    }
}

/// 経路の上の 1 点。
struct Sample {
    at: LonLat,
    xy: Xy,
    route_m: f64,
    /// 進む向きの単位ベクトル。
    heading: Xy,
}

fn unit(from: Xy, to: Xy) -> Option<Xy> {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let length = dx.hypot(dy);
    (length > 0.0).then(|| [dx / length, dy / length])
}

fn resample(line: &[LonLat], plane: Plane, step_m: f64) -> Vec<Sample> {
    let mut samples = Vec::new();
    // 区切りの始まりまでの距離と、次に点を置く距離。
    let (mut start_m, mut next_m) = (0.0, 0.0);
    for pair in line.windows(2) {
        let length_m = geo::distance_m(pair[0], pair[1]);
        let Some(heading) = unit(plane.xy(pair[0]), plane.xy(pair[1])) else {
            continue;
        };
        while next_m <= start_m + length_m {
            let t = (next_m - start_m) / length_m;
            let at = [
                pair[0][0] + (pair[1][0] - pair[0][0]) * t,
                pair[0][1] + (pair[1][1] - pair[0][1]) * t,
            ];
            samples.push(Sample {
                at,
                xy: plane.xy(at),
                route_m: next_m,
                heading,
            });
            next_m += step_m;
        }
        start_m += length_m;
    }
    samples
}

/// 同じ値が続く範囲 `[start, end)`。
fn runs<T: PartialEq>(values: &[T]) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for i in 0..values.len() {
        match runs.last_mut() {
            Some(run) if values[run.0] == values[i] => run.1 = i + 1,
            _ => runs.push((i, i + 1)),
        }
    }
    runs
}

/// `min` 個より短い範囲を、手前の値で埋める。先頭の短い範囲は、次の値で埋める。
fn absorb_short<T: PartialEq + Clone>(values: &mut [T], min: usize) {
    let found = runs(values);
    for (n, &(start, end)) in found.iter().enumerate() {
        if end - start >= min {
            continue;
        }
        let fill = match n {
            0 => found.get(1).map(|next| values[next.0].clone()),
            _ => Some(values[start - 1].clone()),
        };
        if let Some(fill) = fill {
            values[start..end].fill(fill);
        }
    }
}

fn inside(ring: &[LonLat], point: LonLat) -> bool {
    let mut inside = false;
    let mut previous = match ring.last() {
        Some(last) => *last,
        None => return false,
    };
    for &current in ring {
        let crosses = (current[1] > point[1]) != (previous[1] > point[1]);
        if crosses {
            let x = current[0]
                + (point[1] - current[1]) / (previous[1] - current[1]) * (previous[0] - current[0]);
            if point[0] < x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

fn contains(area: &Area, point: LonLat) -> bool {
    let mut rings = area.rings.iter();
    rings.next().is_some_and(|exterior| inside(exterior, point))
        && !rings.any(|hole| inside(hole, point))
}

fn event(kind: EventKind, label: String, route_m: f64) -> Event {
    Event {
        id: String::new(),
        kind,
        detail: EventDetail {
            extracted_label: Some(label.clone()),
            ..EventDetail::default()
        },
        label,
        route_m,
        time_ms: None,
        status: EventStatus::Estimated,
    }
}

/// ある範囲の判定用データに、探すための索引を付けたもの。
pub struct Prepared {
    data: GeoData,
    bbox: BBox,
    plane: Plane,
    /// 道の線分（平面）と、その道の番号。
    segments: RTree<GeomWithData<Line<Xy>, (usize, usize)>>,
    /// 吸着に使う道のグラフ。初めて吸着するときに作る。
    graph: OnceLock<Graph>,
    /// 面を囲む矩形（経度・緯度）と、その面の番号。
    areas: RTree<GeomWithData<Rectangle<LonLat>, usize>>,
}

impl Prepared {
    /// `bbox` は `data` を読んだ範囲。
    pub fn new(data: GeoData, bbox: BBox) -> Self {
        let plane = Plane::at((bbox.min[1] + bbox.max[1]) / 2.0);
        let segments = data
            .roads
            .iter()
            .enumerate()
            .flat_map(|(road, r)| {
                r.line.windows(2).enumerate().map(move |(segment, pair)| {
                    let line = Line::new(plane.xy(pair[0]), plane.xy(pair[1]));
                    GeomWithData::new(line, (road, segment))
                })
            })
            .collect();
        let areas = data
            .areas
            .iter()
            .enumerate()
            .filter_map(|(area, a)| {
                let bbox = BBox::around(a.rings.first()?)?;
                Some(GeomWithData::new(
                    Rectangle::from_corners(bbox.min, bbox.max),
                    area,
                ))
            })
            .collect();
        Self {
            data,
            bbox,
            plane,
            segments: RTree::bulk_load(segments),
            graph: OnceLock::new(),
            areas: RTree::bulk_load(areas),
        }
    }

    /// 読んである範囲。経路がこの中に収まっていれば、読み直さずに使える。
    pub fn bbox(&self) -> BBox {
        self.bbox
    }

    /// 道の上の、その点にいちばん近い位置。吸着に使える道が `radius_m` 以内に無ければ `None`。
    fn nearest(&self, graph: &Graph, point: LonLat, radius_m: f64) -> Option<OnRoad> {
        let xy = self.plane.xy(point);
        let segment = self
            .segments
            .locate_within_distance(xy, radius_m.powi(2))
            .filter(|segment| usable(&self.data.roads[segment.data.0]))
            .min_by(|a, b| {
                a.geom()
                    .distance_2(&xy)
                    .total_cmp(&b.geom().distance_2(&xy))
            })?;
        let line = segment.geom();
        let (dx, dy) = (line.to[0] - line.from[0], line.to[1] - line.from[1]);
        let length2 = dx * dx + dy * dy;
        let t = if length2 > 0.0 {
            ((xy[0] - line.from[0]) * dx + (xy[1] - line.from[1]) * dy) / length2
        } else {
            0.0
        };
        graph.on_road(segment.data.0, segment.data.1, t)
    }

    /// 吸着に使う道の辺の総数と、いちばん大きい繋がりに入る辺の数。
    pub fn road_connectivity(&self) -> (usize, usize) {
        self.graph
            .get_or_init(|| Graph::new(&self.data.roads))
            .connectivity()
    }

    /// その点にいちばん近い、道の上の点。吸着に使える道が `radius_m` 以内に無ければ `None`。
    pub fn snap_point(&self, point: LonLat, radius_m: f64) -> Option<LonLat> {
        let graph = self.graph.get_or_init(|| Graph::new(&self.data.roads));
        Some(graph.point(self.nearest(graph, point, radius_m)?))
    }

    /// 2 点の間を、道に沿って結ぶ線。両端は道の上の点になる。
    ///
    /// どちらかの近くに道が無いときと、道のりが直線距離に比べて長すぎるときは `None`。
    pub fn snap(&self, from: LonLat, to: LonLat, options: &SnapOptions) -> Option<Vec<LonLat>> {
        let graph = self.graph.get_or_init(|| Graph::new(&self.data.roads));
        let a = self.nearest(graph, from, options.road_m)?;
        let b = self.nearest(graph, to, options.road_m)?;
        // 経由点から道までの分だけ、道のりは直線より長くなりうる。
        let limit_m = geo::distance_m(from, to) * options.max_ratio + 2.0 * options.road_m;
        graph.path(a, b, limit_m)
    }

    /// 経路の折れ線からイベントを抽出する。位置の順に並ぶ。ID は空で、時刻は無い。
    pub fn extract(&self, line: &[LonLat], options: &Options) -> Vec<Event> {
        let samples = resample(line, self.plane, options.step_m);
        if samples.len() < 2 {
            return Vec::new();
        }
        let matched: Vec<Vec<(usize, f64)>> =
            samples.iter().map(|s| self.roads_at(s, options)).collect();
        let mut events = self.area_events(&samples, options);
        events.extend(self.road_events(&samples, &matched, options));
        events.extend(self.structure_events(&samples, &matched, options));
        events.extend(self.station_events(&samples, options));
        events.sort_by(|a, b| a.route_m.total_cmp(&b.route_m));
        events
    }

    /// その点で走っている道と、そこまでの距離。近い順。
    fn roads_at(&self, sample: &Sample, options: &Options) -> Vec<(usize, f64)> {
        let min_cos = options.road_angle_degrees.to_radians().cos();
        let slack = options.step_m / 2.0;
        let mut roads: Vec<(usize, f64)> = Vec::new();
        for segment in self
            .segments
            .locate_within_distance(sample.xy, options.road_m.powi(2))
        {
            let line = segment.geom();
            let Some(direction) = unit(line.from, line.to) else {
                continue;
            };
            let aligned = (direction[0] * sample.heading[0] + direction[1] * sample.heading[1])
                .abs()
                >= min_cos;
            // 線分の横にいるときだけ数える。端の先にいる点まで拾うと、道の始まりが手前へずれる。
            let offset = [sample.xy[0] - line.from[0], sample.xy[1] - line.from[1]];
            let along = offset[0] * direction[0] + offset[1] * direction[1];
            let length = (line.to[0] - line.from[0]).hypot(line.to[1] - line.from[1]);
            let beside = (-slack..=length + slack).contains(&along);
            if !aligned || !beside {
                continue;
            }
            let distance = line.distance_2(&sample.xy).sqrt();
            match roads.iter_mut().find(|(road, _)| *road == segment.data.0) {
                Some(found) => found.1 = found.1.min(distance),
                None => roads.push((segment.data.0, distance)),
            }
        }
        roads.sort_by(|a, b| a.1.total_cmp(&b.1));
        roads
    }

    fn road_events(
        &self,
        samples: &[Sample],
        matched: &[Vec<(usize, f64)>],
        options: &Options,
    ) -> Vec<Event> {
        let mut routes: Vec<Vec<RouteRef>> = matched
            .iter()
            .map(|roads| {
                let set: BTreeSet<&RouteRef> = roads
                    .iter()
                    .map(|(road, _)| &self.data.roads[*road])
                    .filter(|road| road.kind == "road")
                    .flat_map(|road| &road.routes)
                    .collect();
                set.into_iter().cloned().collect()
            })
            .collect();
        absorb_short(
            &mut routes,
            (options.road_noise_m / options.step_m).ceil() as usize,
        );

        let mut events = Vec::new();
        let mut last: Option<&Vec<RouteRef>> = None;
        for (start, end) in runs(&routes) {
            let set = &routes[start];
            if set.is_empty() {
                if (end - start) as f64 * options.step_m >= options.road_again_m {
                    last = None;
                }
                continue;
            }
            if last != Some(set) {
                let names: Vec<String> = set.iter().map(route_name).collect();
                let mut event = event(EventKind::Road, names.join("・"), samples[start].route_m);
                event.detail.routes = set.clone();
                events.push(event);
            }
            last = Some(set);
        }
        events
    }

    fn structure_events(
        &self,
        samples: &[Sample],
        matched: &[Vec<(usize, f64)>],
        options: &Options,
    ) -> Vec<Event> {
        // 点ごとに、いちばん近い道の橋・トンネル。
        let on: Vec<Option<(Structure, &str)>> = matched
            .iter()
            .map(|roads| {
                let road = roads
                    .iter()
                    .map(|(road, _)| &self.data.roads[*road])
                    .find(|road| road.structure != Structure::None)?;
                Some((road.structure, road.structure_name.as_str()))
            })
            .collect();

        // 同じ橋・トンネルが way の切れ目で分かれているものを、1 つにまとめる。
        let join = (options.structure_join_m / options.step_m).ceil() as usize;
        let mut spans: Vec<(Structure, &str, usize, usize)> = Vec::new();
        for (start, end) in runs(&on) {
            let Some((structure, name)) = on[start] else {
                continue;
            };
            match spans.last_mut() {
                Some(last) if last.0 == structure && last.1 == name && start - last.3 <= join => {
                    last.3 = end
                }
                _ => spans.push((structure, name, start, end)),
            }
        }

        spans
            .into_iter()
            .map(|(structure, name, start, end)| {
                let kind = match structure {
                    Structure::Bridge => EventKind::Bridge,
                    _ => EventKind::Tunnel,
                };
                let end_m = samples[end - 1].route_m;
                let mut event = event(kind, name.to_owned(), samples[start].route_m);
                event.detail.end_m = Some(end_m);
                let short = end_m - samples[start].route_m < options.structure_min_m;
                if short || structure == Structure::Shed {
                    event.status = EventStatus::Hidden;
                }
                event
            })
            .collect()
    }

    fn area_events(&self, samples: &[Sample], options: &Options) -> Vec<Event> {
        // 点ごとに、その点がある面の番号。面と面の隙間では、直前の面にいることにする。
        let mut current: Option<usize> = None;
        let mut codes: Vec<Option<&str>> = Vec::with_capacity(samples.len());
        let mut by_code: Vec<(&str, usize)> = Vec::new();
        for sample in samples {
            let stays = current.is_some_and(|area| contains(&self.data.areas[area], sample.at));
            if !stays {
                let found = self
                    .areas
                    .locate_all_at_point(sample.at)
                    .map(|rectangle| rectangle.data)
                    .find(|area| contains(&self.data.areas[*area], sample.at));
                current = found.or(current);
            }
            let code = current.map(|area| self.data.areas[area].code.as_str());
            if let (Some(code), Some(area)) = (code, current)
                && !by_code.iter().any(|(known, _)| *known == code)
            {
                by_code.push((code, area));
            }
            codes.push(code);
        }
        absorb_short(
            &mut codes,
            (options.area_noise_m / options.step_m).ceil() as usize,
        );

        let area_of = |code: &str| {
            let (_, area) = by_code.iter().find(|(known, _)| *known == code)?;
            Some(&self.data.areas[*area])
        };
        let found = runs(&codes);
        let mut events = Vec::new();
        for pair in found.windows(2) {
            let (Some(from), Some(to)) = (codes[pair[0].0], codes[pair[1].0]) else {
                continue;
            };
            let (Some(from), Some(to)) = (area_of(from), area_of(to)) else {
                continue;
            };
            let route_m = samples[pair[1].0].route_m;
            if from.pref != to.pref {
                let label = format!("{} → {}", from.pref, to.pref);
                events.push(event(EventKind::PrefBoundary, label, route_m));
            }
            let label = format!("{} → {}", from.muni, to.muni);
            events.push(event(EventKind::MuniBoundary, label, route_m));
        }
        events
    }

    fn station_events(&self, samples: &[Sample], options: &Options) -> Vec<Event> {
        let apart = (options.station_apart_m / options.step_m).ceil() as usize;
        let mut events = Vec::new();
        for station in &self.data.stations {
            let xy = self.plane.xy(station.at);
            let distance = |sample: &Sample| (sample.xy[0] - xy[0]).hypot(sample.xy[1] - xy[1]);
            // 近くを通っている間の、いちばん近い点。離れてから戻ってきたら、別の通過にする。
            let mut pass: Option<(usize, f64, usize)> = None;
            for (i, sample) in samples.iter().enumerate() {
                let d = distance(sample);
                if d > options.station_m {
                    continue;
                }
                pass = match pass {
                    Some((best, best_d, last)) if i - last <= apart => Some(if d < best_d {
                        (i, d, i)
                    } else {
                        (best, best_d, i)
                    }),
                    Some((best, _, _)) => {
                        events.push(event(
                            EventKind::MichiNoEki,
                            station.name.clone(),
                            samples[best].route_m,
                        ));
                        Some((i, d, i))
                    }
                    None => Some((i, d, i)),
                };
            }
            if let Some((best, _, _)) = pass {
                events.push(event(
                    EventKind::MichiNoEki,
                    station.name.clone(),
                    samples[best].route_m,
                ));
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use routelog_core::RouteClass;

    use super::*;
    use crate::data::{Road, Station};

    const ORIGIN: LonLat = [138.0, 36.0];

    /// 原点から東へ `x`、北へ `y`（m）の点。
    fn at(x: f64, y: f64) -> LonLat {
        let plane = Plane::at(ORIGIN[1]);
        [
            ORIGIN[0] + x / plane.m_per_lon_degree,
            ORIGIN[1] + y / M_PER_DEGREE,
        ]
    }

    fn national(number: u32) -> RouteRef {
        RouteRef {
            class: RouteClass::National,
            pref: String::new(),
            number,
        }
    }

    fn prefectural(number: u32) -> RouteRef {
        RouteRef {
            class: RouteClass::Prefectural,
            pref: "nagano".into(),
            number,
        }
    }

    fn road(id: i64, line: Vec<LonLat>, routes: Vec<RouteRef>) -> Road {
        Road {
            id,
            line,
            routes,
            kind: "road".into(),
            grade: "trunk".into(),
            motorroad: false,
            structure: Structure::None,
            structure_name: String::new(),
        }
    }

    fn structure(id: i64, from_x: f64, to_x: f64, structure: Structure, name: &str) -> Road {
        Road {
            structure,
            structure_name: name.into(),
            ..road(id, vec![at(from_x, 0.0), at(to_x, 0.0)], vec![national(19)])
        }
    }

    fn square(code: &str, pref: &str, muni: &str, from_x: f64, to_x: f64) -> Area {
        Area {
            code: code.into(),
            pref: pref.into(),
            muni: muni.into(),
            rings: vec![vec![
                at(from_x, -5000.0),
                at(to_x, -5000.0),
                at(to_x, 5000.0),
                at(from_x, 5000.0),
                at(from_x, -5000.0),
            ]],
        }
    }

    fn prepared(data: GeoData) -> Prepared {
        let bbox = BBox::around(&[at(-10_000.0, -10_000.0), at(10_000.0, 10_000.0)]).unwrap();
        Prepared::new(data, bbox)
    }

    /// 原点から東へ `length` m のまっすぐな経路で抽出する。
    fn along(data: GeoData, length: f64) -> Vec<Event> {
        prepared(data).extract(&[at(0.0, 0.0), at(length, 0.0)], &Options::default())
    }

    fn summary(events: &[Event], kind: EventKind) -> Vec<(String, i64)> {
        events
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| (e.label.clone(), (e.route_m / 10.0).round() as i64 * 10))
            .collect()
    }

    fn roads(roads: Vec<Road>) -> GeoData {
        GeoData {
            roads,
            ..GeoData::default()
        }
    }

    #[test]
    fn a_change_of_routes_is_an_event_with_every_route_on_the_road() {
        let events = along(
            roads(vec![
                road(1, vec![at(0.0, 0.0), at(1000.0, 0.0)], vec![national(19)]),
                road(
                    2,
                    vec![at(1000.0, 0.0), at(2000.0, 0.0)],
                    vec![national(19), prefectural(80)],
                ),
            ]),
            3000.0,
        );
        assert_eq!(
            summary(&events, EventKind::Road),
            [
                ("国道19号".to_owned(), 0),
                ("国道19号・長野県道80号".to_owned(), 1000)
            ]
        );
        assert_eq!(events[1].detail.routes, [national(19), prefectural(80)]);
    }

    #[test]
    fn a_road_that_only_crosses_is_not_the_road_taken() {
        let events = along(
            roads(vec![
                road(1, vec![at(0.0, 0.0), at(2000.0, 0.0)], vec![national(19)]),
                road(
                    2,
                    vec![at(500.0, -500.0), at(500.0, 500.0)],
                    vec![prefectural(12)],
                ),
            ]),
            2000.0,
        );
        assert_eq!(
            summary(&events, EventKind::Road),
            [("国道19号".to_owned(), 0)]
        );
    }

    #[test]
    fn a_road_alongside_counts_only_when_it_is_close() {
        let near = roads(vec![road(
            1,
            vec![at(0.0, 10.0), at(2000.0, 10.0)],
            vec![national(19)],
        )]);
        assert_eq!(summary(&along(near, 2000.0), EventKind::Road).len(), 1);
        let far = roads(vec![road(
            1,
            vec![at(0.0, 30.0), at(2000.0, 30.0)],
            vec![national(19)],
        )]);
        assert_eq!(summary(&along(far, 2000.0), EventKind::Road), []);
    }

    #[test]
    fn a_short_stretch_of_another_route_is_noise() {
        let events = along(
            roads(vec![
                road(1, vec![at(0.0, 0.0), at(1000.0, 0.0)], vec![national(19)]),
                road(
                    2,
                    vec![at(1000.0, 0.0), at(1050.0, 0.0)],
                    vec![national(403)],
                ),
                road(
                    3,
                    vec![at(1050.0, 0.0), at(2000.0, 0.0)],
                    vec![national(19)],
                ),
            ]),
            2000.0,
        );
        assert_eq!(
            summary(&events, EventKind::Road),
            [("国道19号".to_owned(), 0)]
        );
    }

    #[test]
    fn coming_back_to_the_same_route_is_an_event_only_after_a_long_way_off_it() {
        let back_soon = roads(vec![
            road(1, vec![at(0.0, 0.0), at(1000.0, 0.0)], vec![national(19)]),
            road(
                2,
                vec![at(1500.0, 0.0), at(3000.0, 0.0)],
                vec![national(19)],
            ),
        ]);
        assert_eq!(summary(&along(back_soon, 3000.0), EventKind::Road).len(), 1);
        let back_late = roads(vec![
            road(1, vec![at(0.0, 0.0), at(1000.0, 0.0)], vec![national(19)]),
            road(
                2,
                vec![at(2500.0, 0.0), at(4000.0, 0.0)],
                vec![national(19)],
            ),
        ]);
        assert_eq!(
            summary(&along(back_late, 4000.0), EventKind::Road),
            [("国道19号".to_owned(), 0), ("国道19号".to_owned(), 2500)]
        );
    }

    #[test]
    fn a_road_that_is_not_an_ordinary_road_names_no_route() {
        let mut expressway = road(1, vec![at(0.0, 0.0), at(2000.0, 0.0)], vec![national(19)]);
        expressway.kind = "expressway".into();
        assert_eq!(
            summary(&along(roads(vec![expressway]), 2000.0), EventKind::Road),
            []
        );
    }

    #[test]
    fn a_bridge_and_a_tunnel_are_events_with_their_ends() {
        let events = along(
            roads(vec![
                structure(1, 500.0, 800.0, Structure::Bridge, "犀川大橋"),
                structure(2, 1500.0, 2500.0, Structure::Tunnel, "白地トンネル"),
            ]),
            3000.0,
        );
        assert_eq!(
            summary(&events, EventKind::Bridge),
            [("犀川大橋".to_owned(), 500)]
        );
        assert_eq!(
            summary(&events, EventKind::Tunnel),
            [("白地トンネル".to_owned(), 1500)]
        );
        let bridge = events.iter().find(|e| e.kind == EventKind::Bridge).unwrap();
        assert_eq!(bridge.detail.end_m.map(|m| m.round()), Some(800.0));
        assert_eq!(bridge.status, EventStatus::Estimated);
    }

    #[test]
    fn a_short_bridge_and_a_shed_are_hidden() {
        let events = along(
            roads(vec![
                structure(1, 500.0, 530.0, Structure::Bridge, "小さな橋"),
                structure(2, 1000.0, 1300.0, Structure::Shed, "四の岩覆道"),
            ]),
            2000.0,
        );
        let hidden: Vec<_> = events
            .iter()
            .filter(|e| e.status == EventStatus::Hidden)
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(hidden, ["小さな橋", "四の岩覆道"]);
    }

    #[test]
    fn a_bridge_split_into_ways_is_one_bridge_but_two_bridges_stay_two() {
        let events = along(
            roads(vec![
                structure(1, 500.0, 700.0, Structure::Bridge, "犀川大橋"),
                structure(2, 700.0, 900.0, Structure::Bridge, "犀川大橋"),
                structure(3, 900.0, 1100.0, Structure::Bridge, "別の橋"),
                structure(4, 2000.0, 2200.0, Structure::Bridge, ""),
                structure(5, 2200.0, 2400.0, Structure::Bridge, ""),
            ]),
            3000.0,
        );
        assert_eq!(
            summary(&events, EventKind::Bridge),
            [
                ("犀川大橋".to_owned(), 500),
                ("別の橋".to_owned(), 900),
                (String::new(), 2000)
            ]
        );
    }

    fn areas(areas: Vec<Area>) -> GeoData {
        GeoData {
            areas,
            ..GeoData::default()
        }
    }

    #[test]
    fn crossing_into_another_prefecture_is_two_events() {
        let events = along(
            areas(vec![
                square("20321", "長野県", "軽井沢町", -1000.0, 1500.0),
                square("10211", "群馬県", "安中市", 1500.0, 4000.0),
            ]),
            3000.0,
        );
        assert_eq!(
            summary(&events, EventKind::PrefBoundary),
            [("長野県 → 群馬県".to_owned(), 1500)]
        );
        assert_eq!(
            summary(&events, EventKind::MuniBoundary),
            [("軽井沢町 → 安中市".to_owned(), 1500)]
        );
    }

    #[test]
    fn crossing_into_another_ward_is_one_event() {
        let events = along(
            areas(vec![
                square("15101", "新潟県", "新潟市北区", -1000.0, 1500.0),
                square("15102", "新潟県", "新潟市東区", 1500.0, 4000.0),
            ]),
            3000.0,
        );
        assert_eq!(summary(&events, EventKind::PrefBoundary), []);
        assert_eq!(
            summary(&events, EventKind::MuniBoundary),
            [("新潟市北区 → 新潟市東区".to_owned(), 1500)]
        );
    }

    #[test]
    fn a_gap_between_two_areas_is_still_one_crossing() {
        let events = along(
            areas(vec![
                square("20321", "長野県", "軽井沢町", -1000.0, 1490.0),
                square("10211", "群馬県", "安中市", 1520.0, 4000.0),
            ]),
            3000.0,
        );
        assert_eq!(
            summary(&events, EventKind::MuniBoundary),
            [("軽井沢町 → 安中市".to_owned(), 1520)]
        );
    }

    #[test]
    fn dipping_into_the_next_area_for_a_moment_is_not_a_crossing() {
        let events = along(
            areas(vec![
                square("20321", "長野県", "軽井沢町", -1000.0, 1500.0),
                square("10211", "群馬県", "安中市", 1500.0, 1530.0),
                square("20321", "長野県", "軽井沢町", 1530.0, 4000.0),
            ]),
            3000.0,
        );
        assert_eq!(summary(&events, EventKind::MuniBoundary), []);
    }

    #[test]
    fn two_parts_of_the_same_municipality_are_not_a_crossing() {
        let events = along(
            areas(vec![
                square("20321", "長野県", "軽井沢町", -1000.0, 1500.0),
                square("20321", "長野県", "軽井沢町", 1500.0, 4000.0),
            ]),
            3000.0,
        );
        assert_eq!(summary(&events, EventKind::MuniBoundary), []);
    }

    #[test]
    fn a_hole_is_outside() {
        let mut area = square("20321", "長野県", "軽井沢町", -1000.0, 4000.0);
        area.rings.push(vec![
            at(1000.0, -100.0),
            at(2000.0, -100.0),
            at(2000.0, 100.0),
            at(1000.0, 100.0),
            at(1000.0, -100.0),
        ]);
        assert!(contains(&area, at(500.0, 0.0)));
        assert!(!contains(&area, at(1500.0, 0.0)));
        assert!(!contains(&area, at(5000.0, 0.0)));
    }

    fn stations(stations: Vec<(&str, LonLat)>) -> GeoData {
        GeoData {
            stations: stations
                .into_iter()
                .map(|(name, at)| Station {
                    name: name.into(),
                    at,
                })
                .collect(),
            ..GeoData::default()
        }
    }

    #[test]
    fn a_station_by_the_route_is_an_event_at_the_nearest_point() {
        let events = along(
            stations(vec![
                ("道の駅 しなの", at(800.0, 50.0)),
                ("道の駅 遠い", at(1500.0, 300.0)),
            ]),
            3000.0,
        );
        assert_eq!(
            summary(&events, EventKind::MichiNoEki),
            [("道の駅 しなの".to_owned(), 800)]
        );
    }

    #[test]
    fn passing_the_same_station_on_the_way_back_is_another_event() {
        let data = stations(vec![("道の駅 しなの", at(800.0, 50.0))]);
        let line = [at(0.0, 0.0), at(3000.0, 0.0), at(0.0, 0.0)];
        let events = prepared(data).extract(&line, &Options::default());
        assert_eq!(
            summary(&events, EventKind::MichiNoEki),
            [
                ("道の駅 しなの".to_owned(), 800),
                ("道の駅 しなの".to_owned(), 5200)
            ]
        );
    }

    #[test]
    fn events_come_in_the_order_of_the_route() {
        let data = GeoData {
            roads: vec![structure(1, 2000.0, 2300.0, Structure::Bridge, "犀川大橋")],
            areas: vec![
                square("20321", "長野県", "軽井沢町", -1000.0, 1500.0),
                square("10211", "群馬県", "安中市", 1500.0, 4000.0),
            ],
            stations: vec![Station {
                name: "道の駅 しなの".into(),
                at: at(800.0, 50.0),
            }],
        };
        let kinds: Vec<_> = along(data, 3000.0).iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [
                EventKind::MichiNoEki,
                EventKind::PrefBoundary,
                EventKind::MuniBoundary,
                EventKind::Road,
                EventKind::Bridge
            ]
        );
    }

    fn length(line: &[LonLat]) -> f64 {
        geo::length_m(line).round()
    }

    fn snapped(data: GeoData, from: LonLat, to: LonLat) -> Option<Vec<LonLat>> {
        prepared(data).snap(from, to, &SnapOptions::default())
    }

    /// 東西の道（0〜1000 m）と、その東の端から北へ延びる道（0〜1000 m）。
    fn corner() -> Vec<Road> {
        vec![
            road(
                1,
                vec![at(0.0, 0.0), at(500.0, 0.0), at(1000.0, 0.0)],
                vec![national(19)],
            ),
            road(
                2,
                vec![at(1000.0, 0.0), at(1000.0, 1000.0)],
                vec![prefectural(80)],
            ),
        ]
    }

    #[test]
    fn two_points_on_one_road_are_joined_along_it() {
        let line = snapped(roads(corner()), at(100.0, 10.0), at(900.0, -10.0)).unwrap();
        assert_eq!(length(&line), 800.0);
        // 両端は道の上に寄り、途中の頂点も通る。
        assert_eq!(line.len(), 3);
        assert!((line[0][1] - ORIGIN[1]).abs() < 1e-9);
    }

    #[test]
    fn the_line_turns_the_corner_where_two_roads_meet() {
        let line = snapped(roads(corner()), at(100.0, 5.0), at(995.0, 800.0)).unwrap();
        assert_eq!(length(&line), 900.0 + 800.0);
        assert!(line.contains(&at(1000.0, 0.0)));
    }

    #[test]
    fn it_works_in_either_direction() {
        let line = snapped(roads(corner()), at(995.0, 800.0), at(100.0, 5.0)).unwrap();
        assert_eq!(length(&line), 1700.0);
        assert!((line[0][0] - at(1000.0, 800.0)[0]).abs() < 1e-9);
    }

    #[test]
    fn a_road_that_ends_in_the_middle_of_another_is_a_junction() {
        // 東西の道の途中（500 m）から、北へ延びる道。
        let data = roads(vec![
            road(
                1,
                vec![at(0.0, 0.0), at(500.0, 0.0), at(1000.0, 0.0)],
                vec![national(19)],
            ),
            road(
                2,
                vec![at(500.0, 0.0), at(500.0, 600.0)],
                vec![prefectural(80)],
            ),
        ]);
        let line = snapped(data, at(200.0, 0.0), at(500.0, 400.0)).unwrap();
        assert_eq!(length(&line), 300.0 + 400.0);
    }

    #[test]
    fn a_detour_much_longer_than_the_straight_line_is_refused() {
        // 東へ 1000 m、北へ 200 m、西へ 1000 m と折り返す道。吸着を諦めるのは、直線の 3 倍から。
        let hairpin = || {
            roads(vec![
                road(1, vec![at(0.0, 0.0), at(1000.0, 0.0)], vec![national(19)]),
                road(
                    2,
                    vec![at(1000.0, 0.0), at(1000.0, 200.0)],
                    vec![national(19)],
                ),
                road(
                    3,
                    vec![at(1000.0, 200.0), at(0.0, 200.0)],
                    vec![national(19)],
                ),
            ])
        };
        // 直線なら 200 m の 2 点が、道では 2000 m 離れている。
        assert_eq!(snapped(hairpin(), at(100.0, 0.0), at(100.0, 200.0)), None);
        // 折り返しの近くなら、道のりは直線の 1.5 倍に収まる。
        let near = snapped(hairpin(), at(950.0, 0.0), at(1000.0, 200.0));
        assert_eq!(near.map(|l| length(&l)), Some(250.0));
    }

    #[test]
    fn a_road_with_a_route_number_is_preferred_to_a_slightly_shorter_one() {
        // 番号のある道は 1000 m、番号の無い道は斜めに 950 m で、同じ 2 点を結ぶ。
        let mut shortcut = road(
            2,
            vec![at(0.0, 0.0), at(500.0, 150.0), at(1000.0, 0.0)],
            vec![],
        );
        shortcut.grade = "tertiary".into();
        let data = roads(vec![
            road(
                1,
                vec![at(0.0, 0.0), at(500.0, -5.0), at(1000.0, 0.0)],
                vec![national(19)],
            ),
            shortcut,
            road(3, vec![at(-500.0, 0.0), at(0.0, 0.0)], vec![national(19)]),
            road(
                4,
                vec![at(1000.0, 0.0), at(1500.0, 0.0)],
                vec![national(19)],
            ),
        ]);
        let line = snapped(data, at(-400.0, 0.0), at(1400.0, 0.0)).unwrap();
        assert!(line.contains(&at(500.0, -5.0)), "番号のある道を通る");
    }

    #[test]
    fn a_much_shorter_road_without_a_number_is_still_taken() {
        // 番号のある道は大回りで 3000 m、番号の無い道はまっすぐ 1000 m。
        let mut direct = road(2, vec![at(0.0, 0.0), at(1000.0, 0.0)], vec![]);
        direct.grade = "tertiary".into();
        let data = roads(vec![
            road(
                1,
                vec![at(0.0, 0.0), at(500.0, 1400.0), at(1000.0, 0.0)],
                vec![national(19)],
            ),
            direct,
        ]);
        let line = snapped(data, at(0.0, 0.0), at(1000.0, 0.0)).unwrap();
        assert_eq!(length(&line), 1000.0);
    }

    #[test]
    fn roads_that_share_points_form_one_network() {
        assert_eq!(prepared(roads(corner())).road_connectivity(), (2, 2));
        // 端の座標がわずかに違う道は、繋がらない。
        let apart = roads(vec![
            road(1, vec![at(0.0, 0.0), at(1000.0, 0.0)], vec![national(19)]),
            road(
                2,
                vec![at(1000.0, 0.1), at(1000.0, 1000.0)],
                vec![prefectural(80)],
            ),
        ]);
        assert_eq!(prepared(apart).road_connectivity(), (2, 1));
    }

    #[test]
    fn a_point_is_pulled_onto_the_nearest_road() {
        let prepared = prepared(roads(corner()));
        let on = prepared.snap_point(at(300.0, 80.0), 100.0).unwrap();
        assert!((on[0] - at(300.0, 0.0)[0]).abs() < 1e-9 && (on[1] - ORIGIN[1]).abs() < 1e-9);
        assert_eq!(prepared.snap_point(at(300.0, 80.0), 50.0), None);
    }

    #[test]
    fn a_point_with_no_road_nearby_is_not_snapped() {
        assert_eq!(
            snapped(roads(corner()), at(100.0, 50.0), at(900.0, 0.0)),
            None
        );
        assert_eq!(
            snapped(roads(corner()), at(100.0, 0.0), at(900.0, 50.0)),
            None
        );
    }

    #[test]
    fn roads_a_small_bike_cannot_take_are_not_used() {
        let mut motor = corner();
        motor[1].motorroad = true;
        assert_eq!(
            snapped(roads(motor), at(100.0, 5.0), at(995.0, 800.0)),
            None
        );
        let mut expressway = corner();
        expressway[0].kind = "expressway".into();
        assert_eq!(
            snapped(roads(expressway), at(100.0, 5.0), at(900.0, 0.0)),
            None
        );
    }

    #[test]
    fn roads_that_cross_without_sharing_a_point_are_not_joined() {
        // 立体交差。線は交わるが、同じ点を持たない。
        let data = roads(vec![
            road(1, vec![at(0.0, 0.0), at(1000.0, 0.0)], vec![national(19)]),
            road(
                2,
                vec![at(500.0, -500.0), at(500.0, 500.0)],
                vec![prefectural(80)],
            ),
        ]);
        assert_eq!(snapped(data, at(400.0, 0.0), at(500.0, 100.0)), None);
    }

    #[test]
    fn a_route_with_no_length_has_no_events() {
        let data = roads(vec![road(
            1,
            vec![at(0.0, 0.0), at(1000.0, 0.0)],
            vec![national(19)],
        )]);
        let prepared = prepared(data);
        assert_eq!(prepared.extract(&[], &Options::default()), []);
        assert_eq!(prepared.extract(&[at(0.0, 0.0)], &Options::default()), []);
        assert_eq!(
            prepared.extract(&[at(0.0, 0.0), at(0.0, 0.0)], &Options::default()),
            []
        );
    }
}
