import type { EventKind } from "./types";

/** イベントの種類の呼び名。 */
export const KINDS: Record<EventKind, string> = {
  departure: "出発",
  arrival: "到着",
  pref_boundary: "県境",
  muni_boundary: "市区町村境",
  road: "路線",
  bridge: "橋",
  tunnel: "トンネル",
  michi_no_eki: "道の駅",
  custom: "手動",
};

/** 種類を 1 字で表す札。色は `app.css` の `--kind-*` から取る。 */
export const KIND_MARKS: Record<EventKind, { mark: string; color: string; outline?: true }> = {
  departure: { mark: "発", color: "--kind-ends" },
  arrival: { mark: "着", color: "--kind-ends" },
  road: { mark: "路", color: "--kind-road" },
  pref_boundary: { mark: "県", color: "--kind-boundary" },
  // 県境と同じ色を、白地に枠で出して見分ける。
  muni_boundary: { mark: "市", color: "--kind-boundary", outline: true },
  bridge: { mark: "橋", color: "--kind-bridge" },
  tunnel: { mark: "ト", color: "--kind-tunnel" },
  michi_no_eki: { mark: "駅", color: "--kind-station" },
  custom: { mark: "手", color: "--kind-custom" },
};

/** 一覧やタイムラインに種類を並べるときの順。 */
export const KIND_ORDER = Object.keys(KIND_MARKS) as EventKind[];
