//! 走行 1 つぶんの記録。ローカルの保存とクラウドの API が同じ形を使う。

use serde::{Deserialize, Serialize};

use crate::geo::{self, LonLat};

/// 走行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ride {
    /// UUIDv7。
    pub id: String,
    pub title: String,
    /// `YYYY-MM-DD`（JST）。
    pub ride_date: String,
    /// カメラの時計の補正値。実時刻 = ファイル名の時刻 + 補正値。
    pub clock_offset_ms: i64,
    pub is_public: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 経由点から次の経由点までの結び方。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentMode {
    /// 直線。
    Straight,
    /// 国道・都道府県道への吸着。
    Snap,
    /// 手で置いた頂点を通る。
    Manual,
}

/// 経由点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waypoint {
    pub lon: f64,
    pub lat: f64,
    /// 次の経由点までの結び方。最後の経由点では使わない。
    pub segment_mode: SegmentMode,
}

/// 経路の線。経由点そのものは含まず、経由点の間の頂点だけを持つ。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Route {
    /// `segments[i]` は、経由点 `i` と `i + 1` の間の頂点。直線なら空。
    pub segments: Vec<Vec<LonLat>>,
}

/// 走行 1 つぶんの全データ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RideDocument {
    pub ride: Ride,
    #[serde(default)]
    pub waypoints: Vec<Waypoint>,
    #[serde(default)]
    pub route: Route,
}

/// 走行のデータが形として正しくない理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// 区間の数が、経由点の数から 1 を引いた数と合わない。
    SegmentCount { waypoints: usize, segments: usize },
    /// 経度・緯度が範囲の外か、数でない。
    Coordinate,
    /// 直線の区間が途中の頂点を持つ。
    StraightWithVertices { segment: usize },
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::SegmentCount {
                waypoints,
                segments,
            } => {
                write!(
                    f,
                    "経由点 {waypoints} 個に対して区間が {segments} 個あります"
                )
            }
            Self::Coordinate => write!(f, "経度・緯度が範囲の外です"),
            Self::StraightWithVertices { segment } => {
                write!(f, "直線の区間 {segment} が途中の頂点を持っています")
            }
        }
    }
}

impl std::error::Error for Invalid {}

fn on_earth(point: LonLat) -> bool {
    (-180.0..=180.0).contains(&point[0]) && (-90.0..=90.0).contains(&point[1])
}

impl RideDocument {
    pub fn validate(&self) -> Result<(), Invalid> {
        let (waypoints, segments) = (self.waypoints.len(), self.route.segments.len());
        if segments != waypoints.saturating_sub(1) {
            return Err(Invalid::SegmentCount {
                waypoints,
                segments,
            });
        }
        let points = self.waypoints.iter().map(|w| [w.lon, w.lat]);
        let vertices = self.route.segments.iter().flatten().copied();
        if !points.chain(vertices).all(on_earth) {
            return Err(Invalid::Coordinate);
        }
        for (segment, vertices) in self.route.segments.iter().enumerate() {
            let straight = self.waypoints[segment].segment_mode == SegmentMode::Straight;
            if straight && !vertices.is_empty() {
                return Err(Invalid::StraightWithVertices { segment });
            }
        }
        Ok(())
    }

    /// 経路の折れ線。経由点と、その間の頂点を順に並べる。
    pub fn line(&self) -> Vec<LonLat> {
        let mut line = Vec::new();
        for (i, waypoint) in self.waypoints.iter().enumerate() {
            line.push([waypoint.lon, waypoint.lat]);
            if let Some(vertices) = self.route.segments.get(i) {
                line.extend_from_slice(vertices);
            }
        }
        line
    }

    /// 経路の長さ（m）。
    pub fn length_m(&self) -> f64 {
        geo::length_m(&self.line())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waypoint(lon: f64, lat: f64, segment_mode: SegmentMode) -> Waypoint {
        Waypoint {
            lon,
            lat,
            segment_mode,
        }
    }

    fn document(waypoints: Vec<Waypoint>, segments: Vec<Vec<LonLat>>) -> RideDocument {
        RideDocument {
            ride: Ride {
                id: "0199c0de-0000-7000-8000-000000000000".into(),
                title: "テスト".into(),
                ride_date: "2026-10-03".into(),
                clock_offset_ms: 0,
                is_public: false,
                created_at: "2026-10-03T05:00:00+09:00".into(),
                updated_at: "2026-10-03T05:00:00+09:00".into(),
            },
            waypoints,
            route: Route { segments },
        }
    }

    #[test]
    fn the_line_goes_through_waypoints_and_vertices_in_order() {
        let doc = document(
            vec![
                waypoint(138.0, 36.0, SegmentMode::Manual),
                waypoint(138.2, 36.0, SegmentMode::Straight),
                waypoint(138.3, 36.0, SegmentMode::Straight),
            ],
            vec![vec![[138.1, 36.1]], vec![]],
        );
        assert_eq!(doc.validate(), Ok(()));
        assert_eq!(
            doc.line(),
            [[138.0, 36.0], [138.1, 36.1], [138.2, 36.0], [138.3, 36.0]]
        );
        assert!(doc.length_m() > geo::distance_m([138.0, 36.0], [138.3, 36.0]));
    }

    #[test]
    fn an_empty_route_is_valid() {
        let doc = document(vec![], vec![]);
        assert_eq!(doc.validate(), Ok(()));
        assert_eq!(doc.length_m(), 0.0);
        let single = document(vec![waypoint(138.0, 36.0, SegmentMode::Straight)], vec![]);
        assert_eq!(single.validate(), Ok(()));
    }

    #[test]
    fn the_number_of_segments_must_match() {
        let doc = document(
            vec![waypoint(138.0, 36.0, SegmentMode::Straight)],
            vec![vec![]],
        );
        assert_eq!(
            doc.validate(),
            Err(Invalid::SegmentCount {
                waypoints: 1,
                segments: 1
            })
        );
    }

    #[test]
    fn coordinates_must_be_on_earth() {
        let doc = document(vec![waypoint(138.0, 96.0, SegmentMode::Straight)], vec![]);
        assert_eq!(doc.validate(), Err(Invalid::Coordinate));
        let doc = document(
            vec![waypoint(f64::NAN, 36.0, SegmentMode::Straight)],
            vec![],
        );
        assert_eq!(doc.validate(), Err(Invalid::Coordinate));
    }

    #[test]
    fn a_straight_segment_has_no_vertices() {
        let doc = document(
            vec![
                waypoint(138.0, 36.0, SegmentMode::Straight),
                waypoint(138.2, 36.0, SegmentMode::Straight),
            ],
            vec![vec![[138.1, 36.1]]],
        );
        assert_eq!(
            doc.validate(),
            Err(Invalid::StraightWithVertices { segment: 0 })
        );
    }

    #[test]
    fn a_file_written_before_the_route_existed_still_loads() {
        let json = r#"{ "ride": { "id": "x", "title": "t", "ride_date": "2026-10-03",
            "clock_offset_ms": 0, "is_public": false, "created_at": "a", "updated_at": "b" } }"#;
        let doc: RideDocument = serde_json::from_str(json).unwrap();
        assert!(doc.waypoints.is_empty() && doc.route.segments.is_empty());
    }

    #[test]
    fn the_mode_is_written_in_lower_case() {
        assert_eq!(
            serde_json::to_string(&SegmentMode::Manual).unwrap(),
            r#""manual""#
        );
    }
}
