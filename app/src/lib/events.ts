/**
 * イベントの時刻の確定と、次の候補の選び方。どれも元の値を変えない。
 *
 * 時刻の推定そのものは Rust 側（routelog-core の estimate）が保存のたびに行う。ここにあるのは、
 * 画面がその場で答えなければならないことだけである。
 */

import type { LonLat, RideEvent } from "./types";

/** 時刻推定の基準。経路の上の位置と、そこを通った時刻の組。 */
export interface Knot {
  route_m: number;
  time_ms: number;
}

/** 停車していた時間。その間は、時刻が進んでも位置が変わらない。 */
export interface Stop {
  start_ms: number;
  end_ms: number;
}

const isKnot = (event: RideEvent) => event.status === "confirmed" && event.time_ms !== null && !event.detail.by_time;

export function knotsOf(events: RideEvent[]): Knot[] {
  return events.filter(isKnot).map((event) => ({ route_m: event.route_m, time_ms: event.time_ms as number }));
}

/** 停車のイベントを、停車していた時間の並びにする。 */
export function stopsOf(events: RideEvent[]): Stop[] {
  return events.flatMap((event) => {
    const [start, stay] = [event.time_ms, event.detail.stay_ms ?? 0];
    return event.kind === "stop" && start !== null && stay > 0 ? [{ start_ms: start, end_ms: start + stay }] : [];
  });
}

/** その時刻が、停車の間か。始まりと終わりの時刻も、停車の間に数える。 */
export function inStop(stops: Stop[], timeMs: number): boolean {
  return stops.some((stop) => stop.start_ms <= timeMs && timeMs <= stop.end_ms);
}

/** その時刻までに、走っていた時間。時刻から、それまでの停車の長さを引いたもの。 */
function movingMs(stops: Stop[], timeMs: number): number {
  // 重なる停車を 2 度引かないよう、まとめてから引く。
  let [stopped, covered] = [0, Number.NEGATIVE_INFINITY];
  for (const stop of [...stops].sort((a, b) => a.start_ms - b.start_ms)) {
    const [from, to] = [Math.max(stop.start_ms, covered), Math.min(stop.end_ms, timeMs)];
    if (to > from) stopped += to - from;
    covered = Math.max(covered, stop.end_ms);
  }
  return timeMs - stopped;
}

/**
 * その時刻にいる位置（m）。最初の基準より前と、最後の基準より後は分からない。
 *
 * 基準の間は、走っていた時間に比例して進んだものとする。停車の間は、同じ位置を返す。
 * Rust 側（routelog-core の estimate）の `position_at` と同じ決まりである。
 */
export function positionAt(knots: Knot[], stops: Stop[], timeMs: number): number | null {
  const byTime = [...knots].sort((a, b) => a.time_ms - b.time_ms);
  for (let i = 0; i < byTime.length; i++) {
    const [from, to] = [byTime[i], byTime[i + 1]];
    if (from.time_ms === timeMs) return from.route_m;
    if (to !== undefined && from.time_ms < timeMs && timeMs < to.time_ms) {
      const [start, end] = [movingMs(stops, from.time_ms), movingMs(stops, to.time_ms)];
      if (start === end) return from.route_m;
      const t = (movingMs(stops, timeMs) - start) / (end - start);
      return from.route_m + (to.route_m - from.route_m) * t;
    }
  }
  return null;
}

/**
 * 時刻だけが分かっているイベントを置く位置（m）。
 *
 * 基準の間なら、その時刻にいる位置。基準の外なら、時刻の近いほうの端の基準の位置。基準が無ければ 0。
 * Rust 側が保存のときに求め直すが、それまでの間も、一覧の正しい所に出すために使う。
 */
export function placeOf(events: RideEvent[], timeMs: number): number {
  const knots = knotsOf(events);
  const at = positionAt(knots, stopsOf(events), timeMs);
  if (at !== null) return at;
  const byTime = [...knots].sort((a, b) => a.time_ms - b.time_ms);
  if (byTime.length === 0) return 0;
  return timeMs < byTime[0].time_ms ? byTime[0].route_m : byTime[byTime.length - 1].route_m;
}

/**
 * イベントを、位置の順を保って足す。同じ位置のイベントがあれば、その後ろに入れる。
 *
 * 末尾に足すと、保存の応答で並び直されるまでの間、一覧の最後に出てしまう。
 */
export function insertByPosition(events: RideEvent[], added: RideEvent): RideEvent[] {
  const index = events.findIndex((event) => event.route_m > added.route_m);
  return index < 0 ? [...events, added] : [...events.slice(0, index), added, ...events.slice(index)];
}

/**
 * 次の候補として自動で出すイベント。字幕にしないものと、時刻から置いたものは除く。
 *
 * 字幕にしないものも、人が選べば確定できる。自動では出さないだけである。
 */
const confirmable = (event: RideEvent) => event.status === "estimated" && !event.hidden && !event.detail.by_time;

/**
 * 何も選んでいないときの、次の候補。
 *
 * 推定時刻がいまより後の最初のイベント。推定の無いうちは、位置の順で最初の未確定のイベント。
 */
export function defaultTarget(events: RideEvent[], nowMs: number): RideEvent | null {
  const candidates = events.filter(confirmable);
  return (
    candidates.find((event) => event.time_ms !== null && event.time_ms >= nowMs) ??
    candidates.find((event) => event.time_ms === null) ??
    null
  );
}

/** 字幕にするイベントの中で、隣のもの。端では null。 */
export function neighbor(events: RideEvent[], id: string | null, step: 1 | -1): RideEvent | null {
  const shown = events.filter((event) => !event.hidden || event.id === id);
  const index = shown.findIndex((event) => event.id === id);
  if (index < 0) return step === 1 ? (shown[0] ?? null) : null;
  return shown[index + step] ?? null;
}

/** そのイベントより後にある、次の候補。 */
export function nextConfirmable(events: RideEvent[], id: string): RideEvent | null {
  const index = events.findIndex((event) => event.id === id);
  return events.slice(index + 1).find(confirmable) ?? null;
}

export function patch(events: RideEvent[], id: string, change: (event: RideEvent) => RideEvent): RideEvent[] {
  return events.map((event) => (event.id === id ? change(event) : event));
}

/**
 * 動画のいまの位置で確定する。
 *
 * カメラの時計での時刻も残す。時計の補正値を後から変えたとき、Rust 側がそこから時刻を求め直す。
 */
export function confirmOnVideo(event: RideEvent, realMs: number, clockOffsetMs: number): RideEvent {
  // 動画の再生位置は小数の秒で返る。時刻は整数のミリ秒で持つ決まりなので、ここで丸める。
  const timeMs = Math.round(realMs);
  return {
    ...event,
    status: "confirmed",
    time_ms: timeMs,
    detail: { ...event.detail, camera_ms: timeMs - Math.round(clockOffsetMs) },
  };
}

/** 手で入れた時刻で確定する。時計の補正値を変えても動かない。 */
export function confirmTyped(event: RideEvent, timeMs: number): RideEvent {
  const { camera_ms: _, ...detail } = event.detail;
  return { ...event, status: "confirmed", time_ms: Math.round(timeMs), detail };
}

/** 確定を外す。時刻は次の保存で推定し直される。 */
export function unconfirm(event: RideEvent): RideEvent {
  const { camera_ms: _, ...detail } = event.detail;
  return { ...event, status: "estimated", time_ms: null, detail };
}

/** 字幕にするかどうかを切り替える。確定した時刻は、そのまま残る。 */
export function setHidden(event: RideEvent, hidden: boolean): RideEvent {
  const { hidden: _, ...rest } = event;
  return hidden ? { ...rest, hidden: true } : rest;
}

/**
 * 経由点に時刻を入れるためのイベントを作る。字幕にはしない。
 *
 * `index` は経由点の番号、`routeM` は経路の始点からの距離。位置と名前は、Rust 側が保存のときに
 * 経由点から決め直す。ここでも入れておくのは、それまでの間も、一覧の正しい所に正しい名前で出すため。
 */
export function waypointEvent(at: LonLat, index: number, routeM: number): RideEvent {
  const label = `経由点 ${index + 1}`;
  return {
    id: crypto.randomUUID(),
    kind: "waypoint",
    label,
    route_m: routeM,
    time_ms: null,
    status: "estimated",
    hidden: true,
    detail: { at, extracted_label: label },
  };
}

/** その経由点のイベント。まだ作っていなければ null。 */
export function waypointEventAt(events: RideEvent[], at: LonLat): RideEvent | null {
  const same = (event: RideEvent) => event.detail.at?.[0] === at[0] && event.detail.at?.[1] === at[1];
  return events.find((event) => event.kind === "waypoint" && same(event)) ?? null;
}

/**
 * 停車のイベントを作る。その間は位置が変わらないものとして、時刻が推定される。字幕にはしない。
 *
 * 位置は、Rust 側が時刻から求める。時刻は動画の録画の切れ目から取るので、カメラの時計での時刻も
 * 残し、時計の補正値と一緒に動くようにする。
 */
export function stopBetween(startMs: number, endMs: number, clockOffsetMs: number): RideEvent {
  const start = Math.round(startMs);
  return {
    id: crypto.randomUUID(),
    kind: "stop",
    label: "停車",
    route_m: 0,
    time_ms: start,
    status: "confirmed",
    hidden: true,
    detail: { by_time: true, camera_ms: start - Math.round(clockOffsetMs), stay_ms: Math.round(endMs) - start },
  };
}

/** 始まりだけを入れて、終わりをまだ入れていない停車。無ければ null。 */
export function openStop(events: RideEvent[]): RideEvent | null {
  return events.find((event) => event.kind === "stop" && event.time_ms !== null && (event.detail.stay_ms ?? 0) <= 0) ?? null;
}

/**
 * 始まりだけを入れた停車に、動画のいまの位置でもう一方の端を入れる。
 *
 * いまの位置が始まりより前なら、いまの位置を始まりにして、元の始まりを終わりにする。
 */
export function closeStop(event: RideEvent, realMs: number, clockOffsetMs: number): RideEvent {
  const [first, now] = [event.time_ms, Math.round(realMs)];
  if (first === null || now === first) return event;
  if (now > first) return { ...event, detail: { ...event.detail, stay_ms: now - first } };
  const detail = { ...event.detail, camera_ms: now - Math.round(clockOffsetMs), stay_ms: first - now };
  return { ...event, time_ms: now, detail };
}

/**
 * 停車の始まりか終わりの時刻を、手で直す。もう一方の時刻は動かさない。
 *
 * 始まりが終わりより後になる直し方は、受け付けずにそのまま返す。
 */
export function retimeStop(event: RideEvent, edge: "start" | "end", timeMs: number): RideEvent {
  if (event.time_ms === null) return event;
  const end = event.time_ms + (event.detail.stay_ms ?? 0);
  const [start, until] = edge === "start" ? [Math.round(timeMs), end] : [event.time_ms, Math.round(timeMs)];
  if (until <= start) return event;
  if (edge === "end") return { ...event, detail: { ...event.detail, stay_ms: until - start } };
  // 手で入れた始まりの時刻は、カメラの時刻を持たない。時計の補正値を変えても動かない。
  const { camera_ms: _, ...detail } = event.detail;
  return { ...event, time_ms: start, detail: { ...detail, stay_ms: until - start } };
}

/** 動画のいまの位置に、手動のイベントを作る。位置は Rust 側が時刻から求める。 */
export function customAt(realMs: number, clockOffsetMs: number, label: string): RideEvent {
  const timeMs = Math.round(realMs);
  return {
    id: crypto.randomUUID(),
    kind: "custom",
    label,
    route_m: 0,
    time_ms: timeMs,
    status: "confirmed",
    detail: { by_time: true, camera_ms: timeMs - Math.round(clockOffsetMs) },
  };
}
