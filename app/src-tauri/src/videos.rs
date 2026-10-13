//! 動画フォルダの読み込みと、実時刻の軸。

use std::path::PathBuf;
use std::sync::Mutex;

use routelog_core::{Position, RideVideos, Timeline, VideoFile, group_by_date};
use routelog_video::{Ffprobe, NamePattern, Skipped};
use serde::Serialize;
use tauri::Manager;

use crate::config::ConfigFile;

/// いま開いている走行のタイムライン。時刻と位置の変換は、ここに問い合わせる。
#[derive(Default)]
pub struct State {
    timeline: Mutex<Option<Timeline>>,
}

#[derive(Serialize)]
pub struct Scan {
    rides: Vec<RideVideos>,
    skipped: Vec<Skipped>,
}

/// フォルダの下の動画を読み、日ごとに分ける。そのフォルダをアセットプロトコルで読めるようにし、
/// 次の起動でも開けるよう場所を覚える。
#[tauri::command]
pub async fn scan_videos(
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigFile>,
    dir: String,
) -> Result<Scan, String> {
    config.update(|c| c.video_dir = Some(dir.clone()))?;
    let dir = PathBuf::from(dir);
    let scanned = {
        let dir = dir.clone();
        tauri::async_runtime::spawn_blocking(move || {
            routelog_video::scan(&dir, &NamePattern::dji_action4(), &Ffprobe::from_env())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
    };
    app.asset_protocol_scope()
        .allow_directory(&dir, true)
        .map_err(|e| e.to_string())?;
    Ok(Scan {
        rides: group_by_date(scanned.files),
        skipped: scanned.skipped,
    })
}

/// 走行 1 つぶんのファイルを実時刻の軸に並べ、以後の `locate` の相手にする。
#[tauri::command]
pub fn build_timeline(
    state: tauri::State<State>,
    files: Vec<VideoFile>,
    clock_offset_ms: i64,
) -> Timeline {
    let timeline = Timeline::new(files, clock_offset_ms);
    *state.timeline.lock().unwrap() = Some(timeline.clone());
    timeline
}

/// 実時刻を動画の位置にする。録画していない時刻は、次に録画が始まる位置にする。
#[tauri::command]
pub fn locate(state: tauri::State<State>, real_ms: i64) -> Option<Position> {
    state
        .timeline
        .lock()
        .unwrap()
        .as_ref()?
        .locate_or_next(real_ms)
}
