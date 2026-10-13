//! 判定用データ（FlatGeobuf）の読み込み。

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use flatgeobuf::{FallibleStreamingIterator, FeatureProperties, FgbReader};
use geo_types::Geometry;
use geozero::ToGeo;
use routelog_core::geo::LonLat;
use routelog_core::{RouteClass, RouteRef};

use crate::Error;

const ROADS: &str = "roads.fgb";
const AREAS: &str = "admin.fgb";
const STATIONS: &str = "michi-no-eki.fgb";

/// 経度・緯度の矩形。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub min: LonLat,
    pub max: LonLat,
}

impl BBox {
    /// 点の集まりを囲む矩形。点が無ければ `None`。
    pub fn around(points: &[LonLat]) -> Option<Self> {
        let first = *points.first()?;
        Some(points.iter().fold(
            Self {
                min: first,
                max: first,
            },
            |b, p| Self {
                min: [b.min[0].min(p[0]), b.min[1].min(p[1])],
                max: [b.max[0].max(p[0]), b.max[1].max(p[1])],
            },
        ))
    }

    /// 四方へ `degrees` だけ広げる。
    pub fn expand(self, degrees: f64) -> Self {
        Self {
            min: [self.min[0] - degrees, self.min[1] - degrees],
            max: [self.max[0] + degrees, self.max[1] + degrees],
        }
    }

    pub fn contains(&self, other: &Self) -> bool {
        self.min[0] <= other.min[0]
            && self.min[1] <= other.min[1]
            && self.max[0] >= other.max[0]
            && self.max[1] >= other.max[1]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Structure {
    None,
    Bridge,
    Tunnel,
    /// 覆道。雪や落石を避ける屋根。
    Shed,
}

/// 国道・都道府県道の way 1 本。
#[derive(Debug, Clone, PartialEq)]
pub struct Road {
    pub id: i64,
    pub line: Vec<LonLat>,
    /// 重用区間を含む全路線。国道、都道府県道の順で、番号の若い順。
    pub routes: Vec<RouteRef>,
    /// 国道マップの区分。一般の道は `road`。
    pub kind: String,
    pub motorroad: bool,
    pub structure: Structure,
    pub structure_name: String,
}

/// 市区町村（政令市は区）の面 1 つ。飛び地は別の面になる。
#[derive(Debug, Clone, PartialEq)]
pub struct Area {
    /// 全国地方公共団体コード。
    pub code: String,
    pub pref: String,
    pub muni: String,
    /// 外周と、その中の穴。
    pub rings: Vec<Vec<LonLat>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Station {
    pub name: String,
    pub at: LonLat,
}

/// ある範囲の判定用データ。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeoData {
    pub roads: Vec<Road>,
    pub areas: Vec<Area>,
    pub stations: Vec<Station>,
}

/// `"18;117"` や `"nagano-80;gunma-12"` を路線にする。読めないものは飛ばす。
fn routes(class: RouteClass, text: &str) -> impl Iterator<Item = RouteRef> + '_ {
    text.split(';').filter_map(move |token| {
        let (pref, number) = match class {
            RouteClass::National => ("", token),
            RouteClass::Prefectural => token.rsplit_once('-')?,
        };
        Some(RouteRef {
            class,
            pref: pref.to_owned(),
            number: number.parse().ok()?,
        })
    })
}

fn ring(line: &geo_types::LineString) -> Vec<LonLat> {
    line.coords().map(|c| [c.x, c.y]).collect()
}

/// ファイルから、矩形に掛かる特徴量を 1 つずつ読む。
fn read<T>(
    path: &Path,
    bbox: BBox,
    mut each: impl FnMut(
        &flatgeobuf::FgbFeature,
        Geometry,
    ) -> Result<Option<T>, geozero::error::GeozeroError>,
) -> Result<Vec<T>, Error> {
    let shown = || path.to_string_lossy().into_owned();
    let file = File::open(path).map_err(|e| Error::Open(shown(), e))?;
    let read = |e: flatgeobuf::Error| Error::Read(shown(), e.to_string());
    let mut features = FgbReader::open(BufReader::new(file))
        .map_err(read)?
        .select_bbox(bbox.min[0], bbox.min[1], bbox.max[0], bbox.max[1])
        .map_err(read)?;
    let mut items = Vec::new();
    while let Some(feature) = features.next().map_err(read)? {
        let item = feature
            .to_geo()
            .and_then(|geometry| each(feature, geometry))
            .map_err(|e| Error::Read(shown(), e.to_string()))?;
        items.extend(item);
    }
    Ok(items)
}

impl GeoData {
    /// フォルダから、矩形に掛かるものを読む。
    pub fn load(dir: &Path, bbox: BBox) -> Result<Self, Error> {
        let roads = read(&dir.join(ROADS), bbox, |feature, geometry| {
            let Geometry::LineString(line) = geometry else {
                return Ok(None);
            };
            let national: String = feature.property("national")?;
            let prefectural: String = feature.property("prefectural")?;
            let structure: String = feature.property("structure")?;
            Ok(Some(Road {
                id: feature.property("id")?,
                line: ring(&line),
                routes: routes(RouteClass::National, &national)
                    .chain(routes(RouteClass::Prefectural, &prefectural))
                    .collect(),
                kind: feature.property("kind")?,
                motorroad: feature.property::<i32>("motorroad")? != 0,
                structure: match structure.as_str() {
                    "bridge" => Structure::Bridge,
                    "tunnel" => Structure::Tunnel,
                    "shed" => Structure::Shed,
                    _ => Structure::None,
                },
                structure_name: feature.property("structure_name")?,
            }))
        })?;
        let areas = read(&dir.join(AREAS), bbox, |feature, geometry| {
            let Geometry::Polygon(polygon) = geometry else {
                return Ok(None);
            };
            let rings = std::iter::once(polygon.exterior())
                .chain(polygon.interiors())
                .map(ring)
                .collect();
            Ok(Some(Area {
                code: feature.property("code")?,
                pref: feature.property("pref")?,
                muni: feature.property("muni")?,
                rings,
            }))
        })?;
        let stations = read(&dir.join(STATIONS), bbox, |feature, geometry| {
            let Geometry::Point(point) = geometry else {
                return Ok(None);
            };
            Ok(Some(Station {
                name: feature.property("name")?,
                at: [point.x(), point.y()],
            }))
        })?;
        Ok(Self {
            roads,
            areas,
            stations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_lists_are_read_in_order_and_junk_is_skipped() {
        let national: Vec<_> = routes(RouteClass::National, "18;117;x;").collect();
        assert_eq!(
            national.iter().map(|r| r.number).collect::<Vec<_>>(),
            [18, 117]
        );
        let prefectural: Vec<_> =
            routes(RouteClass::Prefectural, "nagano-80;gunma-12;13").collect();
        let pairs: Vec<_> = prefectural
            .iter()
            .map(|r| (r.pref.as_str(), r.number))
            .collect();
        assert_eq!(pairs, [("nagano", 80), ("gunma", 12)]);
        assert_eq!(routes(RouteClass::National, "").count(), 0);
    }

    #[test]
    fn a_box_grows_and_knows_what_it_holds() {
        let bbox = BBox::around(&[[138.0, 36.0], [138.5, 36.2], [137.9, 36.1]]).unwrap();
        assert_eq!((bbox.min, bbox.max), ([137.9, 36.0], [138.5, 36.2]));
        assert!(bbox.expand(0.1).contains(&bbox));
        assert!(!bbox.contains(&bbox.expand(0.1)));
        assert_eq!(BBox::around(&[]), None);
    }

    #[test]
    fn a_missing_file_says_which_one() {
        let dir = std::env::temp_dir().join("routelog-geo-no-such-folder");
        let bbox = BBox::around(&[[138.0, 36.0]]).unwrap();
        let error = GeoData::load(&dir, bbox).unwrap_err();
        assert!(error.to_string().contains("roads.fgb"), "{error}");
    }
}
