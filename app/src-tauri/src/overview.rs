//! 経路の概略図の動画。見本の 1 枚と、動画の書き出し。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use routelog_overview::{Credits, Ffmpeg, Fps, LonLat, Motion, Progress, Sources, TileStore};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri::ipc::{Channel, Response};

/// 見本の大きさ。書き出す動画の 4 分の 1。
const PREVIEW_SIZE: [u32; 2] = [960, 540];

/// 取りかかっている仕事を止める旗と、ffmpeg。新しい見本を頼まれたら、前の見本はやめる。
pub struct State {
    preview: Mutex<Arc<AtomicBool>>,
    render: Mutex<Arc<AtomicBool>>,
    /// GPU で書けるかを、起動してから 1 度だけ試すために持っておく
    ffmpeg: Arc<Ffmpeg>,
}

impl State {
    pub fn new(ffmpeg_dir: Option<&Path>) -> Self {
        State {
            preview: Mutex::default(),
            render: Mutex::default(),
            ffmpeg: Arc::new(Ffmpeg::find(ffmpeg_dir)),
        }
    }
}

fn renew(slot: &Mutex<Arc<AtomicBool>>) -> Arc<AtomicBool> {
    let mut slot = slot.lock().expect("旗を持ったまま落ちていない");
    slot.store(true, Ordering::Relaxed);
    *slot = Arc::new(AtomicBool::new(false));
    Arc::clone(&slot)
}

/// 出典の画像。フロントが、幅 3840 px の画面に合わせて文字を描いた PNG。
#[derive(Deserialize)]
pub struct CreditImages {
    photo: Vec<u8>,
    landsat: Vec<u8>,
}

impl CreditImages {
    fn decode(&self) -> Result<Credits, String> {
        Credits::from_png(&self.photo, &self.landsat).map_err(|e| e.to_string())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionInput {
    /// フレームレートの分子と分母。59.94 fps なら [60000, 1001]
    fps: [u32; 2],
    zoom_seconds: f64,
    hold_seconds: f64,
    start_scale: f64,
}

#[derive(Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum ProgressEvent {
    Tiles { done: usize, total: usize },
    Frames { done: u32, total: u32 },
}

impl From<Progress> for ProgressEvent {
    fn from(progress: Progress) -> Self {
        match progress {
            Progress::Tiles { done, total } => ProgressEvent::Tiles { done, total },
            Progress::Frames { done, total } => ProgressEvent::Frames { done, total },
        }
    }
}

/// 取ってきた写真のタイルの置き場所。自動の確認では、確認の走行の隣に置く。
fn tiles(app: &tauri::AppHandle) -> Result<TileStore, String> {
    let dir = match crate::check::data_dir() {
        Some(dir) => dir,
        None => app.path().app_cache_dir().map_err(|e| e.to_string())?,
    };
    Ok(TileStore::new(dir.join("tiles").join("seamlessphoto")))
}

/// 止まった所の 1 枚を PNG で返す。前に頼まれた見本がまだ描けていなければ、それはやめる。
#[tauri::command]
pub async fn overview_preview(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
    line: Vec<LonLat>,
    credits: CreditImages,
) -> Result<Response, String> {
    let cancel = renew(&state.preview);
    let store = tiles(&app)?;
    let credits = credits.decode()?;
    let png = tauri::async_runtime::spawn_blocking(move || {
        let sources = Sources {
            tiles: &store,
            credits: &credits,
        };
        routelog_overview::preview(&line, PREVIEW_SIZE, sources, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(Response::new(png))
}

/// 動画を `path` に書き出す。進み具合を `on_progress` に送る。
#[tauri::command]
pub async fn render_overview(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
    line: Vec<LonLat>,
    motion: MotionInput,
    credits: CreditImages,
    path: String,
    on_progress: Channel<ProgressEvent>,
) -> Result<(), String> {
    let cancel = renew(&state.render);
    let store = tiles(&app)?;
    let credits = credits.decode()?;
    let ffmpeg = Arc::clone(&state.ffmpeg);
    let [num, den] = motion.fps;
    let motion = Motion {
        fps: Fps::new(num, den)
            .ok_or_else(|| format!("フレームレート {num}/{den} では書き出せません"))?,
        zoom_seconds: motion.zoom_seconds.clamp(0.0, 30.0),
        hold_seconds: motion.hold_seconds.clamp(0.0, 120.0),
        start_scale: motion.start_scale.clamp(0.1, 1.0),
    };
    tauri::async_runtime::spawn_blocking(move || {
        let sources = Sources {
            tiles: &store,
            credits: &credits,
        };
        routelog_overview::render(
            &line,
            motion,
            sources,
            &ffmpeg,
            &PathBuf::from(path),
            &mut |progress| {
                let _ = on_progress.send(progress.into());
            },
            &cancel,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// 書き出しをやめる。書きかけのファイルは消える。
#[tauri::command]
pub fn cancel_overview(state: tauri::State<'_, State>) {
    state
        .render
        .lock()
        .expect("旗を持ったまま落ちていない")
        .store(true, Ordering::Relaxed);
}
