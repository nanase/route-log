//! ルートログのデータ型と、時刻の変換。
//!
//! 時刻はすべて Unix ミリ秒で持つ。入出力もスレッドも使わないので、
//! デスクトップアプリと Worker（wasm）の両方から使える。

pub mod episode;
pub mod estimate;
pub mod event;
pub mod export;
pub mod geo;
pub mod jst;
pub mod prefecture;
pub mod ride;
pub mod timeline;

pub use episode::{Episode, Extent, RECORDED_LIMIT_MS, recorded_ms};
pub use estimate::{Knot, Stop, clear_estimates, knots, position_at, refresh, stops, time_at};
pub use event::{Event, EventDetail, EventKind, EventStatus, RouteClass, RouteRef, carry_over};
pub use prefecture::route_name;
pub use ride::{Ride, RideDocument, Route, SegmentMode, Waypoint};
pub use timeline::{Gap, Position, RideVideos, Span, Timeline, VideoFile, group_by_date};
