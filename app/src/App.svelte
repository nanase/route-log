<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
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
  import { KINDS } from "./lib/kinds";
  import { runMapCheck } from "./lib/map-check";
  import { runSyncCheck } from "./lib/sync-check";
  import SyncPanel from "./lib/SyncPanel.svelte";
  import PlaybackCheck from "./lib/PlaybackCheck.svelte";
  import Player from "./lib/Player.svelte";
  import { line, type Path } from "./lib/route";
  import RouteMap from "./lib/RouteMap.svelte";
  import { clock, hms } from "./lib/time";
  import { runTimelineCheck } from "./lib/timeline-check";
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

  /** 保存を待つ時間。続けて編集している間は書かない */
  const SAVE_DELAY_MS = 400;
  const EMPTY: Path = { waypoints: [], route: { segments: [] } };

  /** 自動の確認の対象。`undefined` は問い合わせ中 */
  let check = $state<
    { file: string | null; dir: string | null; map: string | null; geo: string | null; sync: string | null } | undefined
  >(undefined);
  let player = $state<Player>();
  let routeMap = $state<RouteMap>();
  let message = $state("");
  let videos = $state<RideVideos[]>([]);
  let stored = $state<RideSummary[]>([]);
  let roads = $state<RoadData | null>(null);
  /** 開いている走行の日付 */
  let date = $state<string | null>(null);
  /** 開いている走行。まだ何も記録していない日付では null。編集のたびに丸ごと入れ替える */
  let doc = $state.raw<RideDocument | null>(null);
  /** 判定用データを開いてあるか */
  let geoReady = $state(false);
  /** 新しく足す区間を、道に沿わせるか */
  let snapNew = $state(false);
  /** 同期の画面を開いているか */
  let syncOpen = $state(false);
  let syncPanel = $state<SyncPanel>();
  /** 経路が変わっていて、次の保存でイベントを抽出し直すか */
  let extractNeeded = false;
  /** 動画の再生位置の実時刻 */
  let nowMs = $state(0);
  /** 人が選んだ、次に確定する相手。選んでいなければ推定時刻から決める */
  let chosenId = $state<string | null>(null);
  /** 位置が先なのに時刻が後になっている、確定済みのイベントの ID */
  let conflicts = $state<string[]>([]);
  /** 動画を見て確定したイベントの ID。Backspace で新しい順に戻す */
  let confirmedOnVideo: string[] = [];
  let lengthM = $state(0);
  let timeline = $state<Timeline | null>(null);
  let newDate = $state("");
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

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
  const ticks = $derived(
    events.flatMap((event) =>
      event.status === "hidden" || event.time_ms === null
        ? []
        : [{ time_ms: event.time_ms, confirmed: event.status === "confirmed" }],
    ),
  );
  const note = $derived.by(() => {
    if (target === null || timeline === null) return "";
    // 出発と到着は、名前を直していなければ種類と同じ語になる。重ねて出さない。
    const kind = KINDS[target.kind];
    const name = target.label === kind ? kind : `${kind} ${target.label || "（名前なし）"}`;
    if (target.status === "confirmed") return `選択中: ${name} — 確定済み。Enter で確定し直す`;
    if (target.time_ms === null) return `次: ${name} — Enter で確定`;
    const rest = target.time_ms - nowMs;
    const when = rest >= 0 ? `あと ${hms(rest)}` : `${hms(-rest)} 前`;
    return `次: ${name} — 推定 ${clock(target.time_ms)}（${when}）Enter で確定`;
  });
  const marks = $derived(
    (doc?.events ?? []).flatMap((event) => {
      const at = pointAt(routeLine, event.route_m);
      return at === null ? [] : [{ at, label: event.label, hidden: event.status === "hidden" }];
    }),
  );

  onMount(async () => {
    check = await invoke("check_target");
    stored = await invoke("list_rides");
    if (check?.file) return;
    if (check?.dir && check.geo) return checkConfirm(check.dir, check.geo);
    if (check?.dir) return checkTimeline(check.dir);
    if (check?.sync) return checkSync(check.sync);
    if (check?.map) return checkMap(check.map);
    const config = await invoke<Config>("get_config");
    if (config.road_data_dir) await loadRoads(config.road_data_dir);
    if (config.geo_data_dir) await loadGeo(config.geo_data_dir);
    if (config.video_dir) await loadVideos(config.video_dir);
  });

  async function loadVideos(dir: string) {
    message = "動画を読み込み中…";
    try {
      const scan = await invoke<Scan>("scan_videos", { dir });
      videos = scan.rides;
      const count = videos.reduce((n, ride) => n + ride.files.length, 0);
      message = `動画 ${count} 本`;
      if (scan.skipped.length > 0) {
        message += `、読めないファイル ${scan.skipped.length} 本（${scan.skipped[0].reason}）`;
      }
      if (date === null && dates.length > 0) await select(dates[0]);
    } catch (e) {
      message = String(e);
    }
  }

  async function loadRoads(dir: string) {
    try {
      roads = await invoke<RoadData>("open_road_data", { dir });
    } catch (e) {
      message = String(e);
    }
  }

  async function loadGeo(dir: string) {
    try {
      await invoke("open_geo_data", { dir });
      geoReady = true;
      extractAgain();
    } catch (e) {
      message = String(e);
    }
  }

  /** 開いている走行のイベントを、いまの経路から抽出し直す。 */
  function extractAgain() {
    if (doc === null) return;
    extractNeeded = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, 0);
  }

  async function pick(title: string, load: (dir: string) => Promise<void>) {
    const picked = await open({ directory: true, title });
    if (picked !== null) await load(picked);
  }

  async function select(next: string) {
    await flush();
    const summary = stored.find((s) => s.ride_date === next);
    doc = summary ? await invoke<RideDocument>("load_ride", { id: summary.id }) : null;
    lengthM = summary?.length_m ?? 0;
    date = next;
    chosenId = null;
    conflicts = [];
    confirmedOnVideo = [];
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
      message = "走行を選ぶか、日付を入れて作ってください";
      return;
    }
    const next = change(current);
    if (next.waypoints !== current.waypoints || next.route !== current.route) extractNeeded = true;
    doc = next;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, delayMs);
  }

  /** 待っている保存があれば、いま書く。 */
  async function flush() {
    if (saveTimer === undefined || doc === null) return;
    clearTimeout(saveTimer);
    saveTimer = undefined;
    const saving = doc;
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
      lengthM = saved.length_m;
      // 保存を待つ間に編集が進んでいたら、返ってきたイベントは古い。次の保存に任せる。
      if (doc === saving) {
        doc = { ...doc, route: saved.route, events: saved.events };
        conflicts = saved.conflicts;
      }
      stored = stored.map((s) =>
        s.id === saving.ride.id
          ? { ...s, updated_at: saved.updated_at, length_m: saved.length_m, waypoints: saving.waypoints.length }
          : s,
      );
    } catch (e) {
      extractNeeded ||= extract;
      message = `保存できません: ${e}`;
    }
  }

  /** イベントを書き換える。時刻の推定をすぐ見せたいので、待たずに保存する。 */
  function editEvents(change: (events: RideEvent[]) => RideEvent[]) {
    return edit((current) => ({ ...current, events: change(current.events) }), 0);
  }

  /** 一覧で選んだイベントを次に確定する相手にして、地図と動画をそこへ動かす。 */
  function choose(event: RideEvent) {
    chosenId = event.id;
    const at = pointAt(routeLine, event.route_m);
    if (at !== null) routeMap?.focus(at, event.label || KINDS[event.kind]);
    if (event.time_ms !== null) void player?.seek(event.time_ms);
  }

  /** 次に確定する相手を、動画のいまの位置の時刻で確定する。 */
  function confirmTarget() {
    if (player === undefined || target === null || doc === null) return;
    const [id, realMs, offsetMs] = [target.id, player.now(), clockOffsetMs];
    confirmedOnVideo.push(id);
    chosenId = nextConfirmable(events, id)?.id ?? null;
    void editEvents((current) => patch(current, id, (event) => confirmOnVideo(event, realMs, offsetMs)));
  }

  /** 動画を見て確定したもののうち、いちばん新しい確定を外す。 */
  function undoConfirm() {
    const id = confirmedOnVideo.pop();
    if (id === undefined) return;
    chosenId = id;
    void editEvents((current) => patch(current, id, unconfirm));
  }

  /** 動画のいまの位置に、手動のイベントを足す。 */
  function addCustom() {
    if (player === undefined || doc === null) return;
    const custom = customAt(player.now(), clockOffsetMs, "メモ");
    chosenId = custom.id;
    void editEvents((current) => [...current, custom]);
  }

  /** 次に確定する相手を隣のイベントへ移し、動画をその時刻へ動かす。 */
  function move(step: 1 | -1) {
    const next = neighbor(events, target?.id ?? chosenId, step);
    if (next !== null) choose(next);
  }

  function onkeydown(event: KeyboardEvent) {
    const from = event.target;
    if (from instanceof HTMLInputElement || from instanceof HTMLTextAreaElement) return;
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const key = event.key.toLowerCase();
    if (key === "enter") confirmTarget();
    else if (key === "n") move(1);
    else if (key === "p") move(-1);
    else if (key === "backspace") undoConfirm();
    else if (key === "a") addCustom();
    else return;
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
    try {
      const rows = await invoke<number>("export_csv", { doc, path });
      message = `${rows} 件を書き出しました`;
    } catch (e) {
      message = `書き出せません: ${e}`;
    }
  }

  async function createRide() {
    if (newDate === "") return;
    await select(newDate);
    await ensureDoc();
    newDate = "";
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
      while (timeline === null || player === undefined) await sleep(100);
      report = await runTimelineCheck(timeline, player, async (seconds) => {
        await edit((current) => ({ ...current, ride: { ...current.ride, clock_offset_ms: seconds * 1000 } }));
      });
      await invoke("capture_check", { name: "played" });
    } catch (e) {
      report = { error: String(e), message };
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
        events: () => events,
        target: () => target?.id ?? null,
        choose: (id) => {
          const event = events.find((e) => e.id === id);
          if (event) choose(event);
        },
        setClockOffsetSeconds: (seconds) =>
          edit((current) => ({ ...current, ride: { ...current.ride, clock_offset_ms: seconds * 1000 } })),
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
        message: () => message,
      });
    } catch (e) {
      report = { error: String(e), message };
    }
    await invoke("finish_check", { report: JSON.stringify(report, null, 2) });
  }

  /** 受け取りで手元の走行が変わった。一覧と、開いている走行を読み直す。 */
  async function reload() {
    stored = await invoke("list_rides");
    if (date !== null) {
      clearTimeout(saveTimer);
      saveTimer = undefined;
      await select(date);
    }
  }

  /** クラウドとの同期を確かめて、報告を書いて終了する。 */
  async function checkSync(apiUrl: string) {
    let report: unknown;
    try {
      while (routeMap === undefined) await sleep(100);
      report = await runSyncCheck(apiUrl, routeMap, {
        create: async (rideDate) => {
          newDate = rideDate;
          await createRide();
        },
        setTitle: async (title) => {
          await edit((current) => ({ ...current, ride: { ...current.ride, title } }));
          await flush();
        },
        doc: () => doc,
        openSync: async () => {
          await flush();
          syncOpen = true;
          while (syncPanel === undefined) await sleep(100);
          return syncPanel;
        },
      });
    } catch (e) {
      report = { error: String(e), message };
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
        events: () => events,
        setSnap: (on) => (snapNew = on),
        create: async (rideDate) => {
          newDate = rideDate;
          await createRide();
        },
        edit: (change) => edit((current) => ({ ...current, ...change(current) })),
        path: () => doc ?? EMPTY,
        saved: async () => {
          await flush();
          return doc === null ? null : await invoke<RideDocument>("load_ride", { id: doc.ride.id });
        },
        list: () => invoke<RideSummary[]>("list_rides"),
        message: () => message,
      });
    } catch (e) {
      report = { error: String(e), message };
    }
    await invoke("finish_check", { report: JSON.stringify(report, null, 2) });
  }

  const summaryOf = (rideDate: string) => stored.find((s) => s.ride_date === rideDate);
  const videosOf = (rideDate: string) => videos.find((v) => v.date === rideDate);
  const totalMs = (ride: RideVideos) => ride.files.reduce((sum, file) => sum + file.duration_ms, 0);
</script>

<svelte:window {onkeydown} />

{#if check?.file}
  <PlaybackCheck target={check.file} />
{:else if check !== undefined}
  <main>
    <header>
      <button type="button" onclick={() => pick("動画フォルダを選ぶ", loadVideos)}>動画フォルダを開く</button>
      <button type="button" onclick={() => pick("道路データのフォルダを選ぶ", loadRoads)}>道路データを開く</button>
      <button type="button" onclick={() => pick("判定データのフォルダを選ぶ", loadGeo)}>判定データを開く</button>
      <label
        title="新しく足す区間を、国道・都道府県道に沿わせます。Shift を押しながら経由点をクリックすると、区間ごとに切り替えられます"
      >
        <input type="checkbox" bind:checked={snapNew} disabled={!geoReady} />道に沿わせる
      </label>
      <button
        type="button"
        onclick={async () => {
          await flush();
          syncOpen = true;
        }}>同期</button
      >
      <span class="message">{message}</span>
      {#if date !== null}
        <input
          class="title"
          aria-label="走行の題"
          value={doc?.ride.title ?? date}
          onchange={(event) => {
            const title = event.currentTarget.value.trim() || date!;
            void edit((current) => ({ ...current, ride: { ...current.ride, title } }));
          }}
        />
        <span>経由点 {doc?.waypoints.length ?? 0}・{(lengthM / 1000).toFixed(1)} km</span>
        <button type="button" onclick={exportCsv} disabled={doc === null}>CSV を書き出す</button>
        <label>
          時計の補正（秒）
          <input
            type="number"
            step="1"
            value={clockOffsetMs / 1000}
            onchange={(event) => {
              const clock_offset_ms = Math.round((event.currentTarget.valueAsNumber || 0) * 1000);
              void edit((current) => ({ ...current, ride: { ...current.ride, clock_offset_ms } }));
            }}
          />
        </label>
      {/if}
    </header>
    <nav>
      <form onsubmit={(event) => (event.preventDefault(), createRide())}>
        <input type="date" aria-label="新しい走行の日付" bind:value={newDate} />
        <button type="submit" disabled={newDate === ""}>作る</button>
      </form>
      {#each dates as rideDate (rideDate)}
        {@const ride = videosOf(rideDate)}
        {@const summary = summaryOf(rideDate)}
        <button type="button" class="ride" class:selected={rideDate === date} onclick={() => select(rideDate)}>
          <strong>{rideDate}</strong>
          <span>{ride ? `動画 ${ride.files.length} 本・${hms(totalMs(ride))}` : "動画なし"}</span>
          <span>{summary ? `経路 ${(summary.length_m / 1000).toFixed(1)} km` : "記録なし"}</span>
        </button>
      {/each}
    </nav>
    <div class="map">
      <RouteMap
        bind:this={routeMap}
        path={doc ?? EMPTY}
        rideId={date}
        {roads}
        {marks}
        {here}
        snap={snapNew && geoReady}
        onchange={(path) => edit((current) => ({ ...current, ...path }))}
      />
    </div>
    <div class="side">
      <div class="player">
        {#if timeline !== null}
          <Player
            bind:this={player}
            {timeline}
            {ticks}
            {note}
            muted={check.dir !== null}
            ontime={(realMs) => (nowMs = realMs)}
          />
        {:else if date !== null}
          <p>この日付の動画はありません。</p>
        {/if}
      </div>
      {#if date !== null}
        <EventList
          {events}
          targetId={target?.id ?? null}
          {conflicts}
          gaps={timeline?.gaps ?? []}
          rideDate={date}
          onselect={choose}
          onchange={(id, change) => editEvents((current) => patch(current, id, change))}
          onremove={(id) => editEvents((current) => current.filter((event) => event.id !== id))}
          onextract={geoReady ? extractAgain : null}
        />
      {/if}
    </div>
  </main>
  {#if syncOpen}
    <SyncPanel bind:this={syncPanel} onchanged={reload} onclose={() => (syncOpen = false)} />
  {/if}
{/if}

<style>
  :global(body) {
    margin: 0;
    overflow: hidden;
    font-family: system-ui, sans-serif;
  }
  main {
    display: grid;
    grid-template: auto minmax(0, 1fr) / 11rem minmax(0, 3fr) minmax(0, 2fr);
    gap: 0.5rem;
    height: 100vh;
    box-sizing: border-box;
    padding: 0.5rem;
  }
  header {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem 0.75rem;
    align-items: center;
    font-size: 0.9rem;
  }
  /* 幅が足りなければ段を分ける。ボタンや欄の中では折り返さない。 */
  header > :global(*) {
    flex-shrink: 0;
    white-space: nowrap;
  }
  header > .message {
    flex: 1 1 4rem;
    min-width: 4rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #555;
  }
  label input[type="number"] {
    width: 5rem;
  }
  .title {
    width: 11rem;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    overflow: auto;
  }
  nav form {
    display: flex;
    gap: 0.25rem;
  }
  nav form input {
    min-width: 0;
    flex: 1;
  }
  .ride {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 0.4rem 0.5rem;
    border: 1px solid #ccc;
    background: #fff;
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .ride.selected {
    border-color: #1f4e79;
    background: #e6eef7;
  }
  .map,
  .side,
  .player {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .side {
    display: grid;
    grid-template-rows: minmax(0, 3fr) minmax(0, 2fr);
    gap: 0.5rem;
  }
  .player p {
    color: #777;
    font-size: 0.9rem;
  }
</style>
