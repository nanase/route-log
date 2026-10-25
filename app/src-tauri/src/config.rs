//! 次に起動したときも使う設定。フォルダの場所は環境ごとに違うので、アプリのデータとして持つ。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use routelog_core::resolve::MarkerLeads;
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
    /// 走行ログの API の URL。
    #[serde(default)]
    pub api_url: Option<String>,
    /// ffprobe と ffmpeg があるフォルダ。開発中の起動で覚え、リリースビルドを直に起動したときに使う。
    #[serde(default)]
    pub ffmpeg_dir: Option<String>,
    /// Resolve へ書き出すマーカーを、種類ごとに前へずらす長さ。
    #[serde(default)]
    pub marker_leads: MarkerLeads,
}

impl Config {
    pub fn ffmpeg_dir(&self) -> Option<&Path> {
        self.ffmpeg_dir.as_deref().map(Path::new)
    }
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

/// 開発中に使っている ffprobe と ffmpeg のフォルダを PATH から探し、覚える。
///
/// mise exec 経由で起動すると、PATH の先頭近くに、mise が ffmpeg の環境を整えてから呼ぶ `.cmd` が
/// ある。mise の shim しか無ければ、mise にそのフォルダを尋ねる。リリースビルドをエクスプローラー
/// から起動したときや、exe をリポジトリの外に置いたときは、覚えておいた場所を使う。
///
/// 書けなくても起動は止めない。この起動では PATH から探せている。
pub fn remember_ffmpeg_dir(config: &ConfigFile) {
    let Some(dir) = routelog_video::dir_on_path("ffprobe") else {
        return;
    };
    let dir = if is_mise_shims(&dir) {
        match mise_bin_dir("ffprobe") {
            Some(dir) => dir,
            None => return,
        }
    } else {
        dir
    };
    if routelog_video::find_in(&dir, "ffmpeg").is_none() {
        return;
    }
    let dir = dir.to_string_lossy().into_owned();
    if config.get().ffmpeg_dir.as_ref() != Some(&dir) {
        let _ = config.update(|c| c.ffmpeg_dir = Some(dir));
    }
}

/// mise の shim のフォルダか。shim は起動した場所の mise.toml で版を決めるので、リポジトリの外では
/// 動かない。mise exec を通さずに起動すると、PATH で最初に見つかるのはこれである。
fn is_mise_shims(dir: &Path) -> bool {
    dir.ends_with("mise/shims")
}

/// いまいる場所の mise.toml で使う道具 `name` の、shim ではない本当のフォルダ。リポジトリの外では、
/// mise.toml が無いので見つからない。
fn mise_bin_dir(name: &str) -> Option<PathBuf> {
    let mut command = Command::new("mise");
    command
        .args(["which", name])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    routelog_video::hide_console(&mut command);
    let output = command.output().ok().filter(|o| o.status.success())?;
    let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    path.parent().map(Path::to_path_buf)
}

#[tauri::command]
pub fn get_config(config: tauri::State<ConfigFile>) -> Config {
    config.get()
}

/// マーカーを前へずらす長さを覚える。
#[tauri::command]
pub fn set_marker_leads(
    config: tauri::State<ConfigFile>,
    leads: MarkerLeads,
) -> Result<(), String> {
    config.update(|c| c.marker_leads = leads)
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
