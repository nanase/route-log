//! 動画フォルダの読み込みと、実時刻の軸。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use routelog_core::{Position, RideVideos, Timeline, VideoFile, group_by_date, jst};
use routelog_video::{Ffprobe, NamePattern, Skipped};
use serde::Serialize;
use tauri::Manager;

use crate::config::ConfigFile;
use crate::folders::{Summary, VideoFolders};

/// いま開いている走行のタイムライン。時刻と位置の変換は、ここに問い合わせる。
#[derive(Default)]
pub struct State {
    timeline: Mutex<Option<Timeline>>,
}

impl State {
    /// いま開いている走行のタイムライン。まだ作っていなければ `None`。
    pub fn current(&self) -> Option<Timeline> {
        self.timeline.lock().unwrap().clone()
    }
}

#[derive(Serialize)]
pub struct Scan {
    rides: Vec<RideVideos>,
    skipped: Vec<Skipped>,
}

/// 走行の動画フォルダ 1 つの、いまの様子。
#[derive(Serialize)]
pub struct FolderState {
    dir: String,
    /// フォルダがあるか。ディスクを外したときや、消したときは無い。
    found: bool,
    /// その日付の動画の本数。
    count: usize,
}

/// 走行の動画フォルダと、その中の、走行の日付の動画。
#[derive(Serialize)]
pub struct DateVideos {
    folders: Vec<FolderState>,
    /// どのフォルダの動画も合わせて、開始時刻の順。
    files: Vec<VideoFile>,
    skipped: Vec<Skipped>,
}

/// フォルダの下から、録画開始の日付が `date` の動画だけを読む。ほかの日付の動画は ffprobe にかけない。
async fn scan_date(
    dir: &Path,
    date: &str,
    ffprobe: &Ffprobe,
) -> Result<routelog_video::Scan, String> {
    let (dir, date, ffprobe) = (dir.to_path_buf(), date.to_owned(), ffprobe.clone());
    tauri::async_runtime::spawn_blocking(move || {
        routelog_video::scan_where(&dir, &NamePattern::dji_action4(), &ffprobe, |parsed| {
            jst::date(parsed.start_ms) == date
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// 走行が覚えている動画フォルダを読み直す。見つかったフォルダは、アセットプロトコルで読めるようにする。
///
/// 同じ名前の動画が 2 つのフォルダにあれば、先に覚えたフォルダのほうを使う。
async fn load(
    app: &tauri::AppHandle,
    config: &ConfigFile,
    folders: &VideoFolders,
    date: &str,
) -> Result<DateVideos, String> {
    let ffprobe = Ffprobe::find(config.get().ffmpeg_dir());
    let mut loaded = DateVideos {
        folders: Vec::new(),
        files: Vec::new(),
        skipped: Vec::new(),
    };
    for folder in folders.get(date) {
        let dir = PathBuf::from(&folder.dir);
        if !dir.is_dir() {
            loaded.folders.push(FolderState {
                dir: folder.dir,
                found: false,
                count: 0,
            });
            continue;
        }
        let scanned = scan_date(&dir, date, &ffprobe).await?;
        app.asset_protocol_scope()
            .allow_directory(&dir, true)
            .map_err(|e| e.to_string())?;
        folders.remember(date, &folder.dir, scanned.files.clone())?;
        loaded.folders.push(FolderState {
            dir: folder.dir,
            found: true,
            count: scanned.files.len(),
        });
        for file in scanned.files {
            if !loaded.files.iter().any(|known| known.name == file.name) {
                loaded.files.push(file);
            }
        }
        loaded.skipped.extend(scanned.skipped);
    }
    loaded.files.sort_by_key(|f| (f.start_ms, f.seq));
    Ok(loaded)
}

/// その日付の走行の動画フォルダを読み、走行に使う動画を返す。
#[tauri::command]
pub async fn load_date_videos(
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigFile>,
    folders: tauri::State<'_, VideoFolders>,
    date: String,
) -> Result<DateVideos, String> {
    load(&app, &config, &folders, &date).await
}

/// 動画フォルダを走行に足す。フォルダにその日付の動画が無ければ、覚えずに `None` を返す。
#[tauri::command]
pub async fn add_video_folder(
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigFile>,
    folders: tauri::State<'_, VideoFolders>,
    date: String,
    dir: String,
) -> Result<Option<DateVideos>, String> {
    let ffprobe = Ffprobe::find(config.get().ffmpeg_dir());
    let scanned = scan_date(Path::new(&dir), &date, &ffprobe).await?;
    if scanned.files.is_empty() {
        return Ok(None);
    }
    folders.remember(&date, &dir, scanned.files)?;
    load(&app, &config, &folders, &date).await.map(Some)
}

/// 動画フォルダを走行から外す。動画のファイルには触らない。
#[tauri::command]
pub async fn remove_video_folder(
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigFile>,
    folders: tauri::State<'_, VideoFolders>,
    date: String,
    dir: String,
) -> Result<DateVideos, String> {
    folders.forget(&date, &dir)?;
    load(&app, &config, &folders, &date).await
}

/// 動画フォルダを覚えている日付ごとの、本数と長さ。走行の一覧に出す。
#[tauri::command]
pub fn video_summaries(folders: tauri::State<VideoFolders>) -> BTreeMap<String, Summary> {
    folders.summaries()
}

/// 以前の版が 1 つだけ覚えていた動画フォルダを、その中の動画の日付ごとの動画フォルダとして覚え直す。
/// 覚え直したら、以前の覚えは消す。覚え直すものが無ければ偽を返す。
#[tauri::command]
pub async fn move_video_dir(
    config: tauri::State<'_, ConfigFile>,
    folders: tauri::State<'_, VideoFolders>,
) -> Result<bool, String> {
    let Some(dir) = config.get().video_dir else {
        return Ok(false);
    };
    // フォルダが見つからないうちは、次の起動でまた試す。
    if !Path::new(&dir).is_dir() {
        return Ok(false);
    }
    let ffprobe = Ffprobe::find(config.get().ffmpeg_dir());
    let scanned = {
        let dir = PathBuf::from(&dir);
        tauri::async_runtime::spawn_blocking(move || {
            routelog_video::scan(&dir, &NamePattern::dji_action4(), &ffprobe)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
    };
    let dates = group_by_date(scanned.files)
        .into_iter()
        .map(|ride| (ride.date, ride.files))
        .collect();
    folders.remember_new(&dir, dates)?;
    config.update(|c| c.video_dir = None)?;
    Ok(true)
}

/// フォルダの下の動画を読み、日ごとに分ける。何も覚えない。自動の確認が、フォルダの中の日付を知るのに使う。
#[tauri::command]
pub async fn scan_videos(
    config: tauri::State<'_, ConfigFile>,
    dir: String,
) -> Result<Scan, String> {
    let ffprobe = Ffprobe::find(config.get().ffmpeg_dir());
    let scanned = tauri::async_runtime::spawn_blocking(move || {
        routelog_video::scan(Path::new(&dir), &NamePattern::dji_action4(), &ffprobe)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(Scan {
        rides: group_by_date(scanned.files),
        skipped: scanned.skipped,
    })
}

/// 走行 1 つぶんのファイルを実時刻の軸に並べ、以後の `locate` と CSV の書き出しの相手にする。
///
/// 渡すのは、走行に使う動画だけである。走行から外した動画は渡さないので、その区間は録画停止に
/// なる。動画の無い走行では空で呼び、前に開いた走行の動画を相手に残さない。
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

/// イベントの通過時刻の一覧を CSV に書き出す。動画の中の位置は、いま開いているタイムラインから求める。
///
/// 表計算ソフトが文字コードを取り違えないよう、先頭に BOM を付ける。
#[tauri::command]
pub fn export_csv(
    state: tauri::State<State>,
    doc: routelog_core::RideDocument,
    path: String,
) -> Result<usize, String> {
    let timeline = state.timeline.lock().unwrap();
    let empty = Timeline::new(Vec::new(), doc.ride.clock_offset_ms);
    let text = routelog_core::export::csv(&doc.events, timeline.as_ref().unwrap_or(&empty));
    std::fs::write(&path, format!("\u{feff}{text}")).map_err(|e| format!("{path}: {e}"))?;
    Ok(text.lines().count() - 1)
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
