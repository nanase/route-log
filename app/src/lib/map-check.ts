/** 地図の表示と、経路の編集・保存を、画面を人が見なくても確かめる。 */

import { invoke } from "@tauri-apps/api/core";
import { line, type Path } from "./route";
import type { LonLat, RideDocument, RideEvent, RideSummary } from "./types";

/** 確認のために `RouteMap` が見せる口。 */
export interface MapHandle {
  jump(center: LonLat, zoom: number): void;
  click(at: LonLat): void;
  drag(from: LonLat, to: LonLat): void;
  rightClick(at: LonLat): void;
  idle(timeoutMs?: number): Promise<boolean>;
  snapshot(): Record<string, unknown>;
}

/** 確認のために画面全体が見せる口。 */
export interface AppHandle {
  /** 道路データを開く。重ねないなら null */
  loadRoads: (() => Promise<void>) | null;
  /** 判定用データを開く。抽出を確かめないなら null */
  loadGeo: (() => Promise<void>) | null;
  events(): RideEvent[];
  create(rideDate: string): Promise<void>;
  edit(change: (path: Path) => Path): Promise<void>;
  path(): Path;
  /** 保存を済ませ、保存されたものを読み直す */
  saved(): Promise<RideDocument | null>;
  list(): Promise<RideSummary[]>;
  message(): string;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

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

  await app.create("2026-10-03");
  for (const at of CLICKS) {
    map.click(at);
    await sleep(300);
  }
  note("地図を 4 回クリック", "経由点 4 個、頂点を足す丸 3 個", { line: line(app.path()) });

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
  const edited = await capture("edited");

  map.rightClick([138.17, 36.66]);
  await sleep(300);
  note("頂点を右クリック", "頂点 0 個。最初の区間が直線に戻る", {
    modes: app.path().waypoints.map((w) => w.segment_mode),
  });

  map.rightClick(CLICKS[3]);
  await sleep(300);
  const saved = await app.saved();
  note("最後の経由点を右クリックして保存", "保存されたものが画面と同じで、経由点 3 個", {
    same:
      JSON.stringify({ waypoints: saved?.waypoints, route: saved?.route }) ===
      JSON.stringify({ waypoints: app.path().waypoints, route: app.path().route }),
    saved: saved && { waypoints: saved.waypoints.length, segments: saved.route.segments, title: saved.ride.title },
    list: await app.list(),
  });

  const captures = [edited];
  if (app.loadGeo !== null) {
    // 碓氷峠。軽井沢町から安中市へ、県境をまたぐ線を引く。
    await app.loadGeo();
    await app.create("2026-10-04");
    map.jump([138.66, 36.35], 12);
    await map.idle();
    map.click([138.62, 36.345]);
    await sleep(300);
    map.click([138.7, 36.36]);
    await sleep(2500);
    note("県境をまたぐ線を引く", "県境と市区町村境のイベントが 1 件ずつできる", {
      events: app.events().map((e) => `${(e.route_m / 1000).toFixed(1)} km ${e.kind} ${e.label} ${e.status}`),
      message: app.message(),
    });
    captures.push(await capture("events"));
  }

  return { steps, captures };
}
