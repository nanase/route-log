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
