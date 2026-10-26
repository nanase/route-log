//! 走行ごとの動画フォルダの覚え。
//!
//! 動画は走行ごとに別のフォルダへ置くことがあるので、日付ごとにフォルダを覚える。フォルダの場所は
//! この PC だけのものなので、クラウドへ送る走行のデータには入れず、アプリのデータとして持つ。
//! 走行の一覧に本数と長さを出せるよう、最後に読んだ動画も覚える。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use routelog_core::VideoFile;
use serde::{Deserialize, Serialize};

/// 覚えた動画フォルダ 1 つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub dir: String,
    /// 最後に読んだときの、その日付の動画。
    pub files: Vec<VideoFile>,
}

/// 日付（`YYYY-MM-DD`）ごとの動画フォルダ。覚えた順。
type Dates = BTreeMap<String, Vec<Folder>>;

/// 走行の一覧に出す、日付ごとの動画の要約。最後に読んだときの動画から数える。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub count: usize,
    pub duration_ms: i64,
    /// 見つからないフォルダがあるか。
    pub missing: bool,
}

pub struct VideoFolders {
    path: PathBuf,
    dates: Mutex<Dates>,
}

impl VideoFolders {
    /// 読めないときは、何も覚えていない状態から始める。
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

    /// その日付の走行の動画フォルダ。覚えた順。
    pub fn get(&self, date: &str) -> Vec<Folder> {
        self.dates
            .lock()
            .unwrap()
            .get(date)
            .cloned()
            .unwrap_or_default()
    }

    /// フォルダとその中の動画を覚える。覚えていたフォルダなら、動画だけを差し替える。
    pub fn remember(&self, date: &str, dir: &str, files: Vec<VideoFile>) -> Result<(), String> {
        self.change(|dates| {
            let folders = dates.entry(date.to_owned()).or_default();
            match folders.iter_mut().find(|folder| folder.dir == dir) {
                Some(folder) => folder.files = files,
                None => folders.push(Folder {
                    dir: dir.to_owned(),
                    files,
                }),
            }
        })
    }

    /// 日付ごとの動画を、まだ覚えていないフォルダのときだけ覚える。
    pub fn remember_new(
        &self,
        dir: &str,
        dates: Vec<(String, Vec<VideoFile>)>,
    ) -> Result<(), String> {
        self.change(|known| {
            for (date, files) in dates {
                let folders = known.entry(date).or_default();
                if !folders.iter().any(|folder| folder.dir == dir) {
                    folders.push(Folder {
                        dir: dir.to_owned(),
                        files,
                    });
                }
            }
        })
    }

    /// フォルダを忘れる。動画のファイルには触らない。
    pub fn forget(&self, date: &str, dir: &str) -> Result<(), String> {
        self.change(|dates| {
            if let Some(folders) = dates.get_mut(date) {
                folders.retain(|folder| folder.dir != dir);
            }
        })
    }

    /// 動画フォルダを覚えている日付ごとの要約。見つからないフォルダの動画は数えない。同じ名前の動画は
    /// 1 本と数える。
    pub fn summaries(&self) -> BTreeMap<String, Summary> {
        self.dates
            .lock()
            .unwrap()
            .iter()
            .map(|(date, folders)| {
                let mut summary = Summary {
                    count: 0,
                    duration_ms: 0,
                    missing: false,
                };
                let mut names = BTreeSet::new();
                for folder in folders {
                    if !Path::new(&folder.dir).is_dir() {
                        summary.missing = true;
                        continue;
                    }
                    for file in &folder.files {
                        if names.insert(&file.name) {
                            summary.count += 1;
                            summary.duration_ms += file.duration_ms;
                        }
                    }
                }
                (date.clone(), summary)
            })
            .collect()
    }

    /// 覚えを変えて書く。書けなかったときは、覚えも変えない。何も変わらなければ、何も書かない。
    fn change(&self, edit: impl FnOnce(&mut Dates)) -> Result<(), String> {
        let mut dates = self.dates.lock().unwrap();
        let mut next = dates.clone();
        edit(&mut next);
        // フォルダが無くなった日付は、残さない。
        next.retain(|_, folders| !folders.is_empty());
        if next == *dates {
            return Ok(());
        }
        let json = serde_json::to_string_pretty(&next).map_err(|e| e.to_string())?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&self.path, json).map_err(|e| format!("{}: {e}", self.path.display()))?;
        *dates = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATE: &str = "2026-10-03";

    /// テストごとの一時フォルダ。落ちても残骸が次の実行を邪魔しないよう、先に消す。
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("routelog-folders-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn video(seq: u32, duration_s: i64) -> VideoFile {
        let name = format!("DJI_20261003050500_{seq:04}_D.MP4");
        VideoFile {
            path: name.clone(),
            name,
            seq,
            start_ms: 0,
            duration_ms: duration_s * 1000,
            codec: "hevc".into(),
        }
    }

    #[test]
    fn a_folder_is_still_there_the_next_time_the_file_is_opened() {
        let dir = temp_dir("reopen");
        let path = dir.join("video-folders.json");
        let folders = VideoFolders::open(path.clone());
        folders.remember(DATE, "first", vec![video(4, 60)]).unwrap();
        folders
            .remember(DATE, "second", vec![video(5, 30)])
            .unwrap();

        let reopened = VideoFolders::open(path).get(DATE);
        let dirs: Vec<_> = reopened.iter().map(|folder| folder.dir.as_str()).collect();
        assert_eq!(dirs, ["first", "second"], "覚えた順に返る");
        assert_eq!(reopened[0].files, [video(4, 60)]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remembering_a_known_folder_again_only_replaces_its_videos() {
        let dir = temp_dir("again");
        let folders = VideoFolders::open(dir.join("video-folders.json"));
        folders.remember(DATE, "first", vec![video(4, 60)]).unwrap();
        folders
            .remember(DATE, "second", vec![video(5, 30)])
            .unwrap();
        folders.remember(DATE, "first", vec![]).unwrap();

        let got = folders.get(DATE);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].dir, "first");
        assert!(got[0].files.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_folder_forgotten_is_gone_and_so_is_a_date_with_none_left() {
        let dir = temp_dir("forget");
        let path = dir.join("video-folders.json");
        let folders = VideoFolders::open(path.clone());
        folders.remember(DATE, "first", vec![video(4, 60)]).unwrap();
        folders.remember("2026-10-10", "other", vec![]).unwrap();
        folders.forget(DATE, "first").unwrap();

        assert!(folders.get(DATE).is_empty());
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(!json.contains(DATE), "{json}");
        assert_eq!(VideoFolders::open(path).get("2026-10-10").len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remember_new_leaves_folders_already_known_as_they_are() {
        let dir = temp_dir("new");
        let folders = VideoFolders::open(dir.join("video-folders.json"));
        folders.remember(DATE, "old", vec![video(4, 60)]).unwrap();
        folders
            .remember_new(
                "old",
                vec![
                    (DATE.to_owned(), vec![video(9, 10)]),
                    ("2026-10-10".to_owned(), vec![video(1, 10)]),
                ],
            )
            .unwrap();

        assert_eq!(folders.get(DATE)[0].files, [video(4, 60)]);
        assert_eq!(folders.get("2026-10-10")[0].files, [video(1, 10)]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_summary_counts_each_video_once_and_tells_a_folder_that_is_gone() {
        let dir = temp_dir("summary");
        let here = dir.join("here");
        std::fs::create_dir_all(&here).unwrap();
        let folders = VideoFolders::open(dir.join("video-folders.json"));
        let here = here.to_string_lossy();
        folders
            .remember(DATE, &here, vec![video(4, 60), video(5, 30)])
            .unwrap();
        let there = dir.join("there");
        std::fs::create_dir_all(&there).unwrap();
        folders
            .remember(DATE, &there.to_string_lossy(), vec![video(5, 30)])
            .unwrap();
        folders
            .remember("2026-10-10", &here, vec![video(1, 10)])
            .unwrap();
        let gone = dir.join("gone").to_string_lossy().into_owned();
        folders
            .remember("2026-10-10", &gone, vec![video(2, 20)])
            .unwrap();

        let summaries = folders.summaries();
        assert_eq!(
            summaries[DATE],
            Summary {
                count: 2,
                duration_ms: 90_000,
                missing: false
            }
        );
        assert_eq!(
            summaries["2026-10-10"],
            Summary {
                count: 1,
                duration_ms: 10_000,
                missing: true
            },
            "見つからないフォルダの動画は数えない"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nothing_is_written_when_nothing_changes() {
        let dir = temp_dir("unchanged");
        let path = dir.join("video-folders.json");
        let folders = VideoFolders::open(path.clone());
        folders.forget(DATE, "first").unwrap();
        assert!(
            !dir.exists(),
            "覚えていないフォルダを忘れても、何も書かない"
        );

        folders.remember(DATE, "first", vec![video(4, 60)]).unwrap();
        std::fs::write(&path, "written by hand").unwrap();
        folders.remember(DATE, "first", vec![video(4, 60)]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "written by hand");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_cannot_be_written_is_not_remembered() {
        let dir = temp_dir("unwritable");
        let path = dir.join("video-folders.json");
        // 書き先と同じ名前のフォルダがあるので、ファイルを書けない。
        std::fs::create_dir_all(&path).unwrap();
        let folders = VideoFolders::open(path);
        assert!(folders.remember(DATE, "first", vec![]).is_err());
        assert!(folders.get(DATE).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
