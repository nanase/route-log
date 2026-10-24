mod check;
mod config;
mod events;
mod excluded;
mod overview;
mod resolve;
mod rides;
mod stops;
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
            let config = config::ConfigFile::open(dir.join("config.json"));
            config::remember_ffmpeg_dir(&config);
            app.manage(overview::State::new(config.get().ffmpeg_dir()));
            app.manage(stops::Stops::new(
                dir.join("stops"),
                config.get().ffmpeg_dir(),
            ));
            app.manage(config);
            app.manage(excluded::ExcludedVideos::open(
                dir.join("excluded-videos.json"),
            ));
            app.manage(sync::Synced::open(dir.join("sync.json")));
            sync::init_credentials();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            allow_video_file,
            check::check_target,
            check::finish_check,
            check::capture_check,
            check::close_check,
            check::check_path,
            config::get_config,
            config::open_road_data,
            config::open_geo_data,
            excluded::get_excluded_videos,
            excluded::set_video_excluded,
            overview::overview_preview,
            overview::render_overview,
            overview::cancel_overview,
            resolve::export_resolve,
            rides::list_rides,
            rides::load_ride,
            rides::create_ride,
            rides::save_ride,
            rides::delete_ride,
            rides::snap_point,
            stops::get_stops,
            stops::find_stops,
            stops::cancel_find_stops,
            stops::set_stop_taken,
            sync::get_sync_settings,
            sync::set_sync_settings,
            sync::sync_list,
            sync::sync_push,
            sync::sync_fetch,
            sync::sync_apply,
            sync::sync_delete,
            videos::scan_videos,
            videos::build_timeline,
            videos::locate,
            videos::export_csv,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the app");
}
