<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { onMount, untrack } from "svelte";
  import { runConfirmCheck } from "./lib/confirm-check";
  import EventList from "./lib/EventList.svelte";
  import {
    aheadAt,
    confirmOnVideo,
    closeStop,
    customAt,
    defaultTarget,
    insertByPosition,
    knotsOf,
    neighbor,
    nextConfirmable,
    openStop,
    patch,
    pinStop,
    placeOf,
    positionAt,
    setHidden,
    stopBetween,
    stopsOf,
    unconfirm,
    waypointEvent,
    waypointEventAt,
  } from "./lib/events";
  import { pointAt } from "./lib/geo";
  import Icon from "./lib/Icon.svelte";
  import Hints from "./lib/Hints.svelte";
  import { forShortcut, keepKeys } from "./lib/keys";
  import { KINDS } from "./lib/kinds";
  import { runMapCheck } from "./lib/map-check";
  import NextGuide from "./lib/NextGuide.svelte";
  import Notices from "./lib/Notices.svelte";
  import OverviewDialog from "./lib/OverviewDialog.svelte";
  import PlaybackCheck from "./lib/PlaybackCheck.svelte";
  import Player from "./lib/Player.svelte";
  import RidePanel from "./lib/RidePanel.svelte";
  import { line, type Path, type PathEdit, segmentLengths } from "./lib/route";
  import RouteMap from "./lib/RouteMap.svelte";
  import RouteSide from "./lib/RouteSide.svelte";
  import RouteTools from "./lib/RouteTools.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import { barState, CloudSync, NO_SYNC, type SyncView } from "./lib/sync";
  import { runSyncCheck } from "./lib/sync-check";
  import { hms, short } from "./lib/time";
  import TimelineBar from "./lib/Timeline.svelte";
  import { runTimelineCheck } from "./lib/timeline-check";
  import TopBar from "./lib/TopBar.svelte";
  import type {
    Config,
    LonLat,
    RideDocument,
    RideEvent,
    RideSummary,
    RideVideos,
    RoadData,
    Scan,
    Timeline,
  } from "./lib/types";
  import {
    type Basemap,
    type ExcludedClip,
    type Folder,
    type FolderState,
    type Mode,
    type Notice,
    remembered,
    type SaveState,
    startDate,
  } from "./lib/ui";

  /** 保存を待つ時間。続けて編集している間は書かない */
  const SAVE_DELAY_MS = 400;
  /** できたことの知らせを出しておく時間 */
  const TOAST_MS = 4000;
  /** 取り消せる編集の数 */
  const HISTORY_LIMIT = 100;
  const EMPTY: Path = { waypoints: [], route: { segments: [] } };
  /** 画面の好みを覚えておく場所 */
  const HINTS_KEY = "routelog.hints";
  const BASEMAP_KEY = "routelog.basemap";
  const LAST_RIDE_KEY = "routelog.last-ride";
  const SIDE_KEY = "routelog.side-width";
  /** 時刻入力のときの、動画と一覧の列の幅（px）。動画は、この幅いっぱいに出る */
  const SIDE_DEFAULT = 520;
  const SIDE_MIN = 420;
  /** 動画と一覧の列を広げても、地図に残す幅（px）。上の帯が収まる幅にする */
  const MAP_MIN = 540;
  /** 動画のほかに、縦に要る高さ（px）。タイムライン、再生の操作、次の候補、一覧の数行 */
  const BESIDE_VIDEO = 416;

  /** 自動の確認の対象。`undefined` は問い合わせ中 */
  let check = $state<
    | {
        file: string | null;
        dir: string | null;
        map: string | null;
        geo: string | null;
        sync: string | null;
        step: string | null;
      }
    | undefined
  >(undefined);
  let player = $state<Player>();
  let routeMap = $state<RouteMap>();
  let ridePanel = $state<RidePanel>();
  let timelineBar = $state<TimelineBar>();
  /** いまの作業。経路入力か、動画を見ながらの時刻入力か */
  let mode = $state<Mode>("route");
  let settingsOpen = $state(false);
  let overviewOpen = $state(false);
  let ridesOpen = $state(false);
  /** 操作の手がかりを出しているか */
  let hintsShown = $state(remembered(HINTS_KEY, ["on", "off"], "on") === "on");
  let basemap = $state<Basemap>(remembered(BASEMAP_KEY, ["pale", "std", "photo"], "pale"));
  /** 動画と一覧の列の幅。境目をドラッグして変える */
  let sideWidth = $state(Number(localStorage.getItem(SIDE_KEY)) || SIDE_DEFAULT);
  /** 道路の線を地図に重ねるか */
  let showRoads = $state(true);
  /** 時刻入力のとき、地図が現在位置を追うか */
  let follow = $state(true);
  /** 選んでいる経由点の番号 */
  let selectedWaypoint = $state<number | null>(null);
  /** 経路入力のときに、一覧で選んで地図で目立たせているイベント */
  let focusedId = $state<string | null>(null);
  let config = $state<Config>({ video_dir: null, road_data_dir: null, geo_data_dir: null });
  let videoState = $state<FolderState>("none");
  let roadState = $state<FolderState>("none");
  let geoState = $state<FolderState>("none");
  /** 上の帯の下に出している知らせ */
  let notices = $state<Notice[]>([]);
  /** 右下に出している、短い知らせ */
  let toast = $state("");
  /** 知らせの種類。できたことか、控えめに伝える注意か */
  let toastTone = $state<"ok" | "warning">("ok");
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  /** 最後に出した知らせ。自動の確認が読む */
  let lastMessage = "";
  let videos = $state<RideVideos[]>([]);
  /** 開いている走行で、タイムラインから外している動画のファイル名 */
  let excludedNames = $state.raw<string[]>([]);
  let stored = $state<RideSummary[]>([]);
  let roads = $state<RoadData | null>(null);
  /** 開いている走行の日付 */
  let date = $state<string | null>(null);
  /** 開いている走行。まだ何も記録していない日付では null。編集のたびに丸ごと入れ替える */
  let doc = $state.raw<RideDocument | null>(null);
  /** 新しく足す区間を、道に沿わせるか */
  let snapNew = $state(false);
  /** 経路が変わっていて、次の保存でイベントを抽出し直すか */
  let extractNeeded = false;
  /** 動画の再生位置の実時刻 */
  let nowMs = $state(0);
  /** 人が選んだ、次の候補。選んでいなければ推定時刻から決める */
  let chosenId = $state<string | null>(null);
  /** 一覧で、直す欄を開いているイベント */
  let selectedId = $state<string | null>(null);
  /** 地図で場所を決めている停車 */
  let placingId = $state<string | null>(null);
  /** 時刻の合わない、確定済みのイベントの ID */
  let conflicts = $state<string[]>([]);
  /** 動画を見て確定したイベントの ID。Backspace で新しい順に戻す */
  let confirmedOnVideo = $state.raw<string[]>([]);
  let timeline = $state<Timeline | null>(null);
  let saveState = $state<SaveState>("saved");
  /**
   * 待っている保存の予約。立てるのは、`queued` の中からだけにする。列の外で立てると、受け取った
   * 走行を置いた直後や、走行を消した直後に、画面に残っていた古い内容を書いてしまう。
   */
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  /** 書いている最中の保存。走行を消す前に、終わるのを待つ */
  let writing: Promise<void> | undefined;
  /** 順に行う編集の列の、いちばん後ろ。`queued` が継ぎ足す */
  let editing: Promise<void> = Promise.resolve();
  /** クラウドとの同期の状態 */
  let syncView = $state.raw<SyncView>(NO_SYNC);
  /** 起動のときに開く走行を、選び終えたか */
  let booted = false;
  /** 経路の編集の取り消しとやり直し。編集の前の経路を、古い順に持つ */
  let undoStack = $state.raw<Path[]>([]);
  let redoStack = $state.raw<Path[]>([]);

  /** 判定用データを開いてあるか */
  const geoReady = $derived(geoState === "ready");
  /** 動画のある日付と、記録のある日付を合わせた一覧。新しい順 */
  const dates = $derived(
    [...new Set([...videos.map((v) => v.date), ...stored.map((s) => s.ride_date)])].sort().reverse(),
  );
  const files = $derived(videos.find((v) => v.date === date)?.files ?? []);
  /** 走行に使う動画。再生、タイムライン、時刻と動画の位置の変換は、これだけを相手にする */
  const usedFiles = $derived(files.filter((file) => !excludedNames.includes(file.name)));
  const clockOffsetMs = $derived(doc?.ride.clock_offset_ms ?? 0);
  /** 走行から外している動画の、実時刻の範囲。タイムラインに薄く出して、戻せるようにする */
  const excludedClips = $derived<ExcludedClip[]>(
    files
      .filter((file) => excludedNames.includes(file.name))
      .map(({ name, start_ms, duration_ms }) => {
        const start = start_ms + clockOffsetMs;
        return { name, start_ms: start, end_ms: start + duration_ms };
      }),
  );
  const routeLine = $derived(doc === null ? [] : line(doc));
  const events = $derived(doc?.events ?? []);
  const target = $derived(
    events.find((event) => event.id === chosenId) ?? (timeline === null ? null : defaultTarget(events, nowMs)),
  );
  /** 動画のいまの時刻にいると推定される位置。最後の確定より後は、直前の速さから置いた仮の位置 */
  const here = $derived.by(() => {
    if (timeline === null) return null;
    const [knots, stops] = [knotsOf(events), stopsOf(events)];
    const routeM = positionAt(knots, stops, nowMs);
    if (routeM !== null) return { at: pointAt(routeLine, routeM), guessed: false };
    const aheadM = aheadAt(knots, stops, nowMs, openStop(events)?.time_ms ?? null);
    return aheadM === null ? null : { at: pointAt(routeLine, aheadM), guessed: true };
  });
  // 経由点のイベントは、地図に経由点の丸があるので、点を重ねない。
  const marks = $derived(
    events.flatMap((event) => {
      const at = event.kind === "waypoint" ? null : pointAt(routeLine, event.route_m);
      return at === null ? [] : [{ at, kind: event.kind, hidden: event.hidden === true }];
    }),
  );
  /** 経由点ごとの、そこを通った時刻を持つイベント。始まりと終わりは出発と到着で、途中は作ってあれば */
  const waypointEvents = $derived.by(() => {
    const waypoints = doc?.waypoints ?? [];
    return waypoints.map((waypoint, index) => {
      if (index === 0) return events.find((event) => event.kind === "departure") ?? null;
      if (index === waypoints.length - 1) return events.find((event) => event.kind === "arrival") ?? null;
      return waypointEventAt(events, [waypoint.lon, waypoint.lat]);
    });
  });
  /** 通過時刻を確定してある経由点の番号 */
  const timedWaypoints = $derived(
    waypointEvents.flatMap((event, index) => (event?.status === "confirmed" ? [index] : [])),
  );
  /** 地図で名前を添えるイベント。時刻入力のときは次の候補、経路入力のときは一覧で選んだもの */
  const callout = $derived.by(() => {
    const event = mode === "confirm" ? target : (events.find((e) => e.id === focusedId) ?? null);
    const at = event === null ? null : pointAt(routeLine, event.route_m);
    if (event === null || at === null) return null;
    let note = `${(event.route_m / 1000).toFixed(1)} km`;
    if (mode === "confirm") {
      // 動画が無ければ、いまの時刻も無い。あと何分かは出さない。
      const rest = event.time_ms === null || timeline === null ? null : event.time_ms - nowMs;
      if (event.status === "confirmed") note = "確定済み";
      else if (rest === null) note = "";
      else note = rest >= 0 ? `あと ${short(rest)}` : `${short(-rest)} 前`;
    }
    return { at, kind: event.kind, label: event.label || KINDS[event.kind], note };
  });
  /** 浮かせた板に隠れる幅（px）。経路の全体を出すときや、位置をまん中に出すときに避ける */
  const pad = $derived(
    mode === "route" ? { top: 68, right: 324, bottom: 12, left: 248 } : { top: 68, right: 12, bottom: 12, left: 12 },
  );
  /** 地図の縮尺を置く位置。左下に浮かせた板の右へ逃がす */
  const scaleLeft = $derived(mode === "confirm" ? 168 : hintsShown ? 362 : 112);
  /** 上の帯に 1 つだけ出す、保存と同期の状態 */
  const bar = $derived(barState(doc === null ? null : saveState, syncView));
  /** 上の帯に出す、開いている走行の動画の要約 */
  const videoSummary = $derived.by(() => {
    if (videoState === "loading") return "動画を読み込み中…";
    if (date === null) return "";
    if (files.length === 0) return "動画なし";
    const used = `動画 ${usedFiles.length} 本・${hms(usedFiles.reduce((sum, file) => sum + file.duration_ms, 0))}`;
    const out = files.length - usedFiles.length;
    return out === 0 ? used : `${used}（外した動画 ${out} 本）`;
  });

  onMount(async () => {
    // 窓を閉じるときに、保存を待っている編集を手元に書き終える。書き終えると、窓が閉じる。
    void getCurrentWindow().onCloseRequested(async () => {
      await editing;
      await flush();
    });
    check = await invoke("check_target");
    stored = await invoke("list_rides");
    if (check?.file) return;
    if (check?.dir && check.geo) return checkConfirm(check.dir, check.geo);
    if (check?.dir) return checkTimeline(check.dir);
    if (check?.sync) return checkSync(check.sync);
    if (check?.map) return checkMap(check.map);
    // クラウドとの突き合わせは裏で行い、画面を出すのを待たせない。
    void sync.start();
    config = await invoke<Config>("get_config");
    // 前に開いていた走行に記録があれば、動画フォルダを読むのを待たずに開く。
    const last = localStorage.getItem(LAST_RIDE_KEY);
    if (stored.some((ride) => ride.ride_date === last)) await select(last as string);
    if (config.road_data_dir) await loadRoads(config.road_data_dir);
    if (config.geo_data_dir) await loadGeo(config.geo_data_dir);
    if (config.video_dir) await loadVideos(config.video_dir);
    // 動画が無くても、記録のある走行があれば、それを開いて始める。
    await openFirst();
    booted = true;
  });

  /** 何も開いていなければ、前に開いていた走行を開く。それがもう無ければ、いちばん新しいものを開く。 */
  async function openFirst() {
    const first = startDate(localStorage.getItem(LAST_RIDE_KEY), dates);
    if (date === null && first !== undefined) await select(first);
  }

  /** エラーか注意を、上の帯の下に出す。同じ事柄の知らせがあれば置き換える。 */
  function report(notice: Notice) {
    notices = [...notices.filter((old) => old.key !== notice.key), notice];
    lastMessage = `${notice.title}${notice.detail}`;
  }

  function dismiss(key: string) {
    if (notices.some((notice) => notice.key === key)) notices = notices.filter((notice) => notice.key !== key);
  }

  /** できたことを、右下に短い間だけ出す。`tone` が warning なら、控えめに伝える注意として出す。 */
  function tell(text: string, tone: "ok" | "warning" = "ok") {
    toast = text;
    toastTone = tone;
    lastMessage = text;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = ""), TOAST_MS);
  }

  async function loadVideos(dir: string) {
    videoState = "loading";
    dismiss("video");
    try {
      const scan = await invoke<Scan>("scan_videos", { dir });
      videos = scan.rides;
      videoState = "ready";
      const count = videos.reduce((n, ride) => n + ride.files.length, 0);
      tell(`動画 ${count} 本を読み込みました`);
      if (scan.skipped.length > 0) {
        report({
          key: "video",
          level: "warning",
          title: `読めないファイルが ${scan.skipped.length} 本あります。`,
          detail: scan.skipped[0].reason,
        });
      }
      await openFirst();
    } catch (e) {
      videoState = "error";
      report({ key: "video", level: "error", title: "動画フォルダを読めません。", detail: String(e), action: repick("video") });
    }
    config = await invoke<Config>("get_config");
  }

  async function loadRoads(dir: string) {
    roadState = "loading";
    dismiss("road");
    try {
      roads = await invoke<RoadData>("open_road_data", { dir });
      roadState = "ready";
    } catch (e) {
      roadState = "error";
      report({ key: "road", level: "error", title: "道路データを開けません。", detail: String(e), action: repick("road") });
    }
    config = await invoke<Config>("get_config");
  }

  async function loadGeo(dir: string) {
    geoState = "loading";
    dismiss("geo");
    try {
      await invoke("open_geo_data", { dir });
      geoState = "ready";
      tell("判定データを開きました");
      void extractAgain();
    } catch (e) {
      // 選ぶのは、roads.fgb・admin.fgb・michi-no-eki.fgb の 3 つが入ったフォルダである。
      geoState = "error";
      report({ key: "geo", level: "error", title: "判定データを開けません。", detail: String(e), action: repick("geo") });
    }
    config = await invoke<Config>("get_config");
  }

  /** 開いている走行のイベントを、いまの経路から抽出し直す。 */
  function extractAgain() {
    return inTurn(() => {
      if (doc === null) return;
      extractNeeded = true;
      clearTimeout(saveTimer);
      saveTimer = setTimeout(flush, 0);
      saveState = "pending";
    });
  }

  const FOLDERS: Record<Folder, { title: string; load: (dir: string) => Promise<void> }> = {
    video: { title: "動画フォルダを選ぶ", load: loadVideos },
    road: { title: "道路データのフォルダを選ぶ", load: loadRoads },
    geo: { title: "判定データのフォルダを選ぶ", load: loadGeo },
  };

  async function pick(folder: Folder) {
    const picked = await open({ directory: true, title: FOLDERS[folder].title });
    if (picked !== null) await FOLDERS[folder].load(picked);
  }

  const repick = (folder: Folder) => ({ label: "フォルダを選び直す", run: () => void pick(folder) });

  /** 走行を切り替える。待っている編集を、いまの走行に入れ終えてから切り替える。 */
  function select(next: string): Promise<void> {
    return queued(() => switchTo(next));
  }

  /** その日付の走行を読んで、画面に出す。`queued` の中からだけ呼ぶ。 */
  async function switchTo(next: string) {
    await flush();
    const summary = stored.find((s) => s.ride_date === next);
    const loaded = summary ? await invoke<RideDocument>("load_ride", { id: summary.id }) : null;
    const names = await invoke<string[]>("get_excluded_videos", { date: next });
    doc = loaded;
    date = next;
    // 次に起動したときに、同じ走行から始められるようにする。
    localStorage.setItem(LAST_RIDE_KEY, next);
    excludedNames = names;
    chosenId = null;
    selectedId = null;
    conflicts = [];
    confirmedOnVideo = [];
    undoStack = [];
    redoStack = [];
    selectedWaypoint = null;
    focusedId = null;
    dismiss("ride");
  }

  /**
   * 頼まれた順に 1 つずつ行う。前の仕事が済むまで、次を始めない。失敗は、頼んだ側に返す。
   *
   * 編集のほかに、走行の切り替え、クラウドから受け取った走行の置き換え、走行の削除も並べる。
   * どれも開いている走行を入れ替えるので、編集の途中に挟まると、古い内容で上書きしてしまう。
   */
  function queued<T>(task: () => T | Promise<T>): Promise<T> {
    const done = editing.then(task);
    editing = done.then(
      () => undefined,
      () => undefined,
    );
    return done;
  }

  /**
   * 編集を、頼まれた順に 1 つずつ行う。前の編集が済むまで、次を始めない。
   *
   * 編集には、走行を作る、点を道へ寄せるといった、待つ処理が挟まる。順に行うので、待つ間に
   * 来た次の編集が、古い状態から作った結果で上書きすることも、走行を 2 つ作ることもない。
   */
  function inTurn(task: () => void | Promise<void>): Promise<void> {
    return queued(task).catch((e) => {
      report({ key: "edit", level: "error", title: "編集できません。", detail: String(e) });
    });
  }

  /**
   * 開いている日付の走行を、無ければ作って返す。日付を選んでいなければ null。
   *
   * `inTurn` の中からだけ呼ぶ。作るのを待つ間に、もう 1 度呼ばれることが無い。
   */
  async function ensureDoc(): Promise<RideDocument | null> {
    if (doc === null && date !== null) {
      doc = await invoke<RideDocument>("create_ride", { rideDate: date });
      stored = await invoke("list_rides");
      sync.saved(doc.ride.id);
    }
    if (doc === null) {
      report({
        key: "ride",
        level: "warning",
        title: "走行を開いていません。",
        detail: "走行を選ぶか、日付を入れて作ってください。",
        action: { label: "走行を選ぶ", run: openRides },
      });
    }
    return doc;
  }

  /** 走行を入れ替えて、保存を予約する。 */
  function put(next: RideDocument, delayMs = SAVE_DELAY_MS) {
    if (next.waypoints !== doc?.waypoints || next.route !== doc?.route) extractNeeded = true;
    doc = next;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, delayMs);
    saveState = "pending";
  }

  /** 走行を書き換える。`change` は、順番が来たときの走行に当てる。変える所が無ければ、そのまま返す。 */
  function edit(change: (current: RideDocument) => RideDocument, delayMs = SAVE_DELAY_MS) {
    return inTurn(async () => {
      const current = await ensureDoc();
      if (current === null) return;
      const next = change(current);
      if (next !== current) put(next, delayMs);
    });
  }

  /** 待っている保存があれば、いま書く。 */
  async function flush() {
    // 前の保存が済んでから書く。重ねて書くと、古い内容が後から書かれて残ることがある。
    while (writing !== undefined) await writing;
    if (saveTimer === undefined || doc === null) return;
    clearTimeout(saveTimer);
    saveTimer = undefined;
    saveState = "saving";
    writing = write(doc).finally(() => (writing = undefined));
    await writing;
  }

  async function write(saving: RideDocument) {
    const extract = extractNeeded;
    extractNeeded = false;
    try {
      const saved = await invoke<{
        updated_at: string;
        length_m: number;
        route: RideDocument["route"];
        events: RideEvent[];
        conflicts: string[];
      }>("save_ride", { doc: saving, extract });
      // 保存を待つ間に編集が進んでいたら、返ってきたイベントは古い。次の保存に任せる。
      if (doc === saving) {
        doc = { ...doc, route: saved.route, events: saved.events };
        conflicts = saved.conflicts;
      }
      stored = stored.map((s) =>
        s.id === saving.ride.id
          ? {
              ...s,
              title: saving.ride.title,
              updated_at: saved.updated_at,
              length_m: saved.length_m,
              waypoints: saving.waypoints.length,
            }
          : s,
      );
      dismiss("save");
      // 書いている間に次の編集が来ていれば、その保存がまだ待っている。
      if (saveTimer === undefined) saveState = "saved";
      sync.saved(saving.ride.id);
    } catch (e) {
      extractNeeded ||= extract;
      saveState = "error";
      report({
        key: "save",
        level: "error",
        title: "保存できません。",
        detail: String(e),
        action: { label: "もう一度", run: retrySave },
      });
    }
  }

  /** 保存できなかった内容を、もう一度書く。 */
  function retrySave() {
    return inTurn(() => {
      if (doc === null) return;
      clearTimeout(saveTimer);
      saveTimer = setTimeout(flush, 0);
      saveState = "pending";
    });
  }

  const pathOf = (current: RideDocument | null): Path =>
    current === null ? EMPTY : { waypoints: current.waypoints, route: current.route };

  /**
   * 地図での操作を、経路に入れる。取り消せるよう、書き換える前の経路を覚えておく。
   *
   * `make` は、順番が来てから呼ぶ。いまの経路を見て、点を道へ寄せるなどして、経路に当てる
   * 編集を返す。編集は、返ってきたその場で、そのときの経路に当てる。
   */
  function editPath(make: () => PathEdit | null | Promise<PathEdit | null>) {
    return inTurn(async () => {
      const change = await make();
      const current = change === null ? null : await ensureDoc();
      if (change === null || current === null) return;
      const before = pathOf(current);
      const after = change(before);
      if (after === before) return;
      undoStack = [...undoStack.slice(1 - HISTORY_LIMIT), before];
      redoStack = [];
      put({ ...current, ...after });
    });
  }

  /** 経路の編集を 1 つ取り消す。先に頼まれた編集が、経路に入ってから取り消す。 */
  function undo() {
    return inTurn(() => {
      const previous = undoStack.at(-1);
      if (previous === undefined || doc === null) return;
      undoStack = undoStack.slice(0, -1);
      redoStack = [...redoStack, pathOf(doc)];
      put({ ...doc, ...previous });
    });
  }

  /** 取り消した編集を 1 つやり直す。 */
  function redo() {
    return inTurn(() => {
      const next = redoStack.at(-1);
      if (next === undefined || doc === null) return;
      redoStack = redoStack.slice(0, -1);
      undoStack = [...undoStack, pathOf(doc)];
      put({ ...doc, ...next });
    });
  }

  /** イベントを書き換える。時刻の推定をすぐ見せたいので、待たずに保存する。 */
  function editEvents(change: (events: RideEvent[]) => RideEvent[]) {
    return edit((current) => ({ ...current, events: change(current.events) }), 0);
  }

  function setClockOffset(clock_offset_ms: number) {
    return edit((current) => ({ ...current, ride: { ...current.ride, clock_offset_ms } }));
  }

  function setTitle(title: string) {
    return edit((current) => ({ ...current, ride: { ...current.ride, title } }));
  }

  /** 一覧で選んだイベントを次の候補にして、地図と動画をそこへ動かす。 */
  function choose(event: RideEvent) {
    chosenId = event.id;
    const at = pointAt(routeLine, event.route_m);
    if (at !== null) routeMap?.focus(at);
    if (event.time_ms !== null) void player?.seek(event.time_ms);
  }

  /** 一覧の行を押した。次の候補にして、直す欄を開く。開いている行なら、欄を閉じる。 */
  function selectEvent(event: RideEvent) {
    if (selectedId === event.id) {
      selectedId = null;
      return;
    }
    selectedId = event.id;
    choose(event);
  }

  /**
   * 時刻入力のときに、地図の経由点を押した。その経由点を、次の候補にする。
   *
   * 経由点を通った時刻を入れるためのイベントを、無ければ作る。字幕にはしない。押すのは、動画が
   * そこを通るのを見たときなので、一覧の行を押したときと違って、動画は動かさない。
   */
  function chooseWaypoint(index: number) {
    const waypoint = doc?.waypoints[index];
    if (waypoint === undefined) return;
    const existing = waypointEvents[index];
    if (existing !== null) {
      chosenId = existing.id;
      return;
    }
    const at: LonLat = [waypoint.lon, waypoint.lat];
    // 位置の順の所に足す。末尾に足すと、保存の応答で並び直すまでの間、一覧が最後まで送られてしまう。
    const routeM = segmentLengths(doc ?? EMPTY)
      .slice(0, index)
      .reduce((sum, length) => sum + length, 0);
    const made = waypointEvent(at, index, routeM);
    chosenId = made.id;
    void editEvents((current) => (waypointEventAt(current, at) === null ? insertByPosition(current, made) : current));
  }

  /**
   * 録画の止まっていた間を、停車にする。その間は進んでいないものとして、時刻が推定される。
   *
   * 食事などで長く止まり、同じ場所から録画を始め直したときに使う。
   */
  function addStop(startMs: number, endMs: number) {
    const stop = stopBetween(startMs, endMs, clockOffsetMs);
    return editEvents((current) => insertByPosition(current, { ...stop, route_m: placeOf(current, startMs) }));
  }

  /**
   * 動画のいまの位置を、停車の始まりにする。始まりだけを入れた停車があれば、その終わりにする。
   *
   * 給油のように、録画したまま止まっていた間を停車にするのに使う。
   */
  function markStop() {
    const realMs = videoNow();
    if (realMs === null || doc === null) return;
    const [open, offsetMs] = [openStop(events), clockOffsetMs];
    if (open !== null) {
      void editEvents((current) => patch(current, open.id, (event) => closeStop(event, realMs, offsetMs)));
      tell("停車の終わりを入れました");
      return;
    }
    const stop = stopBetween(realMs, realMs, offsetMs);
    void editEvents((current) => insertByPosition(current, { ...stop, route_m: placeOf(current, realMs) }));
    tell("停車の始まりを入れました。出るところまで動画を進めて、もう一度押します");
  }

  const removeEvent = (id: string) => editEvents((current) => current.filter((event) => event.id !== id));

  /** 地図で場所を決めている停車。消えたか、時刻入力を離れたら、決めるのをやめる */
  const placing = $derived(
    mode === "confirm" ? (events.find((event) => event.id === placingId && event.kind === "stop") ?? null) : null,
  );

  /** 地図で押した経路の上の点を、停車の場所にする。 */
  function placeStop(at: LonLat, routeM: number) {
    const id = placing?.id;
    placingId = null;
    if (id === undefined) return;
    void editEvents((current) => patch(current, id, (event) => pinStop(event, at, routeM)));
    tell("停車の場所を決めました");
  }

  /** まとめて選んだイベントを、字幕なし（`hidden` が真）か字幕ありにする。 */
  function hideEvents(ids: string[], hidden: boolean) {
    return editEvents((current) => current.map((event) => (ids.includes(event.id) ? setHidden(event, hidden) : event)));
  }

  /** 動画と一覧の列に取れる、いちばん広い幅。地図と、一覧の数行の場所を残す。 */
  const sideMax = () =>
    Math.max(SIDE_MIN, Math.min(window.innerWidth - MAP_MIN, ((window.innerHeight - BESIDE_VIDEO) * 16) / 9));

  /** 動画と一覧の列の境目をドラッグして、幅を変える。動画は、列の幅に合わせて大きくなる。 */
  function resizeSide(event: PointerEvent) {
    if (event.button !== 0) return;
    // 字を選び始めないようにする。
    event.preventDefault();
    const move = (moved: PointerEvent) => {
      sideWidth = Math.round(Math.min(Math.max(window.innerWidth - moved.clientX, SIDE_MIN), sideMax()));
    };
    // 境目は細いので、動かす先は窓の全体で受ける。
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      localStorage.setItem(SIDE_KEY, String(sideWidth));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
  }

  function resetSide() {
    sideWidth = SIDE_DEFAULT;
    localStorage.setItem(SIDE_KEY, String(sideWidth));
  }

  /** 経路入力のときに、一覧で選んだイベントの位置を地図に出す。もう一度選ぶと、目立たせるのをやめる。 */
  function focusEvent(event: RideEvent) {
    if (focusedId === event.id) {
      focusedId = null;
      return;
    }
    focusedId = event.id;
    const at = pointAt(routeLine, event.route_m);
    if (at !== null) routeMap?.focus(at);
  }

  /** 一覧で選んだ経由点を、地図でも選んで、そこへ動かす。 */
  function focusWaypoint(index: number) {
    const waypoint = doc?.waypoints[index];
    if (waypoint === undefined) return;
    selectedWaypoint = index;
    routeMap?.focus([waypoint.lon, waypoint.lat]);
  }

  function toggleHints() {
    hintsShown = !hintsShown;
    localStorage.setItem(HINTS_KEY, hintsShown ? "on" : "off");
  }

  function setBasemap(next: Basemap) {
    basemap = next;
    localStorage.setItem(BASEMAP_KEY, next);
  }

  // 経由点が増減したら、番号がずれるので、選ぶのをやめる。走行は保存のたびに入れ替わるので、
  // 走行そのものではなく、経由点の数が変わったかを見る。
  const waypointCount = $derived(doc?.waypoints.length ?? 0);
  $effect(() => {
    void waypointCount;
    untrack(() => (selectedWaypoint = null));
  });

  /**
   * 動画の再生位置の実時刻。動画が無ければ null。
   *
   * 動画を出さなくなると、`player` は null になる。動画に頼る操作は、これが null なら何もしない。
   */
  const videoNow = () => (timeline === null || player == null ? null : player.now());

  /** 次の候補を、動画のいまの位置の時刻で確定する。 */
  function confirmTarget() {
    const realMs = videoNow();
    if (realMs === null || target === null || doc === null) return;
    const [id, offsetMs] = [target.id, clockOffsetMs];
    confirmedOnVideo = [...confirmedOnVideo, id];
    chosenId = nextConfirmable(events, id)?.id ?? null;
    void editEvents((current) => patch(current, id, (event) => confirmOnVideo(event, realMs, offsetMs)));
  }

  /** 動画を見て確定したもののうち、いちばん新しい確定を外す。 */
  function undoConfirm() {
    const id = confirmedOnVideo.at(-1);
    if (id === undefined) return;
    confirmedOnVideo = confirmedOnVideo.slice(0, -1);
    chosenId = id;
    void editEvents((current) => patch(current, id, unconfirm));
  }

  /** 動画のいまの位置に、手動のイベントを足す。 */
  function addCustom() {
    const realMs = videoNow();
    if (realMs === null || doc === null) return;
    const custom = customAt(realMs, clockOffsetMs, "メモ");
    // 足したら、すぐ名前を付けられるように、直す欄を開く。
    chosenId = custom.id;
    selectedId = custom.id;
    void editEvents((current) => insertByPosition(current, { ...custom, route_m: placeOf(current, realMs) }));
  }

  /** 次の候補を隣のイベントへ移し、動画をその時刻へ動かす。 */
  function move(step: 1 | -1) {
    const next = neighbor(events, target?.id ?? chosenId, step);
    if (next !== null) choose(next);
  }

  function setMode(next: Mode) {
    if (next !== "confirm") player?.pause();
    mode = next;
  }

  function onkeydown(event: KeyboardEvent) {
    // 字を打つ欄では、Esc で欄を離れる。離れると、キーが画面の操作に戻る。
    const from = event.target;
    if (event.key === "Escape" && from instanceof HTMLInputElement && from.closest("dialog") === null) {
      from.blur();
      return;
    }
    if (!forShortcut(event) || event.altKey) return;
    const key = event.key.toLowerCase();
    if (event.ctrlKey || event.metaKey) {
      // 取り消しとやり直しは、経路の編集が相手である。
      if (mode !== "route") return;
      if (key === "z" && !event.shiftKey) void undo();
      else if (key === "y" || (key === "z" && event.shiftKey)) void redo();
      else return;
    } else if (key === "escape") {
      placingId = null;
      selectedWaypoint = null;
      focusedId = null;
      selectedId = null;
    } else if (mode === "route") {
      if (key === "?") toggleHints();
      else return;
    } else if (mode === "confirm") {
      if (key === "enter") confirmTarget();
      else if (key === "n") move(1);
      else if (key === "p") move(-1);
      else if (key === "backspace") undoConfirm();
      else if (key === "a") addCustom();
      else if (key === "s") markStop();
      else return;
    }
    event.preventDefault();
  }

  /** イベントの通過時刻の一覧を CSV に書き出す。書き出し先は、その場で選ぶ。 */
  async function exportCsv() {
    if (doc === null) return;
    const path = await save({
      title: "通過時刻の一覧を書き出す",
      defaultPath: `${doc.ride.title}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (path !== null) await writeCsv(path);
  }

  async function writeCsv(path: string) {
    await flush();
    dismiss("export");
    try {
      const rows = await invoke<number>("export_csv", { doc, path });
      tell(`${rows} 件を書き出しました`);
    } catch (e) {
      report({ key: "export", level: "error", title: "書き出せません。", detail: String(e) });
    }
  }

  /** その日付の走行を開き、記録が無ければ作る。何も変えない編集を頼むと、走行だけができる。 */
  async function createRide(rideDate: string) {
    await select(rideDate);
    await edit((current) => current);
  }

  /**
   * 走行の一覧を開く。待っている保存を書いてから、クラウドと突き合わせ直す。
   *
   * 別の場所で直した走行があれば、ここで受け取る。
   */
  async function openRides() {
    await flush();
    ridesOpen = true;
    void sync.check();
  }

  /**
   * 開いている走行のファイルを、保存とは別の口から入れ替えるか消す。待っている保存は捨てる。
   * `queued` の中からだけ呼ぶ。
   *
   * 書いている最中の保存が済むのを待ってから始める。保存の予約は列の中でしか立たないので、
   * 仕事の間に次の保存が始まることは無い。仕事に失敗したら、捨てた保存を予約し直す。
   */
  async function replacing<T>(work: () => Promise<T>): Promise<T> {
    while (writing !== undefined) await writing;
    const dropped = saveTimer !== undefined;
    clearTimeout(saveTimer);
    saveTimer = undefined;
    try {
      return await work();
    } catch (e) {
      if (dropped) {
        saveTimer = setTimeout(flush, 0);
        saveState = "pending";
      }
      throw e;
    }
  }

  /**
   * クラウドから取ってきた走行を、手元に置く。置けたら true。
   *
   * 開いている走行なら、待っている編集を先に保存する。保存で手元が変わっていれば、Rust 側が
   * 更新時刻の違いに気づいて置かない。置いたら、画面を読み直す。`overwrite` は、人がクラウドの
   * 内容を選んだときで、待っている編集は捨てる。
   */
  function applyPulled(pulled: RideDocument, expect: string | null, overwrite: boolean): Promise<boolean> {
    return queued(async () => {
      const opened = doc?.ride.id === pulled.ride.id;
      if (opened && !overwrite) {
        await flush();
        // 保存できていない編集が残っている。クラウドの内容で上書きしない。
        if (saveTimer !== undefined || saveState !== "saved") return false;
      }
      const place = () => invoke<boolean>("sync_apply", { doc: pulled, expect, overwrite });
      if (!(await (opened ? replacing(place) : place()))) return false;
      stored = await invoke("list_rides");
      if (opened) {
        extractNeeded = false;
        saveState = "saved";
        dismiss("save");
        await switchTo(pulled.ride.ride_date);
        tell("クラウドの新しい内容を読み込みました");
      } else if (doc === null && (date === null ? booted : date === pulled.ride.ride_date)) {
        // 記録の無かった日付を開いているか、起動が済んでも何も開いていない。受け取った走行を出す。
        await switchTo(pulled.ride.ride_date);
      }
      return true;
    });
  }

  /** 走行の日付。手元に無ければ、クラウドにだけある走行から探す。 */
  const rideDateOf = (id: string) =>
    stored.find((ride) => ride.id === id)?.ride_date ?? syncView.remote.find((row) => row.id === id)?.ride_date;

  /** クラウドとの同期。保存のたびに知らせると、少し置いてから送る。 */
  const sync = new CloudSync(
    {
      configured: async () => {
        const settings = await invoke<{ api_url: string | null; has_token: boolean }>("get_sync_settings");
        return Boolean(settings.api_url) && settings.has_token;
      },
      list: () => invoke("sync_list"),
      push: (id, overwrite) => invoke("sync_push", { id, overwrite }),
      fetch: (id) => invoke("sync_fetch", { id }),
      remove: (id) => invoke("sync_delete", { id }),
    },
    {
      settle: async () => {
        await editing;
        await flush();
      },
      apply: applyPulled,
      show: (view) => {
        syncView = view;
        if (!Object.values(view.rides).includes("conflict")) dismiss("sync");
      },
      // 同期できなくても、編集と保存は続けられる。赤い帯は出さず、右下に短く出す。
      offline: () => tell("手元には保存してあります。クラウドと同期できません。少し置いて、もう一度試します", "warning"),
      conflicted: (id) =>
        report({
          key: "sync",
          level: "warning",
          title: `${rideDateOf(id) ?? "ある日付"} の走行が、手元とクラウドの両方で変更されています。`,
          detail: "どちらを残すかを選んでください。",
          action: { label: "選ぶ", run: openRides },
        }),
    },
  );

  /**
   * 走行の記録を消す。同期を設定していれば、クラウドから先に消す。
   *
   * クラウドから消せなければ、手元の記録も消さずに投げる。手元だけを消すと、次の起動で
   * 受け取り直して戻ってしまう。開いている走行なら、待っている保存を捨ててから消す。
   */
  async function removeRide(id: string) {
    try {
      await sync.remove(id);
    } catch (e) {
      const advice = "クラウドから消せないので、記録を消していません。通信できるときに、もう一度消してください。";
      throw new Error(`${advice}${e}`);
    }
    try {
      await queued(async () => {
        const opened = doc?.ride.id === id;
        const erase = () => invoke("delete_ride", { id });
        await (opened ? replacing(erase) : erase());
        stored = await invoke("list_rides");
        if (!opened) return;
        doc = null;
        chosenId = null;
        selectedId = null;
        conflicts = [];
        confirmedOnVideo = [];
        undoStack = [];
        redoStack = [];
        extractNeeded = false;
        saveState = "saved";
        dismiss("save");
        // 動画のある日付は、記録の無い走行として開いたままにする。
        if (!videos.some((ride) => ride.date === date)) date = null;
      });
    } finally {
      sync.forget(id);
    }
    tell("走行の記録を消しました");
  }

  /** タイムラインを作り直すたびに進める番号。遅れて返った古い応答を使わないために持つ */
  let builds = 0;
  $effect(() => {
    const build = ++builds;
    if (usedFiles.length === 0) timeline = null;
    // 動画が無いときも呼んで、Rust 側のタイムラインを空にする。前に開いた走行や、外した動画が、
    // CSV の動画の位置に残らないようにする。
    void invoke<Timeline>("build_timeline", { files: usedFiles, clockOffsetMs }).then((built) => {
      if (build === builds) timeline = built.files.length > 0 ? built : null;
    });
  });

  /** 動画を、開いている走行のタイムラインから外す。`excluded` が偽なら、外した動画を戻す。 */
  async function setVideoExcluded(name: string, excluded: boolean) {
    const rideDate = date;
    if (rideDate === null) return;
    dismiss("exclude");
    try {
      const names = await invoke<string[]>("set_video_excluded", { date: rideDate, name, excluded });
      if (date === rideDate) excludedNames = names;
      tell(excluded ? "動画を走行から外しました" : "動画を走行に戻しました");
    } catch (e) {
      const title = excluded ? "動画を走行から外せません。" : "動画を走行に戻せません。";
      report({ key: "exclude", level: "error", title, detail: String(e) });
    }
  }

  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  /** フォルダを読み、最初の走行でファイルをまたぐ再生を確かめて、報告を書いて終了する。 */
  async function checkTimeline(target: string) {
    let report: unknown;
    try {
      await loadVideos(target);
      await select(dates[dates.length - 1]);
      setMode("confirm");
      while (timeline === null || player == null) await sleep(100);
      report = await runTimelineCheck(timeline, player, (seconds) => setClockOffset(seconds * 1000), {
        timeline: () => timeline,
        player: () => (timeline === null ? null : (player ?? null)),
        bar: () => timelineBar,
        remembered: () => invoke<string[]>("get_excluded_videos", { date }),
      });
      await invoke("capture_check", { name: "played" });
    } catch (e) {
      report = { error: String(e), message: lastMessage };
    }
    await invoke("finish_check", { report: JSON.stringify(report, null, 2) });
  }

  /** 動画を見ながらの時刻の確定を確かめて、報告を書いて終了する。 */
  async function checkConfirm(videoDir: string, geoDir: string) {
    let report: unknown;
    try {
      await loadVideos(videoDir);
      await select(dates[dates.length - 1]);
      await loadGeo(geoDir);
      while (timeline === null || player == null || routeMap === undefined) await sleep(100);
      report = await runConfirmCheck(timeline, player, routeMap, {
        setMode,
        create: createRide,
        events: () => events,
        target: () => target?.id ?? null,
        choose: (id) => {
          const event = events.find((e) => e.id === id);
          if (event) choose(event);
        },
        setClockOffsetSeconds: (seconds) => setClockOffset(seconds * 1000),
        changeEvent: (id, change) => editEvents((current) => patch(current, id, change)),
        settled: async () => {
          await sleep(100);
          await flush();
          await sleep(300);
        },
        rideDate: () => date ?? "",
        exportCsv: async (name) => {
          const path = await invoke<string>("check_path", { name });
          await writeCsv(path);
          return path;
        },
        message: () => lastMessage,
        timeline: () => timelineBar,
      });
    } catch (e) {
      report = { error: String(e), message: lastMessage };
    }
    await invoke("finish_check", { report: JSON.stringify(report, null, 2) });
  }

  /** クラウドとの自動の同期を確かめて、報告を書いて終了する。 */
  async function checkSync(apiUrl: string) {
    let report: unknown;
    try {
      while (routeMap === undefined) await sleep(100);
      report = await runSyncCheck(apiUrl, check?.step ?? "", routeMap, {
        create: createRide,
        open: select,
        setTitle,
        doc: () => doc,
        bar: () => bar,
        view: () => syncView,
        start: () => sync.start(),
        openRides: async () => {
          await openRides();
          while (ridePanel === undefined) await sleep(100);
          return ridePanel;
        },
        closeRides: () => (ridesOpen = false),
        list: () => invoke<RideSummary[]>("list_rides"),
        message: () => lastMessage,
        notices: () => notices.map((notice) => notice.title),
      });
    } catch (e) {
      report = { error: String(e), message: lastMessage };
    }
    await invoke("finish_check", { report: JSON.stringify(report, null, 2) });
  }

  /**
   * 経路を編集した直後に、窓を閉じる。保存を待っている編集が、閉じる前に書かれることを確かめる。
   *
   * 2 つ目の経由点は、保存を待つ時間が過ぎる前に閉じる。書かれたかは、起動した側が走行のファイルを見る。
   */
  async function checkClose(map: RouteMap) {
    await createRide("2031-03-01");
    map.jump([138.66, 36.35], 12);
    await map.idle();
    map.click([138.62, 36.345]);
    await sleep(1500);
    map.click([138.7, 36.36]);
    while ((doc?.waypoints.length ?? 0) < 2) await sleep(10);
    await invoke("close_check");
  }

  /** 地図の表示と、経路の編集・保存を確かめて、報告を書いて終了する。 */
  async function checkMap(roadDir: string) {
    let report: unknown;
    try {
      while (routeMap === undefined) await sleep(100);
      if (check?.step === "close") return await checkClose(routeMap);
      report = await runMapCheck(routeMap, {
        loadRoads: roadDir === "-" ? null : () => loadRoads(roadDir),
        loadGeo: check?.geo ? () => loadGeo(check!.geo!) : null,
        failGeo: () => loadGeo("-"),
        clearNotices: () => (notices = []),
        events: () => events,
        setSnap: (on) => (snapNew = on),
        create: createRide,
        open: select,
        path: () => doc ?? EMPTY,
        saved: async () => {
          await flush();
          return doc === null ? null : await invoke<RideDocument>("load_ride", { id: doc.ride.id });
        },
        list: () => invoke<RideSummary[]>("list_rides"),
        message: () => lastMessage,
        undo: async () => void (await undo()),
        redo: async () => void (await redo()),
        history: () => ({ undo: undoStack.length, redo: redoStack.length }),
        setBasemap,
        setRoads: (shown) => (showRoads = shown),
        fit: () => routeMap?.fitRoute(),
        focusEvent: (id) => {
          const event = events.find((e) => e.id === id);
          if (event) focusEvent(event);
        },
        showSettings: async (shown) => {
          settingsOpen = shown;
          await sleep(300);
        },
        showRides: async (shown) => {
          if (shown) await openRides();
          else ridesOpen = false;
          await sleep(300);
        },
        showOverview: async (shown) => {
          overviewOpen = shown;
          await sleep(300);
        },
      });
    } catch (e) {
      report = { error: String(e), message: lastMessage };
    }
    await invoke("finish_check", { report: JSON.stringify(report, null, 2) });
  }
</script>

<svelte:window {onkeydown} onmousedowncapture={keepKeys} />

{#if check?.file}
  <PlaybackCheck target={check.file} />
{:else if check !== undefined}
  <main class={mode} style:--side="{sideWidth}px">
    <div class="map" style:--map-left="{scaleLeft}px" style:--map-right="{pad.right - 12}px">
      <RouteMap
        bind:this={routeMap}
        path={doc ?? EMPTY}
        rideId={date}
        editable={mode === "route"}
        {roads}
        {showRoads}
        {basemap}
        {marks}
        {callout}
        here={here?.at ?? null}
        guessed={here?.guessed ?? false}
        follow={follow && mode === "confirm"}
        snap={snapNew && geoReady}
        canSnap={geoReady}
        selected={selectedWaypoint}
        timed={timedWaypoints}
        placing={placing?.route_m ?? null}
        {pad}
        onsnap={(at, radiusM) => invoke<[number, number] | null>("snap_point", { point: at, radiusM })}
        onselect={(index) => (selectedWaypoint = index)}
        onwaypoint={chooseWaypoint}
        onplace={placeStop}
        onedit={editPath}
      />
      <div class="overlay">
        <TopBar
          {date}
          title={doc?.ride.title ?? date ?? ""}
          {videoSummary}
          {mode}
          status={bar}
          failure={syncView.failure}
          compact={mode === "confirm"}
          canExport={doc !== null}
          canOverview={(doc?.waypoints.length ?? 0) >= 2}
          onrides={openRides}
          onmode={setMode}
          onexport={exportCsv}
          onoverview={() => (overviewOpen = true)}
          onsettings={() => (settingsOpen = true)}
          onretry={retrySave}
        />
        <Notices {notices} onclose={dismiss} />
        <div class="stage">
          <div class="zoom float" class:beside={mode === "route"}>
            <button type="button" aria-label="拡大" title="拡大" onclick={() => routeMap?.zoomBy(1)}>
              <Icon name="plus" />
            </button>
            <button type="button" aria-label="縮小" title="縮小" onclick={() => routeMap?.zoomBy(-1)}>
              <Icon name="minus" />
            </button>
          </div>
          {#if mode === "route"}
            <div class="left">
              <RouteTools
                canUndo={undoStack.length > 0}
                canRedo={redoStack.length > 0}
                snap={snapNew && geoReady}
                canSnap={geoReady}
                {basemap}
                hasRoads={roads !== null}
                {showRoads}
                canFit={(doc?.waypoints.length ?? 0) > 0}
                onundo={undo}
                onredo={redo}
                onsnap={(on) => (snapNew = on)}
                onbasemap={setBasemap}
                onroads={(shown) => (showRoads = shown)}
                onfit={() => routeMap?.fitRoute()}
                onsettings={() => (settingsOpen = true)}
              />
              <Hints shown={hintsShown} ontoggle={toggleHints} />
            </div>
            <div class="right">
              <RouteSide
                path={doc ?? EMPTY}
                {events}
                selected={selectedWaypoint}
                {focusedId}
                canExtract={geoReady}
                onwaypoint={focusWaypoint}
                onevent={focusEvent}
                onextract={extractAgain}
                onsettings={() => (settingsOpen = true)}
              />
            </div>
          {:else}
            <label class="follow float"><input type="checkbox" bind:checked={follow} />現在位置を追う</label>
            {#if placing !== null}
              <div class="placing float" role="status">
                停車した場所を、経路の上で押します
                <button type="button" class="btn" onclick={() => (placingId = null)}>やめる Esc</button>
              </div>
            {/if}
          {/if}
        </div>
      </div>
    </div>
    <div class="side">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="grip"
        title="ドラッグで、動画と一覧の幅を変える。ダブルクリックで元の幅に戻す"
        onpointerdown={resizeSide}
        ondblclick={resetSide}
      ></div>
      {#if timeline !== null}
        <Player
          bind:this={player}
          {timeline}
          muted={check.dir !== null}
          active={mode === "confirm"}
          ontime={(realMs) => (nowMs = realMs)}
        />
      {:else}
        <div class="no-video">
          {#if date === null}
            <b>走行を開いていません</b>
            <button type="button" class="btn" onclick={openRides}>走行を選ぶ</button>
          {:else if files.length > 0}
            <b>この日付の動画は、すべて走行から外してあります</b>
            <span>下のタイムラインで、薄い帯を押すと戻せます。</span>
            <span>時刻は、一覧の行を押して手で入れられます。</span>
          {:else}
            <b>この日付の動画はありません</b>
            <span>時刻は、一覧の行を押して手で入れられます。</span>
            <button type="button" class="btn" onclick={() => (settingsOpen = true)}>設定で動画フォルダを選ぶ</button>
          {/if}
        </div>
      {/if}
      <NextGuide
        {target}
        count={events.length}
        {nowMs}
        hasVideo={timeline !== null}
        canUndo={confirmedOnVideo.length > 0}
        stopOpen={openStop(events) !== null}
        onconfirm={confirmTarget}
        onmove={move}
        onundo={undoConfirm}
        onadd={addCustom}
        onstop={markStop}
      />
      <EventList
        {events}
        targetId={target?.id ?? null}
        {selectedId}
        {conflicts}
        gaps={timeline?.gaps ?? []}
        rideDate={date ?? ""}
        placingId={placing?.id ?? null}
        onselect={selectEvent}
        onchange={(id, change) => editEvents((current) => patch(current, id, change))}
        onhide={hideEvents}
        onremove={removeEvent}
        onplace={(id) => (placingId = id)}
        onextract={geoReady && doc !== null ? extractAgain : null}
      />
    </div>
    <div class="timeline">
      <TimelineBar
        bind:this={timelineBar}
        {timeline}
        excluded={excludedClips}
        {events}
        {conflicts}
        {nowMs}
        targetId={target?.id ?? null}
        onseek={(realMs) => void player?.seek(realMs)}
        onselect={choose}
        onexclude={setVideoExcluded}
        onstop={addStop}
        onunstop={removeEvent}
      />
    </div>
    {#if toast !== ""}
      <div class="toast {toastTone}" role="status">
        <Icon name={toastTone === "ok" ? "check" : "warning"} />{toast}
      </div>
    {/if}
  </main>
  {#if ridesOpen}
    <RidePanel
      bind:this={ridePanel}
      {date}
      title={doc?.ride.title ?? date ?? ""}
      {videos}
      {stored}
      sync={syncView}
      onselect={select}
      oncreate={createRide}
      ontitle={setTitle}
      onremove={removeRide}
      onresolve={(id, keep) => sync.resolve(id, keep)}
      onsettings={() => {
        ridesOpen = false;
        settingsOpen = true;
      }}
      onclose={() => (ridesOpen = false)}
    />
  {/if}
  {#if settingsOpen}
    <SettingsDialog
      {config}
      {videoState}
      {roadState}
      {geoState}
      {clockOffsetMs}
      rideDate={date}
      onpick={pick}
      onclock={setClockOffset}
      onsynced={() => {
        tell("同期の接続先を保存しました");
        void sync.start();
      }}
      onclose={() => (settingsOpen = false)}
    />
  {/if}
  {#if overviewOpen && doc !== null}
    <OverviewDialog {doc} onclose={() => (overviewOpen = false)} />
  {/if}
{/if}

<style>
  main {
    position: relative;
    display: grid;
    height: 100vh;
  }
  /* 経路入力のときは、地図を画面いっぱいに置く。 */
  main.route {
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
  }
  /*
   * 時刻入力のときは、地図が左、動画と一覧が右、タイムラインが下に全幅。右の列の幅は、境目を
   * ドラッグして変える。窓を狭めても、地図の幅が残るようにする。
   */
  main.confirm {
    grid-template: minmax(0, 1fr) 96px / minmax(0, 1fr) clamp(420px, var(--side), max(420px, 100vw - 540px));
  }
  .map {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--map-ground);
  }
  /* 地図の上に浮かせるもの。すき間では、下の地図を操作できる。 */
  .overlay {
    position: absolute;
    inset: var(--gutter);
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }
  .overlay > :global(*) {
    pointer-events: auto;
  }
  /* 上の帯と知らせの下の、残りの場所。道具と一覧を、この中の四隅に置く。 */
  .overlay > .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    margin-top: 4px;
    pointer-events: none;
  }
  .stage > * {
    position: absolute;
    pointer-events: auto;
  }
  .left {
    top: 0;
    bottom: 0;
    left: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--gutter);
    pointer-events: none;
  }
  .left > :global(*) {
    flex: none;
    pointer-events: auto;
  }
  /* 窓が低いときは、道具のほうを縮めて、中を送って見られるようにする。 */
  .left > :global(:first-child) {
    flex: 0 1 auto;
    min-height: 0;
    overflow-y: auto;
  }
  .right {
    top: 0;
    right: 0;
    bottom: 0;
    pointer-events: none;
  }
  .right > :global(*) {
    pointer-events: auto;
  }
  .zoom {
    top: 0;
    right: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: var(--radius-tile);
  }
  .zoom.beside {
    right: calc(300px + var(--gutter));
  }
  .zoom button {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 0;
    background: var(--surface);
  }
  .zoom button:hover {
    background: var(--surface-2);
  }
  .zoom button + button {
    border-top: 1px solid var(--line);
  }
  .follow {
    bottom: 0;
    left: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--radius-tile);
  }
  /* 停車の場所を決めている間、地図の上に出す。 */
  .placing {
    top: 0;
    left: 50%;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 6px 0 12px;
    border-radius: var(--radius-tile);
    transform: translateX(-50%);
    font-weight: 700;
    white-space: nowrap;
  }
  .follow input {
    width: 14px;
    height: 14px;
    margin: 0;
  }
  .side {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    border-left: 1px solid var(--line);
    background: var(--surface);
  }
  /* 地図との境目。ドラッグすると、右の列の幅が変わる。 */
  .grip {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -4px;
    z-index: 2;
    width: 8px;
    cursor: col-resize;
    touch-action: none;
  }
  .grip:hover,
  .grip:active {
    background: linear-gradient(90deg, transparent 2px, var(--primary) 2px 5px, transparent 5px);
  }
  .timeline {
    grid-column: 1 / -1;
    min-width: 0;
  }
  /* 経路入力の間も動画は読み込んだままにして、戻ったときに同じ位置から続けられるようにする。 */
  main.route .side,
  main.route .timeline {
    display: none;
  }
  .no-video {
    display: flex;
    flex: none;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    aspect-ratio: 16 / 9;
    max-height: calc(100vh - 416px);
    background: linear-gradient(180deg, #3a4552 0%, #262d36 62%, var(--video-ground) 100%);
    color: #d5dbe2;
  }
  .no-video b {
    color: #fff;
    font-size: 15px;
  }
  /* 白いボタンの字が、暗い地に合わせた淡い色にならないようにする。 */
  .no-video .btn {
    color: var(--ink);
  }
  main.confirm .toast {
    bottom: calc(96px + var(--gutter));
  }
  .toast {
    position: absolute;
    right: var(--gutter);
    bottom: var(--gutter);
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 40px;
    padding: 0 14px 0 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: 0 4px 14px rgba(26, 29, 33, 0.16);
  }
  .toast :global(.icon) {
    color: var(--ok);
  }
  .toast.warning :global(.icon) {
    color: var(--warn);
  }
</style>
