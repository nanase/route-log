import { describe, expect, test } from "bun:test";
import { waypointEvent } from "./events";
import {
  clampMotion,
  drawnEpisode,
  drawnWaypoints,
  endpointsOf,
  episodeChoices,
  episodeFileName,
  fileName,
  type FrameRate,
  motionInput,
  othersOf,
} from "./overview";
import type { RideDocument, RideEvent } from "./types";

const ride = { id: "r", title: "2026-10-18", ride_date: "2026-10-18", clock_offset_ms: 0, is_public: false, labels: [] };
const doc = (events: RideEvent[]): RideDocument => ({
  ride: { ...ride, created_at: "", updated_at: "" },
  // 経度 0.01 度ずつ東へ。赤道の上なので、1 区間がおよそ 1.11 km。
  waypoints: [0, 0.01, 0.02, 0.03].map((lon, i) => ({ id: `w${i}`, lon, lat: 0, segment_mode: "straight" as const })),
  route: { segments: [[], [], []] },
  events,
  episodes: [],
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

describe("回の範囲", () => {
  const episodes = [
    { id: "e1", title: "", from: "w0", to: "w1" },
    // 終わりが始まりより前なので、選べない。
    { id: "bad", title: "", from: "w3", to: "w1" },
    { id: "e2", title: "後半", from: "w1", to: "w2" },
  ];
  const choices = episodeChoices(doc([]).waypoints, episodes);

  test("範囲の正しい回だけを、名前と端の経由点の番号で出す", () => {
    expect(choices).toEqual([
      { id: "e1", name: "第 1 回", from: 0, to: 1 },
      { id: "e2", name: "後半", from: 1, to: 2 },
    ]);
    expect(episodeFileName("御嶽山", choices[1])).toBe("御嶽山 後半 概略図.mp4");
    expect(episodeFileName("10/03", { ...choices[1], name: "往路." })).toBe("10_03 往路 概略図.mp4");
  });

  test("全体は、全部の回を合わせた範囲。その外の経由点 4 は描かない", () => {
    const first = drawnEpisode(choices, "e1");
    expect(first).toEqual({ from: 0, to: 1, whole: { from: 0, to: 2 } });
    expect(drawnEpisode(choices, "bad")).toBeNull();
    // 前半なら後ろの経由点 2 から 3 まで、後半なら前の 1 から 2 までを薄く描く。
    expect(othersOf(doc([]), first!)).toEqual([[[0.01, 0], [0.02, 0]]]);
    expect(othersOf(doc([]), drawnEpisode(choices, "e2")!)).toEqual([[[0, 0], [0.01, 0]]]);
  });
});

test("経由点で選んだ範囲には、薄く描く所が無い。終点が始点より後でなければ描かない", () => {
  const drawn = drawnWaypoints(1, 3);
  expect(drawn).toEqual({ from: 1, to: 3, whole: { from: 1, to: 3 } });
  expect(othersOf(doc([]), drawn!)).toEqual([]);
  expect([drawnWaypoints(2, 2), drawnWaypoints(3, 1)]).toEqual([null, null]);
});

test("動きの設定は、受け付ける範囲に収める", () => {
  expect(clampMotion({ fps: "24", leadSeconds: 60, zoomSeconds: -1, holdSeconds: 500, startScale: 0.05 })).toEqual({
    fps: "24",
    leadSeconds: 30,
    zoomSeconds: 0,
    holdSeconds: 120,
    startScale: 0.1,
  });
});

test("知らないフレームレートは既定の 59.94 にし、Rust 側には分数で渡す", () => {
  const motion = clampMotion({
    fps: "50" as FrameRate,
    leadSeconds: 3,
    zoomSeconds: 5,
    holdSeconds: 5,
    startScale: 0.25,
  });
  expect(motion.fps).toBe("59.94");
  expect(motionInput(motion, false).fps).toEqual([60000, 1001]);
  expect(clampMotion({ ...motion, fps: "toString" as FrameRate }).fps).toBe("59.94");
});

test("全経路を見せる時間は、薄く描く経路があるときだけ渡す", () => {
  const motion = { fps: "59.94", leadSeconds: 3, zoomSeconds: 5, holdSeconds: 5, startScale: 0.25 } as const;
  expect([motionInput(motion, true).leadSeconds, motionInput(motion, false).leadSeconds]).toEqual([3, 0]);
});
