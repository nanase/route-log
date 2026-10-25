/**
 * ファイルをまたぐ再生と時刻の変換、動画を走行から外す操作、停車の候補を探して外す操作を、画面を人が
 * 見なくても確かめる。
 */

import { invoke } from "@tauri-apps/api/core";
import { clock } from "./time";
import type { FileStops, Timeline } from "./types";

/** 確認のために `Player` が見せる口。 */
export interface PlayerHandle {
  seek(realMs: number): Promise<void>;
  play(): void;
  pause(): void;
  snapshot(): { file: string; clock: string; offsetSeconds: number; paused: boolean; readyState: number };
}

/** 確認のために `Timeline`（タイムラインの帯）が見せる口。 */
export interface BarHandle {
  /** その名前の動画の帯を右クリックする。帯が出ていなければ false */
  rightClick(name: string): boolean;
  /** `index` 番目の停車の候補を右クリックする。出ていなければ false */
  rightClickCandidate(index: number): boolean;
  /** 開いている板のボタンを押す */
  pressMenu(): void;
  snapshot(): Record<string, unknown>;
}

/** 確認のために画面全体が見せる口。 */
export interface AppHandle {
  /** いまのタイムライン。走行に使う動画が無ければ null */
  timeline(): Timeline | null;
  /** いまの動画の部品。動画を出していない間は null */
  player(): PlayerHandle | null;
  bar(): BarHandle | undefined;
  /** 外した動画として、手元に覚えているファイル名 */
  remembered(): Promise<string[]>;
  /** 「停車を探す」を押す。探し終えるまで待つ */
  findStops(): Promise<void>;
  /** 停車を探している間の進み具合。探していなければ null */
  stopProgress(): number | null;
  /** 最後に出した知らせ */
  message(): string;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** 条件が成り立つまで待つ。待ちきれなければ、そのまま進む。 */
async function until(done: () => boolean, timeoutMs = 8000) {
  const deadline = performance.now() + timeoutMs;
  while (!done() && performance.now() < deadline) await sleep(100);
}

export async function runTimelineCheck(
  timeline: Timeline,
  player: PlayerHandle,
  setClockOffsetSeconds: (seconds: number) => Promise<void>,
  app: AppHandle,
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
  await setClockOffsetSeconds(0);
  await sleep(1000);

  // ここからは、走行に使わない動画を外す。帯を右クリックして出る板で外し、同じ板で戻す。
  const names = () => (app.timeline()?.files ?? []).map((f) => f.name);
  const state = async () => ({
    files: names(),
    gaps: (app.timeline()?.gaps ?? []).map((g) => `${clock(g.start_ms)}-${clock(g.end_ms)}`),
    bar: app.bar()?.snapshot(),
    remembered: await app.remembered(),
    player: app.player()?.snapshot() ?? null,
    noVideo: document.querySelector(".no-video")?.textContent?.replace(/\s+/g, " ").trim() ?? null,
  });
  /** 帯を右クリックし、出た板のボタンを押して、タイムラインが作り直されるのを待つ。 */
  const toggle = async (name: string) => {
    const count = names().length;
    const found = app.bar()?.rightClick(name) ?? false;
    await sleep(200);
    const menu = app.bar()?.snapshot().menu;
    app.bar()?.pressMenu();
    await until(() => names().length !== count);
    return { found, menu };
  };
  const captures: string[] = [];
  const capture = async (name: string) =>
    captures.push(await invoke<string>("capture_check", { name }).catch((e) => `写せない: ${e}`));

  // いま見ている動画を外す。継ぎ目の後ろの動画で、前の動画と続けて録画したものである。
  const out = files[Math.min(seam.file + 1, files.length - 1)];
  const outSpan = spans[Math.min(seam.file + 1, files.length - 1)];
  const next = files[Math.min(seam.file + 2, files.length - 1)];
  app.bar()?.rightClick(out.name);
  await sleep(300);
  await capture("menu");
  const opened = app.bar()?.snapshot().menu;
  app.bar()?.pressMenu();
  await until(() => names().length === files.length - 1);
  await sleep(2500);
  steps.push({
    step: "見ている動画の帯を右クリックして、走行から外す",
    expected: `板に「走行から外す」と出る。押すと ${out.name} が外れ、動画が ${files.length - 1} 本になる。その時間は録画停止になり、帯は薄く残る。再生位置は、次の動画 ${next.name} の 0 秒へ移る`,
    opened,
    ...(await state()),
  });

  await player.seek((outSpan.start_ms + outSpan.end_ms) / 2);
  await sleep(2500);
  steps.push({
    step: "外した動画の時刻へ",
    expected: `録画停止と同じで、次の動画 ${next.name} の 0 秒になる`,
    ...(await state()),
  });
  await capture("excluded");

  const back = await toggle(out.name);
  await sleep(1500);
  steps.push({
    step: "薄い帯を右クリックして、走行に戻す",
    expected: `板に「走行に戻す」と出る。動画が ${files.length} 本に戻り、手元の覚えも空になる。再生位置は ${next.name} の 0 秒のまま`,
    ...back,
    ...(await state()),
  });

  for (const file of files) await toggle(file.name);
  await sleep(800);
  steps.push({
    step: "すべての動画を外す",
    expected: `動画の場所に、すべて外してあることが出る。タイムラインに、外した動画が ${files.length} 本、薄く並ぶ`,
    ...(await state()),
  });
  await capture("all-excluded");

  for (const file of files) await toggle(file.name);
  await sleep(2500);
  steps.push({
    step: "すべて戻す",
    expected: `動画が ${files.length} 本に戻り、最初の動画 ${files[0].name} の 0 秒が読み込まれる（readyState が 4）`,
    ...(await state()),
  });

  // ここからは、停車の候補を探し、右クリックで外して、また採る。
  const progress: number[] = [];
  const watching = setInterval(() => {
    const done = app.stopProgress();
    const percent = done === null ? null : Math.round(done * 100);
    if (percent !== null && progress[progress.length - 1] !== percent) progress.push(percent);
  }, 500);
  const started = performance.now();
  await app.findStops();
  clearInterval(watching);
  await sleep(500);
  const candidates = () => (app.bar()?.snapshot().candidates as string[] | undefined) ?? [];
  /** 手元に覚えている候補。動画ごとに、採るか外すかを並べる */
  const stored = async () => {
    const found = await invoke<FileStops[]>("get_stops", { files: app.timeline()?.files ?? [] });
    return found.map((entry) => `${entry.name} ${entry.candidates.map((c) => (c.taken ? "採る" : "外す")).join(" ")}`);
  };
  steps.push({
    step: "停車を探す",
    expected: "進み具合が 0% から 100% へ進み、終わると知らせが出る。タイムラインに候補が橙の帯で出る",
    seconds: Math.round((performance.now() - started) / 1000),
    progress,
    message: app.message(),
    candidates: candidates(),
    stored: await stored(),
  });
  await capture("stops");

  /** 最初の候補を右クリックし、出た板を写してから、ボタンを押す。 */
  const flip = async (expected: string, shot: string) => {
    const found = app.bar()?.rightClickCandidate(0) ?? false;
    await sleep(300);
    const menu = app.bar()?.snapshot().candidateMenu;
    await capture(shot);
    const before = candidates()[0];
    app.bar()?.pressMenu();
    await until(() => candidates()[0] !== before, 3000);
    const step = "最初の候補を右クリックして、板のボタンを押す";
    steps.push({ step, expected, found, menu, candidates: candidates(), stored: await stored() });
  };
  await flip("板に「除外」と出る。押すと、最初の候補が除外になり、手元の覚えでも外れる", "stop-menu");
  await flip("板に「採用」と出る。押すと、最初の候補が採用に戻る", "stop-out-menu");

  return {
    files: files.map((f) => f.name),
    spans: spans.map((s) => `${clock(s.start_ms)}-${clock(s.end_ms)}`),
    gaps: gaps.map((g) => `${clock(g.start_ms)}-${clock(g.end_ms)}`),
    steps,
    captures,
  };
}
