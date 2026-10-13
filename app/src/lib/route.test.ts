import { describe, expect, test } from "bun:test";
import {
  appendWaypoint,
  insertVertex,
  line,
  moveVertex,
  moveWaypoint,
  type Path,
  pieces,
  removeVertex,
  removeWaypoint,
} from "./route";
import type { LonLat } from "./types";

const A: LonLat = [138.0, 36.0];
const B: LonLat = [138.1, 36.0];
const C: LonLat = [138.2, 36.0];
const D: LonLat = [138.3, 36.0];
const P: LonLat = [138.05, 36.05];
const Q: LonLat = [138.15, 36.05];

const empty: Path = { waypoints: [], route: { segments: [] } };
const build = (...points: LonLat[]) => points.reduce(appendWaypoint, empty);
const modes = (path: Path) => path.waypoints.map((w) => w.segment_mode);

describe("経由点", () => {
  test("足すたびに区間が 1 つ増え、区間の数は経由点より 1 つ少ない", () => {
    expect(build(A).route.segments).toEqual([]);
    expect(build(A, B, C).route.segments).toEqual([[], []]);
    expect(line(build(A, B, C))).toEqual([A, B, C]);
  });

  test("動かしても、ほかは変わらない", () => {
    const path = insertVertex(build(A, B, C), 0, 0, P);
    expect(line(moveWaypoint(path, 1, D))).toEqual([A, P, D, C]);
  });

  test("先頭と末尾を消すと、その区間ごと消える", () => {
    const path = insertVertex(insertVertex(build(A, B, C), 0, 0, P), 1, 0, Q);
    expect(line(removeWaypoint(path, 0))).toEqual([B, Q, C]);
    const withoutLast = removeWaypoint(path, 2);
    expect(line(withoutLast)).toEqual([A, P, B]);
    expect(modes(withoutLast)).toEqual(["manual", "straight"]);
  });

  test("途中を消すと前後の区間が 1 つになり、手で置いた頂点は残る", () => {
    const path = insertVertex(insertVertex(build(A, B, C), 0, 0, P), 1, 0, Q);
    const removed = removeWaypoint(path, 1);
    expect(line(removed)).toEqual([A, P, Q, C]);
    expect(removed.route.segments).toEqual([[P, Q]]);
    expect(modes(removed)).toEqual(["manual", "straight"]);
  });

  test("直線どうしの間を消すと、直線のままになる", () => {
    const removed = removeWaypoint(build(A, B, C), 1);
    expect(removed.route.segments).toEqual([[]]);
    expect(modes(removed)).toEqual(["straight", "straight"]);
  });

  test("最後の 1 つを消すと空になる", () => {
    expect(removeWaypoint(build(A), 0)).toEqual(empty);
  });

  test("無い番号は何も変えない", () => {
    const path = build(A, B);
    expect(removeWaypoint(path, 2)).toBe(path);
  });
});

describe("頂点", () => {
  test("足した区間は手修正になり、消して無くなれば直線に戻る", () => {
    const manual = insertVertex(build(A, B), 0, 0, P);
    expect(modes(manual)).toEqual(["manual", "straight"]);
    expect(line(manual)).toEqual([A, P, B]);
    const straight = removeVertex(manual, 0, 0);
    expect(modes(straight)).toEqual(["straight", "straight"]);
    expect(straight.route.segments).toEqual([[]]);
  });

  test("区切りの番号の位置に入る", () => {
    const path = insertVertex(insertVertex(build(A, C), 0, 0, P), 0, 1, Q);
    expect(line(path)).toEqual([A, P, Q, C]);
    expect(line(moveVertex(path, 0, 1, B))).toEqual([A, P, B, C]);
  });

  test("元の経路は変わらない", () => {
    const path = build(A, B);
    insertVertex(path, 0, 0, P);
    expect(line(path)).toEqual([A, B]);
  });
});

test("線の区切りは、頂点を足す場所を順に指す", () => {
  const path = insertVertex(build(A, B, C), 0, 0, P);
  expect(pieces(path)).toEqual([
    { segment: 0, index: 0, from: A, to: P },
    { segment: 0, index: 1, from: P, to: B },
    { segment: 1, index: 0, from: B, to: C },
  ]);
  expect(pieces(build(A))).toEqual([]);
});
