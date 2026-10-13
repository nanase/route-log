//! 画面を人が見なくても確かめられるようにするための入口。
//!
//! 環境変数で対象と報告先を受け取り、画面側が確認を終えたら報告を書いて終了する。
//! 環境変数が無ければ何もしない。
//!
//! - `ROUTELOG_CHECK_FILE`: 動画 1 本の読み込み・再生・シークを測る
//! - `ROUTELOG_CHECK_DIR`: フォルダを読み、ファイルをまたぐ再生と時刻の変換、動画を走行から外す操作を
//!   確かめる
//! - `ROUTELOG_CHECK_MAP`: 地図の表示と経路の編集・保存を確かめる。値は道路データのフォルダで、
//!   重ねないなら `-`
//! - `ROUTELOG_CHECK_GEO`: 地図の確認に、イベントの抽出と吸着を足す。フォルダの確認に足すと、動画を見ながらの
//!   時刻の確定と、動画の無い走行を確かめる。値は判定用データのフォルダ
//! - `ROUTELOG_CHECK_SYNC`: クラウドとの同期を確かめる。値は API の URL で、手元で動かした Worker を指す
//! - `ROUTELOG_CHECK_REPORT`: 報告（JSON）の書き先。画面の写しは同じ場所に PNG で書く

use std::path::PathBuf;

use serde::Serialize;

const FILE: &str = "ROUTELOG_CHECK_FILE";
const DIR: &str = "ROUTELOG_CHECK_DIR";
const MAP: &str = "ROUTELOG_CHECK_MAP";
const GEO: &str = "ROUTELOG_CHECK_GEO";
const SYNC: &str = "ROUTELOG_CHECK_SYNC";
const REPORT: &str = "ROUTELOG_CHECK_REPORT";

#[derive(Serialize)]
pub struct Target {
    file: Option<String>,
    dir: Option<String>,
    map: Option<String>,
    geo: Option<String>,
    sync: Option<String>,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn report_path() -> Result<PathBuf, String> {
    var(REPORT)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{REPORT} がありません"))
}

/// 自動の確認の間に使う、アプリのデータの置き場所。報告の隣に作る。確認でなければ `None`。
pub fn data_dir() -> Option<PathBuf> {
    let report = var(REPORT).map(PathBuf::from)?;
    Some(report.with_file_name("routelog-check-data"))
}

/// 自動で確かめる対象。指定が無ければどれも `None`。
#[tauri::command]
pub fn check_target() -> Target {
    Target {
        file: var(FILE),
        dir: var(DIR),
        map: var(MAP),
        geo: var(GEO),
        sync: var(SYNC),
    }
}

/// 結果を報告先へ書き、アプリを終了する。
#[tauri::command]
pub fn finish_check(app: tauri::AppHandle, report: String) -> Result<(), String> {
    let path = report_path()?;
    std::fs::write(&path, report).map_err(|e| format!("{}: {e}", path.display()))?;
    app.exit(0);
    Ok(())
}

/// 確認のために書くファイルの場所。報告と同じ場所で、名前に `name` を足す。
#[tauri::command]
pub fn check_path(name: String) -> Result<String, String> {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        return Err(format!("名前に使えない文字があります: {name}"));
    }
    let report = report_path()?;
    let stem = report.file_stem().unwrap_or_default().to_string_lossy();
    let path = report.with_file_name(format!("{stem}-{name}"));
    Ok(path.to_string_lossy().into_owned())
}

/// いまの画面を PNG に写す。書き先は報告と同じ場所で、名前に `name` を足す。
///
/// 写るのはこのアプリの画面だけで、デスクトップやほかのウィンドウは写らない。
#[tauri::command]
pub async fn capture_check(window: tauri::WebviewWindow, name: String) -> Result<String, String> {
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("名前に使えない文字があります: {name}"));
    }
    let report = report_path()?;
    let stem = report.file_stem().unwrap_or_default().to_string_lossy();
    let path = report.with_file_name(format!("{stem}-{name}.png"));
    capture::to_file(window, path.clone()).await?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(windows)]
mod capture {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
    use windows::Win32::System::Com::{STGC_DEFAULT, STGM_CREATE, STGM_WRITE};
    use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
    use windows::core::HSTRING;

    pub async fn to_file(window: tauri::WebviewWindow, path: PathBuf) -> Result<(), String> {
        let (done, wait) = mpsc::channel::<Result<(), String>>();
        window
            .with_webview(move |webview| {
                let started = (|| -> windows::core::Result<()> {
                    // SAFETY: WebView2 の COM の呼び出し。この閉包は UI スレッドで動き、
                    // ストリームは完了の通知まで閉包が持ち続ける。
                    unsafe {
                        let stream = SHCreateStreamOnFileEx(
                            &HSTRING::from(path.as_os_str()),
                            (STGM_CREATE | STGM_WRITE).0,
                            0,
                            true,
                            None,
                        )?;
                        let written = stream.clone();
                        let done = done.clone();
                        let handler =
                            CapturePreviewCompletedHandler::create(Box::new(move |result| {
                                let result = result.and_then(|()| written.Commit(STGC_DEFAULT));
                                drop(written);
                                let _ = done.send(result.map_err(|e| e.to_string()));
                                Ok(())
                            }));
                        webview.controller().CoreWebView2()?.CapturePreview(
                            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                            &stream,
                            &handler,
                        )
                    }
                })();
                if let Err(e) = started {
                    let _ = done.send(Err(e.to_string()));
                }
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || {
            wait.recv_timeout(Duration::from_secs(10))
                .map_err(|_| "画面の写しが終わりません".to_owned())?
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

#[cfg(not(windows))]
mod capture {
    pub async fn to_file(
        _window: tauri::WebviewWindow,
        _path: std::path::PathBuf,
    ) -> Result<(), String> {
        Err("画面の写しは Windows でだけ使えます".into())
    }
}
