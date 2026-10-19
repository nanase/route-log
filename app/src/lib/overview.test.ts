import { describe, expect, test } from "bun:test";
import { waypointEvent } from "./events";
import { clampMotion, endpointsOf, fileName } from "./overview";
import type { RideDocument, RideEvent } from "./types";

const ride = { id: "r", title: "2026-10-18", ride_date: "2026-10-18", clock_offset_ms: 0, is_public: false };
const doc = (events: RideEvent[]): RideDocument => ({
  ride: { ...ride, created_at: "", updated_at: "" },
  // 経度 0.01 度ずつ東へ。赤道の上なので、1 区間がおよそ 1.11 km。
  waypoints: [0, 0.01, 0.02, 0.03].map((lon) => ({ lon, lat: 0, segment_mode: "straight" as const })),
  route: { segments: [[], [], []] },
  events,
});

describe("範囲の端", () => {
  test("経由点ごとに、名前と始点からの距離を出す。名前は、付けてあればそれを使う", () => {
    const renamed = { ...waypointEvent([0.02, 0], 2, 2224), label: "峠" };
    const arrival: RideEvent = { ...renamed, id: "a", kind: "arrival", label: "宿", detail: {} };
    const ends = endpointsOf(doc([renamed, arrival]));
    expect(ends.map((e) => e.name)).toEqual(["出発", "経由点 2", "峠", "宿"]);
    expect(ends.map((e) => e.km.toFixed(2))).toEqual(["0.00", "1.11", "2.22", "3.34"]);
  });

  test("ファイルの名前に、経路の一部なら経由点の番号を添える", () => {
    expect(fileName("帰省", 0, 3, 4)).toBe("帰省 概略図.mp4");
    expect(fileName("帰省", 0, 2, 4)).toBe("帰省 概略図 1-3.mp4");
  });
});

test("動きの設定は、受け付ける範囲に収める", () => {
  expect(clampMotion({ zoomSeconds: -1, holdSeconds: 500, startScale: 0.05 })).toEqual({
    zoomSeconds: 0,
    holdSeconds: 120,
    startScale: 0.1,
  });
});
