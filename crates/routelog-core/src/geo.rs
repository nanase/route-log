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

#[cfg(test)]
mod tests {
    use super::*;

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
