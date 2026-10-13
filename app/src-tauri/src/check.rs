//! 画面を人が見なくても確かめられるようにするための入口。
//!
//! 環境変数で対象と報告先を受け取り、画面側が確認を終えたら報告を書いて終了する。
//! 環境変数が無ければ何もしない。
//!
//! - `ROUTELOG_CHECK_FILE`: 動画 1 本の読み込み・再生・シークを測る
//! - `ROUTELOG_CHECK_DIR`: フォルダを読み、ファイルをまたぐ再生と時刻の変換を確かめる
//! - `ROUTELOG_CHECK_REPORT`: 報告（JSON）の書き先

use serde::Serialize;

const FILE: &str = "ROUTELOG_CHECK_FILE";
const DIR: &str = "ROUTELOG_CHECK_DIR";
const REPORT: &str = "ROUTELOG_CHECK_REPORT";

#[derive(Serialize)]
pub struct Target {
    file: Option<String>,
    dir: Option<String>,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// 自動で確かめる対象。指定が無ければどちらも `None`。
#[tauri::command]
pub fn check_target() -> Target {
    Target {
        file: var(FILE),
        dir: var(DIR),
    }
}

/// 結果を報告先へ書き、アプリを終了する。
#[tauri::command]
pub fn finish_check(app: tauri::AppHandle, report: String) -> Result<(), String> {
    let path = var(REPORT).ok_or_else(|| format!("{REPORT} がありません"))?;
    std::fs::write(&path, report).map_err(|e| format!("{path}: {e}"))?;
    app.exit(0);
    Ok(())
}
