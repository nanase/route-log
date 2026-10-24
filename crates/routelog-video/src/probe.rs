use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::Error;

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
        let mut command = Command::new(&self.program);
        command
            .args(["-v", "error", "-print_format", "json"])
            // サムネイルも映像のストリームとして付いているので、先頭の 1 本に絞る。
            .args(["-select_streams", "v:0"])
            .args(["-show_entries", "format=duration:stream=codec_name"])
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
        parse_probe(&String::from_utf8_lossy(&output.stdout))
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
