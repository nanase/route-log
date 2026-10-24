use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use routelog_core::resolve::Media;
use serde::Deserialize;

use crate::Error;
use crate::scan::workers;

/// ffprobe の場所を上書きする環境変数。
const PROGRAM_ENV: &str = "ROUTELOG_FFPROBE";
/// ffmpeg の場所を上書きする環境変数。
const FFMPEG_ENV: &str = "ROUTELOG_FFMPEG";

/// 動画ファイル 1 本について ffprobe が答えたこと。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub duration_ms: i64,
    /// 映像のコーデック。`hevc` など。
    pub codec: String,
}

#[derive(Deserialize)]
struct Output {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    codec_name: Option<String>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

/// `ffprobe -print_format json` の出力を読む。
pub fn parse_probe(json: &str) -> Result<Probe, Error> {
    let output: Output = serde_json::from_str(json).map_err(|e| Error::Output(e.to_string()))?;
    let seconds: f64 = output
        .format
        .and_then(|f| f.duration)
        .ok_or_else(|| Error::Output("長さがありません".into()))?
        .parse()
        .map_err(|_| Error::Output("長さが数ではありません".into()))?;
    let codec = output
        .streams
        .into_iter()
        .find_map(|s| s.codec_name)
        .ok_or_else(|| Error::Output("映像のストリームがありません".into()))?;
    Ok(Probe {
        duration_ms: (seconds * 1000.0).round() as i64,
        codec,
    })
}

#[derive(Deserialize)]
struct MediaOutput {
    #[serde(default)]
    streams: Vec<MediaStream>,
}

#[derive(Deserialize)]
struct MediaStream {
    codec_type: Option<String>,
    r_frame_rate: Option<String>,
    nb_frames: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u32>,
    #[serde(default)]
    disposition: Disposition,
    #[serde(default)]
    tags: Tags,
}

#[derive(Default, Deserialize)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Default, Deserialize)]
struct Tags {
    timecode: Option<String>,
}

/// Resolve へ書き出すために、`ffprobe -print_format json` の出力から映像と音の中身を読む。
pub fn parse_media(json: &str) -> Result<Media, Error> {
    let output: MediaOutput =
        serde_json::from_str(json).map_err(|e| Error::Output(e.to_string()))?;
    // サムネイルも映像のストリームとして付いている。
    let video = output
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video") && s.disposition.attached_pic == 0)
        .ok_or_else(|| Error::Output("映像のストリームがありません".into()))?;
    let frame_rate = video
        .r_frame_rate
        .as_deref()
        .and_then(|rate| rate.split_once('/'))
        .and_then(|(num, den)| Some((num.parse().ok()?, den.parse().ok()?)))
        .ok_or_else(|| Error::Output("フレームレートがありません".into()))?;
    let frames = video
        .nb_frames
        .as_deref()
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| Error::Output("フレームの数がありません".into()))?;
    let (Some(width), Some(height)) = (video.width, video.height) else {
        return Err(Error::Output("映像の大きさがありません".into()));
    };
    let audio_channels = output
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .and_then(|s| s.channels)
        .unwrap_or(0);
    // タイムコードは映像のストリームに付く。付いていなければ、タイムコードのトラックのものを使う。
    let timecode = video
        .tags
        .timecode
        .clone()
        .or_else(|| output.streams.iter().find_map(|s| s.tags.timecode.clone()));
    Ok(Media {
        frame_rate,
        frames,
        width,
        height,
        audio_channels,
        timecode,
    })
}

/// ffprobe の呼び出し口。
#[derive(Debug, Clone)]
pub struct Ffprobe {
    program: OsString,
}

impl Ffprobe {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// 環境変数 `ROUTELOG_FFPROBE`、フォルダ `dir` の中、PATH の順に探す。
    pub fn find(dir: Option<&Path>) -> Self {
        Self::new(find_program(PROGRAM_ENV, "ffprobe", dir))
    }

    pub fn run(&self, path: &Path) -> Result<Probe, Error> {
        let json = self.json(
            path,
            // サムネイルも映像のストリームとして付いているので、先頭の 1 本に絞る。
            &[
                "-select_streams",
                "v:0",
                "-show_entries",
                "format=duration:stream=codec_name",
            ],
        )?;
        parse_probe(&json)
    }

    /// Resolve へ書き出すために、映像と音の中身を読む。
    pub fn media(&self, path: &Path) -> Result<Media, Error> {
        let entries = "stream=codec_type,r_frame_rate,nb_frames,width,height,channels\
                       :stream_disposition=attached_pic:stream_tags=timecode";
        parse_media(&self.json(path, &["-show_entries", entries])?)
    }

    /// 何本かの動画を、並べて `media` で読む。結果は `paths` と同じ順に並ぶ。
    pub fn media_all(&self, paths: &[&Path]) -> Vec<Result<Media, Error>> {
        let next = AtomicUsize::new(0);
        let read = Mutex::new(Vec::with_capacity(paths.len()));
        std::thread::scope(|scope| {
            for _ in 0..workers() {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = paths.get(index) else { break };
                        read.lock().unwrap().push((index, self.media(path)));
                    }
                });
            }
        });
        let mut read = read.into_inner().unwrap();
        read.sort_by_key(|(index, _)| *index);
        read.into_iter().map(|(_, media)| media).collect()
    }

    /// ffprobe に `path` を JSON で答えさせる。
    fn json(&self, path: &Path, args: &[&str]) -> Result<String, Error> {
        let mut command = Command::new(&self.program);
        command
            .args(["-v", "error", "-print_format", "json"])
            .args(args)
            .arg(path);
        hide_console(&mut command);
        let output = command.output().map_err(|source| Error::Spawn {
            program: self.program.to_string_lossy().into_owned(),
            source,
        })?;
        if !output.status.success() {
            return Err(Error::Ffprobe(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// ffmpeg の場所。環境変数 `ROUTELOG_FFMPEG`、フォルダ `dir` の中、PATH の順に探す。
pub fn find_ffmpeg(dir: Option<&Path>) -> OsString {
    find_program(FFMPEG_ENV, "ffmpeg", dir)
}

/// 外の道具の場所。環境変数 `env`、フォルダ `dir` の中、PATH の順に探す。どこにも無ければ名前だけを返す。
pub fn find_program(env: &str, name: &str, dir: Option<&Path>) -> OsString {
    std::env::var_os(env)
        .or_else(|| dir.and_then(|dir| find_in(dir, name)).map(Into::into))
        .or_else(|| find_in(&dir_on_path(name)?, name).map(Into::into))
        .unwrap_or_else(|| name.into())
}

/// PATH の中で、実行ファイル `name` が最初に見つかるフォルダ。
pub fn dir_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find(|dir| find_in(dir, name).is_some())
}

/// フォルダの中の実行ファイル。
///
/// Windows の `Command` は名前に `.exe` だけを補う。道具の管理ソフトは `.cmd` の
/// 形で置くことがあるので、それも探す。
#[cfg(windows)]
pub fn find_in(dir: &Path, name: &str) -> Option<PathBuf> {
    ["exe", "cmd", "bat"]
        .map(|ext| dir.join(name).with_extension(ext))
        .into_iter()
        .find(|candidate| candidate.is_file())
}

#[cfg(not(windows))]
pub fn find_in(dir: &Path, name: &str) -> Option<PathBuf> {
    Some(dir.join(name)).filter(|candidate| candidate.is_file())
}

/// ウィンドウを持つアプリから呼ぶと、ファイルごとにコンソールが一瞬開く。それを出さない。
#[cfg(windows)]
pub fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_duration_and_the_codec() {
        let json = r#"{
            "programs": [],
            "streams": [{ "codec_name": "hevc" }],
            "format": { "duration": "280.192000" }
        }"#;
        assert_eq!(
            parse_probe(json).unwrap(),
            Probe {
                duration_ms: 280_192,
                codec: "hevc".into()
            }
        );
    }

    /// ffprobe の出力を縮めたもの。値は作り物である。
    const MEDIA: &str = r#"{
        "streams": [
            { "index": 0, "codec_type": "video", "width": 1920, "height": 1080,
              "r_frame_rate": "60000/1001", "nb_frames": "216000",
              "disposition": { "attached_pic": 0 }, "tags": { "timecode": "05:05:26;07" } },
            { "index": 1, "codec_type": "audio", "channels": 1, "r_frame_rate": "0/0",
              "disposition": { "attached_pic": 0 }, "tags": {} },
            { "index": 4, "codec_type": "data", "r_frame_rate": "0/0", "nb_frames": "1",
              "disposition": { "attached_pic": 0 }, "tags": { "timecode": "05:05:26;07" } },
            { "index": 5, "codec_type": "video", "width": 1280, "height": 720,
              "r_frame_rate": "90000/1", "disposition": { "attached_pic": 1 } }
        ]
    }"#;

    #[test]
    fn reads_what_resolve_needs() {
        assert_eq!(
            parse_media(MEDIA).unwrap(),
            Media {
                frame_rate: (60000, 1001),
                frames: 216_000,
                width: 1920,
                height: 1080,
                audio_channels: 1,
                timecode: Some("05:05:26;07".into()),
            }
        );
    }

    #[test]
    fn the_timecode_track_stands_in_and_no_audio_is_zero_channels() {
        let json = r#"{
            "streams": [
                { "codec_type": "video", "width": 1920, "height": 1080,
                  "r_frame_rate": "60000/1001", "nb_frames": "100", "tags": {} },
                { "codec_type": "data", "tags": { "timecode": "05:05:26;07" } }
            ]
        }"#;
        let media = parse_media(json).unwrap();
        assert_eq!(media.timecode.as_deref(), Some("05:05:26;07"));
        assert_eq!(media.audio_channels, 0);
    }

    #[test]
    fn several_files_come_back_in_the_order_asked() {
        let ffprobe = Ffprobe::new("routelog-no-such-ffprobe");
        let read = ffprobe.media_all(&[Path::new("a.MP4"), Path::new("b.MP4")]);
        assert_eq!(read.len(), 2);
        assert!(read.iter().all(|r| matches!(r, Err(Error::Spawn { .. }))));
    }

    #[test]
    fn a_thumbnail_alone_is_not_the_video() {
        let json = r#"{ "streams": [{ "codec_type": "video", "width": 1280, "height": 720,
            "r_frame_rate": "90000/1", "disposition": { "attached_pic": 1 } }] }"#;
        assert!(matches!(parse_media(json), Err(Error::Output(_))));
    }

    #[test]
    fn a_file_without_video_is_an_error() {
        let json = r#"{ "streams": [], "format": { "duration": "1.0" } }"#;
        assert!(matches!(parse_probe(json), Err(Error::Output(_))));
    }

    #[test]
    fn a_file_without_a_duration_is_an_error() {
        let json = r#"{ "streams": [{ "codec_name": "hevc" }], "format": {} }"#;
        assert!(matches!(parse_probe(json), Err(Error::Output(_))));
    }

    #[test]
    fn a_missing_program_says_which_one() {
        let error = Ffprobe::new("routelog-no-such-ffprobe")
            .run(Path::new("x.MP4"))
            .unwrap_err();
        assert!(error.to_string().contains("routelog-no-such-ffprobe"));
    }

    #[test]
    fn a_program_in_the_folder_is_used() {
        let dir = std::env::temp_dir().join(format!("routelog-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir
            .join("routelog-tool")
            .with_extension(std::env::consts::EXE_EXTENSION);
        std::fs::write(&program, "").unwrap();

        let found = find_program("ROUTELOG_NO_SUCH_ENV", "routelog-tool", Some(&dir));
        let missing = find_program("ROUTELOG_NO_SUCH_ENV", "routelog-no-such-tool", Some(&dir));
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(found, program.into_os_string());
        assert_eq!(missing, "routelog-no-such-tool");
    }
}
