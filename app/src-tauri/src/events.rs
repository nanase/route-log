//! 経路からのイベント抽出。判定用データを読んだ結果は、次の抽出のために持っておく。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use routelog_core::{Event, RideDocument, carry_over};
use routelog_geo::{BBox, GeoData, Options, Prepared};

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

    /// 経路からイベントを抽出し直し、人が手を入れた結果を引き継ぐ。新しいイベントには ID を振る。
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
        let mut events = carry_over(&doc.events, extracted);
        for event in &mut events {
            if event.id.is_empty() {
                event.id = uuid::Uuid::now_v7().hyphenated().to_string();
            }
        }
        Ok(events)
    }
}
