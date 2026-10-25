//! 国土交通省のデータベースの橋・トンネルのタイルを、走行の経路のまわりの分だけ取って手元に置く。
//! 抽出のときは、手元のタイルだけを読む（events.rs）。

use std::path::PathBuf;
use std::sync::Arc;

use routelog_core::RideDocument;
use routelog_geo::official::{self, Store};
use tauri::Manager;

/// 経路からこの距離（m）までに掛かるタイルを取る。対応づけは 100 m までの施設を見る。
const MARGIN_M: f64 = 150.0;

/// 取ってきたタイルの置き場所。
pub struct Structures(pub Arc<Store>);

impl Structures {
    /// アプリのキャッシュのフォルダの `qchizu` に置く。自動の確認では、確認の走行の隣に置く。
    pub fn open(app: &tauri::AppHandle) -> Result<Self, tauri::Error> {
        let dir: PathBuf = match crate::check::data_dir() {
            Some(dir) => dir,
            None => app.path().app_cache_dir()?,
        };
        Ok(Self(Arc::new(Store::new(dir.join("qchizu")))))
    }
}

/// 経路のまわりの、橋・トンネルのタイル。
pub fn tiles_of(doc: &RideDocument) -> std::collections::BTreeSet<(u32, u32)> {
    official::tiles_along(&doc.line(), MARGIN_M)
}

/// その走行の経路のまわりの、橋・トンネルのタイルのうち手元に無いものを取り、抽出し直すと公称の
/// 名前・長さか橋の字幕の有無が変わるかを返す。
///
/// 取れないタイルがあっても、手元にあるタイルで決める。別の走行で取ったタイルも使う。
#[tauri::command]
pub async fn fetch_structures(
    structures: tauri::State<'_, Structures>,
    doc: RideDocument,
) -> Result<bool, String> {
    let store = structures.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let tiles = tiles_of(&doc);
        if store.missing(&tiles) > 0 {
            // 通信できなくても、手元のタイルで決める。
            let _ = store.fetch(&tiles);
        }
        let facilities = store.facilities(&tiles);
        official::needs_attach(&doc.events, &doc.line(), &facilities)
    })
    .await
    .map_err(|e| e.to_string())
}
