//! 動画ファイルから、実時刻の軸に並べるための情報と、停車の候補を取り出す。
//!
//! 開始時刻はファイル名から、長さは ffprobe から得る。停車は、ffmpeg で音と映像の動きを測って
//! 見分ける。

mod meter;
mod name;
mod probe;
mod scan;
pub mod stops;

pub use meter::StopMeter;
pub use name::{NamePattern, ParsedName};
pub use probe::{
    Ffprobe, Probe, dir_on_path, find_ffmpeg, find_in, find_program, hide_console, parse_media,
    parse_probe,
};
pub use scan::{Scan, Skipped, scan};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ファイル名の正規表現が正しくありません: {0}")]
    Pattern(#[from] regex::Error),
    #[error("ファイル名の正規表現に名前付きグループ {0} がありません")]
    MissingGroup(&'static str),
    #[error("{program} を起動できません: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("ffprobe が失敗しました: {0}")]
    Ffprobe(String),
    #[error("ffprobe の出力を読めません: {0}")]
    Output(String),
    #[error("ffmpeg が失敗しました: {0}")]
    Ffmpeg(String),
    #[error("やめました")]
    Cancelled,
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}
