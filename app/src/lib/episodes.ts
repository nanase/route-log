/**
 * 回。YouTube に上げる 1 本ぶんの範囲で、経路の一部を、始まりと終わりの経由点の ID で決める。
 * どれも元の値を変えない。
 *
 * 回の範囲から経路の区間と時刻を求めるのは、Rust 側（routelog-core の episode）にもある。ここに
 * あるのは、画面がその場で答えなければならないことだけである。
 */

import { knotsOf, mergedStops, type Stop, stopsOf, timeAt } from "./events";
import { newId } from "./ids";
import { type Path, segmentLengths } from "./route";
import { hms, minutesSeconds } from "./time";
import type { Episode, Exported, RideEvent, Span, Waypoint } from "./types";

/** 回の録画の長さの目安の上限（ms）。Resolve のソースのタイムラインを、これより長くしない。Rust 側と同じ値 */
export const RECORDED_LIMIT_MS = 5 * 60 * 60 * 1000;

/** 回の範囲の、端の経由点の番号。端の経由点が無いか、始まりが終わりより前でなければ null。 */
export function extentOf(waypoints: Waypoint[], episode: Episode): { from: number; to: number } | null {
  const index = (id: string) => (id === "" ? -1 : waypoints.findIndex((waypoint) => waypoint.id === id));
  const [from, to] = [index(episode.from), index(episode.to)];
  return from >= 0 && to >= 0 && from < to ? { from, to } : null;
}

/**
 * 経路を編集したあと、消えた経由点を端にしていた回を、内側に残っている経由点へ付け替える。
 * 始まりなら後ろの、終わりなら手前の経由点にする。
 *
 * 範囲が縮む向きにするので、隣の回へはみ出さない。付け替えた結果、範囲が無くなっても回は消さず、
 * 画面で範囲が正しくないと知らせる。付け替える先が無ければ、消えた経由点の ID のまま残す。
 */
export function repoint(episodes: Episode[], before: Waypoint[], after: Waypoint[]): Episode[] {
  const kept = new Set(after.map((waypoint) => waypoint.id));
  if (before.every((waypoint) => kept.has(waypoint.id))) return episodes;
  const inward = (id: string, step: 1 | -1): string => {
    if (kept.has(id)) return id;
    const start = before.findIndex((waypoint) => waypoint.id === id);
    if (start < 0) return id;
    for (let i = start + step; i >= 0 && i < before.length; i += step) {
      if (kept.has(before[i].id)) return before[i].id;
    }
    return id;
  };
  return episodes.map((episode) => {
    const [from, to] = [inward(episode.from, 1), inward(episode.to, -1)];
    return from === episode.from && to === episode.to ? episode : { ...episode, from, to };
  });
}

/** 回を足す。範囲は経路の全体にしておく。経由点が 2 つ無ければ足さない。 */
export function addEpisode(episodes: Episode[], waypoints: Waypoint[]): Episode[] {
  if (waypoints.length < 2) return episodes;
  const [first, last] = [waypoints[0], waypoints[waypoints.length - 1]];
  return [...episodes, { id: newId(), title: "", from: first.id, to: last.id }];
}

/** 画面に出す回の名前。名前が無ければ、並びの番号で呼ぶ。 */
export function nameOf(episode: Episode, index: number): string {
  return episode.title.trim() || `パート ${index + 1}`;
}

/**
 * `[startMs, endMs)` のうち、録画していて、停車していない時間（ms）。Rust 側の `recorded_ms` と
 * 同じ決まりである。
 *
 * `spans` はタイムラインの範囲で、走行から外した動画を含まない。重ならず、時刻の順に並ぶ。
 * `stops` も重ならないものを渡す。
 */
export function recordedMs(spans: Span[], stops: Stop[], startMs: number, endMs: number): number {
  const overlap = (from: number, to: number) => Math.max(Math.min(to, endMs) - Math.max(from, startMs), 0);
  let recorded = 0;
  for (const span of spans) {
    recorded += overlap(span.start_ms, span.end_ms);
    // 停車のうち、録画していた部分だけを引く。停車の多くは、録画を止めた間にある。
    for (const stop of stops) {
      const [from, to] = [Math.max(stop.start_ms, span.start_ms), Math.min(stop.end_ms, span.end_ms)];
      if (from < to) recorded -= overlap(from, to);
    }
  }
  return recorded;
}

/**
 * 回の録画の長さ（ms）。両端の経由点を通った時刻の間で数える。範囲が正しくないか、どちらかの
 * 時刻が推定できなければ null。
 */
export function recordedOf(doc: Path & { events: RideEvent[] }, spans: Span[], episode: Episode): number | null {
  const extent = extentOf(doc.waypoints, episode);
  if (extent === null) return null;
  const lengths = segmentLengths(doc);
  const reached = (index: number) => lengths.slice(0, index).reduce((sum, length) => sum + length, 0);
  const [knots, stops] = [knotsOf(doc.events), mergedStops(stopsOf(doc.events))];
  const [start, end] = [timeAt(knots, stops, reached(extent.from)), timeAt(knots, stops, reached(extent.to))];
  return start === null || end === null ? null : recordedMs(spans, stops, start, end);
}

/**
 * 回を Resolve へ書き出せない理由。書き出せるなら null。`recorded` は `recordedOf` の値で、
 * `spans` が null なら動画が無い。
 */
export function exportBlocker(hasExtent: boolean, spans: Span[] | null, recorded: number | null): string | null {
  if (!hasExtent) return "パートの範囲が正しくありません";
  if (spans === null) return "この走行ログには動画が指定されていません";
  if (recorded === null) return "始点と終点の経由地の通過時刻が推定されるとエクスポートできます";
  if (recorded <= 0) return "パートの範囲に動画がありません";
  return null;
}

/** Resolve へ書き出したあとに出す知らせ。マーカーの読み込み方も添える。 */
export function exportedMessage(name: string, written: Extract<Exported, { outcome: "written" }>): string {
  const cuts = written.cuts > 0 ? `停車 ${written.cuts} か所（${minutesSeconds(written.cutMs)}）を抜き、` : "";
  const unplaced =
    written.unplaced > 0
      ? `${written.unplaced} 個のイベントは、録画の外か抜いた所にあるので、マーカーにしていません。`
      : "";
  return (
    `「${name}」のソース ${hms(written.lengthMs)}。${cuts}マーカーは ${written.markers} 個です。${unplaced}` +
    "マーカーは、メディアプールでタイムラインを右クリックし、「タイムライン」>「読み込み」>「EDL からタイムラインマーカー」で読み込みます。"
  );
}
