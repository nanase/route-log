//! 停車の候補。動画を測った値と、外した候補を、動画 1 本ごとに手元に持つ。
//!
//! 動画は手元にしか無いので、クラウドへ送る走行のデータには入れず、アプリのデータとして持つ。
//! 動画はファイル名で見分ける。候補は、読むたびにいまの規則で測った値から出す。外した候補は
//! 区間で覚え、規則を変えて候補がずれても、重なる候補を外したままにする。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use routelog_core::VideoFile;
use routelog_video::StopMeter;
use routelog_video::stops::{self, Candidate, MEASURE_VERSION, Measured};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri::ipc::Channel;

use crate::overview::renew;

/// 動画 1 本ぶんの覚え。
#[derive(Serialize, Deserialize)]
struct Record {
    /// 測った動画の長さ。同じ名前の別の動画を取り違えないために持つ
    duration_ms: i64,
    measured: Measured,
    /// 外した候補
    #[serde(default)]
    rejected: Vec<Candidate>,
}

/// 動画 1 本の停車の候補。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileStops {
    /// 動画のファイル名
    pub name: String,
    pub candidates: Vec<Choice>,
}

/// 候補 1 つと、採るか。位置は動画の中のミリ秒。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Choice {
    pub start_ms: i64,
    pub end_ms: i64,
    pub taken: bool,
}

/// 停車の候補の覚えと、停車を探す ffmpeg。探すのは 1 度に 1 つで、新しく探し始めたら前の仕事はやめる。
pub struct Stops {
    dir: PathBuf,
    meter: StopMeter,
    /// 探している仕事を止める旗。新しく探し始めたら、前の仕事はやめる
    cancel: Mutex<Arc<AtomicBool>>,
    /// 覚えを読んで書き直す間、ほかに書かせない
    writing: Mutex<()>,
}

impl Stops {
    pub fn new(dir: PathBuf, ffmpeg_dir: Option<&Path>) -> Self {
        Self {
            dir,
            meter: StopMeter::find(ffmpeg_dir),
            cancel: Mutex::default(),
            writing: Mutex::default(),
        }
    }

    /// 覚えのファイル。動画のファイル名は画面から届くので、フォルダの外を指す名前は断る。
    fn path(&self, name: &str) -> Result<PathBuf, String> {
        if Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name) {
            return Err(format!("{name}: 動画のファイル名ではありません"));
        }
        Ok(self.dir.join(format!("{name}.json")))
    }

    /// その動画の覚え。測っていない、測り方が古い、長さが違うときは `None`。
    fn read(&self, file: &VideoFile) -> Option<Record> {
        let json = std::fs::read_to_string(self.path(&file.name).ok()?).ok()?;
        let record: Record = serde_json::from_str(&json).ok()?;
        (record.measured.version == MEASURE_VERSION && record.duration_ms == file.duration_ms)
            .then_some(record)
    }

    /// 書きかけのファイルを残さないよう、別名で書いてから置き換える。
    fn write(&self, name: &str, record: &Record) -> Result<(), String> {
        let path = self.path(name)?;
        let json = serde_json::to_string(record).map_err(|e| e.to_string())?;
        let io = |path: &Path| {
            let shown = path.display().to_string();
            move |e| format!("{shown}: {e}")
        };
        std::fs::create_dir_all(&self.dir).map_err(io(&self.dir))?;
        let partial = path.with_extension("json.part");
        std::fs::write(&partial, json).map_err(io(&partial))?;
        std::fs::rename(&partial, &path).map_err(io(&path))
    }

    /// その動画の候補。測っていなければ `None`。
    pub fn get(&self, file: &VideoFile) -> Option<FileStops> {
        self.read(file).map(|record| shown(&file.name, &record))
    }

    /// その動画で採った候補。書き出しで抜く区間である。測っていなければ空。
    pub fn taken(&self, file: &VideoFile) -> Vec<Candidate> {
        self.get(file)
            .map(|stops| {
                stops
                    .candidates
                    .into_iter()
                    .filter(|c| c.taken)
                    .map(|c| Candidate {
                        start_ms: c.start_ms,
                        end_ms: c.end_ms,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 測った値を覚える。前に外した候補は、そのまま外しておく。
    fn remember(&self, file: &VideoFile, measured: Measured) -> Result<FileStops, String> {
        let _writing = self.writing.lock().unwrap();
        let rejected = std::fs::read_to_string(self.path(&file.name)?)
            .ok()
            .and_then(|json| serde_json::from_str::<Record>(&json).ok())
            .map(|old| old.rejected)
            .unwrap_or_default();
        let record = Record {
            duration_ms: file.duration_ms,
            measured,
            rejected,
        };
        self.write(&file.name, &record)?;
        Ok(shown(&file.name, &record))
    }

    /// 候補を採るか外すかを決める。書けなかったときは、覚えも変えない。
    pub fn set_taken(
        &self,
        file: &VideoFile,
        candidate: Candidate,
        taken: bool,
    ) -> Result<FileStops, String> {
        let _writing = self.writing.lock().unwrap();
        let mut record = self
            .read(file)
            .ok_or_else(|| format!("{} の停車は、まだ探していません", file.name))?;
        record.rejected.retain(|r| !overlaps(r, &candidate));
        if !taken {
            record.rejected.push(candidate);
            record.rejected.sort_by_key(|r| r.start_ms);
        }
        self.write(&file.name, &record)?;
        Ok(shown(&file.name, &record))
    }

    /// 探していない動画を、順に測る。測れた動画は、その場で覚える。
    fn find(
        &self,
        files: &[VideoFile],
        progress: &mut dyn FnMut(f64),
        cancel: &AtomicBool,
    ) -> Result<Vec<FileStops>, String> {
        let todo: Vec<&VideoFile> = files.iter().filter(|f| self.read(f).is_none()).collect();
        // 長さに比例して進めて見せる。
        let total: f64 = todo.iter().map(|f| f.duration_ms.max(1) as f64).sum();
        let mut done = 0.0;
        for file in todo {
            let share = file.duration_ms.max(1) as f64;
            let measured = self
                .meter
                .measure(
                    Path::new(&file.path),
                    file.duration_ms,
                    &mut |part| progress((done + part * share) / total),
                    &|| cancel.load(Ordering::Relaxed),
                )
                .map_err(|e| format!("{}: {e}", file.name))?;
            self.remember(file, measured)?;
            done += share;
        }
        Ok(files.iter().filter_map(|f| self.get(f)).collect())
    }
}

fn overlaps(a: &Candidate, b: &Candidate) -> bool {
    a.start_ms < b.end_ms && b.start_ms < a.end_ms
}

fn shown(name: &str, record: &Record) -> FileStops {
    FileStops {
        name: name.to_owned(),
        candidates: stops::candidates(&record.measured)
            .into_iter()
            .map(|c| Choice {
                start_ms: c.start_ms,
                end_ms: c.end_ms,
                taken: !record.rejected.iter().any(|r| overlaps(r, &c)),
            })
            .collect(),
    }
}

/// 走行の動画のうち、停車を探してある動画の候補。
#[tauri::command]
pub fn get_stops(store: tauri::State<Stops>, files: Vec<VideoFile>) -> Vec<FileStops> {
    files.iter().filter_map(|f| store.get(f)).collect()
}

/// 走行の動画のうち、まだ探していない動画の停車を探す。進み具合（0〜1）を `on_progress` に送る。
///
/// 探してあった動画も含めて、候補を返す。動画のファイルは読むだけで、何も書かない。
#[tauri::command]
pub async fn find_stops(
    app: tauri::AppHandle,
    files: Vec<VideoFile>,
    on_progress: Channel<f64>,
) -> Result<Vec<FileStops>, String> {
    let cancel = renew(&app.state::<Stops>().cancel);
    tauri::async_runtime::spawn_blocking(move || {
        // 値は 1 秒ぶんの絵や音ごとに進む。画面へは 0.1% 進むごとに送る。
        let mut sent = -1.0;
        app.state::<Stops>().find(
            &files,
            &mut |done| {
                if done - sent >= 0.001 {
                    sent = done;
                    let _ = on_progress.send(done);
                }
            },
            &cancel,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 探すのをやめる。測り終えた動画の候補は残る。
#[tauri::command]
pub fn cancel_find_stops(store: tauri::State<Stops>) {
    store
        .cancel
        .lock()
        .expect("旗を持ったまま落ちていない")
        .store(true, Ordering::Relaxed);
}

/// 候補を採るか外すかを決める。その動画の候補を返す。
#[tauri::command]
pub fn set_stop_taken(
    store: tauri::State<Stops>,
    file: VideoFile,
    start_ms: i64,
    end_ms: i64,
    taken: bool,
) -> Result<FileStops, String> {
    store.set_taken(&file, Candidate { start_ms, end_ms }, taken)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME: &str = "DJI_20261003050500_0004_D.MP4";

    fn video(duration_s: i64) -> VideoFile {
        VideoFile {
            path: NAME.into(),
            name: NAME.into(),
            seq: 4,
            start_ms: 0,
            duration_ms: duration_s * 1000,
            codec: "hevc".into(),
        }
    }

    /// 2〜6 秒目と 20〜24 秒目に止まっていた、30 秒の動画を測った値。
    fn measured() -> Measured {
        let still = |s: usize| (2..7).contains(&s) || (20..25).contains(&s);
        Measured {
            version: MEASURE_VERSION,
            audio_db: (0..30)
                .map(|s| if still(s) { -14.0 } else { -17.5 })
                .collect(),
            coarse_pct: (0..30).map(|s| if still(s) { 0.1 } else { 30.0 }).collect(),
            fine_pct: (0..30)
                .map(|s| Some(if still(s) { 0.1 } else { 20.0 }))
                .collect(),
        }
    }

    fn candidate(start_s: i64, end_s: i64) -> Candidate {
        Candidate {
            start_ms: start_s * 1000,
            end_ms: end_s * 1000,
        }
    }

    fn choice(start_s: i64, end_s: i64, taken: bool) -> Choice {
        Choice {
            start_ms: start_s * 1000,
            end_ms: end_s * 1000,
            taken,
        }
    }

    /// テストごとの一時フォルダ。落ちても残骸が次の実行を邪魔しないよう、先に消す。
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("routelog-stops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn candidates_are_taken_by_default_and_kept_for_the_next_time() {
        let dir = temp_dir("reopen");
        let stops = Stops::new(dir.clone(), None);
        assert_eq!(stops.get(&video(30)), None, "まだ探していない");

        stops.remember(&video(30), measured()).unwrap();
        assert_eq!(
            Stops::new(dir.clone(), None)
                .get(&video(30))
                .unwrap()
                .candidates,
            [choice(2, 7, true), choice(20, 25, true)]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_rejected_candidate_stays_rejected_and_can_be_taken_back() {
        let dir = temp_dir("reject");
        let stops = Stops::new(dir.clone(), None);
        stops.remember(&video(30), measured()).unwrap();

        let after = stops.set_taken(&video(30), candidate(2, 7), false).unwrap();
        assert_eq!(
            after.candidates,
            [choice(2, 7, false), choice(20, 25, true)]
        );
        assert_eq!(
            Stops::new(dir.clone(), None).taken(&video(30)),
            [candidate(20, 25)]
        );

        let back = stops.set_taken(&video(30), candidate(2, 7), true).unwrap();
        assert_eq!(back.candidates, [choice(2, 7, true), choice(20, 25, true)]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_rejection_follows_the_candidate_when_the_rule_moves_its_edges() {
        let dir = temp_dir("moved");
        let stops = Stops::new(dir.clone(), None);
        stops.remember(&video(30), measured()).unwrap();
        // 前の規則で出ていた、少しずれた候補を外したことにする。
        stops.set_taken(&video(30), candidate(4, 9), false).unwrap();
        assert_eq!(
            stops.get(&video(30)).unwrap().candidates,
            [choice(2, 7, false), choice(20, 25, true)]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn measuring_again_keeps_the_rejections() {
        let dir = temp_dir("again");
        let stops = Stops::new(dir.clone(), None);
        stops.remember(&video(30), measured()).unwrap();
        stops.set_taken(&video(30), candidate(2, 7), false).unwrap();
        let again = stops.remember(&video(30), measured()).unwrap();
        assert_eq!(
            again.candidates,
            [choice(2, 7, false), choice(20, 25, true)]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_old_measurement_or_another_video_of_the_same_name_is_not_used() {
        let dir = temp_dir("stale");
        let stops = Stops::new(dir.clone(), None);
        stops.remember(&video(30), measured()).unwrap();
        assert_eq!(stops.get(&video(31)), None, "長さが違えば、別の動画");

        let old = Measured {
            version: MEASURE_VERSION - 1,
            ..measured()
        };
        stops.remember(&video(30), old).unwrap();
        assert_eq!(stops.get(&video(30)), None, "測り方が古い");
        assert!(stops.set_taken(&video(30), candidate(2, 7), false).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_cannot_be_written_is_not_remembered() {
        let dir = temp_dir("unwritable");
        let stops = Stops::new(dir.clone(), None);
        stops.remember(&video(30), measured()).unwrap();
        // 書きかけの名前と同じ名前のフォルダがあるので、ファイルを書けない。
        std::fs::create_dir_all(stops.path(NAME).unwrap().with_extension("json.part")).unwrap();

        assert!(stops.set_taken(&video(30), candidate(2, 7), false).is_err());
        assert_eq!(
            stops.get(&video(30)).unwrap().candidates,
            [choice(2, 7, true), choice(20, 25, true)]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_name_that_points_outside_the_folder_is_refused() {
        let dir = temp_dir("outside");
        let stops = Stops::new(dir.join("stops"), None);
        let outside = VideoFile {
            name: "../config".into(),
            ..video(30)
        };
        assert!(stops.remember(&outside, measured()).is_err());
        assert!(!dir.join("config.json").exists());
        assert_eq!(stops.get(&outside), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
