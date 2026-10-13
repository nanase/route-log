//! 次に起動したときも使う設定。フォルダの場所は環境ごとに違うので、アプリのデータとして持つ。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::Manager;

/// アプリのデータの置き場所を上書きする環境変数。自動の確認が、本物のデータに触らないために使う。
const DATA_DIR_ENV: &str = "ROUTELOG_DATA_DIR";

const NATIONAL: &str = "national-routes.pmtiles";
const PREFECTURAL: &str = "prefectural-routes.pmtiles";
const GEO_FILES: [&str; 3] = ["roads.fgb", "admin.fgb", "michi-no-eki.fgb"];

pub fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, tauri::Error> {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
        return Ok(dir.into());
    }
    // 自動の確認は走行を作って保存する。置き場所の指定を忘れても、本物のデータには書かない。
    match crate::check::data_dir() {
        Some(dir) => Ok(dir),
        None => app.path().app_data_dir(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// 最後に開いた動画フォルダ。
    pub video_dir: Option<String>,
    /// 国道マップの配信データ（PMTiles）があるフォルダ。
    pub road_data_dir: Option<String>,
    /// 判定用データ（FlatGeobuf）があるフォルダ。
    #[serde(default)]
    pub geo_data_dir: Option<String>,
}

pub struct ConfigFile {
    path: PathBuf,
    config: Mutex<Config>,
}

impl ConfigFile {
    /// 読めないときは、何も設定していない状態から始める。
    pub fn open(path: PathBuf) -> Self {
        let config = std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            path,
            config: Mutex::new(config),
        }
    }

    pub fn get(&self) -> Config {
        self.config.lock().unwrap().clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Config)) -> Result<(), String> {
        let mut config = self.config.lock().unwrap();
        change(&mut config);
        let json = serde_json::to_string_pretty(&*config).map_err(|e| e.to_string())?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&self.path, json).map_err(|e| format!("{}: {e}", self.path.display()))
    }
}

#[tauri::command]
pub fn get_config(config: tauri::State<ConfigFile>) -> Config {
    config.get()
}

/// 地図に重ねる道路データの場所。
#[derive(Serialize)]
pub struct RoadData {
    national: String,
    prefectural: String,
}

/// 国道マップの配信データのフォルダを開き、中の PMTiles をアセットプロトコルで読めるようにする。
#[tauri::command]
pub fn open_road_data(
    app: tauri::AppHandle,
    config: tauri::State<ConfigFile>,
    dir: String,
) -> Result<RoadData, String> {
    let allow = |name: &str| -> Result<String, String> {
        let path = Path::new(&dir).join(name);
        if !path.is_file() {
            return Err(format!("{} がありません", path.display()));
        }
        app.asset_protocol_scope()
            .allow_file(&path)
            .map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().into_owned())
    };
    let data = RoadData {
        national: allow(NATIONAL)?,
        prefectural: allow(PREFECTURAL)?,
    };
    config.update(|c| c.road_data_dir = Some(dir.clone()))?;
    Ok(data)
}

/// 判定用データのフォルダを開く。以後、経路を保存するたびにイベントを抽出する。
#[tauri::command]
pub fn open_geo_data(config: tauri::State<ConfigFile>, dir: String) -> Result<(), String> {
    for name in GEO_FILES {
        let path = Path::new(&dir).join(name);
        if !path.is_file() {
            return Err(format!("{} がありません", path.display()));
        }
    }
    config.update(|c| c.geo_data_dir = Some(dir.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_there_the_next_time_the_file_is_opened() {
        let dir = std::env::temp_dir().join(format!("routelog-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.json");

        let file = ConfigFile::open(path.clone());
        assert_eq!(file.get(), Config::default());
        file.update(|c| c.video_dir = Some("videos".into()))
            .unwrap();

        assert_eq!(
            ConfigFile::open(path).get().video_dir.as_deref(),
            Some("videos")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_read_starts_from_nothing() {
        let dir =
            std::env::temp_dir().join(format!("routelog-config-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, "{").unwrap();
        assert_eq!(ConfigFile::open(path).get(), Config::default());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
