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

/** 折れ線の始点から `routeM`（m）進んだ点。線より先は終点、線が無ければ null。 */
export function pointAt(line: LonLat[], routeM: number): LonLat | null {
  if (line.length === 0) return null;
  let rest = Math.max(routeM, 0);
  for (let i = 1; i < line.length; i++) {
    const [from, to] = [line[i - 1], line[i]];
    const length = distanceM(from, to);
    if (rest <= length && length > 0) {
      const t = rest / length;
      return [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t];
    }
    rest -= length;
  }
  return line[line.length - 1];
}
