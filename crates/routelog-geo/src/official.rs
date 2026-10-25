//! 国土交通省の全国道路施設点検データベースの橋・トンネルを、抽出した橋・トンネルに対応づける。
//!
//! データは、全国Q地図が配るベクトルタイル（2024 年度 全国トンネルマップ・全国橋梁マップ）から
//! 読む。施設ごとに 1 点で、施設名、路線名、公称の長さを持つ。

use std::collections::BTreeSet;

use geozero::mvt::{Message, Tile, tile};
use routelog_core::geo::{self, LonLat};
use routelog_core::{Event, EventKind, RouteClass, RouteRef};

/// 緯度 1 度の長さ（m）。
const M_PER_DEGREE: f64 = 111_195.08;

/// 読むタイルのズーム。施設の点が欠けずに入る、いちばん細かいズームである。
pub const ZOOM: u8 = 13;

/// 施設の種類。タイルが種類ごとに分かれている。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FacilityKind {
    Bridge,
    Tunnel,
}

/// データベースの施設 1 件。
#[derive(Debug, Clone, PartialEq)]
pub struct Facility {
    pub kind: FacilityKind,
    pub at: LonLat,
    pub name: String,
    /// 公称の長さ（m）。トンネルは延長、橋は橋長。
    pub length_m: Option<f64>,
    /// 路線名。「国道19号」「県道26号線」「町道宇野尾線」のような文字列。
    pub route: String,
}

/// タイル 1 枚を読む。`x`・`y` はズーム `ZOOM` のタイルの番号。読めないタイルは空を返す。
pub fn decode(bytes: &[u8], kind: FacilityKind, x: u32, y: u32) -> Vec<Facility> {
    let Ok(tile) = Tile::decode(bytes) else {
        return Vec::new();
    };
    let (layer_name, length_key) = match kind {
        FacilityKind::Tunnel => ("tunnel", "延長（ｍ）"),
        FacilityKind::Bridge => ("bridge", "橋長（ｍ）"),
    };
    let Some(layer) = tile.layers.iter().find(|l| l.name == layer_name) else {
        return Vec::new();
    };
    let extent = f64::from(layer.extent.unwrap_or(4096));
    let mut found = Vec::new();
    for feature in &layer.features {
        if feature.r#type != Some(tile::GeomType::Point as i32) {
            continue;
        }
        let Some((px, py)) = first_point(&feature.geometry) else {
            continue;
        };
        let property = |key: &str| -> Option<&tile::Value> {
            feature.tags.as_chunks::<2>().0.iter().find_map(|pair| {
                let name = layer.keys.get(pair[0] as usize)?;
                (name == key).then(|| layer.values.get(pair[1] as usize))?
            })
        };
        let text = |key: &str| property(key).and_then(value_text).unwrap_or_default();
        found.push(Facility {
            kind,
            at: lon_lat(f64::from(x) + px / extent, f64::from(y) + py / extent),
            name: text("施設名"),
            length_m: property(length_key).and_then(value_number),
            route: text("路線名"),
        });
    }
    found
}

/// 点のジオメトリの最初の点（タイルの中の位置）。MoveTo とジグザグの x・y だけを読む。
fn first_point(geometry: &[u32]) -> Option<(f64, f64)> {
    let command = *geometry.first()?;
    if command & 0x7 != 1 || command >> 3 == 0 {
        return None;
    }
    let unzigzag = |v: u32| f64::from(((v >> 1) as i32) ^ -((v & 1) as i32));
    Some((unzigzag(*geometry.get(1)?), unzigzag(*geometry.get(2)?)))
}

fn value_text(value: &tile::Value) -> Option<String> {
    if let Some(text) = &value.string_value {
        return Some(text.clone());
    }
    value_number(value).map(|n| n.to_string())
}

fn value_number(value: &tile::Value) -> Option<f64> {
    value
        .double_value
        .or(value.float_value.map(f64::from))
        .or(value.int_value.map(|v| v as f64))
        .or(value.uint_value.map(|v| v as f64))
        .or(value.sint_value.map(|v| v as f64))
        .or_else(|| value.string_value.as_deref()?.trim().parse().ok())
}

/// ズーム `ZOOM` のタイル座標（小数を含む）を、経度・緯度にする。
fn lon_lat(x: f64, y: f64) -> LonLat {
    let n = f64::from(1u32 << ZOOM);
    let lon = x / n * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * y / n))
        .sinh()
        .atan()
        .to_degrees();
    [lon, lat]
}

/// 経度・緯度を含む、ズーム `ZOOM` のタイルの番号。
fn tile_of(at: LonLat) -> (u32, u32) {
    let n = f64::from(1u32 << ZOOM);
    let x = (at[0] + 180.0) / 360.0 * n;
    let lat = at[1].to_radians();
    let y = (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * n;
    (x.floor() as u32, y.floor() as u32)
}

/// 経路から `margin_m` 以内に掛かる、ズーム `ZOOM` のタイルの番号。
///
/// 経路を `margin_m` ごとの点に区切り、それぞれの点のまわり `margin_m` の四隅と点そのものの
/// タイルを集める。タイルは数 km 四方なので、取りこぼさない。
pub fn tiles_along(line: &[LonLat], margin_m: f64) -> BTreeSet<(u32, u32)> {
    let mut tiles = BTreeSet::new();
    let mut add = |at: LonLat| {
        let dlat = margin_m / M_PER_DEGREE;
        let dlon = dlat / at[1].to_radians().cos();
        for (dx, dy) in [
            (0.0, 0.0),
            (-1.0, -1.0),
            (-1.0, 1.0),
            (1.0, -1.0),
            (1.0, 1.0),
        ] {
            tiles.insert(tile_of([at[0] + dx * dlon, at[1] + dy * dlat]));
        }
    };
    for pair in line.windows(2) {
        let length_m = geo::distance_m(pair[0], pair[1]);
        let steps = (length_m / margin_m).ceil().max(1.0) as usize;
        for i in 0..steps {
            let t = i as f64 / steps as f64;
            add([
                pair[0][0] + (pair[1][0] - pair[0][0]) * t,
                pair[0][1] + (pair[1][1] - pair[0][1]) * t,
            ]);
        }
    }
    if let Some(last) = line.last() {
        add(*last);
    }
    tiles
}

/// 経路の橋・トンネルの範囲から、この距離（m）までの施設を候補にする。施設の点は、施設の
/// どこを指すか決まっておらず、試しでは 90 m ほど離れたものもあった。
const MATCH_M: f64 = 100.0;
/// 名前で決められないときに、長さの比（短い方÷長い方）がこれ以上なら同じ施設とみなす。
const LENGTH_RATIO: f64 = 0.7;
/// OSM で名前の無い橋・トンネルを、路線を見ずに長さと近さだけで決めるときの距離（m）。
const UNNAMED_M: f64 = 50.0;
/// 公称の長さ（無ければ OSM の長さ）がこれ未満の橋は、字幕にしない。
const BRIDGE_MIN_M: f64 = 100.0;

/// 経路の上の位置（始点からの距離）を、経度・緯度にする。
struct Along<'a> {
    line: &'a [LonLat],
    /// `line` の各点までの距離。
    at_m: Vec<f64>,
}

impl<'a> Along<'a> {
    fn new(line: &'a [LonLat]) -> Self {
        let mut at_m = Vec::with_capacity(line.len());
        let mut total = 0.0;
        for (i, point) in line.iter().enumerate() {
            if i > 0 {
                total += geo::distance_m(line[i - 1], *point);
            }
            at_m.push(total);
        }
        Self { line, at_m }
    }

    fn point(&self, m: f64) -> Option<LonLat> {
        let last = self.line.len().checked_sub(1)?;
        let i = self.at_m.partition_point(|a| *a <= m).clamp(1, last.max(1));
        if last == 0 {
            return Some(self.line[0]);
        }
        let (a, b) = (self.at_m[i - 1], self.at_m[i]);
        let t = if b > a {
            ((m - a) / (b - a)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (p, q) = (self.line[i - 1], self.line[i]);
        Some([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t])
    }

    /// `from_m` から `to_m` までの経路から、`at` までの近似の最短距離（10 m おきの点で測る）。
    fn distance_m(&self, from_m: f64, to_m: f64, at: LonLat) -> f64 {
        let steps = ((to_m - from_m) / 10.0).ceil().max(1.0) as usize;
        (0..=steps)
            .filter_map(|i| self.point(from_m + (to_m - from_m) * i as f64 / steps as f64))
            .map(|p| geo::distance_m(p, at))
            .fold(f64::INFINITY, f64::min)
    }
}

/// 対応づけで比べるための名前。字幕で区別しない揺れのほか、隧道とトンネル、第と号、括弧の
/// 中、空白を揃える。
fn match_key(name: &str) -> String {
    let mut key = String::new();
    let mut depth = 0;
    for c in name.chars() {
        let c = match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            'ヶ' | 'ヵ' => 'ケ',
            _ => c,
        };
        match c {
            '(' | '（' => depth += 1,
            ')' | '）' => depth -= 1,
            _ if depth > 0 => {}
            '第' | '号' | ' ' | '\u{3000}' => {}
            _ => key.push(c),
        }
    }
    key.replace("隧道", "トンネル")
}

/// 施設の路線名にある国道・都道府県道の番号。「国道19号」「国道１５８号」「県道26号線」。
fn route_number(route: &str) -> Option<(RouteClass, u32)> {
    let route = match_key(route);
    let (class, rest) = if let Some(rest) = route.strip_prefix("国道") {
        (RouteClass::National, rest)
    } else {
        // 都道・道道・府道・県道。市道・町道・村道は番号を持たない。
        let at = route.find("道")?;
        let head = &route[..at];
        if !["都", "道", "府", "県"].iter().any(|t| head.ends_with(t)) {
            return None;
        }
        (RouteClass::Prefectural, &route[at + "道".len()..])
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((class, digits.parse().ok()?))
}

/// 候補の施設 1 件と、その点数の材料。
struct Candidate<'a> {
    facility: &'a Facility,
    distance_m: f64,
    name_match: bool,
    route_match: bool,
}

/// 長さの比（短い方÷長い方）。どちらかが分からなければ 0。
fn length_ratio(a: f64, b: Option<f64>) -> f64 {
    match b {
        Some(b) if a > 0.0 && b > 0.0 => a.min(b) / a.max(b),
        _ => 0.0,
    }
}

/// 抽出した橋・トンネルに、データベースの施設を対応づける。
///
/// 候補は、同じ種類で、橋・トンネルの範囲の経路から `MATCH_M` 以内の施設である。いちばん近い
/// 点が別の施設のことがあるので、名前、路線、長さで選ぶ。OSM で 1 本のものがデータベースで
/// 何件かに分かれていれば、まとめる。字幕なしのトンネル（覆道）は相手にしない。橋は、公称の
/// 長さで字幕にするかどうかを決め直す。
pub fn attach(events: &mut [Event], line: &[LonLat], facilities: &[Facility]) {
    let along = Along::new(line);
    let mut routes: Vec<RouteRef> = Vec::new();
    for event in events.iter_mut() {
        let kind = match event.kind {
            EventKind::Road => {
                routes = event.detail.routes.clone();
                continue;
            }
            EventKind::Bridge => FacilityKind::Bridge,
            EventKind::Tunnel if !event.hidden => FacilityKind::Tunnel,
            _ => continue,
        };
        let (from_m, to_m) = (event.route_m, event.detail.end_m.unwrap_or(event.route_m));
        let osm_m = (to_m - from_m).max(10.0);
        let name = event
            .detail
            .extracted_label
            .clone()
            .unwrap_or(event.label.clone());
        let key = match_key(&name);
        let on_route = |facility: &Facility| {
            route_number(&facility.route).is_some_and(|(class, number)| {
                routes
                    .iter()
                    .any(|r| r.class == class && r.number == number)
            })
        };
        let mut candidates: Vec<Candidate> = facilities
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| Candidate {
                facility: f,
                distance_m: along.distance_m(from_m, to_m, f.at),
                name_match: !key.is_empty() && match_key(&f.name) == key,
                route_match: on_route(f),
            })
            .filter(|c| c.distance_m <= MATCH_M)
            .collect();
        // 同じ名前と長さの施設が並んでいることがある。1 件と数える。
        candidates.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
        let mut seen = Vec::new();
        candidates.retain(|c| {
            let id = (
                c.facility.name.clone(),
                c.facility.length_m.map(f64::to_bits),
            );
            !seen.contains(&id) && {
                seen.push(id);
                true
            }
        });

        let acceptable = |c: &Candidate| {
            let ratio = length_ratio(osm_m, c.facility.length_m);
            c.name_match
                || (ratio >= LENGTH_RATIO && c.route_match)
                || (key.is_empty() && ratio >= LENGTH_RATIO && c.distance_m <= UNNAMED_M)
        };
        let score = |c: &Candidate| {
            f64::from(u8::from(c.name_match)) * 3.0
                + f64::from(u8::from(c.route_match)) * 2.0
                + length_ratio(osm_m, c.facility.length_m) * 2.0
                - c.distance_m / MATCH_M
        };
        let single = candidates
            .iter()
            .filter(|c| acceptable(c))
            .max_by(|a, b| score(a).total_cmp(&score(b)));
        // 路線の合う、名前の違う施設を合わせると長さが合うなら、まとめる。
        let group: Vec<&Candidate> = candidates
            .iter()
            .filter(|c| c.route_match && c.facility.length_m.is_some())
            .collect();
        let group_m: f64 = group.iter().filter_map(|c| c.facility.length_m).sum();
        let group_ratio = length_ratio(osm_m, Some(group_m));
        let single_ratio = single.map_or(0.0, |c| length_ratio(osm_m, c.facility.length_m));
        let chosen: Vec<&Facility> = if group.len() >= 2
            && group_ratio >= LENGTH_RATIO
            && group_ratio > single_ratio
            && !single.is_some_and(|c| c.name_match)
        {
            let mut joined: Vec<(f64, &Facility)> = group
                .iter()
                .map(|c| (along_m(&along, from_m, to_m, c.facility.at), c.facility))
                .collect();
            joined.sort_by(|a, b| a.0.total_cmp(&b.0));
            joined.into_iter().map(|(_, f)| f).collect()
        } else {
            single.map(|c| c.facility).into_iter().collect()
        };
        if chosen.is_empty() {
            continue;
        }
        let names: Vec<&str> = chosen.iter().map(|f| f.name.as_str()).collect();
        let length_m: Option<f64> = chosen.iter().map(|f| f.length_m).sum();
        event.detail.official_name = Some(names.join("・"));
        event.detail.official_length_m = length_m.map(|m| (m * 10.0).round() / 10.0);
        if kind == FacilityKind::Bridge {
            event.hidden = length_m.unwrap_or(osm_m) < BRIDGE_MIN_M;
        }
    }
}

/// 施設の点が、経路の範囲のどこに近いか（始点からの距離）。まとめた名前を経路の順に並べる。
fn along_m(along: &Along, from_m: f64, to_m: f64, at: LonLat) -> f64 {
    let steps = ((to_m - from_m) / 10.0).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|i| from_m + (to_m - from_m) * i as f64 / steps as f64)
        .filter_map(|m| along.point(m).map(|p| (m, geo::distance_m(p, at))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(from_m, |(m, _)| m)
}

#[cfg(test)]
mod tests {
    use routelog_core::{EventDetail, EventStatus};

    use super::*;

    const ORIGIN: LonLat = [138.0, 36.0];

    /// 原点から東へ `x`、北へ `y`（m）の点。
    fn at(x: f64, y: f64) -> LonLat {
        [
            ORIGIN[0] + x / (M_PER_DEGREE * ORIGIN[1].to_radians().cos()),
            ORIGIN[1] + y / M_PER_DEGREE,
        ]
    }

    /// 原点から東へ 5 km のまっすぐな経路。
    fn line() -> Vec<LonLat> {
        vec![at(0.0, 0.0), at(5000.0, 0.0)]
    }

    fn structure(kind: EventKind, label: &str, from_m: f64, to_m: f64) -> Event {
        Event {
            id: String::new(),
            kind,
            label: label.into(),
            route_m: from_m,
            time_ms: None,
            status: EventStatus::Estimated,
            hidden: false,
            detail: EventDetail {
                end_m: Some(to_m),
                extracted_label: Some(label.into()),
                ..EventDetail::default()
            },
        }
    }

    /// その位置から先を走る路線の `road`。
    fn road(route_m: f64, routes: Vec<RouteRef>) -> Event {
        let mut event = structure(EventKind::Road, "", route_m, route_m);
        event.detail.end_m = None;
        event.detail.routes = routes;
        event
    }

    fn national(number: u32) -> RouteRef {
        RouteRef {
            class: RouteClass::National,
            pref: String::new(),
            number,
        }
    }

    fn facility(
        kind: FacilityKind,
        x: f64,
        y: f64,
        name: &str,
        length_m: f64,
        route: &str,
    ) -> Facility {
        Facility {
            kind,
            at: at(x, y),
            name: name.into(),
            length_m: Some(length_m),
            route: route.into(),
        }
    }

    fn tunnel(x: f64, y: f64, name: &str, length_m: f64, route: &str) -> Facility {
        facility(FacilityKind::Tunnel, x, y, name, length_m, route)
    }

    fn bridge(x: f64, y: f64, name: &str, length_m: f64, route: &str) -> Facility {
        facility(FacilityKind::Bridge, x, y, name, length_m, route)
    }

    fn official(event: &Event) -> (Option<&str>, Option<f64>) {
        (
            event.detail.official_name.as_deref(),
            event.detail.official_length_m,
        )
    }

    #[test]
    fn the_nearest_facility_is_not_always_the_one() {
        let mut events = vec![
            road(0.0, vec![national(158)]),
            structure(EventKind::Tunnel, "大白川2号トンネル", 1000.0, 1190.0),
        ];
        let facilities = [
            tunnel(1095.0, 5.0, "大白川第１トンネル", 50.0, "国道158号"),
            tunnel(1095.0, 28.0, "大白川第２トンネル", 187.0, "国道158号"),
        ];
        attach(&mut events, &line(), &facilities);
        assert_eq!(
            official(&events[1]),
            (Some("大白川第２トンネル"), Some(187.0))
        );
    }

    #[test]
    fn the_route_decides_between_bridges_of_the_same_name() {
        let mut events = vec![
            road(0.0, vec![national(158)]),
            structure(EventKind::Bridge, "稲核橋", 1000.0, 1090.0),
        ];
        let facilities = [
            bridge(1045.0, 30.0, "稲核橋（３０００９）", 48.5, "市道稲核１号線"),
            bridge(1045.0, 88.0, "稲核橋", 92.0, "国道１５８号"),
        ];
        attach(&mut events, &line(), &facilities);
        assert_eq!(official(&events[1]), (Some("稲核橋"), Some(92.0)));
    }

    #[test]
    fn an_unnamed_tunnel_takes_the_official_name() {
        let mut events = vec![
            road(0.0, vec![national(19)]),
            structure(EventKind::Tunnel, "", 1000.0, 1360.0),
        ];
        let facilities = [tunnel(1180.0, 16.0, "池沢隧道", 379.0, "国道19号")];
        attach(&mut events, &line(), &facilities);
        assert_eq!(official(&events[1]), (Some("池沢隧道"), Some(379.0)));
        assert_eq!(events[1].label, "");
        assert_eq!(events[1].shown_name(), "池沢隧道 379m");
    }

    #[test]
    fn two_facilities_in_one_tunnel_are_joined() {
        let mut events = vec![
            road(0.0, vec![national(19)]),
            structure(EventKind::Tunnel, "笹平トンネル", 1000.0, 2140.0),
        ];
        let facilities = [
            tunnel(1300.0, 25.0, "笹平第１トンネル", 586.0, "国道19号"),
            tunnel(1880.0, 4.0, "笹平第２トンネル", 521.0, "国道19号"),
        ];
        attach(&mut events, &line(), &facilities);
        assert_eq!(
            official(&events[1]),
            (Some("笹平第１トンネル・笹平第２トンネル"), Some(1107.0))
        );
    }

    #[test]
    fn a_differently_named_tunnel_gets_the_official_name_alongside() {
        let mut events = vec![
            road(0.0, vec![national(19)]),
            structure(EventKind::Tunnel, "新鳥居トンネル", 1000.0, 2760.0),
        ];
        let facilities = [tunnel(1880.0, 14.0, "鳥居トンネル", 1738.0, "国道19号")];
        attach(&mut events, &line(), &facilities);
        assert_eq!(official(&events[1]), (Some("鳥居トンネル"), Some(1738.0)));
        assert_eq!(events[1].label, "新鳥居トンネル");
    }

    #[test]
    fn a_duplicated_facility_is_not_added_twice() {
        let mut events = vec![
            road(0.0, vec![national(19)]),
            structure(EventKind::Tunnel, "新久米路トンネル", 1000.0, 1250.0),
        ];
        let twin = tunnel(1125.0, 5.0, "新久米路トンネル", 257.0, "国道19号");
        attach(&mut events, &line(), &[twin.clone(), twin]);
        assert_eq!(
            official(&events[1]),
            (Some("新久米路トンネル"), Some(257.0))
        );
    }

    #[test]
    fn both_passes_get_the_same_facility() {
        // 東へ 5 km 行って戻る。
        let there_and_back = vec![at(0.0, 0.0), at(5000.0, 0.0), at(0.0, 0.0)];
        let mut events = vec![
            road(0.0, vec![national(19)]),
            structure(EventKind::Tunnel, "犬戻トンネル", 1000.0, 1740.0),
            structure(EventKind::Tunnel, "犬戻トンネル", 8260.0, 9000.0),
        ];
        let facilities = [tunnel(1370.0, 13.0, "犬戻トンネル", 737.0, "国道19号")];
        attach(&mut events, &there_and_back, &facilities);
        assert_eq!(official(&events[1]), (Some("犬戻トンネル"), Some(737.0)));
        assert_eq!(official(&events[2]), (Some("犬戻トンネル"), Some(737.0)));
    }

    #[test]
    fn a_bridge_is_shown_by_its_official_length() {
        let mut short_osm = structure(EventKind::Bridge, "長い橋", 1000.0, 1095.0);
        short_osm.hidden = true;
        let long_osm = structure(EventKind::Bridge, "短い橋", 3000.0, 3105.0);
        let mut events = vec![road(0.0, vec![national(19)]), short_osm, long_osm];
        let facilities = [
            bridge(1050.0, 5.0, "長い橋", 105.0, "国道19号"),
            bridge(3050.0, 5.0, "短い橋", 92.0, "国道19号"),
        ];
        attach(&mut events, &line(), &facilities);
        assert!(!events[1].hidden);
        assert!(events[2].hidden);
    }

    #[test]
    fn a_shed_gets_nothing() {
        let mut shed = structure(EventKind::Tunnel, "小松原洞門", 1000.0, 1150.0);
        shed.hidden = true;
        let mut events = vec![road(0.0, vec![national(19)]), shed];
        let facilities = [tunnel(1075.0, 50.0, "小松原トンネル", 160.0, "国道19号")];
        attach(&mut events, &line(), &facilities);
        assert_eq!(official(&events[1]), (None, None));
    }

    #[test]
    fn a_far_or_unrelated_facility_is_not_taken() {
        let mut events = vec![
            road(0.0, vec![national(19)]),
            structure(EventKind::Bridge, "裾花橋", 1000.0, 1090.0),
        ];
        let facilities = [
            bridge(1045.0, 17.0, "266.397（溝橋）", 4.0, "国道19号"),
            bridge(1045.0, 300.0, "裾花橋", 90.0, "国道19号"),
        ];
        attach(&mut events, &line(), &facilities);
        assert_eq!(official(&events[1]), (None, None));
    }
    /// 1 点だけのタイル。`at` はタイルの中の位置（0〜4096）。
    fn tile_with(layer: &str, at: (u32, u32), props: &[(&str, tile::Value)]) -> Vec<u8> {
        let zigzag = |v: i32| ((v << 1) ^ (v >> 31)) as u32;
        let feature = tile::Feature {
            id: Some(1),
            tags: (0..props.len() as u32).flat_map(|i| [i, i]).collect(),
            r#type: Some(tile::GeomType::Point as i32),
            geometry: vec![
                (1 & 0x7) | (1 << 3),
                zigzag(at.0 as i32),
                zigzag(at.1 as i32),
            ],
        };
        let layer = tile::Layer {
            version: 2,
            name: layer.into(),
            features: vec![feature],
            keys: props.iter().map(|(k, _)| (*k).to_owned()).collect(),
            values: props.iter().map(|(_, v)| v.clone()).collect(),
            extent: Some(4096),
        };
        Tile {
            layers: vec![layer],
        }
        .encode_to_vec()
    }

    fn text(value: &str) -> tile::Value {
        tile::Value {
            string_value: Some(value.into()),
            ..Default::default()
        }
    }

    fn number(value: f64) -> tile::Value {
        tile::Value {
            double_value: Some(value),
            ..Default::default()
        }
    }

    #[test]
    fn a_point_becomes_a_facility_at_its_place() {
        let bytes = tile_with(
            "tunnel",
            (2048, 2048),
            &[
                ("施設名", text("犬戻トンネル")),
                ("路線名", text("国道19号")),
                ("延長（ｍ）", number(737.0)),
            ],
        );
        let found = decode(&bytes, FacilityKind::Tunnel, 7247, 3231);
        assert_eq!(found.len(), 1);
        let facility = &found[0];
        assert_eq!(facility.name, "犬戻トンネル");
        assert_eq!(facility.route, "国道19号");
        assert_eq!(facility.length_m, Some(737.0));
        // タイル 7247/3231 のまん中。タイルは経度 138.4717〜138.5156、緯度 35.4607〜35.4965。
        assert!(
            (facility.at[0] - 138.4936523).abs() < 1e-4,
            "{:?}",
            facility.at
        );
        assert!((facility.at[1] - 35.4786).abs() < 1e-3, "{:?}", facility.at);
    }

    #[test]
    fn a_length_written_as_text_is_read() {
        let bytes = tile_with(
            "bridge",
            (0, 0),
            &[("施設名", text("大安寺橋")), ("橋長（ｍ）", text("291.3"))],
        );
        let found = decode(&bytes, FacilityKind::Bridge, 7247, 3231);
        assert_eq!(found[0].length_m, Some(291.3));
    }

    #[test]
    fn a_layer_of_the_other_kind_is_ignored() {
        let bytes = tile_with("bridge", (0, 0), &[("施設名", text("大安寺橋"))]);
        assert!(decode(&bytes, FacilityKind::Tunnel, 7247, 3231).is_empty());
    }

    #[test]
    fn a_broken_tile_is_skipped() {
        assert!(decode(&[0xff, 0x00, 0x13, 0x37], FacilityKind::Tunnel, 0, 0).is_empty());
    }

    #[test]
    fn tiles_near_the_route_are_listed() {
        // タイル 7247/3231 の西の境（経度 138.4716797）から東へ 50 m の所を、南北に 1 km 走る。
        let lon = 138.4716797 + 50.0 / (111_195.0 * 35.48_f64.to_radians().cos());
        let line = [[lon, 35.47], [lon, 35.479]];
        let tiles = tiles_along(&line, 150.0);
        assert!(tiles.contains(&(7247, 3231)), "{tiles:?}");
        assert!(tiles.contains(&(7246, 3231)), "{tiles:?}");
        assert!(!tiles.contains(&(7248, 3231)), "{tiles:?}");
    }
}
