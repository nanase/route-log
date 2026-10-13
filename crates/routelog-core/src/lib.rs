//! ルートログのデータ型と、時刻の変換。
//!
//! 時刻はすべて Unix ミリ秒で持つ。入出力もスレッドも使わないので、
//! デスクトップアプリと Worker（wasm）の両方から使える。

pub mod geo;
pub mod jst;
pub mod ride;
pub mod timeline;

pub use ride::{Ride, RideDocument, Route, SegmentMode, Waypoint};
pub use timeline::{Gap, Position, RideVideos, Span, Timeline, VideoFile, group_by_date};
