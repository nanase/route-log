import { describe, expect, test } from "bun:test";
import {
  addEpisode,
  exportBlocker,
  exportedMessage,
  extentOf,
  nameOf,
  RECORDED_LIMIT_MS,
  recordedMs,
  recordedOf,
  repoint,
} from "./episodes";
import { appendWaypoint, insertWaypoint, type Path, removeWaypoint, segmentLengths } from "./route";
import type { Episode, LonLat, RideEvent, Span } from "./types";

const points: LonLat[] = [0, 1, 2, 3, 4].map((i) => [138 + i / 10, 36]);
const path: Path = points.reduce((p, at) => appendWaypoint(p, at), { waypoints: [], route: { segments: [] } } as Path);
const id = (index: number) => path.waypoints[index].id;
const episode = (from: number, to: number, title = ""): Episode => ({ id: `e${from}-${to}`, title, from: id(from), to: id(to) });

describe("回の範囲", () => {
  test("経由点の ID から番号を引く", () => {
    expect(extentOf(path.waypoints, episode(1, 3))).toEqual({ from: 1, to: 3 });
  });

  test("端が無いか、始まりが終わりより前でなければ、範囲は無い", () => {
    expect(extentOf(path.waypoints, episode(3, 1))).toBeNull();
    expect(extentOf(path.waypoints, episode(2, 2))).toBeNull();
    expect(extentOf(path.waypoints, { ...episode(0, 2), to: "gone" })).toBeNull();
  });

  test("経由点を足しても消しても、同じ経由点を指したまま", () => {
    const inserted = insertWaypoint(path, 0, 0, [138.05, 36]);
    expect(extentOf(inserted.waypoints, episode(1, 3))).toEqual({ from: 2, to: 4 });
    const removed = removeWaypoint(path, 2);
    expect(repoint([episode(1, 3)], path.waypoints, removed.waypoints)).toEqual([episode(1, 3)]);
    expect(extentOf(removed.waypoints, episode(1, 3))).toEqual({ from: 1, to: 2 });
  });
});

describe("端の経由点を消したとき", () => {
  test("始まりなら次の、終わりなら手前の経由点へ付け替える", () => {
    const [first, second] = [episode(1, 3), episode(3, 4)];
    const removed = removeWaypoint(path, 3).waypoints;
    const [a, b] = repoint([first, second], path.waypoints, removed);
    expect(extentOf(removed, a)).toEqual({ from: 1, to: 2 });
    expect(extentOf(removed, b)).toBeNull();
    expect(b).toEqual({ ...second, from: id(4) });
  });

  test("範囲が無くなっても、回は消さずに残す", () => {
    const removed = removeWaypoint(path, 2).waypoints;
    const [narrow] = repoint([episode(1, 2)], path.waypoints, removed);
    expect(narrow).toEqual({ ...episode(1, 2), to: id(1) });
    expect(extentOf(removed, narrow)).toBeNull();
  });

  test("付け替える先が無ければ、ID をそのまま残す", () => {
    const removed = removeWaypoint(path, 4).waypoints;
    const [a] = repoint([{ ...episode(1, 2), from: id(4) }], path.waypoints, removed);
    expect(a.from).toBe(id(4));
  });

  test("経由点が減っていなければ、同じ並びを返す", () => {
    const episodes = [episode(0, 4)];
    expect(repoint(episodes, path.waypoints, path.waypoints)).toBe(episodes);
    expect(repoint(episodes, path.waypoints, insertWaypoint(path, 0, 0, [138.05, 36]).waypoints)).toBe(episodes);
  });
});

describe("回を足す", () => {
  test("経路の全体を範囲にする", () => {
    const [added] = addEpisode([], path.waypoints);
    expect(extentOf(path.waypoints, added)).toEqual({ from: 0, to: 4 });
    expect(added.title).toBe("");
  });

  test("経由点が 2 つ無ければ足さない", () => {
    expect(addEpisode([], path.waypoints.slice(0, 1))).toEqual([]);
  });
});

test("名前が無ければ、並びの番号で呼ぶ", () => {
  expect(nameOf(episode(0, 1, "前編"), 0)).toBe("前編");
  expect(nameOf(episode(0, 1, "  "), 1)).toBe("パート 2");
});

describe("録画の長さ", () => {
  const span = (start_ms: number, end_ms: number): Span => ({ file: 0, start_ms, end_ms });

  test("範囲の中の、録画していて停車していない時間を数える", () => {
    const spans = [span(0, 100), span(200, 300)];
    expect(recordedMs(spans, [], 0, 300)).toBe(200);
    expect(recordedMs(spans, [], 50, 250)).toBe(100);
    expect(recordedMs(spans, [], 100, 200)).toBe(0);
    // 録画の間の 20、録画を止めた間の 100、録画をまたぐ 20 + 20。
    const stops = [
      { start_ms: 10, end_ms: 30 },
      { start_ms: 80, end_ms: 220 },
    ];
    expect(recordedMs(spans, stops, 0, 300)).toBe(200 - 20 - 40);
    expect(recordedMs(spans, stops, 40, 300)).toBe(160 - 40);
  });

  test("両端の経由点を通った時刻の間で数え、時刻が推定できなければ分からない", () => {
    const reached = segmentLengths(path).reduce((sum, length, i) => [...sum, sum[i] + length], [0]);
    const confirmed = (route_m: number, time_ms: number): RideEvent => ({
      id: `c${route_m}`,
      kind: "custom",
      label: "",
      route_m,
      time_ms,
      status: "confirmed",
      detail: {},
    });
    const doc = { ...path, events: [confirmed(0, 0), confirmed(reached[2], 6 * 3600_000)] };
    const spans = [span(0, 6 * 3600_000)];
    expect(recordedOf(doc, spans, episode(0, 2))).toBe(6 * 3600_000);
    expect(recordedOf(doc, spans, episode(0, 1))).toBe(3 * 3600_000);
    expect(recordedOf(doc, spans, episode(0, 2))).toBeGreaterThan(RECORDED_LIMIT_MS);
    expect(recordedOf(doc, spans, episode(2, 3))).toBeNull();
    expect(recordedOf(doc, spans, episode(2, 1))).toBeNull();
  });
});

describe("Resolve への書き出し", () => {
  const spans: Span[] = [{ file: 0, start_ms: 0, end_ms: 1000 }];

  test("書き出せない理由を、直す順に言う", () => {
    expect(exportBlocker(false, null, null)).toBe("パートの範囲が正しくありません");
    expect(exportBlocker(true, null, null)).toBe("この走行に使う動画がありません");
    expect(exportBlocker(true, spans, null)).toContain("時刻が推定できると");
    expect(exportBlocker(true, spans, 0)).toBe("パートの範囲に、録画している所がありません");
    expect(exportBlocker(true, spans, 1)).toBeNull();
  });

  test("書き出した中身と、マーカーの読み込み方を知らせる", () => {
    const written = {
      outcome: "written",
      fcpxml: "a.fcpxml",
      edl: "a.edl",
      lengthMs: 9_560_000,
      cuts: 3,
      cutMs: 74_400,
      markers: 28,
      unplaced: 0,
    } as const;
    expect(exportedMessage("前編", written)).toStartWith(
      "「前編」のソース 2:39:20。停車 3 か所（1 分 14 秒）を抜き、マーカーは 28 個です。マーカーは、",
    );
    expect(exportedMessage("前編", written)).toContain("「EDL からタイムラインマーカー」");
    const plain = exportedMessage("前編", { ...written, cuts: 0, cutMs: 0, unplaced: 2 });
    expect(plain).toContain("マーカーは 28 個です。2 個のイベントは、録画の外か抜いた所にあるので");
    expect(plain).not.toContain("停車");
  });
});
