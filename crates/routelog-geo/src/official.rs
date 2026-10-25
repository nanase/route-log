//! 国土交通省の全国道路施設点検データベースの橋・トンネルを、抽出した橋・トンネルに対応づける。
//!
//! データは、全国Q地図が配るベクトルタイル（2024 年度 全国トンネルマップ・全国橋梁マップ）から
//! 読む。施設ごとに 1 点で、施設名、路線名、公称の長さを持つ。

use std::collections::BTreeSet;

use geozero::mvt::{Message, Tile, tile};
use routelog_core::geo::{self, LonLat};

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

#[cfg(test)]
mod tests {
    use super::*;

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
