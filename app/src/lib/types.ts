/** Rust 側（routelog-core）の型の写し。時刻はすべて Unix ミリ秒。 */

export interface VideoFile {
  path: string;
  name: string;
  seq: number;
  start_ms: number;
  duration_ms: number;
  codec: string;
}

export interface Span {
  file: number;
  start_ms: number;
  end_ms: number;
}

export interface Gap {
  start_ms: number;
  end_ms: number;
}

export interface Position {
  file: number;
  offset_ms: number;
}

export interface Timeline {
  files: VideoFile[];
  clock_offset_ms: number;
  spans: Span[];
  gaps: Gap[];
}

export interface RideVideos {
  date: string;
  files: VideoFile[];
}

export interface Scan {
  rides: RideVideos[];
  skipped: { path: string; reason: string }[];
}

/** 経度・緯度（度）。GeoJSON と同じ順。 */
export type LonLat = [number, number];

export type SegmentMode = "straight" | "snap" | "manual";

export interface Ride {
  id: string;
  title: string;
  ride_date: string;
  clock_offset_ms: number;
  is_public: boolean;
  created_at: string;
  updated_at: string;
}

export interface Waypoint {
  lon: number;
  lat: number;
  /** 次の経由点までの結び方。最後の経由点では使わない */
  segment_mode: SegmentMode;
}

export interface Route {
  /** `segments[i]` は、経由点 `i` と `i + 1` の間の頂点。直線なら空 */
  segments: LonLat[][];
}

export interface RideDocument {
  ride: Ride;
  waypoints: Waypoint[];
  route: Route;
}

export interface RideSummary {
  id: string;
  title: string;
  ride_date: string;
  updated_at: string;
  waypoints: number;
  length_m: number;
}

export interface Config {
  video_dir: string | null;
  road_data_dir: string | null;
}

/** 地図に重ねる道路データ（PMTiles）の場所。 */
export interface RoadData {
  national: string;
  prefectural: string;
}
