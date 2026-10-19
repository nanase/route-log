import { expect, test } from "bun:test";
import { distanceM, distancesAlong, nearestOn, pointAt } from "./geo";
import type { LonLat } from "./types";

const A: LonLat = [138, 36];
const B: LonLat = [138, 37];
const C: LonLat = [139, 37];

test("緯度 1 度は約 111 km", () => {
  expect(Math.abs(distanceM(A, B) - 111_195)).toBeLessThan(10);
});

test("線の上の点は、始点からの距離で決まる", () => {
  const half = pointAt([A, B, C], distanceM(A, B) / 2)!;
  expect(half[0]).toBe(138);
  expect(half[1]).toBeCloseTo(36.5, 6);
  const corner = pointAt([A, B, C], distanceM(A, B))!;
  expect(corner[1]).toBeCloseTo(37, 6);
  const beyond = pointAt([A, B, C], distanceM(A, B) + distanceM(B, C) / 2)!;
  expect(beyond[0]).toBeCloseTo(138.5, 6);
});

test("線の外は端に寄せ、線が無ければ null", () => {
  expect(pointAt([A, B], -5)).toEqual(A);
  expect(pointAt([A, B], 1e9)).toEqual(B);
  expect(pointAt([A], 100)).toEqual(A);
  expect(pointAt([], 100)).toBeNull();
});

test("長さの無い区切りは飛ばし、測った距離を渡しても同じ点になる", () => {
  const line = [A, A, B, B, C];
  const along = distancesAlong(line);
  expect(along).toEqual([0, 0, distanceM(A, B), distanceM(A, B), distanceM(A, B) + distanceM(B, C)]);
  expect(pointAt(line, 0)).toEqual(A);
  for (const m of [0, 1000, distanceM(A, B), distanceM(A, B) + 5000, 1e9]) {
    expect(pointAt(line, m, along)).toEqual(pointAt(line, m)!);
  }
  expect(pointAt(line, distanceM(A, B) + distanceM(B, C) / 2)![0]).toBeCloseTo(138.5, 6);
  expect(distancesAlong([])).toEqual([]);
});

const east = (m: number): LonLat => [m / 111_195.08, 0];

test("線にいちばん近い所を、始点からの距離で返す", () => {
  const near = nearestOn([east(0), east(2000)], [east(1000)[0], 30 / 111_195.08], 0)!;
  expect(near.routeM).toBeCloseTo(1000, 0);
  expect(near.offM).toBeCloseTo(30, 0);
  expect(near.at[1]).toBe(0);
  expect(nearestOn([east(0), east(2000)], east(2500), 0)!.routeM).toBeCloseTo(2000, 0);
  expect(nearestOn([], east(0), 0)).toBeNull();
});

test("行って戻る道では、目安の距離に近いほうを選ぶ", () => {
  const line = [east(0), east(1000), east(0)];
  expect(nearestOn(line, east(600), 300)!.routeM).toBeCloseTo(600, 0);
  expect(nearestOn(line, east(600), 1800)!.routeM).toBeCloseTo(1400, 0);
});
