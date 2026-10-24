/** 回の作り方・直し方・消し方と、録画の長さの出し方、Resolve への書き出しを、画面を人が見なくても確かめる。 */

import { invoke } from "@tauri-apps/api/core";
import { confirmTyped } from "./events";
import type { MapHandle } from "./map-check";
import type { Path } from "./route";
import type { Episode, Exported, RideDocument, RideEvent, Timeline } from "./types";
import type { Mode } from "./ui";

/** 確認のために画面全体が見せる口。 */
export interface EpisodeAppHandle {
  setMode(mode: Mode): void;
  path(): Path;
  events(): RideEvent[];
  timeline(): Timeline | null;
  /** イベントを書き換える */
  changeEvent(id: string, change: (event: RideEvent) => RideEvent): Promise<void>;
  undo(): Promise<void>;
  /** 保存を済ませ、保存されたものを読み直す */
  saved(): Promise<RideDocument | null>;
  /** 回を Resolve へ書き出す。フォルダは選ばせずに `dir` へ書く */
  exportEpisode(episode: Episode, name: string, dir: string, overwrite: boolean): Promise<Exported | null>;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** 長野駅のあたりに、東へ 5 つ。 */
const CLICKS: [number, number][] = [
  [138.15, 36.63],
  [138.19, 36.65],
  [138.23, 36.64],
  [138.25, 36.66],
  [138.29, 36.65],
];

const cards = () => [...document.querySelectorAll<HTMLElement>("aside .episode")];
const text = (element: Element | null | undefined) => element?.textContent?.replace(/\s+/g, " ").trim() ?? null;

/** 回の欄に出ているもの。 */
const shown = (card: HTMLElement) => ({
  name: card.querySelector("input")?.value,
  length: text(card.querySelector(".length")),
  over: card.querySelector(".over") !== null,
  range: [...card.querySelectorAll("select")].map((select) => text(select.selectedOptions[0])),
  problem: text(card.querySelector(".problem")),
});

/** 回の欄のボタンを、文字で探して押す。 */
function press(label: string, within: ParentNode = document) {
  const button = [...within.querySelectorAll<HTMLButtonElement>("aside button")].find(
    (b) => b.textContent?.includes(label) || b.getAttribute("aria-label")?.includes(label),
  );
  if (button === undefined) throw new Error(`「${label}」のボタンがありません`);
  button.click();
}

/** 欄に値を入れ、入れ終えたことを知らせる。 */
function enter(field: HTMLInputElement | HTMLSelectElement | null, value: string) {
  if (field === null) throw new Error("欄がありません");
  field.value = value;
  field.dispatchEvent(new Event("change", { bubbles: true }));
}

export async function runEpisodeCheck(map: MapHandle, app: EpisodeAppHandle) {
  const steps: Record<string, unknown>[] = [];
  const ids = () => app.path().waypoints.map((w) => w.id);
  const episodes = () => app.path().episodes ?? [];
  const note = (step: string, expected: string, extra: Record<string, unknown> = {}) =>
    steps.push({ step, expected, episodes: episodes(), cards: cards().map(shown), ...extra });
  const capture = (name: string) => invoke<string>("capture_check", { name }).catch((e) => `写せない: ${e}`);

  app.setMode("route");
  map.jump([138.22, 36.645], 12);
  await map.idle();
  note("経由点の無い走行", "回は 0。「経由点を 2 つ足すと、回を作れます。」と出る", {
    empty: text(document.querySelector("aside .episodes")),
  });
  for (const at of CLICKS) {
    map.click(at);
    await sleep(300);
  }

  // 出発と到着は、保存のときに置かれる。それを、動画の最初と最後の時刻で確定する。
  await app.saved();
  const spans = app.timeline()?.spans ?? [];
  const [first, last] = [spans[0], spans[spans.length - 1]];
  if (first === undefined || last === undefined) throw new Error("この日付には動画がありません");
  for (const [kind, timeMs] of [
    ["departure", first.start_ms],
    ["arrival", last.end_ms],
  ] as const) {
    const event = app.events().find((e) => e.kind === kind);
    if (event === undefined) throw new Error(`${kind} のイベントがありません`);
    await app.changeEvent(event.id, (e) => confirmTyped(e, timeMs));
  }
  await app.saved();
  const recorded = spans.reduce((sum, span) => sum + span.end_ms - span.start_ms, 0);
  note("経由点 5 個。出発と到着を、動画の最初と最後で確定", "経由点 5 個。回は 0", {
    waypoints: ids(),
    recordedHours: recorded / 3600_000,
  });

  press("足す");
  await sleep(300);
  note("「足す」を押す", "回が 1 つでき、範囲は 1 → 5。名前の欄には「第 1 回」が薄く出る。録画は動画の長さの合計と同じ", {
    placeholder: cards()[0]?.querySelector("input")?.placeholder,
  });

  const [, , , w4, w5] = ids();
  enter(cards()[0].querySelector("input"), "前編");
  await sleep(200);
  enter(cards()[0].querySelectorAll("select")[1], w4);
  await sleep(200);
  press("足す");
  await sleep(300);
  enter(cards()[1].querySelectorAll("select")[0], w4);
  await sleep(300);
  note(
    "1 つ目を「前編」の 1 → 4 に、2 つ目を 4 → 5 にする",
    "前編は 1 → 4、第 2 回は 4 → 5。2 つの録画の合計は、動画の長さの合計と同じ。5 時間を超える回には、警告の印が付く",
  );
  const split = await capture("episodes");

  map.rightClick(CLICKS[3]);
  await sleep(500);
  note("境の経由点 4 を右クリックで消す", "前編は 1 → 3 に縮む。第 2 回は始まりが 5 になり、範囲が正しくないと出る", {
    waypoints: ids(),
    from: episodes()[1]?.from === w5,
  });
  const broken = await capture("broken");

  await app.undo();
  await sleep(300);
  note("取り消し", "経由点 5 個に戻り、前編は 1 → 4、第 2 回は 4 → 5 に戻る", { waypoints: ids() });

  press("第 2 回を消す", cards()[1]);
  await sleep(300);
  note("第 2 回のごみ箱を押す", "回は前編だけになる");
  await app.undo();
  await sleep(300);
  note("取り消し", "第 2 回が戻る");

  const saved = await app.saved();
  note("保存して読み直す", "回と経由点の ID が、画面と同じ", {
    same:
      JSON.stringify(saved?.episodes) === JSON.stringify(episodes()) &&
      JSON.stringify(saved?.waypoints.map((w) => w.id)) === JSON.stringify(ids()),
  });

  // 書き出しは、報告と同じ場所へ。フォルダを選ぶダイアログは、人が見ないと閉じられない。
  const dir = (await invoke<string>("check_path", { name: "resolve" })).replace(/[\\/][^\\/]*$/, "");
  const notice = () => text(document.querySelector(".notice"));
  const front = episodes()[0];
  const written = await app.exportEpisode(front, "前編", dir, false);
  note("前編を Resolve へ書き出す", "FCPXML と EDL を書き、長さ・抜いた停車・マーカーの数と、マーカーの読み込み方が緑の帯に出る", {
    result: written,
    notice: notice(),
  });
  const done = await capture("exported");
  const again = await app.exportEpisode(front, "前編", dir, false);
  note("もう一度書き出す", "書かずに、同じ名前のファイルがあると出て、「上書きする」を押せる", {
    result: again,
    notice: notice(),
  });
  const over = await app.exportEpisode(front, "前編", dir, true);
  note("上書きする", "書き直し、緑の帯に戻る", { result: over?.outcome, notice: notice() });

  return { steps, captures: [split, broken, done] };
}
