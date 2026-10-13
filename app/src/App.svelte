<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { onMount, untrack } from "svelte";
  import { runConfirmCheck } from "./lib/confirm-check";
  import EventList from "./lib/EventList.svelte";
  import {
    confirmOnVideo,
    customAt,
    defaultTarget,
    knotsOf,
    neighbor,
    nextConfirmable,
    patch,
    positionAt,
    unconfirm,
  } from "./lib/events";
  import { pointAt } from "./lib/geo";
  import Icon from "./lib/Icon.svelte";
  import Hints from "./lib/Hints.svelte";
  import { forShortcut, keepKeys } from "./lib/keys";
  import { KINDS } from "./lib/kinds";
  import { runMapCheck } from "./lib/map-check";
  import NextGuide from "./lib/NextGuide.svelte";
  import Notices from "./lib/Notices.svelte";
  import PlaybackCheck from "./lib/PlaybackCheck.svelte";
  import Player from "./lib/Player.svelte";
  import RidePanel from "./lib/RidePanel.svelte";
  import { line, type Path } from "./lib/route";
  import RouteMap from "./lib/RouteMap.svelte";
  import RouteSide from "./lib/RouteSide.svelte";
  import RouteTools from "./lib/RouteTools.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import { runSyncCheck } from "./lib/sync-check";
  import { hms, short } from "./lib/time";
  import TimelineBar from "./lib/Timeline.svelte";
  import { runTimelineCheck } from "./lib/timeline-check";
  import TopBar from "./lib/TopBar.svelte";
  import type {
    Config,
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
    type Folder,
    type FolderState,
    type Mode,
    type Notice,
    remembered,
    type SaveState,
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

  /** 自動の確認の対象。`undefined` は問い合わせ中 */
  let check = $state<
    { file: string | null; dir: string | null; map: string | null; geo: string | null; sync: string | null } | undefined
  >(undefined);
  let player = $state<Player>();
  let routeMap = $state<RouteMap>();
  let ridePanel = $state<RidePanel>();
  let timelineBar = $state<TimelineBar>();
  /** いまの作業。経路を入れるか、動画を見て時刻を確定するか */
  let mode = $state<Mode>("route");
  let settingsOpen = $state(false);
  let ridesOpen = $state(false);
  /** 操作の手がかりを出しているか */
  let hintsShown = $state(remembered(HINTS_KEY, ["on", "off"], "on") === "on");
  let basemap = $state<Basemap>(remembered(BASEMAP_KEY, ["pale", "std", "photo"], "pale"));
  /** 道路の線を地図に重ねるか */
  let showRoads = $state(true);
  /** 時刻を確定するとき、地図が現在位置を追うか */
  let follow = $state(true);
  /** 選んでいる経由点の番号 */
  let selectedWaypoint = $state<number | null>(null);
  /** 経路を入れるときに、一覧で選んで地図で目立たせているイベント */
  let focusedId = $state<string | null>(null);
  let config = $state<Config>({ video_dir: null, road_data_dir: null, geo_data_dir: null });
  let videoState = $state<FolderState>("none");
  let roadState = $state<FolderState>("none");
  let geoState = $state<FolderState>("none");
  /** 上の帯の下に出している知らせ */
  let notices = $state<Notice[]>([]);
  /** 右下に出している、できたことの知らせ */
  let toast = $state("");
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  /** 最後に出した知らせ。自動の確認が読む */
  let lastMessage = "";
  let videos = $state<RideVideos[]>([]);
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
  /** 人が選んだ、次に確定する相手。選んでいなければ推定時刻から決める */
  let chosenId = $state<string | null>(null);
  /** 一覧で、直す欄を開いているイベント */
  let selectedId = $state<string | null>(null);
  /** 位置が先なのに時刻が後になっている、確定済みのイベントの ID */
  let conflicts = $state<string[]>([]);
  /** 動画を見て確定したイベントの ID。Backspace で新しい順に戻す */
  let confirmedOnVideo = $state.raw<string[]>([]);
  let timeline = $state<Timeline | null>(null);
  let saveState = $state<SaveState>("saved");
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  /** 書いている最中の保存。走行を消す前に、終わるのを待つ */
  let writing: Promise<void> | undefined;
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
  const clockOffsetMs = $derived(doc?.ride.clock_offset_ms ?? 0);
  const routeLine = $derived(doc === null ? [] : line(doc));
  const events = $derived(doc?.events ?? []);
  const target = $derived(
    events.find((event) => event.id === chosenId) ?? (timeline === null ? null : defaultTarget(events, nowMs)),
  );
  /** 動画のいまの時刻にいると推定される位置 */
  const here = $derived.by(() => {
    if (timeline === null) return null;
    const routeM = positionAt(knotsOf(events), nowMs);
    return routeM === null ? null : pointAt(routeLine, routeM);
  });
  const marks = $derived(
    events.flatMap((event) => {
      const at = pointAt(routeLine, event.route_m);
      return at === null ? [] : [{ at, kind: event.kind, hidden: event.status === "hidden" }];
    }),
  );
  /** 地図で名前を添えるイベント。時刻を確定するときは次の相手、経路を入れるときは一覧で選んだもの */
  const callout = $derived.by(() => {
    const event = mode === "confirm" ? target : (events.find((e) => e.id === focusedId) ?? null);
    const at = event === null ? null : pointAt(routeLine, event.route_m);
    if (event === null || at === null) return null;
    let note = `${(event.route_m / 1000).toFixed(1)} km`;
    if (mode === "confirm") {
      const rest = event.time_ms === null ? null : event.time_ms - nowMs;
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
  /** 上の帯に出す、開いている走行の動画の要約 */
  const videoSummary = $derived.by(() => {
    if (videoState === "loading") return "動画を読み込み中…";
    if (date === null) return "";
    if (files.length === 0) return "動画なし";
    return `動画 ${files.length} 本・${hms(files.reduce((sum, file) => sum + file.duration_ms, 0))}`;
  });

  onMount(async () => {
    check = await invoke("check_target");
    stored = await invoke("list_rides");
    if (check?.file) return;
    if (check?.dir && check.geo) return checkConfirm(check.dir, check.geo);
    if (check?.dir) return checkTimeline(check.dir);
    if (check?.sync) return checkSync(check.sync);
    if (check?.map) return checkMap(check.map);
    config = await invoke<Config>("get_config");
    if (config.road_data_dir) await loadRoads(config.road_data_dir);
    if (config.geo_data_dir) await loadGeo(config.geo_data_dir);
    if (config.video_dir) await loadVideos(config.video_dir);
    // 動画が無くても、記録のある走行があれば、いちばん新しいものを開いて始める。
    if (date === null && dates.length > 0) await select(dates[0]);
  });

  /** エラーか注意を、上の帯の下に出す。同じ事柄の知らせがあれば置き換える。 */
  function report(notice: Notice) {
    notices = [...notices.filter((old) => old.key !== notice.key), notice];
    lastMessage = `${notice.title}${notice.detail}`;
  }

  function dismiss(key: string) {
    notices = notices.filter((notice) => notice.key !== key);
  }

  /** できたことを、右下に短い間だけ出す。 */
  function tell(text: string) {
    toast = text;
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
      if (date === null && dates.length > 0) await select(dates[0]);
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
      extractAgain();
    } catch (e) {
      // 選ぶのは、roads.fgb・admin.fgb・michi-no-eki.fgb の 3 つが入ったフォルダである。
      geoState = "error";
      report({ key: "geo", level: "error", title: "判定データを開けません。", detail: String(e), action: repick("geo") });
    }
    config = await invoke<Config>("get_config");
  }

  /** 開いている走行のイベントを、いまの経路から抽出し直す。 */
  function extractAgain() {
    if (doc === null) return;
    extractNeeded = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, 0);
    saveState = "pending";
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

  async function select(next: string) {
    await flush();
    const summary = stored.find((s) => s.ride_date === next);
    doc = summary ? await invoke<RideDocument>("load_ride", { id: summary.id }) : null;
    date = next;
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

  /** 開いている日付の走行を、無ければ作って返す。日付を選んでいなければ null。 */
  async function ensureDoc(): Promise<RideDocument | null> {
    if (doc === null && date !== null) {
      doc = await invoke<RideDocument>("create_ride", { rideDate: date });
      stored = await invoke("list_rides");
    }
    return doc;
  }

  async function edit(change: (current: RideDocument) => RideDocument, delayMs = SAVE_DELAY_MS) {
    const current = await ensureDoc();
    if (current === null) {
      report({
        key: "ride",
        level: "warning",
        title: "走行を開いていません。",
        detail: "走行を選ぶか、日付を入れて作ってください。",
        action: { label: "走行を選ぶ", run: openRides },
      });
      return;
    }
    const next = change(current);
    if (next.waypoints !== current.waypoints || next.route !== current.route) extractNeeded = true;
    doc = next;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, delayMs);
    saveState = "pending";
  }

  /** 待っている保存があれば、いま書く。 */
  async function flush() {
    if (saveTimer === undefined || doc === null) return;
    clearTimeout(saveTimer);
    saveTimer = undefined;
    saveState = "saving";
    writing = write(doc);
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
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, 0);
    saveState = "pending";
  }

  const pathOf = (current: RideDocument | null): Path =>
    current === null ? EMPTY : { waypoints: current.waypoints, route: current.route };

  /** 経路を書き換える。取り消せるよう、書き換える前の経路を覚えておく。 */
  function editPath(next: Path) {
    if (date !== null) {
      undoStack = [...undoStack.slice(1 - HISTORY_LIMIT), pathOf(doc)];
      redoStack = [];
    }
    return edit((current) => ({ ...current, ...next }));
  }

  /** 経路の編集を 1 つ取り消す。 */
  function undo() {
    const previous = undoStack.at(-1);
    if (previous === undefined || doc === null) return;
    undoStack = undoStack.slice(0, -1);
    redoStack = [...redoStack, pathOf(doc)];
    return edit((current) => ({ ...current, ...previous }));
  }

  /** 取り消した編集を 1 つやり直す。 */
  function redo() {
    const next = redoStack.at(-1);
    if (next === undefined || doc === null) return;
    redoStack = redoStack.slice(0, -1);
    undoStack = [...undoStack, pathOf(doc)];
    return edit((current) => ({ ...current, ...next }));
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

  /** 一覧で選んだイベントを次に確定する相手にして、地図と動画をそこへ動かす。 */
  function choose(event: RideEvent) {
    chosenId = event.id;
    const at = pointAt(routeLine, event.route_m);
    if (at !== null) routeMap?.focus(at);
    if (event.time_ms !== null) void player?.seek(event.time_ms);
  }

  /** 一覧の行を押した。次に確定する相手にして、直す欄を開く。開いている行なら、欄を閉じる。 */
  function selectEvent(event: RideEvent) {
    if (selectedId === event.id) {
      selectedId = null;
      return;
    }
    selectedId = event.id;
    choose(event);
  }

  /** 経路を入れるときに、一覧で選んだイベントの位置を地図に出す。もう一度選ぶと、目立たせるのをやめる。 */
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

  /** 次に確定する相手を、動画のいまの位置の時刻で確定する。 */
  function confirmTarget() {
    if (player === undefined || target === null || doc === null) return;
    const [id, realMs, offsetMs] = [target.id, player.now(), clockOffsetMs];
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
    if (player === undefined || doc === null) return;
    const custom = customAt(player.now(), clockOffsetMs, "メモ");
    // 足したら、すぐ名前を付けられるように、直す欄を開く。
    chosenId = custom.id;
    selectedId = custom.id;
    void editEvents((current) => [...current, custom]);
  }

  /** 次に確定する相手を隣のイベントへ移し、動画をその時刻へ動かす。 */
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

  /** その日付の走行を開き、記録が無ければ作る。 */
  async function createRide(rideDate: string) {
    await select(rideDate);
    await ensureDoc();
  }

  /** 走行の一覧を開く。同期の前に、待っている保存を書いておく。 */
  async function openRides() {
    await flush();
    ridesOpen = true;
  }

  /** 受け取りで手元の走行が変わった。一覧と、開いている走行を読み直す。 */
  async function reload() {
    stored = await invoke("list_rides");
    if (date !== null) {
      clearTimeout(saveTimer);
      saveTimer = undefined;
      saveState = "saved";
      await select(date);
    }
  }

  /** 手元の走行の記録を消す。開いている走行なら、待っている保存を捨ててから消す。 */
  async function removeRide(id: string) {
    const opened = doc?.ride.id === id;
    if (opened) {
      clearTimeout(saveTimer);
      saveTimer = undefined;
      await writing;
    }
    await invoke("delete_ride", { id });
    stored = await invoke("list_rides");
    if (opened) {
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
    }
    tell("走行の記録を消しました");
  }

  $effect(() => {
    if (files.length === 0) {
      timeline = null;
      return;
    }
    void invoke<Timeline>("build_timeline", { files, clockOffsetMs }).then((built) => {
      timeline = built;
    });
  });

  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  /** フォルダを読み、最初の走行でファイルをまたぐ再生を確かめて、報告を書いて終了する。 */
  async function checkTimeline(target: string) {
    let report: unknown;
    try {
      await loadVideos(target);
      await select(dates[dates.length - 1]);
      setMode("confirm");
      while (timeline === null || player === undefined) await sleep(100);
      report = await runTimelineCheck(timeline, player, (seconds) => setClockOffset(seconds * 1000));
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
      while (timeline === null || player === undefined || routeMap === undefined) await sleep(100);
      report = await runConfirmCheck(timeline, player, routeMap, {
        setMode,
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
        exportCsv: async () => {
          const path = await invoke<string>("check_path", { name: "events.csv" });
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

  /** クラウドとの同期を確かめて、報告を書いて終了する。 */
  async function checkSync(apiUrl: string) {
    let report: unknown;
    try {
      while (routeMap === undefined) await sleep(100);
      report = await runSyncCheck(apiUrl, routeMap, {
        create: createRide,
        setTitle: async (title) => {
          await setTitle(title);
          await flush();
        },
        doc: () => doc,
        openRides: async () => {
          await openRides();
          while (ridePanel === undefined) await sleep(100);
          return ridePanel;
        },
        list: () => invoke<RideSummary[]>("list_rides"),
      });
    } catch (e) {
      report = { error: String(e), message: lastMessage };
    }
    await invoke("finish_check", { report: JSON.stringify(report, null, 2) });
  }

  /** 地図の表示と、経路の編集・保存を確かめて、報告を書いて終了する。 */
  async function checkMap(roadDir: string) {
    let report: unknown;
    try {
      while (routeMap === undefined) await sleep(100);
      report = await runMapCheck(routeMap, {
        loadRoads: roadDir === "-" ? null : () => loadRoads(roadDir),
        loadGeo: check?.geo ? () => loadGeo(check!.geo!) : null,
        failGeo: () => loadGeo("-"),
        clearNotices: () => (notices = []),
        events: () => events,
        setSnap: (on) => (snapNew = on),
        create: createRide,
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
  <main class={mode}>
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
        {here}
        follow={follow && mode === "confirm"}
        snap={snapNew && geoReady}
        canSnap={geoReady}
        selected={selectedWaypoint}
        {pad}
        onsnap={(at, radiusM) => invoke<[number, number] | null>("snap_point", { point: at, radiusM })}
        onselect={(index) => (selectedWaypoint = index)}
        onchange={editPath}
      />
      <div class="overlay">
        <TopBar
          {date}
          title={doc?.ride.title ?? date ?? ""}
          {videoSummary}
          {mode}
          saveState={doc === null ? null : saveState}
          compact={mode === "confirm"}
          canExport={doc !== null}
          onrides={openRides}
          onmode={setMode}
          onexport={exportCsv}
          onsync={openRides}
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
          {/if}
        </div>
      </div>
    </div>
    <div class="side">
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
          <b>{date === null ? "走行を開いていません" : "この日付の動画はありません"}</b>
          {#if date === null}
            <button type="button" class="btn" onclick={openRides}>走行を選ぶ</button>
          {:else}
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
        onconfirm={confirmTarget}
        onmove={move}
        onundo={undoConfirm}
        onadd={addCustom}
      />
      <EventList
        {events}
        targetId={target?.id ?? null}
        {selectedId}
        {conflicts}
        gaps={timeline?.gaps ?? []}
        rideDate={date ?? ""}
        onselect={selectEvent}
        onchange={(id, change) => editEvents((current) => patch(current, id, change))}
        onremove={(id) => editEvents((current) => current.filter((event) => event.id !== id))}
        onextract={geoReady && doc !== null ? extractAgain : null}
      />
    </div>
    <div class="timeline">
      <TimelineBar
        bind:this={timelineBar}
        {timeline}
        {events}
        {conflicts}
        {nowMs}
        targetId={target?.id ?? null}
        onseek={(realMs) => void player?.seek(realMs)}
        onselect={choose}
      />
    </div>
    {#if toast !== ""}
      <div class="toast" role="status"><Icon name="check" />{toast}</div>
    {/if}
  </main>
  {#if ridesOpen}
    <RidePanel
      bind:this={ridePanel}
      {date}
      title={doc?.ride.title ?? date ?? ""}
      {videos}
      {stored}
      onselect={select}
      oncreate={createRide}
      ontitle={setTitle}
      onremove={removeRide}
      onchanged={reload}
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
      onsynced={() => tell("同期の接続先を保存しました")}
      onclose={() => (settingsOpen = false)}
    />
  {/if}
{/if}

<style>
  main {
    position: relative;
    display: grid;
    height: 100vh;
  }
  /* 経路を入れるときは、地図を画面いっぱいに置く。 */
  main.route {
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
  }
  /* 時刻を確定するときは、地図が左、動画と一覧が右、タイムラインが下に全幅。 */
  main.confirm {
    grid-template: minmax(0, 1fr) 96px / minmax(0, 1fr) 520px;
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
  .follow input {
    width: 14px;
    height: 14px;
    margin: 0;
  }
  .side {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    border-left: 1px solid var(--line);
    background: var(--surface);
  }
  .timeline {
    grid-column: 1 / -1;
    min-width: 0;
  }
  /* 経路を入れる間も動画は読み込んだままにして、戻ったときに同じ位置から続けられるようにする。 */
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
    background: linear-gradient(180deg, #3a4552 0%, #262d36 62%, var(--video-ground) 100%);
    color: #d5dbe2;
  }
  .no-video b {
    color: #fff;
    font-size: 15px;
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
</style>
