use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use routelog_core::VideoFile;

use crate::{Error, Ffprobe, NamePattern, ParsedName};

/// ffprobe を同時に走らせる数の上限。動画は 1 台のディスクにあるので、増やしても速くならない。
const MAX_WORKERS: usize = 8;

/// 形は合ったが読めなかったファイル。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

/// フォルダを読んだ結果。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Scan {
    /// 開始時刻の順。
    pub files: Vec<VideoFile>,
    pub skipped: Vec<Skipped>,
}

/// フォルダの下から、名前が形に合う動画を集める。下のフォルダも読む。
///
/// ファイルを開くのは ffprobe だけで、何も書き込まない。
pub fn scan(dir: &Path, pattern: &NamePattern, ffprobe: &Ffprobe) -> Result<Scan, Error> {
    let mut candidates = Vec::new();
    collect(dir, pattern, &mut candidates)?;

    let next = AtomicUsize::new(0);
    let files = Mutex::new(Vec::new());
    let skipped = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_WORKERS);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some((path, parsed)) =
                    candidates.get(next.fetch_add(1, Ordering::Relaxed))
                {
                    let shown = path.to_string_lossy().into_owned();
                    match ffprobe.run(path) {
                        Ok(probe) => files.lock().unwrap().push(VideoFile {
                            name: file_name(path),
                            path: shown,
                            seq: parsed.seq,
                            start_ms: parsed.start_ms,
                            duration_ms: probe.duration_ms,
                            codec: probe.codec,
                        }),
                        Err(error) => skipped.lock().unwrap().push(Skipped {
                            path: shown,
                            reason: error.to_string(),
                        }),
                    }
                }
            });
        }
    });

    let mut files = files.into_inner().unwrap();
    files.sort_by_key(|f| (f.start_ms, f.seq));
    let mut skipped = skipped.into_inner().unwrap();
    skipped.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Scan { files, skipped })
}

fn collect(
    dir: &Path,
    pattern: &NamePattern,
    found: &mut Vec<(PathBuf, ParsedName)>,
) -> Result<(), Error> {
    let io = |source| Error::Io {
        path: dir.to_string_lossy().into_owned(),
        source,
    };
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        let path = entry.path();
        if entry.file_type().map_err(io)?.is_dir() {
            collect(&path, pattern, found)?;
        } else if let Some(parsed) = pattern.parse(&file_name(&path)) {
            found.push((path, parsed));
        }
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テストごとの一時フォルダ。落ちても残骸が次の実行を邪魔しないよう、作る前に消す。
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("routelog-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        dir
    }

    #[test]
    fn a_file_ffprobe_cannot_read_is_reported_and_others_are_ignored() {
        let dir = temp_dir("skipped");
        std::fs::write(
            dir.join("sub").join("DJI_20261003050500_0004_D.MP4"),
            b"not a video",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), b"").unwrap();

        let ffprobe = Ffprobe::new("routelog-no-such-ffprobe");
        let scan = scan(&dir, &NamePattern::dji_action4(), &ffprobe).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(scan.files.is_empty());
        assert_eq!(scan.skipped.len(), 1);
        assert!(
            scan.skipped[0]
                .path
                .ends_with("DJI_20261003050500_0004_D.MP4")
        );
        assert!(scan.skipped[0].reason.contains("routelog-no-such-ffprobe"));
    }

    #[test]
    fn a_folder_that_does_not_exist_is_an_error() {
        let dir = std::env::temp_dir().join("routelog-scan-no-such-folder");
        let result = scan(&dir, &NamePattern::dji_action4(), &Ffprobe::from_env());
        assert!(matches!(result, Err(Error::Io { .. })));
    }
}
