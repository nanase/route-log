/** 画面の状態を表す型。Rust 側には無く、保存もしない。 */

/** いまの作業。経路を入れるか、動画を見て時刻を確定するか。 */
export type Mode = "route" | "confirm";

/** 保存済み、保存待ち（続けて編集している間）、保存中、保存できない。 */
export type SaveState = "saved" | "pending" | "saving" | "error";

/** 背景地図。どれも地理院タイル。 */
export type Basemap = "pale" | "std" | "photo";

/** 設定で選ぶフォルダ。動画、道路データ、判定データ。 */
export type Folder = "video" | "road" | "geo";

/** フォルダの読み込みの状態。 */
export type FolderState = "none" | "loading" | "ready" | "error";

/** 上の帯のすぐ下に帯で出す知らせ。直すか閉じるまで残る。 */
export interface Notice {
  /** 同じ事柄の知らせは、新しいもので置き換える */
  key: string;
  level: "error" | "warning";
  title: string;
  detail: string;
  /** 直す手段 */
  action?: { label: string; run: () => void };
}

/** 手元とクラウドの、走行 1 つの関係。Rust 側の `SyncState` と同じ。 */
export type SyncState = "same" | "local_newer" | "remote_newer" | "local_only" | "remote_only" | "conflict";

/** 同期の一覧の 1 行。Rust 側の `SyncRow` と同じ。 */
export interface SyncRow {
  id: string;
  title: string;
  ride_date: string;
  local_updated_at: string | null;
  remote_updated_at: string | null;
  state: SyncState;
}

/** 次の起動でも使う、画面の好み。壊れていれば既定の値にする。 */
export function remembered<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const value = localStorage.getItem(key);
  return (allowed as readonly string[]).includes(value ?? "") ? (value as T) : fallback;
}
