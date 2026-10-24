/**
 * 経路の編集。どれも元の値を変えず、変えた後の経路を返す。
 *
 * 経路は経由点の列と、経由点の間の頂点（`segments`）でできている。区間の数は
 * いつも経由点の数より 1 つ少ない。
 */

import { distanceM } from "./geo";
import { newId } from "./ids";
import type { Episode, LonLat, Route, Waypoint } from "./types";

/** 経路の形。走行のデータのうち、線を決める部分。 */
export interface Path {
  waypoints: Waypoint[];
  route: Route;
  /**
   * 回。回は経由点で決めるので、経路の編集と一緒に取り消せるよう、ここにも持てるようにする。
   * 経路だけを変える編集は、回を持たない経路を返してよい。
   */
  episodes?: Episode[];
}

/**
 * 経路の編集 1 つ。当てる瞬間の経路を受け取り、変えた後の経路を返す。変える所が無ければ、
 * 受け取った経路をそのまま返す。
 *
 * 画面は、編集した結果の経路ではなく、この形の操作を渡す。点を道へ寄せるといった待つ処理の
 * 間に経路が変わっても、古い経路から作った結果で上書きしないためである。
 */
export type PathEdit = (path: Path) => Path;

/** 線の上の 1 区切り。途中をつかんで頂点を足す場所になる。 */
export interface Piece {
  segment: number;
  /** この区切りに頂点を足すとき、`segments[segment]` の何番目に入るか */
  index: number;
  from: LonLat;
  to: LonLat;
}

const point = (waypoint: Waypoint): LonLat => [waypoint.lon, waypoint.lat];

/** 吸着の区間の線は Rust 側が引く。経由点が動いたら、古い線を捨てて引き直しを待つ。 */
const stale = (path: Path, ...segments: number[]): Path => ({
  ...path,
  route: {
    segments: path.route.segments.map((vertices, i) =>
      segments.includes(i) && path.waypoints[i].segment_mode === "snap" ? [] : vertices,
    ),
  },
});

/** 区間の頂点を入れ替える。頂点があれば手修正、無ければ直線になる。 */
const withSegment = (path: Path, segment: number, vertices: LonLat[]): Path => ({
  waypoints: path.waypoints.map((waypoint, i) =>
    i === segment ? { ...waypoint, segment_mode: vertices.length > 0 ? "manual" : "straight" } : waypoint,
  ),
  route: { segments: path.route.segments.map((old, i) => (i === segment ? vertices : old)) },
});

/** 経由点 `index` が、2 つの経路で同じ所にあるか。 */
function sameWaypoint(path: Path, shown: Path, index: number): boolean {
  const [now, then] = [path.waypoints[index], shown.waypoints[index]];
  return now !== undefined && then !== undefined && now.lon === then.lon && now.lat === then.lat;
}

/**
 * 画面に出ていた経路 `shown` の経由点 `index` を相手に決めた編集を、いまの経路に当てられる形にする。
 *
 * 相手は番号で指す。待つ間にほかの編集が入って、その番号の経由点が出ていたときと違う所に
 * あれば、何もしない。番号のずれた別の経由点を書き換えないためである。
 */
export function onWaypoint(shown: Path, index: number, edit: PathEdit): PathEdit {
  return (path) => (sameWaypoint(path, shown, index) ? edit(path) : path);
}

/**
 * 区間 `segment` を相手に決めた編集を、いまの経路に当てられる形にする。
 *
 * 両端の経由点と結び方が、出ていたときと同じであることを確かめる。頂点を番号で指すので、
 * 吸着でない区間では頂点の数も見る。吸着の区間の頂点は、保存のたびに引き直されるので見ない。
 */
export function onSegment(shown: Path, segment: number, edit: PathEdit): PathEdit {
  return (path) => {
    const mode = path.waypoints[segment]?.segment_mode;
    const same =
      sameWaypoint(path, shown, segment) &&
      sameWaypoint(path, shown, segment + 1) &&
      mode === shown.waypoints[segment].segment_mode &&
      (mode === "snap" || path.route.segments[segment]?.length === shown.route.segments[segment]?.length);
    return same ? edit(path) : path;
  };
}

/** 経路の折れ線。経由点と、その間の頂点を順に並べる。 */
export function line(path: Path): LonLat[] {
  return path.waypoints.flatMap((waypoint, i) => [point(waypoint), ...(path.route.segments[i] ?? [])]);
}

/** 経由点 `from` から `to` までの、経路の一部。番号が逆か範囲の外なら、空の経路。 */
export function part(path: Path, from: number, to: number): Path {
  if (from < 0 || to >= path.waypoints.length || from >= to) return { waypoints: [], route: { segments: [] } };
  return { waypoints: path.waypoints.slice(from, to + 1), route: { segments: path.route.segments.slice(from, to) } };
}

/** 線を、隣り合う 2 点ごとに区切る。 */
export function pieces(path: Path): Piece[] {
  return path.route.segments.flatMap((vertices, segment) => {
    const points = [point(path.waypoints[segment]), ...vertices, point(path.waypoints[segment + 1])];
    return points.slice(1).map((to, index) => ({ segment, index, from: points[index], to }));
  });
}

/** 区間ごとの長さ（m）。経由点の一覧に、そこまでの距離と次の点までの距離を出すのに使う。 */
export function segmentLengths(path: Path): number[] {
  return path.route.segments.map((vertices, segment) => {
    const points = [point(path.waypoints[segment]), ...vertices, point(path.waypoints[segment + 1])];
    return points.slice(1).reduce((sum, to, i) => sum + distanceM(points[i], to), 0);
  });
}

/** 終わりに経由点を足す。手前の経由点からは `mode` で結ぶ。 */
export function appendWaypoint(path: Path, at: LonLat, mode: "straight" | "snap" = "straight"): Path {
  const waypoint: Waypoint = { id: newId(), lon: at[0], lat: at[1], segment_mode: "straight" };
  const last = path.waypoints.length - 1;
  return {
    waypoints: [...path.waypoints.map((w, i) => (i === last ? { ...w, segment_mode: mode } : w)), waypoint],
    route: { segments: path.waypoints.length > 0 ? [...path.route.segments, []] : [] },
  };
}

/**
 * 区間の途中に経由点を足し、区間を 2 つに分ける。`index` は、線の区切りの番号。
 *
 * 分けた 2 つの区間は、元の結び方を受け継ぐ。手で置いた頂点は、前後に振り分ける。吸着の
 * 区間は、どちらも引き直しを待つ。
 */
export function insertWaypoint(path: Path, segment: number, index: number, at: LonLat): Path {
  const before = path.waypoints[segment];
  const vertices = path.route.segments[segment];
  if (before === undefined || vertices === undefined) return path;
  const snap = before.segment_mode === "snap";
  const [head, tail] = snap ? [[], []] : [vertices.slice(0, index), vertices.slice(index)];
  const mode = (part: LonLat[]) => (snap ? "snap" : part.length > 0 ? "manual" : "straight");
  return {
    waypoints: [
      ...path.waypoints.slice(0, segment),
      { ...before, segment_mode: mode(head) },
      { id: newId(), lon: at[0], lat: at[1], segment_mode: mode(tail) },
      ...path.waypoints.slice(segment + 1),
    ],
    route: {
      segments: [...path.route.segments.slice(0, segment), head, tail, ...path.route.segments.slice(segment + 1)],
    },
  };
}

export function moveWaypoint(path: Path, index: number, to: LonLat): Path {
  const moved = {
    ...path,
    waypoints: path.waypoints.map((waypoint, i) => (i === index ? { ...waypoint, lon: to[0], lat: to[1] } : waypoint)),
  };
  return stale(moved, index - 1, index);
}

/** 区間の結び方を、吸着と直線で切り替える。手で置いた頂点は捨てる。 */
export function toggleSnap(path: Path, segment: number): Path {
  if (segment < 0 || segment >= path.route.segments.length) return path;
  const mode = path.waypoints[segment].segment_mode === "snap" ? "straight" : "snap";
  return {
    waypoints: path.waypoints.map((waypoint, i) => (i === segment ? { ...waypoint, segment_mode: mode } : waypoint)),
    route: { segments: path.route.segments.map((vertices, i) => (i === segment ? [] : vertices)) },
  };
}

/** 経由点を消す。途中の経由点なら前後の区間を 1 つにし、手で置いた頂点は残す。 */
export function removeWaypoint(path: Path, index: number): Path {
  const { waypoints, route } = path;
  const last = waypoints.length - 1;
  if (index < 0 || index > last) return path;
  const rest = waypoints.filter((_, i) => i !== index);
  if (index === 0) return { waypoints: rest, route: { segments: route.segments.slice(1) } };
  if (index === last) {
    // 最後の経由点の結び方は使わない。直線に戻しておく。
    rest[last - 1] = { ...rest[last - 1], segment_mode: "straight" };
    return { waypoints: rest, route: { segments: route.segments.slice(0, -1) } };
  }
  // 前後のどちらかが吸着なら、1 つになった区間も吸着にして引き直す。
  if (waypoints[index - 1].segment_mode === "snap" || waypoints[index].segment_mode === "snap") {
    rest[index - 1] = { ...rest[index - 1], segment_mode: "snap" };
    const segments = [...route.segments.slice(0, index - 1), [], ...route.segments.slice(index + 1)];
    return { waypoints: rest, route: { segments } };
  }
  const merged = [...route.segments[index - 1], ...route.segments[index]];
  const segments = [...route.segments.slice(0, index - 1), merged, ...route.segments.slice(index + 1)];
  return withSegment({ waypoints: rest, route: { segments } }, index - 1, merged);
}

/** 区間の途中に頂点を足す。その区間は手修正になる。 */
export function insertVertex(path: Path, segment: number, index: number, at: LonLat): Path {
  const vertices = path.route.segments[segment];
  return withSegment(path, segment, [...vertices.slice(0, index), at, ...vertices.slice(index)]);
}

export function moveVertex(path: Path, segment: number, index: number, to: LonLat): Path {
  const vertices = path.route.segments[segment];
  return withSegment(path, segment, vertices.map((vertex, i) => (i === index ? to : vertex)));
}

/** 頂点を消す。頂点が無くなった区間は直線に戻る。 */
export function removeVertex(path: Path, segment: number, index: number): Path {
  const vertices = path.route.segments[segment];
  return withSegment(path, segment, vertices.filter((_, i) => i !== index));
}
