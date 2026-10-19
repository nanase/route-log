/** 経度・緯度の上の距離。Rust 側（routelog-core の geo）と同じ式を使う。 */

import type { LonLat } from "./types";

const EARTH_RADIUS_M = 6_371_008.8;
const radians = (degrees: number) => (degrees * Math.PI) / 180;

/** 2 点の間の大円距離（m）。 */
export function distanceM(a: LonLat, b: LonLat): number {
  const halfLat = radians(b[1] - a[1]) / 2;
  const halfLon = radians(b[0] - a[0]) / 2;
  const h = Math.sin(halfLat) ** 2 + Math.cos(radians(a[1])) * Math.cos(radians(b[1])) * Math.sin(halfLon) ** 2;
  return 2 * EARTH_RADIUS_M * Math.asin(Math.sqrt(h));
}

/** 折れ線の始点から各点までの距離（m）。`pointAt` に渡すと、長い線でも点を探すのが速い。 */
export function distancesAlong(line: LonLat[]): number[] {
  const along = line.length === 0 ? [] : [0];
  for (let i = 1; i < line.length; i++) along.push(along[i - 1] + distanceM(line[i - 1], line[i]));
  return along;
}

/**
 * 折れ線の始点から `routeM`（m）進んだ点。線より先は終点、線が無ければ null。
 *
 * 同じ線で何度も探すときは、`distancesAlong` で測った距離を `along` に渡す。
 */
export function pointAt(line: LonLat[], routeM: number, along = distancesAlong(line)): LonLat | null {
  if (line.length === 0) return null;
  const rest = Math.max(routeM, 0);
  // 始点からの距離が `rest` に届く、最初の点を探す。
  let [low, high] = [1, line.length];
  while (low < high) {
    const middle = (low + high) >> 1;
    if (along[middle] < rest) low = middle + 1;
    else high = middle;
  }
  // 長さの無い区切りは飛ばす。
  while (low < line.length && along[low] === along[low - 1]) low++;
  if (low >= line.length) return line[line.length - 1];
  const [from, to] = [line[low - 1], line[low]];
  const t = (rest - along[low - 1]) / (along[low] - along[low - 1]);
  return [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t];
}

/** 同じ近さとみなす差（m）。行って戻る道では、近さの同じ所が 2 つある。 */
const TIE_M = 1;

/**
 * 折れ線の上で `at` にいちばん近い所。その点と、始点からの距離（m）と、`at` からの距離（m）。
 *
 * 同じ道を行って戻る経路では、同じ近さの所がいくつもある。そのときは、始点からの距離が `nearM`
 * に近いほうを選ぶ。線が無ければ null。Rust 側（routelog-core の geo）の `nearest_m` と同じ決まりである。
 */
export function nearestOn(
  line: LonLat[],
  at: LonLat,
  nearM: number,
): { at: LonLat; routeM: number; offM: number } | null {
  if (line.length === 0) return null;
  // 近くだけを見るので、`at` を原点にした平面に直して測る。
  const scale = (EARTH_RADIUS_M * Math.PI) / 180;
  const shrink = Math.cos(radians(at[1]));
  const flat = (p: LonLat) => [(p[0] - at[0]) * scale * shrink, (p[1] - at[1]) * scale];
  const found = [{ at: line[0], routeM: 0, offM: distanceM(line[0], at) }];
  let walked = 0;
  for (let i = 1; i < line.length; i++) {
    const [from, to] = [line[i - 1], line[i]];
    const [[ax, ay], [bx, by]] = [flat(from), flat(to)];
    const length2 = (bx - ax) ** 2 + (by - ay) ** 2;
    const t = length2 === 0 ? 0 : Math.min(1, Math.max(0, -(ax * (bx - ax) + ay * (by - ay)) / length2));
    const length = distanceM(from, to);
    found.push({
      at: [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t],
      routeM: walked + length * t,
      offM: Math.hypot(ax + (bx - ax) * t, ay + (by - ay) * t),
    });
    walked += length;
  }
  const best = Math.min(...found.map((f) => f.offM));
  return found
    .filter((f) => f.offM <= best + TIE_M)
    .reduce((a, b) => (Math.abs(b.routeM - nearM) < Math.abs(a.routeM - nearM) ? b : a));
}
