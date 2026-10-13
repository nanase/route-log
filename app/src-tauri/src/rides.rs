//! 走行の作成・読み込み・保存。

use std::time::{SystemTime, UNIX_EPOCH};

use routelog_core::{Ride, RideDocument, Route, jst};
use serde::Serialize;

use crate::store::{RideSummary, Store};

fn now() -> String {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    jst::iso(unix_ms)
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
            created_at: now.clone(),
            updated_at: now,
        },
        waypoints: Vec::new(),
        route: Route::default(),
    };
    store.save(&doc).map_err(|e| e.to_string())?;
    Ok(doc)
}

#[derive(Serialize)]
pub struct Saved {
    updated_at: String,
    length_m: f64,
}

/// 走行を保存し、更新時刻と経路の長さを返す。
#[tauri::command]
pub fn save_ride(store: tauri::State<Store>, mut doc: RideDocument) -> Result<Saved, String> {
    doc.ride.updated_at = now();
    store.save(&doc).map_err(|e| e.to_string())?;
    Ok(Saved {
        length_m: doc.length_m(),
        updated_at: doc.ride.updated_at,
    })
}
