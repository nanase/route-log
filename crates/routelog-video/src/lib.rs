//! 動画ファイルから、実時刻の軸に並べるための情報を取り出す。
//!
//! 開始時刻はファイル名から、長さは ffprobe から得る。

mod name;
mod probe;
mod scan;

pub use name::{NamePattern, ParsedName};
pub use probe::{Ffprobe, Probe, hide_console, parse_probe, program_from_env};
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
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}
