//! 実データで抽出を確かめる。国道を 1 本たどった線を経路とみなし、イベントを表示する。
//!
//! 使い方: cargo run -p routelog-geo --example along -- <データのフォルダ> <国道の番号>
//!             <始点の経度> <始点の緯度> <行き先の経度> <行き先の緯度>

use std::path::PathBuf;
use std::time::Instant;

use routelog_core::RouteClass;
use routelog_core::geo::{self, LonLat};
use routelog_geo::{BBox, GeoData, Options, Prepared, Road, SnapOptions};

/// 始点にいちばん近い way から、端が一致する way を行き先へ向けてつないでいく。
fn chain(roads: &[Road], number: u32, start: LonLat, toward: LonLat) -> Vec<LonLat> {
    let on_route = |road: &&Road| {
        road.kind == "road"
            && road
                .routes
                .iter()
                .any(|r| r.class == RouteClass::National && r.number == number)
    };
    let mut rest: Vec<&Road> = roads.iter().filter(on_route).collect();
    let nearest = |road: &&Road| {
        road.line
            .iter()
            .map(|p| geo::distance_m(*p, start))
            .fold(f64::MAX, f64::min)
    };
    let Some(first) =
        (0..rest.len()).min_by(|a, b| nearest(&rest[*a]).total_cmp(&nearest(&rest[*b])))
    else {
        return Vec::new();
    };
    let mut line = rest.swap_remove(first).line.clone();
    if geo::distance_m(line[0], toward) < geo::distance_m(*line.last().unwrap(), toward) {
        line.reverse();
    }
    loop {
        let end = *line.last().unwrap();
        // 端が一致する way のうち、反対の端が行き先にいちばん近いもの。
        let next = rest
            .iter()
            .enumerate()
            .filter_map(|(i, road)| {
                let (head, tail) = (road.line[0], *road.line.last().unwrap());
                if head == end {
                    Some((i, false, geo::distance_m(tail, toward)))
                } else if tail == end {
                    Some((i, true, geo::distance_m(head, toward)))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        let Some((i, reversed, distance)) = next else {
            return line;
        };
        if distance > geo::distance_m(end, toward) + 2000.0 {
            return line;
        }
        let mut piece = rest.swap_remove(i).line.clone();
        if reversed {
            piece.reverse();
        }
        line.extend_from_slice(&piece[1..]);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, number, lon0, lat0, lon1, lat1] = args.as_slice() else {
        return Err(
            "引数: <データのフォルダ> <国道の番号> <始点の経度> <緯度> <行き先の経度> <緯度>"
                .into(),
        );
    };
    let (start, toward): (LonLat, LonLat) = (
        [lon0.parse()?, lat0.parse()?],
        [lon1.parse()?, lat1.parse()?],
    );
    let bbox = BBox::around(&[start, toward]).unwrap().expand(0.1);

    let started = Instant::now();
    let data = GeoData::load(&PathBuf::from(dir), bbox)?;
    println!(
        "読み込み {:?}: 道 {} 本、面 {} 個、道の駅 {} 個",
        started.elapsed(),
        data.roads.len(),
        data.areas.len(),
        data.stations.len()
    );

    let line = chain(&data.roads, number.parse()?, start, toward);
    println!(
        "経路 {:.1} km、{} 点",
        geo::length_m(&line) / 1000.0,
        line.len()
    );

    let started = Instant::now();
    let prepared = Prepared::new(data, bbox);
    let built = started.elapsed();
    let (edges, largest) = prepared.road_connectivity();
    println!(
        "道の繋がり: 辺 {edges} 本のうち {largest} 本（{:.1}%）が、いちばん大きい繋がりに入る",
        largest as f64 / edges.max(1) as f64 * 100.0
    );

    // たどった線の上の 2 点を吸着で結び、たどった道のりと比べる。
    let pick = |m: f64| {
        let mut rest = m;
        for pair in line.windows(2) {
            rest -= geo::distance_m(pair[0], pair[1]);
            if rest <= 0.0 {
                return pair[1];
            }
        }
        *line.last().unwrap()
    };
    let total_m = geo::length_m(&line);
    let (from, to) = (pick(total_m * 0.1), pick(total_m * 0.3));
    let started = Instant::now();
    let first = prepared.snap(from, to, &SnapOptions::default());
    let first_took = started.elapsed();
    let started = Instant::now();
    let second = prepared.snap(to, from, &SnapOptions::default());
    println!(
        "吸着 {:.5},{:.5} → {:.5},{:.5}: 直線 {:.0} m、たどった道のり {:.0} m、吸着 {} m（{} 点）、逆向き {} m。グラフ込み {first_took:?}、2 回目 {:?}",
        from[0],
        from[1],
        to[0],
        to[1],
        geo::distance_m(from, to),
        total_m * 0.2,
        first
            .as_ref()
            .map_or("なし".into(), |l| format!("{:.0}", geo::length_m(l))),
        first.as_ref().map_or(0, Vec::len),
        second
            .as_ref()
            .map_or("なし".into(), |l| format!("{:.0}", geo::length_m(l))),
        started.elapsed(),
    );

    let started = Instant::now();
    let events = prepared.extract(&line, &Options::default());
    println!(
        "索引 {built:?}、抽出 {:?}: イベント {} 件",
        started.elapsed(),
        events.len()
    );
    for event in &events {
        let hidden = if event.hidden { "（非表示）" } else { "" };
        let end = event
            .detail
            .end_m
            .map(|m| format!(" 長さ {:.0} m", m - event.route_m))
            .unwrap_or_default();
        println!(
            "{:7.2} km  {:13}  {}{end}{hidden}",
            event.route_m / 1000.0,
            format!("{:?}", event.kind),
            if event.label.is_empty() {
                "（名前なし）"
            } else {
                &event.label
            },
        );
    }
    Ok(())
}
