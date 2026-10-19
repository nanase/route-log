//! 道への吸着と、経路からのイベント抽出。判定用データを読んだ結果は、次の抽出のために持っておく。

use std::path::Path;
use std::sync::{Arc, Mutex};

use routelog_core::{Event, RideDocument, SegmentMode, carry_over};
use routelog_geo::{Options, SnapOptions, Tiles};

/// 読んである判定用データ。フォルダが変われば読み直す。
#[derive(Default)]
pub struct Extractor {
    loaded: Mutex<Option<Arc<Tiles>>>,
}

impl Extractor {
    /// そのフォルダの判定用データ。読んだ升目は、フォルダが同じ間は覚えておく。
    fn tiles(&self, dir: &Path) -> Arc<Tiles> {
        let mut loaded = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        match loaded.as_ref() {
            Some(tiles) if tiles.dir() == dir => tiles.clone(),
            _ => loaded.insert(Arc::new(Tiles::new(dir))).clone(),
        }
    }

    /// その点を、画面上で近い道の上へ寄せる。近くに道が無ければ `None`。
    pub fn snap_point(
        &self,
        dir: &Path,
        point: [f64; 2],
        radius_m: f64,
    ) -> Result<Option<[f64; 2]>, routelog_geo::Error> {
        self.tiles(dir).snap_point(point, radius_m)
    }

    /// 吸着の区間を、道に沿った線に引き直す。両端の変わらない区間は、前に引いた線を使う。
    ///
    /// 近くに道が無いか、道のりが直線に比べて長すぎる区間は、頂点の無い線（直線）になる。
    /// 結び方は吸着のままなので、経由点を動かせばまた試す。
    pub fn snap(&self, dir: &Path, doc: &mut RideDocument) -> Result<(), routelog_geo::Error> {
        let tiles = self.tiles(dir);
        let points: Vec<_> = doc.waypoints.iter().map(|w| [w.lon, w.lat]).collect();
        for (i, segment) in doc.route.segments.iter_mut().enumerate() {
            if doc.waypoints[i].segment_mode == SegmentMode::Snap {
                let line = tiles.snap(points[i], points[i + 1], &SnapOptions::default())?;
                *segment = line.unwrap_or_default();
            }
        }
        Ok(())
    }

    /// 経路からイベントを抽出し直し、人が手を入れた結果を引き継ぐ。
    pub fn extract(
        &self,
        dir: &Path,
        doc: &RideDocument,
    ) -> Result<Vec<Event>, routelog_geo::Error> {
        let line = doc.line();
        let extracted = if line.len() >= 2 {
            self.tiles(dir).extract(&line, &Options::default())?
        } else {
            Vec::new()
        };
        Ok(carry_over(&doc.events, extracted))
    }
}
