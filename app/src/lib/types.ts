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

/** 停車の候補 1 つと、採るか。位置は動画の中のミリ秒。Rust 側の `Choice` と同じ。 */
export interface StopChoice {
  start_ms: number;
  end_ms: number;
  taken: boolean;
}

/** 動画 1 本の停車の候補。Rust 側の `FileStops` と同じ。 */
export interface FileStops {
  /** 動画のファイル名 */
  name: string;
  candidates: StopChoice[];
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
  /** 走行に付けた目印（「投稿済」など）。付けた順 */
  labels: string[];
  created_at: string;
  updated_at: string;
}

export interface Waypoint {
  /** UUIDv7。回の端を指すのに使う。ID の無い頃の記録には、Rust 側が読むときに振る */
  id: string;
  lon: number;
  lat: number;
  /** 次の経由点までの結び方。最後の経由点では使わない */
  segment_mode: SegmentMode;
}

export interface Route {
  /** `segments[i]` は、経由点 `i` と `i + 1` の間の頂点。直線なら空 */
  segments: LonLat[][];
}

export type EventKind =
  | "departure"
  | "arrival"
  | "pref_boundary"
  | "muni_boundary"
  | "road"
  | "bridge"
  | "tunnel"
  | "michi_no_eki"
  | "waypoint"
  | "stop"
  | "custom";

/** 時刻が推定か、人が確定したか。字幕にするかどうかは、これとは別に `hidden` で持つ。 */
export type EventStatus = "estimated" | "confirmed";

export interface RouteRef {
  class: "national" | "prefectural";
  /** 都道府県道の所属。国道では無い */
  pref?: string;
  number: number;
}

/** イベント。DOM の Event と紛れないよう、こちらは RideEvent と呼ぶ。 */
export interface RideEvent {
  id: string;
  kind: EventKind;
  label: string;
  /** 経路の始点からの距離（m） */
  route_m: number;
  time_ms: number | null;
  status: EventStatus;
  /** 字幕にしない。時刻を確定してあれば、字幕にしなくても時刻推定の基準になる */
  hidden?: boolean;
  detail: {
    routes?: RouteRef[];
    end_m?: number;
    /** 抽出したときの名前 */
    extracted_label?: string;
    /** 動画を見て確定したときの、カメラの時計での時刻 */
    camera_ms?: number;
    /** 位置ではなく時刻が分かっているイベント */
    by_time?: boolean;
    /** 経由点のイベントの、その経由点の位置。停車のイベントでは、地図で決めた停車の場所 */
    at?: LonLat;
    /** 停車のイベントの、停車していた長さ（ミリ秒）。終わりの時刻は、通過時刻にこれを足したもの */
    stay_ms?: number;
    /** 橋・トンネルの、国土交通省のデータベースの施設の名前。何件かをまとめたときは「・」で繋ぐ */
    official_name?: string;
    /** 橋・トンネルの公称の長さ（m） */
    official_length_m?: number;
    /** 人が字幕にするかどうかを切り替えた */
    hidden_by_hand?: boolean;
    /** 人が名前を直したか、候補から選んだ */
    named_by_hand?: boolean;
  };
}

/** 回。YouTube に上げる 1 本ぶんの範囲で、経路の一部を始まりと終わりの経由点で決める。 */
export interface Episode {
  id: string;
  /** 回の名前。空なら、並びの番号で呼ぶ */
  title: string;
  /** 始まりの経由点の ID */
  from: string;
  /** 終わりの経由点の ID */
  to: string;
}

/** 回を Resolve へ書き出した結果。Rust 側の `Exported` と同じ。 */
export type Exported =
  | {
      outcome: "written";
      /** 書いたファイルの名前 */
      fcpxml: string;
      edl: string;
      /** タイムラインの長さ */
      lengthMs: number;
      /** 録画の中で抜いた停車の数と長さ */
      cuts: number;
      cutMs: number;
      markers: number;
      /** 録画の外か、抜いた所にあって、マーカーにできなかったイベントの数 */
      unplaced: number;
    }
  | {
      /** 同じ名前のファイルがあるので、書かなかった */
      outcome: "exists";
      names: string[];
    };

export interface RideDocument {
  ride: Ride;
  waypoints: Waypoint[];
  route: Route;
  events: RideEvent[];
  episodes: Episode[];
}

export interface RideSummary {
  id: string;
  title: string;
  ride_date: string;
  updated_at: string;
  waypoints: number;
  length_m: number;
  labels: string[];
}

export interface Config {
  video_dir: string | null;
  road_data_dir: string | null;
  geo_data_dir: string | null;
}

/** Resolve へ書き出すマーカーを、種類ごとに前へずらす長さ（ms）。分け方はマーカーの色と同じ。 */
export interface MarkerLeads {
  endpoint: number;
  road: number;
  boundary: number;
  bridge: number;
  tunnel: number;
  michi_no_eki: number;
  other: number;
}

/** 地図に重ねる道路データ（PMTiles）の場所。 */
export interface RoadData {
  national: string;
  prefectural: string;
}
