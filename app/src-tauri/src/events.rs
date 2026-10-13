//! 道への吸着と、経路からのイベント抽出。判定用データを読んだ結果は、次の抽出のために持っておく。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use routelog_core::{Event, RideDocument, SegmentMode, carry_over};
use routelog_geo::{BBox, GeoData, Options, Prepared, SnapOptions};

/// 経路を囲む矩形に足す余白（度）。経路から少し離れた道の駅や面を取りこぼさないため。
const MARGIN_DEGREES: f64 = 0.02;
/// 読み込むときに足す余白（度）。経路を少し伸ばしただけで読み直さないよう、広めに読む。
const LOAD_MARGIN_DEGREES: f64 = 0.2;

/// 読んである判定用データ。フォルダが変われば読み直す。
#[derive(Default)]
pub struct Extractor {
    loaded: Mutex<Option<(PathBuf, Arc<Prepared>)>>,
}

impl Extractor {
    fn prepared(&self, dir: &Path, bbox: BBox) -> Result<Arc<Prepared>, routelog_geo::Error> {
        let mut loaded = self.loaded.lock().unwrap();
        if let Some((from, prepared)) = loaded.as_ref()
            && from == dir
            && prepared.bbox().contains(&bbox)
        {
            return Ok(prepared.clone());
        }
        let wide = bbox.expand(LOAD_MARGIN_DEGREES);
        let prepared = Arc::new(Prepared::new(GeoData::load(dir, wide)?, wide));
        *loaded = Some((dir.to_owned(), prepared.clone()));
        Ok(prepared)
    }

    /// 吸着の区間を、道に沿った線に引き直す。
    ///
    /// 近くに道が無いか、道のりが直線に比べて長すぎる区間は、頂点の無い線（直線）になる。
    /// 結び方は吸着のままなので、経由点を動かせばまた試す。
    pub fn snap(&self, dir: &Path, doc: &mut RideDocument) -> Result<(), routelog_geo::Error> {
        let points: Vec<_> = doc.waypoints.iter().map(|w| [w.lon, w.lat]).collect();
        let snapping = doc
            .waypoints
            .iter()
            .any(|w| w.segment_mode == SegmentMode::Snap);
        let Some(bbox) = BBox::around(&points).filter(|_| snapping) else {
            return Ok(());
        };
        let prepared = self.prepared(dir, bbox.expand(MARGIN_DEGREES))?;
        for (i, segment) in doc.route.segments.iter_mut().enumerate() {
            if doc.waypoints[i].segment_mode == SegmentMode::Snap {
                let line = prepared.snap(points[i], points[i + 1], &SnapOptions::default());
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
        let extracted = match BBox::around(&line) {
            Some(bbox) if line.len() >= 2 => self
                .prepared(dir, bbox.expand(MARGIN_DEGREES))?
                .extract(&line, &Options::default()),
            _ => Vec::new(),
        };
        Ok(carry_over(&doc.events, extracted))
    }
}
