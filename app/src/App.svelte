<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import EventList from "./lib/EventList.svelte";
  import { pointAt } from "./lib/geo";
  import { runMapCheck } from "./lib/map-check";
  import PlaybackCheck from "./lib/PlaybackCheck.svelte";
  import Player from "./lib/Player.svelte";
  import { line, type Path } from "./lib/route";
  import RouteMap from "./lib/RouteMap.svelte";
  import { hms } from "./lib/time";
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
    { file: string | null; dir: string | null; map: string | null; geo: string | null } | undefined
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
  /** 経路が変わっていて、次の保存でイベントを抽出し直すか */
  let extractNeeded = false;
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
    if (check?.dir) return checkTimeline(check.dir);
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
  }

  /** 開いている日付の走行を、無ければ作って返す。日付を選んでいなければ null。 */
  async function ensureDoc(): Promise<RideDocument | null> {
    if (doc === null && date !== null) {
      doc = await invoke<RideDocument>("create_ride", { rideDate: date });
      stored = await invoke("list_rides");
    }
    return doc;
  }

  async function edit(change: (current: RideDocument) => RideDocument) {
    const current = await ensureDoc();
    if (current === null) {
      message = "走行を選ぶか、日付を入れて作ってください";
      return;
    }
    const next = change(current);
    if (next.waypoints !== current.waypoints || next.route !== current.route) extractNeeded = true;
    doc = next;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, SAVE_DELAY_MS);
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
      const saved = await invoke<{ updated_at: string; length_m: number; events: RideEvent[] }>("save_ride", {
        doc: saving,
        extract,
      });
      lengthM = saved.length_m;
      // 保存を待つ間に経路が変わっていたら、返ってきたイベントは古い。次の保存に任せる。
      if (doc !== null && doc.waypoints === saving.waypoints && doc.route === saving.route) {
        doc = { ...doc, events: saved.events };
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

  /** 地図の表示と、経路の編集・保存を確かめて、報告を書いて終了する。 */
  async function checkMap(roadDir: string) {
    let report: unknown;
    try {
      while (routeMap === undefined) await sleep(100);
      report = await runMapCheck(routeMap, {
        loadRoads: roadDir === "-" ? null : () => loadRoads(roadDir),
        loadGeo: check?.geo ? () => loadGeo(check!.geo!) : null,
        events: () => doc?.events ?? [],
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

{#if check?.file}
  <PlaybackCheck target={check.file} />
{:else if check !== undefined}
  <main>
    <header>
      <button type="button" onclick={() => pick("動画フォルダを選ぶ", loadVideos)}>動画フォルダを開く</button>
      <button type="button" onclick={() => pick("道路データのフォルダを選ぶ", loadRoads)}>道路データを開く</button>
      <button type="button" onclick={() => pick("判定データのフォルダを選ぶ", loadGeo)}>判定データを開く</button>
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
        onchange={(path) => edit((current) => ({ ...current, ...path }))}
      />
    </div>
    <div class="side">
      <div class="player">
        {#if timeline !== null}
          <Player bind:this={player} {timeline} muted={check.dir !== null} />
        {:else if date !== null}
          <p>この日付の動画はありません。</p>
        {/if}
      </div>
      {#if date !== null}
        <EventList
          events={doc?.events ?? []}
          onselect={(event) => {
            const at = pointAt(routeLine, event.route_m);
            if (at !== null) routeMap?.focus(at, event.label || "（名前なし）");
          }}
          onextract={geoReady ? extractAgain : null}
        />
      {/if}
    </div>
  </main>
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
    gap: 0.75rem;
    align-items: center;
    font-size: 0.9rem;
  }
  .message {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #555;
  }
  label input {
    width: 5rem;
  }
  .title {
    width: 14rem;
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
