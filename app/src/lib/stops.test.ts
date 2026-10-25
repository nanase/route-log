import { describe, expect, test } from "bun:test";
import { foundMessage, replaceStops, stopBands } from "./stops";
import type { FileStops, Timeline, VideoFile } from "./types";

const FIRST = "DJI_20261003050500_0004_D.MP4";
const SECOND = "DJI_20261003055412_0005_D.MP4";

const video = (name: string, startS: number, durationS: number): VideoFile => ({
  path: name,
  name,
  seq: 0,
  start_ms: startS * 1000,
  duration_ms: durationS * 1000,
  codec: "hevc",
});

/** 0 秒から 100 秒の動画と、95 秒から 200 秒の動画。時計を 10 秒進める。 */
const timeline: Timeline = {
  files: [video(FIRST, 0, 100), video(SECOND, 95, 105)],
  clock_offset_ms: 10_000,
  spans: [
    { file: 0, start_ms: 10_000, end_ms: 105_000 },
    { file: 1, start_ms: 105_000, end_ms: 210_000 },
  ],
  gaps: [],
};

const stops = (name: string, ...ranges: [number, number, boolean?][]): FileStops => ({
  name,
  candidates: ranges.map(([start, end, taken]) => ({ start_ms: start * 1000, end_ms: end * 1000, taken: taken ?? true })),
});

describe("停車の候補を実時刻に並べる", () => {
  test("動画の中の位置に、動画の始まりと時計の補正を足す", () => {
    const bands = stopBands(timeline, [stops(SECOND, [10, 20, false])]);
    expect(bands.map((band) => [band.start_ms, band.end_ms, band.file.name, band.choice.taken])).toEqual([
      [115_000, 125_000, SECOND, false],
    ]);
  });

  test("後の動画が始まった所で切る。切って何も残らなければ出さない", () => {
    const bands = stopBands(timeline, [stops(FIRST, [5, 8], [90, 100], [96, 100])]);
    expect(bands.map((band) => [band.start_ms, band.end_ms])).toEqual([
      [15_000, 18_000],
      [100_000, 105_000],
    ]);
  });

  test("走行に使っていない動画と、動画の無い走行は出さない", () => {
    expect(stopBands(timeline, [stops("DJI_20261003063000_0006_D.MP4", [0, 10])])).toEqual([]);
    expect(stopBands(null, [stops(FIRST, [0, 10])])).toEqual([]);
  });
});

test("動画 1 本の候補を差し替え、無ければ足す", () => {
  const before = [stops(FIRST, [0, 10]), stops(SECOND, [0, 10])];
  const next = stops(SECOND, [0, 10, false]);
  expect(replaceStops(before, next)).toEqual([before[0], next]);
  expect(replaceStops([before[0]], next)).toEqual([before[0], next]);
});

test("知らせには、候補の数と外した数を書く", () => {
  expect(foundMessage([])).toBe("停車の候補なし");
  expect(foundMessage([stops(FIRST, [0, 10]), stops(SECOND, [0, 10])])).toBe("停車の候補 2 件");
  expect(foundMessage([stops(FIRST, [0, 10, false], [20, 30])])).toBe("停車の候補 2 件（除外 1 件）");
});
