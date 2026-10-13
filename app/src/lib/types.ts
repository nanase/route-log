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
