import type { FileStops, StopChoice, Timeline, VideoFile } from "./types";

/** タイムラインに出す停車の候補。実時刻の範囲と、元の動画の中の位置を持つ。 */
export interface StopBand {
  file: VideoFile;
  choice: StopChoice;
  start_ms: number;
  end_ms: number;
}

/**
 * 停車の候補を、実時刻の軸に並べる。走行に使っていない動画の候補は出さない。
 *
 * 後の動画が始まった所で前の動画を切っているので、候補もそこで切る。
 */
export function stopBands(timeline: Timeline | null, stops: FileStops[]): StopBand[] {
  if (timeline === null) return [];
  return timeline.spans.flatMap((span) => {
    const file = timeline.files[span.file];
    const found = stops.find((entry) => entry.name === file.name);
    if (found === undefined) return [];
    const fileStart = file.start_ms + timeline.clock_offset_ms;
    return found.candidates.flatMap((choice) => {
      const start_ms = Math.max(fileStart + choice.start_ms, span.start_ms);
      const end_ms = Math.min(fileStart + choice.end_ms, span.end_ms);
      return end_ms > start_ms ? [{ file, choice, start_ms, end_ms }] : [];
    });
  });
}

/** 動画 1 本の候補を、新しく受け取ったものに差し替える。 */
export function replaceStops(stops: FileStops[], next: FileStops): FileStops[] {
  return stops.some((entry) => entry.name === next.name)
    ? stops.map((entry) => (entry.name === next.name ? next : entry))
    : [...stops, next];
}

/** 停車を探し終えた走行に出す知らせ。 */
export function foundMessage(stops: FileStops[]): string {
  const all = stops.flatMap((entry) => entry.candidates);
  if (all.length === 0) return "停車の候補は見つかりませんでした";
  const out = all.filter((choice) => !choice.taken).length;
  return `停車の候補が ${all.length} 件あります${out > 0 ? `（${out} 件は外してあります）` : ""}`;
}
