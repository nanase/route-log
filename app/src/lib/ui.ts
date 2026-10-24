/** 画面の状態を表す型。Rust 側には無く、保存もしない。 */

/** いまの作業。経路入力か、動画を見ながらの時刻入力か。 */
export type Mode = "route" | "confirm";

/** 保存済み、保存待ち（続けて編集している間）、保存中、保存できない。 */
export type SaveState = "saved" | "pending" | "saving" | "error";

/** 背景地図。どれも地理院タイル。 */
export type Basemap = "pale" | "std" | "photo";

/** 設定で選ぶフォルダ。動画、道路データ、判定データ。 */
export type Folder = "video" | "road" | "geo";

/** フォルダの読み込みの状態。 */
export type FolderState = "none" | "loading" | "ready" | "error";

/** 走行から外した動画の、実時刻の範囲。タイムラインに薄く出して、戻せるようにする。 */
export interface ExcludedClip {
  /** 動画のファイル名。外した動画は、これで見分ける */
  name: string;
  start_ms: number;
  end_ms: number;
}

/** 上の帯のすぐ下に帯で出す知らせ。直すか閉じるまで残る。 */
export interface Notice {
  /** 同じ事柄の知らせは、新しいもので置き換える */
  key: string;
  /** done は、済んだことを、あとで読み返せるように残すとき */
  level: "error" | "warning" | "done";
  title: string;
  detail: string;
  /** 直す手段 */
  action?: { label: string; run: () => void };
}

/** 手元とクラウドの、走行 1 つの関係。Rust 側の `SyncState` と同じ。 */
export type SyncState = "same" | "local_newer" | "remote_newer" | "local_only" | "remote_only" | "conflict";

/** 突き合わせた走行に、自動ですること。Rust 側の `Action` と同じ。 */
export type SyncAction = "keep" | "push" | "pull" | "ask";

/** 突き合わせた走行の 1 行。Rust 側の `SyncRow` と同じ。 */
export interface SyncRow {
  id: string;
  title: string;
  ride_date: string;
  local_updated_at: string | null;
  remote_updated_at: string | null;
  state: SyncState;
  action: SyncAction;
}

/**
 * 何も開いていないときに開く日付。前に開いていた走行がまだあればそれ、無ければいちばん新しいもの。
 *
 * `dates` は新しい順。走行が 1 つも無ければ undefined。
 */
export function startDate(last: string | null, dates: string[]): string | undefined {
  return last !== null && dates.includes(last) ? last : dates[0];
}

/** 次の起動でも使う、画面の好み。壊れていれば既定の値にする。 */
export function remembered<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const value = localStorage.getItem(key);
  return (allowed as readonly string[]).includes(value ?? "") ? (value as T) : fallback;
}
