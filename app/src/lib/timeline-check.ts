/** ファイルをまたぐ再生と時刻の変換を、画面を人が見なくても確かめる。 */

import { clock } from "./time";
import type { Timeline } from "./types";

/** 確認のために `Player` が見せる口。 */
export interface PlayerHandle {
  seek(realMs: number): Promise<void>;
  play(): void;
  pause(): void;
  snapshot(): { file: string; clock: string; offsetSeconds: number; paused: boolean; readyState: number };
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function runTimelineCheck(
  timeline: Timeline,
  player: PlayerHandle,
  setClockOffsetSeconds: (seconds: number) => Promise<void>,
) {
  const steps: Record<string, unknown>[] = [];
  const note = (step: string, expected: string) => steps.push({ step, expected, ...player.snapshot() });
  const { spans, gaps, files } = timeline;

  await sleep(2000);
  note("読み込み直後", `${files[0].name} の 0 秒、${clock(spans[0].start_ms)}`);

  const last = spans[spans.length - 1];
  await player.seek(last.start_ms + 60_000);
  await sleep(2500);
  note("最後のファイルの 60 秒目へ", `${files[last.file].name} の 60 秒、${clock(last.start_ms + 60_000)}`);

  if (gaps.length > 0) {
    const gap = gaps[0];
    const after = spans.find((s) => s.start_ms === gap.end_ms);
    await player.seek((gap.start_ms + gap.end_ms) / 2);
    await sleep(2500);
    note("録画停止の中へ", `${after ? files[after.file].name : "?"} の 0 秒、${clock(gap.end_ms)}`);
  }

  // 続けて録画した継ぎ目。無ければ、録画停止をまたぐ所で代える。
  const seam = spans.find((s, i) => i + 1 < spans.length && s.end_ms === spans[i + 1].start_ms) ?? spans[0];
  if (spans.length > 1) {
    await player.seek(seam.end_ms - 3000);
    await sleep(2500);
    player.play();
    await sleep(8000);
    note("ファイルの終わりの 3 秒前から 8 秒再生", `${files[seam.file + 1].name} の 4 秒前後、再生中`);
    player.pause();
    await sleep(500);
  }

  const before = player.snapshot();
  await setClockOffsetSeconds(90);
  await sleep(1000);
  note("時計の補正を +90 秒", `${before.file} の ${before.offsetSeconds.toFixed(1)} 秒のまま、時刻は ${before.clock} の 90 秒後`);

  return {
    files: files.map((f) => f.name),
    spans: spans.map((s) => `${clock(s.start_ms)}-${clock(s.end_ms)}`),
    gaps: gaps.map((g) => `${clock(g.start_ms)}-${clock(g.end_ms)}`),
    steps,
  };
}
