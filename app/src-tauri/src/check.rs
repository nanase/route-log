//! 再生の検証（マイルストーン 1）を人手を介さずに回すための入口。
//!
//! 環境変数で動画と報告先を受け取り、画面側が計測を終えたら報告を書いて終了する。
//! 環境変数が無ければ何もしない。

const TARGET: &str = "ROUTELOG_CHECK_FILE";
const REPORT: &str = "ROUTELOG_CHECK_REPORT";

/// 自動で検証する動画のパス。指定が無ければ `None`。
#[tauri::command]
pub fn check_target() -> Option<String> {
    std::env::var(TARGET).ok().filter(|v| !v.is_empty())
}

/// 計測結果を報告先へ書き、アプリを終了する。
#[tauri::command]
pub fn finish_check(app: tauri::AppHandle, report: String) -> Result<(), String> {
    let path = std::env::var(REPORT).map_err(|_| format!("{REPORT} がありません"))?;
    std::fs::write(&path, report).map_err(|e| format!("{path}: {e}"))?;
    app.exit(0);
    Ok(())
}
