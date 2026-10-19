//! 道路・行政区域のデータを読み、経路からイベントを抽出する。
//!
//! データは `pipeline/export_geo.py` が書き出した FlatGeobuf である。

mod data;
mod extract;
mod snap;

pub use data::{Area, BBox, GeoData, Road, Station, Structure};
pub use extract::{Options, Prepared, Tiles};
pub use snap::SnapOptions;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} を開けません: {1}")]
    Open(String, std::io::Error),
    #[error("{0} を読めません: {1}")]
    Read(String, String),
}
