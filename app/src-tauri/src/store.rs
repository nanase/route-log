//! 走行のローカル保存。走行 1 つを JSON ファイル 1 つに書く。
//!
//! クラウドの API が走行を丸ごと送り受けするので、手元でも同じ形のまま持つ。

use std::path::{Path, PathBuf};

use routelog_core::RideDocument;
use serde::Serialize;

#[derive(Debug)]
pub enum Error {
    /// 走行の ID が UUID の形でない。ファイル名に使うので、他の形は受け付けない。
    Id(String),
    Invalid(routelog_core::ride::Invalid),
    Io(PathBuf, std::io::Error),
    Json(PathBuf, serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Id(id) => write!(f, "走行の ID が正しくありません: {id}"),
            Self::Invalid(reason) => write!(f, "走行のデータが正しくありません: {reason}"),
            Self::Io(path, e) => write!(f, "{}: {e}", path.display()),
            Self::Json(path, e) => write!(f, "{}: {e}", path.display()),
        }
    }
}

/// 一覧に出す、走行の要約。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RideSummary {
    pub id: String,
    pub title: String,
    pub ride_date: String,
    pub updated_at: String,
    pub waypoints: usize,
    pub length_m: f64,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, id: &str) -> Result<PathBuf, Error> {
        let canonical = uuid::Uuid::parse_str(id)
            .map(|uuid| uuid.hyphenated().to_string())
            .map_err(|_| Error::Id(id.to_owned()))?;
        if canonical != id {
            return Err(Error::Id(id.to_owned()));
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    /// 保存してある走行。日付の新しい順。読めないファイルは飛ばす。
    pub fn list(&self) -> Result<Vec<RideSummary>, Error> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::Io(self.dir.clone(), e)),
        };
        let mut rides: Vec<RideSummary> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .filter_map(|path| read(&path).ok())
            .map(|doc| RideSummary {
                waypoints: doc.waypoints.len(),
                length_m: doc.length_m(),
                id: doc.ride.id,
                title: doc.ride.title,
                ride_date: doc.ride.ride_date,
                updated_at: doc.ride.updated_at,
            })
            .collect();
        rides.sort_by(|a, b| (&b.ride_date, &b.id).cmp(&(&a.ride_date, &a.id)));
        Ok(rides)
    }

    pub fn load(&self, id: &str) -> Result<RideDocument, Error> {
        read(&self.path(id)?)
    }

    /// 書きかけのファイルを残さないよう、別名で書いてから置き換える。
    pub fn save(&self, doc: &RideDocument) -> Result<(), Error> {
        doc.validate().map_err(Error::Invalid)?;
        let path = self.path(&doc.ride.id)?;
        let json = serde_json::to_string_pretty(doc).map_err(|e| Error::Json(path.clone(), e))?;
        let io = |path: &Path| {
            let path = path.to_owned();
            move |e| Error::Io(path, e)
        };
        std::fs::create_dir_all(&self.dir).map_err(io(&self.dir))?;
        let partial = path.with_extension("json.part");
        std::fs::write(&partial, json).map_err(io(&partial))?;
        std::fs::rename(&partial, &path).map_err(io(&path))
    }
}

fn read(path: &Path) -> Result<RideDocument, Error> {
    let json = std::fs::read_to_string(path).map_err(|e| Error::Io(path.to_owned(), e))?;
    serde_json::from_str(&json).map_err(|e| Error::Json(path.to_owned(), e))
}

#[cfg(test)]
mod tests {
    use routelog_core::{Ride, Route, SegmentMode, Waypoint};

    use super::*;

    /// テストごとの一時フォルダ。落ちても残骸が次の実行を邪魔しないよう、作る前に消す。
    fn temp_store(name: &str) -> (Store, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("routelog-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Store::new(&dir), dir)
    }

    fn document(id: &str, ride_date: &str) -> RideDocument {
        let waypoint = |lon| Waypoint {
            lon,
            lat: 36.0,
            segment_mode: SegmentMode::Straight,
        };
        RideDocument {
            ride: Ride {
                id: id.into(),
                title: ride_date.into(),
                ride_date: ride_date.into(),
                clock_offset_ms: -30_000,
                is_public: false,
                created_at: "2026-10-03T05:00:00+09:00".into(),
                updated_at: "2026-10-03T05:00:00+09:00".into(),
            },
            waypoints: vec![waypoint(138.0), waypoint(138.1)],
            route: Route {
                segments: vec![vec![]],
            },
            events: Vec::new(),
        }
    }

    const FIRST: &str = "0199c0de-0000-7000-8000-000000000001";
    const SECOND: &str = "0199c0de-0000-7000-8000-000000000002";

    #[test]
    fn what_is_saved_comes_back_the_same() {
        let (store, dir) = temp_store("round-trip");
        let doc = document(FIRST, "2026-10-03");
        store.save(&doc).unwrap();
        assert_eq!(store.load(FIRST).unwrap(), doc);
        store.save(&doc).unwrap();
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(
            files.len(),
            1,
            "保存し直しても、書きかけのファイルは残らない"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_list_is_newest_first_and_skips_what_it_cannot_read() {
        let (store, dir) = temp_store("list");
        assert_eq!(store.list().unwrap(), [], "フォルダがまだ無くても空で返る");
        store.save(&document(FIRST, "2026-10-03")).unwrap();
        store.save(&document(SECOND, "2026-10-10")).unwrap();
        std::fs::write(dir.join("broken.json"), "{").unwrap();
        let list = store.list().unwrap();
        let dates: Vec<_> = list.iter().map(|r| r.ride_date.as_str()).collect();
        assert_eq!(dates, ["2026-10-10", "2026-10-03"]);
        assert_eq!(list[0].waypoints, 2);
        assert!(list[0].length_m > 8000.0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_id_that_is_not_a_uuid_never_becomes_a_path() {
        let (store, dir) = temp_store("id");
        for id in ["../escape", "", "0199C0DE-0000-7000-8000-000000000001", "x"] {
            assert!(matches!(store.load(id), Err(Error::Id(_))), "{id}");
            assert!(matches!(
                store.save(&document(id, "2026-10-03")),
                Err(Error::Id(_))
            ));
        }
        assert!(!dir.exists());
    }

    #[test]
    fn a_document_that_is_not_valid_is_not_written() {
        let (store, dir) = temp_store("invalid");
        let mut doc = document(FIRST, "2026-10-03");
        doc.route.segments.clear();
        assert!(matches!(store.save(&doc), Err(Error::Invalid(_))));
        assert!(!dir.exists());
    }
}
