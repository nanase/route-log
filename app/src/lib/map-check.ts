/** 地図の表示と、経路の編集・保存を、画面を人が見なくても確かめる。 */

import { Channel, invoke } from "@tauri-apps/api/core";
import { creditImages, motionInput } from "./overview";
import { line, part, type Path } from "./route";
import type { LonLat, RideDocument, RideEvent, RideSummary } from "./types";
import type { Basemap } from "./ui";

/** 確認のために `RouteMap` が見せる口。 */
export interface MapHandle {
  jump(center: LonLat, zoom: number): void;
  click(at: LonLat): void;
  /** 丸をドラッグする。`holdMs` を渡すと、つかんだまま途中でその間だけ止まる */
  drag(from: LonLat, to: LonLat, holdMs?: number): Promise<void>;
  /** 丸をつかんで動かし、地図の外で離す。離す直前に丸があった位置を返す */
  dragAway(from: LonLat, to: LonLat): LonLat | null;
  /** 経由点の丸が、いま出ている位置 */
  waypointHandles(): LonLat[];
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
  /** その日付の走行を開く。記録が無くても、作らない */
  open(rideDate: string): Promise<void>;
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
  /** 概略図の動画のダイアログを出し入れする */
  showOverview(shown: boolean): Promise<void>;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** 条件が成り立つまで待つ。待ちきれなければ、そのまま進む。 */
async function until(done: () => boolean, timeoutMs = 15_000) {
  const deadline = performance.now() + timeoutMs;
  while (!done() && performance.now() < deadline) await sleep(200);
}

/** `points` のうち、`at` にいちばん近いものの番号。 */
function nearestIndex(points: LonLat[], at: LonLat): number {
  const far = (p: LonLat) => Math.hypot(p[0] - at[0], p[1] - at[1]);
  return points.reduce((best, p, i) => (far(p) < far(points[best]) ? i : best), 0);
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

  // 回を 2 つ作る。経由点 1 → 2 と、2 → 3。欄やボタンが無ければ、確認を止める（episode-check と同じ）。
  // episode-check の手助けを使わないのは、あちらがこのファイルの型を読むためである。
  const ids = app.path().waypoints.map((w) => w.id);
  const enter = (field: HTMLSelectElement | undefined, value: string) => {
    if (field === undefined) throw new Error("欄がありません");
    field.value = value;
    field.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const episodeCards = () => [...document.querySelectorAll<HTMLElement>("aside .episode")];
  const pressButton = (selector: string, label: string) => {
    const button = [...document.querySelectorAll<HTMLButtonElement>(selector)].find(
      (b) => b.textContent?.trim() === label,
    );
    if (button === undefined) throw new Error(`「${label}」のボタンがありません`);
    button.click();
  };
  const addEpisode = async () => {
    pressButton("aside button", "追加");
    await sleep(300);
  };
  await addEpisode();
  enter(episodeCards()[0]?.querySelectorAll("select")[1], ids[1]);
  await sleep(200);
  await addEpisode();
  enter(episodeCards()[1]?.querySelectorAll("select")[0], ids[1]);
  await sleep(200);
  note("回を 2 つ作る", "パート 1は経由点 1 → 2、パート 2は 2 → 3", {
    episodes: app.path().episodes?.map((e) => [ids.indexOf(e.from) + 1, ids.indexOf(e.to) + 1]),
  });

  // 概略図の動画。見本は画面の写しで確かめる。動画は短くして、報告の隣に書く。
  await app.showOverview(true);
  const previewSrc = () => document.querySelector<HTMLImageElement>("dialog .preview img")?.src ?? "";
  await until(() => previewSrc() !== "", 60_000);
  await sleep(500);
  const dialogText = (selector: string) =>
    document.querySelector(`dialog ${selector}`)?.textContent?.replace(/\s+/g, " ").trim() ?? null;
  note("概略図の動画のダイアログを開く", "範囲はパート 1で、1 出発 → 2。全経路を見せる時間の欄がある。見本は止まった所", {
    scope: document.querySelector<HTMLSelectElement>("dialog select.scope")?.selectedOptions[0]?.textContent,
    ends: dialogText(".ends"),
    motion: dialogText(".motion"),
  });
  captures.push(await capture("overview"));
  const stopped = previewSrc();
  pressButton("dialog .seg button", "最初");
  await until(() => previewSrc() !== stopped, 60_000);
  await sleep(500);
  note("見本を「最初」にする", "全部の回を合わせた経路が、すべてはっきり描かれる");
  captures.push(await capture("overview-start"));
  await app.showOverview(false);
  const phases: string[] = [];
  const onProgress = new Channel<{ phase: string; done: number; total: number }>();
  onProgress.onmessage = (p) => {
    if (phases.at(-1) !== p.phase) phases.push(p.phase);
  };
  const started = performance.now();
  // パート 1の動画。パート 2を薄く描く。
  const overview = await invoke("render_overview", {
    route: { line: line(part(app.path(), 0, 1)), others: [line(part(app.path(), 1, 2))] },
    motion: motionInput({ fps: "59.94", leadSeconds: 1, zoomSeconds: 1, holdSeconds: 1, startScale: 0.7 }, true),
    credits: await creditImages(),
    path: await invoke<string>("check_path", { name: "overview.mp4" }),
    onProgress,
  }).then(
    () => "書き出した",
    (e) => String(e),
  );
  const rendered = "書き出した。進み具合が tiles、frames の順に届く";
  note("前半の回の概略図の動画を、全経路 1 秒、寄る 1 秒、止める 1 秒で書き出す", rendered, {
    overview,
    phases,
    seconds: Math.round((performance.now() - started) / 1000),
  });
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
      events: app.events().map((e) => `${(e.route_m / 1000).toFixed(1)} km ${e.kind} ${e.label} ${e.status}${e.hidden ? " 字幕なし" : ""}`),
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
      events: app.events().map((e) => `${(e.route_m / 1000).toFixed(1)} km ${e.kind} ${e.label} ${e.status}${e.hidden ? " 字幕なし" : ""}`),
    });
    captures.push(await capture("snapped"));
    /** 道をたどった線。あとで、道の上の点を選ぶのに使う */
    const road = line(app.path());

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

    // ここからは、待つ処理が挟まっても、経由点が飛んだり消えたりしないことを確かめる。
    // 記録のまだ無い日付を開き、道に沿わせたまま、待たずに 5 回続けてクリックする。走行を作るのと、
    // 点を道へ寄せるのを待つ間に、次のクリックが来る。
    const on = (fraction: number) => road[Math.round((road.length - 1) * fraction)];
    const along = [0.1, 0.3, 0.5, 0.7, 0.9].map(on);
    const order = () => app.path().waypoints.map((w) => nearestIndex(along, [w.lon, w.lat]));
    const state = () => ({ order: order(), history: app.history() });
    await app.open("2026-10-07");
    for (const at of along) map.click(at);
    await until(() => app.path().waypoints.length >= along.length, 6000);
    await sleep(600);
    note(
      "記録の無い日付で、道に沿わせて待たずに 5 回クリック",
      "経由点が 5 個、押した順（0,1,2,3,4）に並ぶ。その日付の走行は 1 つだけ。取り消せる編集が 5 つ",
      { ...state(), rides: (await app.list()).filter((ride) => ride.ride_date === "2026-10-07").length },
    );

    await app.undo();
    await app.undo();
    await sleep(300);
    note("取り消しを 2 回", "経由点が 3 個（0,1,2）。やり直せる編集が 2 つ", state());
    await app.redo();
    await app.redo();
    await sleep(300);
    note("やり直しを 2 回", "経由点が 5 個、押した順のまま。やり直せる編集は 0", state());

    // 点を道へ寄せるのを待つ間に取り消しても、クリックが先、取り消しが後になる。
    const last = app.path().waypoints.at(-1);
    map.click(on(0.98));
    await app.undo();
    await sleep(600);
    note("クリックの直後に、待たずに取り消す", "足した点が取り消されて、経由点は 5 個のまま。やり直せる編集が 1 つ", {
      ...state(),
      lastKept: JSON.stringify(app.path().waypoints.at(-1)) === JSON.stringify(last),
    });

    // 経由点を足してすぐ、別の経由点をつかみ、保存の応答が返るまで離さない。
    map.click(on(0.98));
    await sleep(150);
    const target = on(0.56);
    await map.drag(along[2], target, 1500);
    await sleep(1500);
    const moved = app.path().waypoints[2];
    const kept = await app.saved();
    note("経由点を足してすぐ、経由点 3 をつかんで 1.5 秒待ってから動かす", "経由点 3 が、離した所へ動く。経由点は 6 個で、保存されたものも 6 個", {
      target,
      moved: moved && [moved.lon, moved.lat],
      offDegrees: moved && Math.hypot(moved.lon - target[0], moved.lat - target[1]).toFixed(5),
      saved: kept?.waypoints.length,
    });
    captures.push(await capture("burst"));

    // 丸をつかんだまま地図の外で離すと、ドラッグは取りやめになる。丸は元の所へ戻り、次のドラッグもできる。
    const apart = (a: LonLat, b: LonLat) => Math.hypot(a[0] - b[0], a[1] - b[1]).toFixed(5);
    const waypointAt = (index: number): LonLat => [app.path().waypoints[index].lon, app.path().waypoints[index].lat];
    const start = waypointAt(0);
    const carried = map.dragAway(along[0], on(0.2));
    await sleep(300);
    const afterAway = {
      carried: carried && apart(carried, on(0.2)),
      waypoint: apart(waypointAt(0), start),
      handle: apart(map.waypointHandles()[0], start),
    };
    await map.drag(along[0], on(0.04));
    await sleep(1500);
    note("丸をつかんで地図の外で離し、そのあと同じ丸をドラッグする", "丸は離す直前まで付いてくる（carried が 0 に近い）。外で離すと、経由点も丸も元の所に戻る。そのあとのドラッグでは、離した所へ動く", {
      afterAway,
      offDegrees: apart(waypointAt(0), on(0.04)),
      shownWaypoints: map.waypointHandles().length,
    });

    // 判定用データをまだ読んでいない地域（上越市のあたり）でも、待たずに 5 回続けてクリックする。
    // 最初のクリックが読み込みを待つ間に、残りのクリックが来る。点を道へ寄せる処理は、頼んだ順に
    // 終わるとは限らない。
    const far: LonLat[] = [
      [138.22, 37.14],
      [138.235, 37.145],
      [138.25, 37.15],
      [138.265, 37.155],
      [138.28, 37.16],
    ];
    await app.create("2026-10-08");
    map.jump([138.25, 37.15], 13);
    await sleep(500);
    for (const at of far) map.click(at);
    await until(() => app.path().waypoints.length >= far.length, 30_000);
    await sleep(600);
    note("まだ読んでいない地域で、待たずに 5 回クリック", "経由点が 5 個、押した順（0,1,2,3,4）に並ぶ", {
      order: app.path().waypoints.map((w) => nearestIndex(far, [w.lon, w.lat])),
    });
    map.jump([138.158, 36.631], 13);
    await app.create("2026-10-07");
  }

  // 走行の切り替えの板と、設定のダイアログ。見た目は画面の写しで確かめる。
  await app.showRides(true);
  // 開いている走行に、欄に打ってラベルを付ける。2 つ目は、候補に無い名前を新しく付ける。
  const labelInput = () => document.querySelector<HTMLInputElement>('input[aria-label="ラベルを付ける"]');
  for (const text of ["投稿済", " 峠 ", "投稿済"]) {
    const input = labelInput();
    if (input === null) break;
    input.focus();
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await sleep(300);
  }
  labelInput()?.blur();
  const labeled = await app.saved();
  const listed = (await app.list()).find((ride) => ride.id === labeled?.ride.id);
  note("開いている走行に、ラベルを打って付ける", "付けた順に 投稿済・峠 の 2 つ。同じ名前は 2 度付かず、前後の空白は除かれる。一覧の要約にも出る", {
    labels: labeled?.ride.labels,
    listed: listed?.labels,
  });
  captures.push(await capture("rides"));
  // 絞り込みの札を 2 回押して、「投稿済」が付いていない走行だけにする。
  const rowCount = () => document.querySelectorAll("dialog .ride").length;
  const before = rowCount();
  const chip = () => [...document.querySelectorAll<HTMLButtonElement>("dialog .choice")].find((b) => b.textContent?.includes("投稿済"));
  chip()?.click();
  await sleep(200);
  const onlyLabeled = rowCount();
  chip()?.click();
  await sleep(200);
  note("絞り込みの札を 1 回押し、もう 1 回押す", "1 回目は付いている走行だけ（1 行）、2 回目は付いていない走行だけ（残りの行）になり、札が赤くなる", {
    rows: { before, onlyLabeled, without: rowCount() },
    chip: chip()?.className,
  });
  captures.push(await capture("rides-filtered"));
  chip()?.click();
  await sleep(200);
  await app.showRides(false);
  await app.showSettings(true);
  captures.push(await capture("settings"));
  await app.showSettings(false);

  // 操作方法を閉じると、縮尺はボタンの右へ寄る。開閉は次の起動にも残る。
  const scaleLeft = () => document.querySelector<HTMLElement>(".maplibregl-ctrl-scale")?.getBoundingClientRect().left ?? null;
  const hintsRight = () =>
    document.querySelector<HTMLElement>(".left > section[aria-label='操作方法'], .left > .closed")?.getBoundingClientRect().right ?? null;
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "?", bubbles: true }));
  await sleep(300);
  note("? で操作方法を閉じる", "閉じたボタンの右に縮尺が出て、重ならない。覚えが off になる", {
    hintsRight: hintsRight(),
    scaleLeft: scaleLeft(),
    stored: localStorage.getItem("routelog.hints"),
  });
  captures.push(await capture("hints-closed"));
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "?", bubbles: true }));
  await sleep(300);

  // 地図の表示の板を、拡大・縮小の下のボタンで開く。
  document.querySelector<HTMLButtonElement>(".layers .toggle")?.click();
  await sleep(300);
  note("拡大・縮小の下のボタンを押す", "背景地図の 3 つと、道路の線の欄の板が開く", {
    tiles: document.querySelectorAll(".layers .tile").length,
  });
  captures.push(await capture("layers"));
  document.querySelector<HTMLButtonElement>(".layers .toggle")?.click();

  return { steps, captures };
}
