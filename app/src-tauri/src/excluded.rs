//! 走行に使わない動画の覚え。
//!
//! 同じ日のフォルダに、走行と関わりの無い動画が混ざることがある。それを日付ごとに覚えて、
//! タイムラインから外す。動画は手元にしか無いので、クラウドへ送る走行のデータには入れず、
//! アプリのデータとして持つ。動画はファイル名で見分ける。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Mutex;

/// 日付（`YYYY-MM-DD`）ごとの、外した動画のファイル名。
type Dates = BTreeMap<String, BTreeSet<String>>;

pub struct ExcludedVideos {
    path: PathBuf,
    dates: Mutex<Dates>,
}

impl ExcludedVideos {
    /// 読めないときは、何も外していない状態から始める。
    pub fn open(path: PathBuf) -> Self {
        let dates = std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            path,
            dates: Mutex::new(dates),
        }
    }

    /// その日付の走行で外している動画のファイル名。名前の順。
    pub fn get(&self, date: &str) -> Vec<String> {
        names(&self.dates.lock().unwrap(), date)
    }

    /// 動画を外す。`excluded` が偽なら、外した動画を戻す。その日付で外している動画を返す。
    ///
    /// 書けなかったときは、覚えも変えない。すでにその状態なら、何も書かない。
    pub fn set(&self, date: &str, name: &str, excluded: bool) -> Result<Vec<String>, String> {
        let mut dates = self.dates.lock().unwrap();
        let mut next = dates.clone();
        let changed = if excluded {
            next.entry(date.to_owned())
                .or_default()
                .insert(name.to_owned())
        } else {
            next.get_mut(date).is_some_and(|names| names.remove(name))
        };
        if changed {
            // 外した動画が無くなった日付は、残さない。
            next.retain(|_, names| !names.is_empty());
            let json = serde_json::to_string_pretty(&next).map_err(|e| e.to_string())?;
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::write(&self.path, json)
                .map_err(|e| format!("{}: {e}", self.path.display()))?;
            *dates = next;
        }
        Ok(names(&dates, date))
    }
}

fn names(dates: &Dates, date: &str) -> Vec<String> {
    dates
        .get(date)
        .map(|names| names.iter().cloned().collect())
        .unwrap_or_default()
}

/// その日付の走行で、タイムラインから外している動画のファイル名。
#[tauri::command]
pub fn get_excluded_videos(videos: tauri::State<ExcludedVideos>, date: String) -> Vec<String> {
    videos.get(&date)
}

/// 動画を、その日付の走行のタイムラインから外す。`excluded` が偽なら戻す。外している動画を返す。
///
/// 動画のファイルには触らない。
#[tauri::command]
pub fn set_video_excluded(
    videos: tauri::State<ExcludedVideos>,
    date: String,
    name: String,
    excluded: bool,
) -> Result<Vec<String>, String> {
    videos.set(&date, &name, excluded)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = "DJI_20261003050500_0004_D.MP4";
    const SECOND: &str = "DJI_20261003055412_0005_D.MP4";

    /// テストごとの一時フォルダの中のファイル。落ちても残骸が次の実行を邪魔しないよう、先に消す。
    fn temp_file(name: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("routelog-excluded-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (dir.join("excluded-videos.json"), dir)
    }

    #[test]
    fn a_video_left_out_is_still_out_the_next_time_the_file_is_opened() {
        let (path, dir) = temp_file("reopen");
        let videos = ExcludedVideos::open(path.clone());
        assert_eq!(videos.get("2026-10-03"), [] as [&str; 0]);
        assert_eq!(videos.set("2026-10-03", SECOND, true).unwrap(), [SECOND]);
        assert_eq!(
            videos.set("2026-10-03", FIRST, true).unwrap(),
            [FIRST, SECOND],
            "名前の順に返る"
        );

        assert_eq!(
            ExcludedVideos::open(path).get("2026-10-03"),
            [FIRST, SECOND]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_date_has_its_own_videos() {
        let (path, dir) = temp_file("dates");
        let videos = ExcludedVideos::open(path);
        videos.set("2026-10-03", FIRST, true).unwrap();
        videos.set("2026-10-10", SECOND, true).unwrap();
        assert_eq!(videos.get("2026-10-03"), [FIRST]);
        assert_eq!(videos.get("2026-10-10"), [SECOND]);
        assert_eq!(videos.get("2026-10-11"), [] as [&str; 0]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_video_put_back_is_forgotten_and_so_is_a_date_with_none_left() {
        let (path, dir) = temp_file("restore");
        let videos = ExcludedVideos::open(path.clone());
        videos.set("2026-10-03", FIRST, true).unwrap();
        videos.set("2026-10-10", SECOND, true).unwrap();
        assert_eq!(
            videos.set("2026-10-03", FIRST, false).unwrap(),
            [] as [&str; 0]
        );

        let json = std::fs::read_to_string(&path).unwrap();
        assert!(!json.contains("2026-10-03"), "{json}");
        assert_eq!(ExcludedVideos::open(path).get("2026-10-10"), [SECOND]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nothing_is_written_when_nothing_changes() {
        let (path, dir) = temp_file("unchanged");
        let videos = ExcludedVideos::open(path.clone());
        assert_eq!(
            videos.set("2026-10-03", FIRST, false).unwrap(),
            [] as [&str; 0]
        );
        assert!(!dir.exists(), "外していない動画を戻しても、何も書かない");

        videos.set("2026-10-03", FIRST, true).unwrap();
        std::fs::write(&path, "written by hand").unwrap();
        assert_eq!(videos.set("2026-10-03", FIRST, true).unwrap(), [FIRST]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "written by hand");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_cannot_be_written_is_not_remembered() {
        let (path, dir) = temp_file("unwritable");
        // 書き先と同じ名前のフォルダがあるので、ファイルを書けない。
        std::fs::create_dir_all(&path).unwrap();
        let videos = ExcludedVideos::open(path);
        assert!(videos.set("2026-10-03", FIRST, true).is_err());
        assert_eq!(videos.get("2026-10-03"), [] as [&str; 0]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_read_starts_from_nothing() {
        let (path, dir) = temp_file("broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{").unwrap();
        assert_eq!(
            ExcludedVideos::open(path).get("2026-10-03"),
            [] as [&str; 0]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
