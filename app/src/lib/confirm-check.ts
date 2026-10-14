/** 動画を見ながらの時刻の確定を、画面を人が見なくても確かめる。キーは実際のキーイベントで送る。 */

import { invoke } from "@tauri-apps/api/core";
import { confirmTyped, unconfirm } from "./events";
import { distanceM } from "./geo";
import { clock, jstToMs } from "./time";
import type { LonLat, RideEvent, Timeline } from "./types";
import type { Mode } from "./ui";

interface PlayerHandle {
  seek(realMs: number): Promise<void>;
  now(): number;
}

interface MapHandle {
  jump(center: LonLat, zoom: number): void;
  click(at: LonLat): void;
  /** その位置にいちばん近い丸を押す */
  clickHandle(at: LonLat): void;
  /** 地図のその位置を押す。丸の上でも、丸ではなく地図を押す */
  clickMap(at: LonLat): void;
  idle(timeoutMs?: number): Promise<boolean>;
  snapshot(): Record<string, unknown>;
}

interface AppHandle {
  /** 経路入力の画面と、時刻入力の画面を切り替える */
  setMode(mode: Mode): void;
  /** その日付の走行を開き、記録が無ければ作る */
  create(rideDate: string): Promise<void>;
  events(): RideEvent[];
  target(): string | null;
  choose(id: string): void;
  setClockOffsetSeconds(seconds: number): Promise<void>;
  changeEvent(id: string, change: (event: RideEvent) => RideEvent): Promise<void>;
  /** 保存と時刻の推定が済むまで待つ */
  settled(): Promise<void>;
  rideDate(): string;
  /** CSV を報告の隣に `name` の名前で書き出し、その場所を返す */
  exportCsv(name: string): Promise<string>;
  message(): string;
  /** タイムライン。動画が無い間は無い */
  timeline(): TimelineHandle | undefined;
}

interface TimelineHandle {
  zoomIn(): void;
  zoomOut(): void;
  /** その名前の動画の帯を右クリックする */
  rightClick(name: string): boolean;
  /** その番号の録画停止を右クリックする */
  rightClickGap(index: number): boolean;
  /** 開いている板のボタンを押す */
  pressMenu(): void;
  snapshot(): Record<string, unknown>;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const key = (name: string) => window.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true }));

export async function runConfirmCheck(timeline: Timeline, player: PlayerHandle, map: MapHandle, app: AppHandle) {
  const steps: Record<string, unknown>[] = [];
  const shown = () =>
    app
      .events()
      .filter((e) => !e.hidden)
      .map((e) => `${e.time_ms === null ? "--:--:--" : clock(e.time_ms)} ${(e.route_m / 1000).toFixed(2)}km ${e.kind} ${e.label} ${e.status}`);
  const note = (step: string, expected: string, extra: Record<string, unknown> = {}) =>
    steps.push({ step, expected, events: shown(), target: targetLabel(), ...extra });
  const targetLabel = () => app.events().find((e) => e.id === app.target())?.label ?? null;
  const find = (kind: string) => app.events().find((e) => e.kind === kind);

  // 碓氷峠。軽井沢町から安中市へ、県境をまたぐ線を引く。経路は、経路入力の画面で引く。
  app.setMode("route");
  map.jump([138.66, 36.35], 12);
  await map.idle();
  map.click([138.62, 36.345]);
  await sleep(300);
  map.click([138.7, 36.36]);
  // 判定用データを初めて読むので、抽出に時間がかかる。
  for (let i = 0; i < 75 && app.events().length === 0; i++) await sleep(200);
  await sleep(500);
  app.setMode("confirm");
  await sleep(500);
  note("線を引く", "出発・県境・市区町村境・到着が並び、どれも時刻が無い。次の候補は出発");

  const start = timeline.spans[0].start_ms;
  await player.seek(start + 60_000);
  await sleep(1500);
  key("Enter");
  await app.settled();
  note("1 分の所で Enter", `出発が ${clock(start + 60_000)} で確定し、次の候補が県境に移る`);

  const arrival = find("arrival");
  if (arrival) app.choose(arrival.id);
  await player.seek(start + 60_000 + 1_200_000);
  await sleep(1500);
  key("Enter");
  await app.settled();
  const length = find("arrival")?.route_m ?? 0;
  const boundary = find("pref_boundary");
  const expectedBoundary = boundary ? start + 60_000 + (1_200_000 * boundary.route_m) / length : 0;
  note("到着を選び、21 分の所で Enter", `到着が確定し、県境の推定が ${clock(expectedBoundary)} になる`, {
    here: map.snapshot().here,
  });

  // 何も選んでいなければ、推定時刻がいまより後の最初のイベントが、次の候補になる。
  await player.seek(expectedBoundary - 20_000);
  await sleep(1500);
  note("県境の推定の 20 秒前へ", "次の候補が県境になる。案内に「あと 0:20」と出る", {
    timeline: app.timeline()?.snapshot(),
  });
  const next = await invoke<string>("capture_check", { name: "next" }).catch((e) => `写せない: ${e}`);

  app.timeline()?.zoomIn();
  app.timeline()?.zoomIn();
  await sleep(300);
  const zoomed = app.timeline()?.snapshot();
  app.timeline()?.zoomOut();
  app.timeline()?.zoomOut();
  await sleep(300);
  note("タイムラインを 2 段広げて、2 段戻す", "広げると範囲が全体の 4 分の 1 になり、再生位置がその中にある。戻すと全体", {
    zoomed,
    timeline: app.timeline()?.snapshot(),
  });

  await player.seek(start + 60_000 + 300_000);
  await sleep(1500);
  key("a");
  await app.settled();
  note("6 分の所で A", `手動のイベントが ${clock(start + 360_000)} にでき、位置は全体の 4 分の 1（${(length / 4000).toFixed(2)} km）`, {
    here: map.snapshot().here,
  });

  key("p");
  await sleep(1500);
  note("P", "次の候補が 1 つ前のイベントに移り、動画がその時刻へ動く", { now: clock(player.now()) });

  await app.setClockOffsetSeconds(90);
  await app.settled();
  await sleep(600);
  note("時計の補正を +90 秒", "動画で確定した出発・到着・手動と、その間の推定が、どれも 90 秒進む");

  const typed = jstToMs(app.rideDate(), "12:00:00");
  if (boundary && typed !== null) await app.changeEvent(boundary.id, (event) => confirmTyped(event, typed));
  await app.settled();
  await app.setClockOffsetSeconds(0);
  await app.settled();
  await sleep(600);
  note("県境に 12:00:00 を手で入れ、補正を 0 に戻す", "県境は 12:00:00 のまま。出発と到着は 90 秒戻る。順序が合わないので警告が出る");

  const csv = await app.exportCsv("events.csv");
  const captured = await invoke<string>("capture_check", { name: "confirm" }).catch((e) => `写せない: ${e}`);

  key("Backspace");
  await app.settled();
  note("Backspace", "最後に Enter で確定した到着の確定が外れる");

  // 再生のキー。動いた量を、動画の時刻の差で見る。
  const moved = async (keys: string[], waitMs: number) => {
    await player.seek(start + 600_000);
    await sleep(1500);
    const from = player.now();
    for (const name of keys) key(name);
    await sleep(waitMs);
    const seconds = (player.now() - from) / 1000;
    key("k");
    await sleep(300);
    return Number(seconds.toFixed(1));
  };
  const transport = {
    "L を 1 回、3 秒（3 秒ほど進む）": await moved(["l"], 3000),
    "L を 2 回、3 秒（6 秒ほど進む）": await moved(["l", "l"], 3000),
    "J を 1 回、3 秒（3 秒ほど戻る）": await moved(["j"], 3000),
    "J を 2 回、3 秒（6 秒ほど戻る）": await moved(["j", "j"], 3000),
    "→ を 1 回（1 秒進む）": await moved(["ArrowRight"], 1500),
    "K のあと 2 秒（動かない）": await moved(["l", "k"], 2000),
  };

  const exported = app.message();
  const text = (selector: string) => document.querySelector(selector)?.textContent?.replace(/\s+/g, " ").trim() ?? null;

  // 走行に使わない動画を外すと、その動画の中の時刻は、CSV に動画の位置が出ない。
  const clips = () => Number(app.timeline()?.snapshot().clips ?? 0);
  const toggle = async (name: string) => {
    const count = clips();
    app.timeline()?.rightClick(name);
    await sleep(200);
    app.timeline()?.pressMenu();
    for (let i = 0; i < 50 && clips() === count; i++) await sleep(100);
    await sleep(500);
  };
  const first = timeline.files[0].name;
  await toggle(first);
  const csvExcluded = await app.exportCsv("excluded.csv");
  note("最初の動画を外して、CSV を書き出す", "出発の行に時刻は出るが、動画のファイル名と位置は空になる。タイムラインの動画が 1 本減る", {
    csv: csvExcluded,
    timeline: app.timeline()?.snapshot(),
    message: app.message(),
  });
  await toggle(first);

  // 経由点を通った時刻。経路の途中に経由点を足し、時刻入力の画面で、地図のその丸を押して確定する。
  const middle: LonLat = [138.66, 36.3525];
  app.setMode("route");
  map.jump([138.66, 36.35], 12);
  await map.idle();
  map.click(middle);
  await sleep(800);
  await app.settled();
  app.setMode("confirm");
  await sleep(500);
  map.clickHandle(middle);
  // 足したイベントが、保存の応答を待つ間も、一覧の正しい所（到着より前）にあることを見る。
  const placed: string[] = [];
  for (let i = 0; i < 6; i++) {
    placed.push(app.events().map((e) => e.kind).join(" "));
    await sleep(15);
  }
  await sleep(300);
  await app.settled();
  const chosen = targetLabel();
  await player.seek(start + 60_000 + 400_000);
  await sleep(1500);
  key("Enter");
  await app.settled();
  note(
    "経路の途中に経由点を足し、時刻入力の画面で地図のその丸を押して、Enter",
    `経由点 2 が次の候補になり、${clock(start + 460_000)} で確定する。字幕なしのまま。動画は、丸を押しても動かない`,
    { chosen, waypoint: find("waypoint"), handles: map.snapshot().handles, placed: [...new Set(placed)] },
  );

  // 字幕なしにしても、確定した時刻は残る。一覧の行を開いて、切り替えのボタンを押す。
  const row = (id: string | undefined) => document.querySelector<HTMLElement>(`.rows [data-id="${id}"]`);
  const press = (root: ParentNode | null | undefined, name: string) =>
    [...(root?.querySelectorAll("button") ?? [])].find((button) => button.textContent?.trim() === name)?.click();
  const departureRow = row(find("departure")?.id);
  if (departureRow?.getAttribute("aria-expanded") !== "true") departureRow?.click();
  await sleep(300);
  press(departureRow?.closest(".item"), "字幕なし");
  await app.settled();
  note("出発の行を開いて「字幕なし」を押す", "出発が字幕なしになる。確定した時刻は残り、まわりの推定も変わらない", {
    departure: find("departure"),
    all: app.events().map((e) => `${e.time_ms === null ? "--:--:--" : clock(e.time_ms)} ${e.kind} ${e.status}${e.hidden ? " 字幕なし" : ""}`),
  });

  // いくつか選んで、まとめて字幕なしにする。2 つ目の行にチェックを入れ、Shift を押しながら 4 つ目に入れる。
  const boxes = () => [...document.querySelectorAll<HTMLInputElement>(".rows .pick")];
  const hiddenNow = () => app.events().map((e) => (e.hidden ? "なし" : "あり")).join(" ");
  boxes()[1]?.click();
  boxes()[3]?.dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: true }));
  await sleep(300);
  const picked = text(".bulk b");
  const bulk = await invoke<string>("capture_check", { name: "bulk" }).catch((e) => `写せない: ${e}`);
  press(document.querySelector(".bulk"), "字幕なし");
  await app.settled();
  const afterHide = hiddenNow();
  document.querySelector<HTMLInputElement>(".narrow .pick")?.click();
  await sleep(300);
  const pickedAll = text(".bulk b");
  press(document.querySelector(".bulk"), "字幕あり");
  await app.settled();
  note("行を 2 つ目から 4 つ目まで選んで「字幕なし」、すべて選んで「字幕あり」", "3 件を選択と出て、その 3 件が字幕なしになる。すべて選んで戻すと、どれも字幕ありになる。確定した時刻は変わらない", {
    picked,
    afterHide,
    pickedAll,
    afterShow: hiddenNow(),
    bulkLeft: text(".bulk b"),
  });

  // 動画の上の操作の帯。帯で再生・停止しても、倍率の表示が合う。
  const video = document.querySelector<HTMLVideoElement>("video");
  const rate = () => text(".chip.rate");
  await video?.play();
  await sleep(600);
  const playing = rate();
  video?.pause();
  await sleep(300);
  note("動画の上の操作の帯で再生し、止める", "controls が付いている。再生すると ×1、止めると 停止 と出る", {
    controls: video?.controls ?? null,
    playing,
    paused: rate(),
  });

  // 動画と一覧の列の幅。境目を左へドラッグすると、動画が大きくなる。
  const grip = document.querySelector<HTMLElement>(".grip");
  const size = () => {
    const screen = document.querySelector(".side .screen")?.getBoundingClientRect();
    const side = document.querySelector(".side")?.getBoundingClientRect();
    return `列 ${Math.round(side?.width ?? 0)}、動画 ${Math.round(screen?.width ?? 0)}x${Math.round(screen?.height ?? 0)}`;
  };
  const pointer = (type: string, target: EventTarget | null, clientX: number) =>
    target?.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, button: 0, pointerId: 1, clientX, clientY: 300 }));
  const sizeBefore = size();
  const edge = grip?.getBoundingClientRect().left ?? 0;
  pointer("pointerdown", grip, edge + 4);
  pointer("pointermove", window, edge - 100);
  await sleep(300);
  const sizeWider = size();
  pointer("pointermove", window, 100);
  pointer("pointerup", window, 100);
  await sleep(500);
  const sizeWidest = size();
  const wide = await invoke<string>("capture_check", { name: "wide" }).catch((e) => `写せない: ${e}`);
  grip?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  await sleep(500);
  note("境目を左へ 100 px、さらに窓の左端までドラッグし、ダブルクリックで戻す", "列と動画が広がる。広げすぎても、地図と一覧の場所が残る。ダブルクリックで元の幅に戻る", {
    before: sizeBefore,
    wider: sizeWider,
    widest: sizeWidest,
    reset: size(),
    remembered: localStorage.getItem("routelog.side-width"),
    window: `${window.innerWidth}x${window.innerHeight}`,
  });

  // 停車。録画の止まっていた間を停車にすると、その間は進んでいないものとして時刻を推定する。
  // 到着を、最初の録画停止の 10 分後にしておく。経由点から到着までの間に、録画停止が挟まる。
  const gap = timeline.gaps[0];
  const boundaryNow = find("pref_boundary");
  if (boundaryNow) await app.changeEvent(boundaryNow.id, unconfirm);
  const arrivalNow = find("arrival");
  if (arrivalNow) await app.changeEvent(arrivalNow.id, (event) => confirmTyped(event, gap.end_ms + 600_000));
  await app.settled();
  const estimate = () => {
    const time = find("bridge")?.time_ms ?? null;
    return time === null ? null : clock(time);
  };
  const [from, to, bridge] = [find("waypoint"), find("arrival"), find("bridge")];
  let expectedBridge = "";
  if (from?.time_ms != null && to?.time_ms != null && bridge) {
    const moving = to.time_ms - from.time_ms - (gap.end_ms - gap.start_ms);
    expectedBridge = clock(from.time_ms + (moving * (bridge.route_m - from.route_m)) / (to.route_m - from.route_m));
  }
  const bridgeBefore = estimate();
  app.timeline()?.rightClickGap(0);
  await sleep(300);
  const asked = app.timeline()?.snapshot().gapMenu;
  const stopMenu = await invoke<string>("capture_check", { name: "stop-menu" }).catch((e) => `写せない: ${e}`);
  app.timeline()?.pressMenu();
  await sleep(300);
  await app.settled();
  note(
    "到着を最初の録画停止の 10 分後にして、その録画停止を右クリックし、停車にする",
    `停車のイベントができる。始まりは ${clock(gap.start_ms)}、長さは録画停止と同じ。橋の推定が ${expectedBridge} に早まり、録画停止より前になる`,
    { asked, stop: find("stop"), bridgeBefore, bridgeAfter: estimate(), stops: app.timeline()?.snapshot().stops },
  );
  const stopped = await invoke<string>("capture_check", { name: "stopped" }).catch((e) => `写せない: ${e}`);
  app.timeline()?.rightClickGap(0);
  await sleep(300);
  const askedAgain = app.timeline()?.snapshot().gapMenu;
  app.timeline()?.pressMenu();
  await sleep(300);
  await app.settled();
  note("同じ録画停止をもう一度右クリックし、停車をやめる", "停車のイベントが消え、橋の推定が元に戻る", {
    asked: askedAgain,
    stop: find("stop") ?? null,
    bridgeAfter: estimate(),
    stops: app.timeline()?.snapshot().stops,
  });

  // 録画したまま止まっていた間。動画の位置で S を押して始まりを入れ、進めてもう一度押して終わりを入れる。
  const stopButton = () => text(".side > section .more .btn:last-child");
  const pauseAt = (from?.time_ms ?? start) + 120_000;
  await player.seek(pauseAt);
  await sleep(1500);
  const buttonBefore = stopButton();
  key("s");
  await sleep(200);
  await app.settled();
  const opened = { button: stopButton(), stop: find("stop"), bridge: estimate(), message: app.message() };
  await player.seek(pauseAt + 600_000);
  await sleep(1500);
  key("s");
  await sleep(200);
  await app.settled();
  note(
    "経由点の 2 分後で S、その 10 分後でもう一度 S",
    `1 度目で、始まりが ${clock(pauseAt)} の停車ができ、ボタンが「停車おわり」に変わる。終わりを入れるまで推定は変わらない。2 度目で長さが 10 分になる。橋は停車より先にあるので、推定が遅くなる`,
    {
      buttonBefore,
      opened,
      button: stopButton(),
      stop: find("stop"),
      bridge: estimate(),
      message: app.message(),
      order: app.events().map((e) => `${e.kind} ${(e.route_m / 1000).toFixed(2)}`),
    },
  );
  const paused = await invoke<string>("capture_check", { name: "paused" }).catch((e) => `写せない: ${e}`);

  // 停車の場所を地図で決める。止まる 1 分前にいたと推定される所を押す。
  const pressIn = (selector: string, label: string) =>
    [...document.querySelectorAll<HTMLButtonElement>(selector)].find((b) => b.textContent?.includes(label))?.click();
  const stopId = find("stop")?.id;
  const stopRow = document.querySelector<HTMLElement>(`.rows [data-id="${stopId}"]`);
  if (stopRow?.getAttribute("aria-expanded") !== "true") stopRow?.click();
  await sleep(300);
  pressIn(".place .btn", "地図で決める");
  await sleep(200);
  const banner = text(".placing");
  await player.seek(pauseAt - 60_000);
  await sleep(1500);
  const placing = await invoke<string>("capture_check", { name: "placing" }).catch((e) => `写せない: ${e}`);
  const spot = map.snapshot().here as LonLat | null;
  const estimatedAt = find("stop")?.route_m ?? null;
  if (spot !== null) map.clickMap(spot);
  await sleep(200);
  await app.settled();
  const hereAt = async (realMs: number) => {
    await player.seek(realMs);
    await sleep(800);
    return map.snapshot().here as LonLat | null;
  };
  const [inside, end, after] = [
    await hereAt(pauseAt + 300_000),
    await hereAt(pauseAt + 600_000),
    await hereAt(pauseAt + 600_000 + 2_000),
  ];
  const apart = (a: LonLat | null, b: LonLat | null) => (a === null || b === null ? null : Math.round(distanceM(a, b)));
  note(
    "停車の行で「地図で決める」を押し、止まる 1 分前の推定位置を地図で押す",
    "地図に案内が出る。停車の位置が押した所に移り、停車の間の現在位置は押した所に止まる。停車の終わりで飛ばず、2 秒後も数十 m 以内",
    {
      banner,
      bannerAfter: text(".placing"),
      estimatedAt,
      stop: find("stop"),
      fromSpot: { inside: apart(spot, inside), end: apart(spot, end), twoSecondsAfter: apart(spot, after) },
      message: app.message(),
    },
  );
  pressIn(".place .btn", "推定に戻す");
  await sleep(200);
  await app.settled();
  note("「推定に戻す」を押す", "停車の位置が、決める前の位置に戻る", { estimatedAt, stop: find("stop") });

  // 動画の無い走行。動画の無い日付の走行を作り、経路を引いて、時刻を一覧で手で入れる。
  const errors: string[] = [];
  const caught = (event: ErrorEvent | PromiseRejectionEvent) =>
    errors.push(String("reason" in event ? event.reason : event.message));
  window.addEventListener("error", caught);
  window.addEventListener("unhandledrejection", caught);
  /** 一覧の行を押して直す欄を開き、時刻の欄に打ち込む。人がするのと同じ道を通す。 */
  const typeTime = async (id: string | undefined, value: string) => {
    const row = document.querySelector<HTMLElement>(`.rows [data-id="${id}"]`);
    if (row?.getAttribute("aria-expanded") !== "true") row?.click();
    await sleep(300);
    const input = row?.closest(".item")?.querySelector<HTMLInputElement>('input[type="time"]');
    if (!input) return false;
    input.value = value;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    await app.settled();
    return true;
  };

  await app.create("2026-10-04");
  app.setMode("route");
  map.jump([138.66, 36.35], 12);
  await map.idle();
  map.click([138.62, 36.345]);
  await sleep(300);
  map.click([138.7, 36.36]);
  for (let i = 0; i < 75 && app.events().length === 0; i++) await sleep(200);
  await sleep(500);
  app.setMode("confirm");
  await sleep(800);
  note(
    "動画の無い走行で、時刻入力の画面を開く",
    "一覧に出発・県境・市区町村境・到着が並ぶ。動画とタイムラインの場所に、動画が無いことと、時刻は一覧で手で入れられることが出る",
    { video: text(".no-video"), timeline: text(".timeline .empty"), guide: text(".side > section .what") },
  );
  const empty = await invoke<string>("capture_check", { name: "no-video" }).catch((e) => `写せない: ${e}`);

  const typedDeparture = await typeTime(find("departure")?.id, "09:00:00");
  note("出発の行を押して、時刻の欄に 09:00:00 を入れる", "出発が 09:00:00 で確定する。選んだ出発が、案内に出る", {
    typed: typedDeparture,
    guide: text(".side > section .what"),
  });

  // 動画に頼るキー。動画が無いので、押しても何も起きない。エラーにもならない。
  const untouched = JSON.stringify(app.events());
  for (const name of ["Enter", "a", " ", "l", "j", "k", "ArrowRight", "Backspace"]) key(name);
  await app.settled();
  note("動画に頼るキーを押す（Enter・A・Space・L・J・K・→・Backspace）", "イベントは何も変わらない。エラーも出ない", {
    unchanged: JSON.stringify(app.events()) === untouched,
    errors: [...errors],
  });

  key("n");
  await sleep(500);
  note("N", "次の候補が、出発の次のイベント（県境）に移る。エラーは出ない", { errors: [...errors] });

  const typedArrival = await typeTime(find("arrival")?.id, "09:20:00");
  note("到着の行を押して、時刻の欄に 09:20:00 を入れる", "到着が確定し、県境と市区町村境の推定が、出発と到着の間（09:07 ごろ）に出る", {
    typed: typedArrival,
  });
  const filled = await invoke<string>("capture_check", { name: "no-video-typed" }).catch((e) => `写せない: ${e}`);

  const csvNoVideo = await app.exportCsv("no-video.csv");
  note("動画の無い走行の CSV を書き出す", "4 件を書き出す。どの行も時刻があり、動画のファイル名と位置は空", {
    csv: csvNoVideo,
    message: app.message(),
    errors: [...errors],
  });
  window.removeEventListener("error", caught);
  window.removeEventListener("unhandledrejection", caught);

  return { steps, transport, captures: [next, captured, bulk, wide, stopMenu, stopped, paused, placing, empty, filled], csv, message: exported };
}
