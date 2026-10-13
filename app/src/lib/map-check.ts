/** 地図の表示と、経路の編集・保存を、画面を人が見なくても確かめる。 */

import { invoke } from "@tauri-apps/api/core";
import { line, type Path } from "./route";
import type { LonLat, RideDocument, RideEvent, RideSummary } from "./types";
import type { Basemap } from "./ui";

/** 確認のために `RouteMap` が見せる口。 */
export interface MapHandle {
  jump(center: LonLat, zoom: number): void;
  click(at: LonLat): void;
  drag(from: LonLat, to: LonLat): void;
  rightClick(at: LonLat): void;
  shiftClick(at: LonLat): void;
  /** 丸をクリックする。経由点なら、選ぶことになる */
  clickHandle(at: LonLat): void;
  idle(timeoutMs?: number): Promise<boolean>;
  snapshot(): Record<string, unknown>;
}

/** 確認のために画面全体が見せる口。 */
export interface AppHandle {
  /** 道路データを開く。重ねないなら null */
  loadRoads: (() => Promise<void>) | null;
  /** 判定用データを開く。抽出を確かめないなら null */
  loadGeo: (() => Promise<void>) | null;
  /** 判定用データの無いフォルダを開こうとして、わざと失敗させる */
  failGeo(): Promise<void>;
  /** 出ている知らせを、すべて閉じる */
  clearNotices(): void;
  events(): RideEvent[];
  setSnap(on: boolean): void;
  create(rideDate: string): Promise<void>;
  path(): Path;
  /** 保存を済ませ、保存されたものを読み直す */
  saved(): Promise<RideDocument | null>;
  list(): Promise<RideSummary[]>;
  message(): string;
  undo(): Promise<void>;
  redo(): Promise<void>;
  /** 取り消せる編集と、やり直せる編集の数 */
  history(): { undo: number; redo: number };
  setBasemap(basemap: Basemap): void;
  /** 道路の線を出し入れする */
  setRoads(shown: boolean): void;
  /** 経路の全体を表示する */
  fit(): void;
  /** 一覧でイベントを選び、地図で目立たせる */
  focusEvent(id: string): void;
  /** 設定のダイアログを出し入れする */
  showSettings(shown: boolean): Promise<void>;
  /** 走行の切り替えの板を出し入れする */
  showRides(shown: boolean): Promise<void>;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** 条件が成り立つまで待つ。待ちきれなければ、そのまま進む。 */
async function until(done: () => boolean, timeoutMs = 15_000) {
  const deadline = performance.now() + timeoutMs;
  while (!done() && performance.now() < deadline) await sleep(200);
}

/** 長野駅のあたり。国道と県道がどちらも画面に入る。 */
const CENTER: LonLat = [138.19, 36.645];
const CLICKS: LonLat[] = [
  [138.15, 36.63],
  [138.19, 36.65],
  [138.23, 36.64],
  [138.25, 36.66],
];

export async function runMapCheck(map: MapHandle, app: AppHandle) {
  const steps: Record<string, unknown>[] = [];
  const note = (step: string, expected: string, extra: Record<string, unknown> = {}) =>
    steps.push({ step, expected, ...map.snapshot(), ...extra });
  const capture = (name: string) => invoke<string>("capture_check", { name }).catch((e) => `写せない: ${e}`);

  map.jump(CENTER, 12);
  const base = await map.idle();
  note("背景地図", "タイルが読み込まれ、エラーが無い", { idle: base });

  if (app.loadRoads !== null) {
    await app.loadRoads();
    await sleep(500);
    const roads = await map.idle();
    note("道路データ", "国道も都道府県道も 1 本以上ある", { idle: roads, message: app.message() });
  }

  // エラーは、上の帯の下に赤い帯で残る。
  await app.failGeo();
  await sleep(300);
  note("判定データの無いフォルダを開く", "「判定データを開けません。」と出る", { message: app.message() });
  const notice = await capture("notice");
  app.clearNotices();

  // 背景地図の切り替え。どれも地理院タイルである。
  for (const basemap of ["std", "photo", "pale"] as const) {
    app.setBasemap(basemap);
    await sleep(300);
    const loaded = await map.idle();
    note(`背景地図を ${basemap} にする`, `出ている背景が ${basemap} だけで、タイルが読み込まれ、エラーが無い`, { idle: loaded });
  }

  if (app.loadRoads !== null) {
    app.setRoads(false);
    await sleep(300);
    note("道路の線を隠す", "roads.shown が false");
    app.setRoads(true);
    await sleep(300);
  }

  await app.create("2026-10-03");
  for (const at of CLICKS) {
    map.click(at);
    await sleep(300);
  }
  note("地図を 4 回クリック", "経由点 4 個、頂点を足す丸 3 個", { line: line(app.path()) });

  await app.undo();
  await sleep(300);
  note("取り消し", "経由点 3 個。やり直せる編集が 1 つ", { history: app.history() });
  await app.redo();
  await sleep(300);
  note("やり直し", "経由点 4 個に戻る。やり直せる編集は 0", { history: app.history(), line: line(app.path()) });

  // 最初の区間のまん中の丸をつかんで、北へ動かす。
  map.drag([138.17, 36.64], [138.17, 36.66]);
  await sleep(300);
  note("線の途中の丸をドラッグ", "頂点 1 個が 138.17, 36.66 のあたりにでき、最初の区間が手修正になる", {
    modes: app.path().waypoints.map((w) => w.segment_mode),
    segments: app.path().route.segments,
  });

  map.drag(CLICKS[2], [138.23, 36.62]);
  await sleep(300);
  note("経由点 3 をドラッグ", "経由点 3 が 138.23, 36.62 のあたりへ動く", { line: line(app.path()) });

  map.clickHandle(CLICKS[1]);
  await sleep(300);
  note("経由点 2 をクリック", "経由点 2 が選ばれる（selected が 1）。結び方と削除のボタンがそばに出る");
  const edited = await capture("edited");
  map.clickHandle(CLICKS[1]);
  await sleep(300);
  note("もう一度クリック", "選ぶのをやめる（selected が null）");

  map.jump([139.5, 35.5], 9);
  await sleep(300);
  app.fit();
  await map.idle();
  note("経路の全体を表示", "地図が経路のある長野市のあたり（138.2, 36.64）へ戻る");
  map.jump(CENTER, 12);
  await map.idle();

  map.rightClick([138.17, 36.66]);
  await sleep(300);
  note("頂点を右クリック", "頂点 0 個。最初の区間が直線に戻る", {
    modes: app.path().waypoints.map((w) => w.segment_mode),
  });

  map.rightClick(CLICKS[3]);
  await sleep(300);
  const saved = await app.saved();
  // 保存は JSON を読み書きするので、数の最後の桁が 1 つずれることがある。位置としては
  // 0.1 mm にも満たないので、小数 9 桁までで比べる。
  const rounded = (path: unknown) =>
    JSON.stringify(path, (_, value) => (typeof value === "number" ? Number(value.toFixed(9)) : value));
  const written = rounded({ waypoints: saved?.waypoints, route: saved?.route });
  const shown = rounded({ waypoints: app.path().waypoints, route: app.path().route });
  note("最後の経由点を右クリックして保存", "保存されたものが画面と同じで、経由点 3 個", {
    same: written === shown,
    ...(written === shown ? {} : { written, shown }),
    saved: saved && { waypoints: saved.waypoints.length, segments: saved.route.segments, title: saved.ride.title },
    list: await app.list(),
  });

  const captures = [notice, edited];
  if (app.loadGeo !== null) {
    // 碓氷峠。軽井沢町から安中市へ、県境をまたぐ線を引く。
    await app.loadGeo();
    await app.create("2026-10-04");
    map.jump([138.66, 36.35], 12);
    await map.idle();
    map.click([138.62, 36.345]);
    await sleep(300);
    map.click([138.7, 36.36]);
    // 判定用データを初めて読むので、抽出に時間がかかる。
    await until(() => app.events().length > 0);
    await sleep(500);
    note("県境をまたぐ線を引く", "県境と市区町村境のイベントが 1 件ずつできる", {
      events: app.events().map((e) => `${(e.route_m / 1000).toFixed(1)} km ${e.kind} ${e.label} ${e.status}`),
      message: app.message(),
    });
    const boundary = app.events().find((event) => event.kind === "pref_boundary");
    if (boundary) app.focusEvent(boundary.id);
    await sleep(1200);
    await map.idle();
    captures.push(await capture("events"));

    // 長野市内の国道 19 号の上の 2 点。道に沿わせると、道のり（約 2.66 km）で結ばれる。
    const [from, to]: LonLat[] = [
      [138.17128, 36.63714],
      [138.14595, 36.62546],
    ];
    app.setSnap(true);
    await app.create("2026-10-05");
    map.jump([138.158, 36.631], 13);
    await map.idle();
    map.click(from);
    await sleep(300);
    map.click(to);
    await sleep(2500);
    note("道に沿わせて 2 点を結ぶ", "線が道をたどり、頂点が 90 個ほどになる。国道 19 号の路線のイベントが出る", {
      points: line(app.path()).length,
      modes: app.path().waypoints.map((w) => w.segment_mode),
      events: app.events().map((e) => `${(e.route_m / 1000).toFixed(1)} km ${e.kind} ${e.label} ${e.status}`),
    });
    captures.push(await capture("snapped"));

    const middle = line(app.path())[50];
    map.click(middle);
    await sleep(2500);
    note("線の途中をクリック", "経由点が 3 個になり、どちらの区間も吸着のまま。線の点の数はほぼ変わらない", {
      points: line(app.path()).length,
      modes: app.path().waypoints.map((w) => w.segment_mode),
    });

    // 道から 150 m ほど外れた所をクリックする。縮尺 13 では画面上で 10 px ほどなので、道の上へ寄る。
    const off: LonLat = [from[0] + 0.0012, from[1] + 0.0011];
    await app.create("2026-10-06");
    map.click(off);
    await sleep(1500);
    const placed = app.path().waypoints[0];
    note("道から外れた所をクリック", "経由点が、クリックした所ではなく道の上に置かれる", {
      clicked: off,
      placed: placed && [placed.lon, placed.lat],
      movedDegrees: placed && Math.hypot(placed.lon - off[0], placed.lat - off[1]).toFixed(5),
    });
    await app.create("2026-10-05");

    map.shiftClick(from);
    await sleep(2500);
    note("Shift を押しながら経由点 1 をクリック", "最初の区間だけが直線に戻る", {
      points: line(app.path()).length,
      modes: app.path().waypoints.map((w) => w.segment_mode),
    });
  }

  // 走行の切り替えの板と、設定のダイアログ。見た目は画面の写しで確かめる。
  await app.showRides(true);
  captures.push(await capture("rides"));
  await app.showRides(false);
  await app.showSettings(true);
  captures.push(await capture("settings"));
  await app.showSettings(false);

  return { steps, captures };
}
