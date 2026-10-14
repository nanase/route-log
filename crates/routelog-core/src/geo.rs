//! 経度・緯度の上の距離。

/// 経度・緯度（度）。GeoJSON と同じ順。
pub type LonLat = [f64; 2];

/// 地球の平均半径（m）。
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// 2 点の間の大円距離（m）。
pub fn distance_m(a: LonLat, b: LonLat) -> f64 {
    let (lat_a, lat_b) = (a[1].to_radians(), b[1].to_radians());
    let half_lat = (lat_b - lat_a) / 2.0;
    let half_lon = (b[0] - a[0]).to_radians() / 2.0;
    let h = half_lat.sin().powi(2) + lat_a.cos() * lat_b.cos() * half_lon.sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}

/// 折れ線の長さ（m）。
pub fn length_m(line: &[LonLat]) -> f64 {
    line.windows(2)
        .map(|pair| distance_m(pair[0], pair[1]))
        .sum()
}

/// 同じ近さとみなす差（m）。行って戻る道では、近さの同じ所が 2 つある。
const TIE_M: f64 = 1.0;

/// 折れ線の上で `at` にいちばん近い所の、始点からの距離（m）と、`at` からの距離（m）。
///
/// 同じ道を行って戻る経路では、同じ近さの所がいくつもある。そのときは、始点からの距離が `near_m`
/// に近いほうを選ぶ。線が無ければ `None`。
pub fn nearest_m(line: &[LonLat], at: LonLat, near_m: f64) -> Option<(f64, f64)> {
    // 近くだけを見るので、`at` を原点にした平面に直して測る。
    let scale = EARTH_RADIUS_M * std::f64::consts::PI / 180.0;
    let shrink = at[1].to_radians().cos();
    let flat = |p: LonLat| ((p[0] - at[0]) * scale * shrink, (p[1] - at[1]) * scale);
    let first = *line.first()?;
    let mut found = vec![(0.0, distance_m(first, at))];
    let mut walked = 0.0;
    for pair in line.windows(2) {
        let ((ax, ay), (bx, by)) = (flat(pair[0]), flat(pair[1]));
        let length2 = (bx - ax).powi(2) + (by - ay).powi(2);
        let t = if length2 == 0.0 {
            0.0
        } else {
            (-(ax * (bx - ax) + ay * (by - ay)) / length2).clamp(0.0, 1.0)
        };
        let off = (ax + (bx - ax) * t).hypot(ay + (by - ay) * t);
        let length = distance_m(pair[0], pair[1]);
        found.push((walked + length * t, off));
        walked += length;
    }
    let best = found.iter().map(|f| f.1).fold(f64::INFINITY, f64::min);
    found
        .into_iter()
        .filter(|f| f.1 <= best + TIE_M)
        .min_by(|a, b| (a.0 - near_m).abs().total_cmp(&(b.0 - near_m).abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_point_is_measured_along_the_line() {
        // 赤道の上を東へ 2000 m 進む線。1000 m の所の、北へ 30 m 離れた点。
        let east = |m: f64| [m / 111_195.08, 0.0];
        let line = [east(0.0), east(2000.0)];
        let (route_m, off) = nearest_m(&line, [east(1000.0)[0], 30.0 / 111_195.08], 0.0).unwrap();
        assert!((route_m - 1000.0).abs() < 0.5, "{route_m}");
        assert!((off - 30.0).abs() < 0.5, "{off}");
        // 線より先の点は、終点に寄せる。
        let (end, _) = nearest_m(&line, east(2500.0), 0.0).unwrap();
        assert!((end - 2000.0).abs() < 0.5, "{end}");
        assert_eq!(nearest_m(&[], east(0.0), 0.0), None);
    }

    #[test]
    fn on_a_road_taken_both_ways_the_pass_near_the_hint_is_chosen() {
        // 1000 m 行って、同じ道を戻る。600 m の所は、行きの 600 m と帰りの 1400 m の 2 か所にある。
        let east = |m: f64| [m / 111_195.08, 0.0];
        let line = [east(0.0), east(1000.0), east(0.0)];
        let (going, _) = nearest_m(&line, east(600.0), 300.0).unwrap();
        let (back, _) = nearest_m(&line, east(600.0), 1800.0).unwrap();
        assert!((going - 600.0).abs() < 0.5, "{going}");
        assert!((back - 1400.0).abs() < 0.5, "{back}");
    }

    #[test]
    fn one_degree_of_latitude_is_about_111_km() {
        let d = distance_m([138.0, 36.0], [138.0, 37.0]);
        assert!((d - 111_195.0).abs() < 10.0, "{d}");
    }

    #[test]
    fn a_degree_of_longitude_shrinks_with_latitude() {
        let d = distance_m([138.0, 36.0], [139.0, 36.0]);
        assert!(
            (d - 111_195.0 * 36f64.to_radians().cos()).abs() < 20.0,
            "{d}"
        );
    }

    #[test]
    fn a_line_is_the_sum_of_its_pieces() {
        let line = [[138.0, 36.0], [138.0, 36.5], [138.0, 37.0]];
        assert!((length_m(&line) - distance_m(line[0], line[2])).abs() < 1e-6);
        assert_eq!(length_m(&line[..1]), 0.0);
        assert_eq!(length_m(&[]), 0.0);
    }
}
