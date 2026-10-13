//! ルートログのデータ型と、時刻の変換。
//!
//! 時刻はすべて Unix ミリ秒で持つ。入出力もスレッドも使わないので、
//! デスクトップアプリと Worker（wasm）の両方から使える。

pub mod estimate;
pub mod event;
pub mod geo;
pub mod jst;
pub mod ride;
pub mod timeline;

pub use estimate::{Knot, knots, position_at, refresh, time_at};
pub use event::{Event, EventDetail, EventKind, EventStatus, RouteClass, RouteRef, carry_over};
pub use ride::{Ride, RideDocument, Route, SegmentMode, Waypoint};
pub use timeline::{Gap, Position, RideVideos, Span, Timeline, VideoFile, group_by_date};
