//! 走行の作成・読み込み・保存。

use std::time::{SystemTime, UNIX_EPOCH};

use std::path::Path;

use routelog_core::{Event, Ride, RideDocument, Route, jst};
use serde::Serialize;
use tauri::Manager;

use crate::config::ConfigFile;
use crate::events::Extractor;
use crate::store::{RideSummary, Store};
use crate::sync::Synced;

/// いまの時刻。更新時刻は、手元とクラウドのどちらが新しいかを見分けるのに使う。同じ秒の
/// うちに保存と同期が続いても見分けられるよう、ミリ秒まで持つ。
fn now() -> String {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    jst::iso_millis(unix_ms)
}

#[tauri::command]
pub fn list_rides(store: tauri::State<Store>) -> Result<Vec<RideSummary>, String> {
    store.list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_ride(store: tauri::State<Store>, id: String) -> Result<RideDocument, String> {
    store.load(&id).map_err(|e| e.to_string())
}

/// その日付の走行を新しく作り、保存する。題は日付にしておく。
#[tauri::command]
pub fn create_ride(store: tauri::State<Store>, ride_date: String) -> Result<RideDocument, String> {
    let now = now();
    let doc = RideDocument {
        ride: Ride {
            id: uuid::Uuid::now_v7().hyphenated().to_string(),
            title: ride_date.clone(),
            ride_date,
            clock_offset_ms: 0,
            is_public: false,
            labels: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        },
        waypoints: Vec::new(),
        route: Route::default(),
        events: Vec::new(),
    };
    store.save(&doc).map_err(|e| e.to_string())?;
    Ok(doc)
}

/// 手元の走行の記録を消す。同期の覚えも消す。動画のファイルと、クラウドの走行には触らない。
///
/// 同期を設定しているときは、画面の側が先に `sync_delete` でクラウドから消す。手元だけを消すと、
/// 次の起動で受け取り直して戻ってしまう。
#[tauri::command]
pub fn delete_ride(
    store: tauri::State<Store>,
    synced: tauri::State<Synced>,
    id: String,
) -> Result<(), String> {
    store.delete(&id).map_err(|e| e.to_string())?;
    synced.remove(&id)
}

/// クリックした点を、近い道の上へ寄せる。判定用データを開いていないか、`radius_m` 以内に
/// 道が無ければ `None`。
#[tauri::command]
pub async fn snap_point(
    app: tauri::AppHandle,
    point: [f64; 2],
    radius_m: f64,
) -> Result<Option<[f64; 2]>, String> {
    let Some(dir) = app.state::<ConfigFile>().get().geo_data_dir else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Extractor>()
            .snap_point(Path::new(&dir), point, radius_m)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct Saved {
    updated_at: String,
    length_m: f64,
    /// 吸着の区間を引き直した後の線。
    route: Route,
    events: Vec<Event>,
    /// 位置が先なのに時刻が後になっている、確定済みのイベントの ID。
    conflicts: Vec<String>,
}

/// 走行を保存し、更新時刻・経路の長さ・イベントを返す。イベントの時刻は、保存の前に推定し直す。
///
/// `extract` なら、保存の前に吸着の区間を引き直し、経路からイベントを抽出し直す。判定用データを開いていなければ、
/// イベントはそのままにする。
#[tauri::command]
pub async fn save_ride(
    app: tauri::AppHandle,
    mut doc: RideDocument,
    extract: bool,
) -> Result<Saved, String> {
    doc.validate().map_err(|e| e.to_string())?;
    let geo_dir = app.state::<ConfigFile>().get().geo_data_dir;
    if let (true, Some(dir)) = (extract, geo_dir) {
        let handle = app.clone();
        let (returned, events) = tauri::async_runtime::spawn_blocking(move || {
            let extractor = handle.state::<Extractor>();
            let events = extractor
                .snap(Path::new(&dir), &mut doc)
                .and_then(|()| extractor.extract(Path::new(&dir), &doc));
            (doc, events)
        })
        .await
        .map_err(|e| e.to_string())?;
        doc = returned;
        doc.events = events.map_err(|e| e.to_string())?;
    }
    let conflicts = routelog_core::refresh(&mut doc);
    for event in &mut doc.events {
        if event.id.is_empty() {
            event.id = uuid::Uuid::now_v7().hyphenated().to_string();
        }
    }
    doc.ride.updated_at = now();
    app.state::<Store>().save(&doc).map_err(|e| e.to_string())?;
    Ok(Saved {
        length_m: doc.length_m(),
        updated_at: doc.ride.updated_at,
        route: doc.route,
        events: doc.events,
        conflicts,
    })
}
