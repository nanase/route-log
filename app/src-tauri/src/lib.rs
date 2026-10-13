mod check;
mod config;
mod events;
mod rides;
mod store;
mod sync;
mod videos;

use std::path::PathBuf;

use serde::Serialize;
use tauri::Manager;

#[derive(Serialize)]
struct VideoFile {
    path: String,
    size: u64,
}

/// 動画ファイルを 1 本、アセットプロトコルで読めるようにする。
///
/// 設定の scope は空にしてあり、ここで名指しされたファイルだけを開ける。
#[tauri::command]
fn allow_video_file(app: tauri::AppHandle, path: String) -> Result<VideoFile, String> {
    let file = PathBuf::from(&path);
    let meta = std::fs::metadata(&file).map_err(|e| format!("{path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path}: ファイルではありません"));
    }
    app.asset_protocol_scope()
        .allow_file(&file)
        .map_err(|e| e.to_string())?;
    Ok(VideoFile {
        path,
        size: meta.len(),
    })
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(videos::State::default())
        .manage(events::Extractor::default())
        .setup(|app| {
            let dir = config::data_dir(app.handle())?;
            app.manage(store::Store::new(dir.join("rides")));
            app.manage(config::ConfigFile::open(dir.join("config.json")));
            app.manage(sync::Synced::open(dir.join("sync.json")));
            sync::init_credentials();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            allow_video_file,
            check::check_target,
            check::finish_check,
            check::capture_check,
            check::check_path,
            config::get_config,
            config::open_road_data,
            config::open_geo_data,
            rides::list_rides,
            rides::load_ride,
            rides::create_ride,
            rides::save_ride,
            sync::get_sync_settings,
            sync::set_sync_settings,
            sync::sync_list,
            sync::sync_push,
            sync::sync_pull,
            videos::scan_videos,
            videos::build_timeline,
            videos::locate,
            videos::export_csv,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the app");
}
