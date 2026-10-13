import { expect, test } from "bun:test";
import { distanceM, pointAt } from "./geo";
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
