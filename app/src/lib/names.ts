/**
 * イベントの、一覧に出す名前。橋・トンネルは、国土交通省のデータベースの名前と公称の長さを使い、
 * 「犬戻トンネル 737m」の形にする。Rust 側（routelog-core の `Event::shown_name`）と同じ決まり。
 */

import type { RideEvent } from "./types";

/** 字幕で区別しない揺れ（全角と半角、ケ・ヶ・ヵ）を揃えた名前 */
function loose(name: string): string {
  return name
    .replace(/[！-～]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0xfee0))
    .replace(/　/g, " ")
    .replace(/[ヶヵ]/g, "ケ");
}

/** 字幕で区別しない揺れだけが違う、同じ名前か */
export function sameName(a: string, b: string): boolean {
  return loose(a) === loose(b);
}

const isStructure = (event: RideEvent) => event.kind === "bridge" || event.kind === "tunnel";

/** 人が名前を直したか、名前の候補から選んだ */
function renamed(event: RideEvent): boolean {
  return event.detail.named_by_hand === true || event.label !== (event.detail.extracted_label ?? event.label);
}

/** 長さの数。小数があれば 1 桁、無ければ整数 */
function length(m: number): string {
  const tenths = Math.round(m * 10);
  return tenths % 10 === 0 ? String(tenths / 10) : (tenths / 10).toFixed(1);
}

/** 一覧とマーカーに出す名前 */
export function shownName(event: RideEvent): string {
  if (!isStructure(event)) return event.label;
  const official = event.detail.official_name;
  let name = event.label;
  if (official !== undefined && !renamed(event)) {
    name = event.label === "" || sameName(event.label, official) ? official : `${event.label}／${official}`;
  }
  const m = event.detail.official_length_m;
  return m !== undefined && name !== "" && !/ \d+(\.\d+)?m$/.test(name) ? `${name} ${length(m)}m` : name;
}

/** 選べる名前の候補。OSM の名前と公称の名前が違うときだけ、その 2 つ */
export function nameChoices(event: RideEvent): string[] {
  const official = event.detail.official_name;
  if (!isStructure(event) || official === undefined) return [];
  const osm = event.detail.extracted_label ?? event.label;
  return osm === "" || sameName(osm, official) ? [] : [osm, official];
}

/** 名前を人が決める。抽出し直しても残る */
export function setName(event: RideEvent, label: string): RideEvent {
  return { ...event, label, detail: { ...event.detail, named_by_hand: true } };
}
