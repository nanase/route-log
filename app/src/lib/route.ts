/**
 * 経路の編集。どれも元の値を変えず、変えた後の経路を返す。
 *
 * 経路は経由点の列と、経由点の間の頂点（`segments`）でできている。区間の数は
 * いつも経由点の数より 1 つ少ない。
 */

import type { LonLat, Route, Waypoint } from "./types";

/** 経路の形。走行のデータのうち、線を決める部分。 */
export interface Path {
  waypoints: Waypoint[];
  route: Route;
}

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

/** 経路の折れ線。経由点と、その間の頂点を順に並べる。 */
export function line(path: Path): LonLat[] {
  return path.waypoints.flatMap((waypoint, i) => [point(waypoint), ...(path.route.segments[i] ?? [])]);
}

/** 線を、隣り合う 2 点ごとに区切る。 */
export function pieces(path: Path): Piece[] {
  return path.route.segments.flatMap((vertices, segment) => {
    const points = [point(path.waypoints[segment]), ...vertices, point(path.waypoints[segment + 1])];
    return points.slice(1).map((to, index) => ({ segment, index, from: points[index], to }));
  });
}

/** 終わりに経由点を足す。手前の経由点からは `mode` で結ぶ。 */
export function appendWaypoint(path: Path, at: LonLat, mode: "straight" | "snap" = "straight"): Path {
  const waypoint: Waypoint = { lon: at[0], lat: at[1], segment_mode: "straight" };
  const last = path.waypoints.length - 1;
  return {
    waypoints: [...path.waypoints.map((w, i) => (i === last ? { ...w, segment_mode: mode } : w)), waypoint],
    route: { segments: path.waypoints.length > 0 ? [...path.route.segments, []] : [] },
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
