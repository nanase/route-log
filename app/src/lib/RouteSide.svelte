<script lang="ts">
  import EpisodeList from "./EpisodeList.svelte";
  import EventName from "./EventName.svelte";
  import KindTag from "./KindTag.svelte";
  import Spinner from "./Spinner.svelte";
  import { type Path, segmentLengths } from "./route";
  import StatusMark from "./StatusMark.svelte";
  import type { Episode, RideEvent, SegmentMode, Span } from "./types";

  /**
   * 経路入力のときに、地図の右に浮かせる一覧。上から経由点、回、経路から出たイベントを並べる。
   */
  let {
    path,
    events,
    episodes,
    spans,
    selected,
    focusedId,
    canExtract,
    extracting,
    exportingId,
    onwaypoint,
    onevent,
    onextract,
    onsettings,
    onaddepisode,
    onchangeepisode,
    onremoveepisode,
    onexportepisode,
  }: {
    path: Path;
    events: RideEvent[];
    episodes: Episode[];
    /** タイムラインの範囲。動画が無ければ null */
    spans: Span[] | null;
    /** 選んでいる経由点の番号 */
    selected: number | null;
    /** 地図で目立たせているイベント */
    focusedId: string | null;
    /** 抽出できるか。判定用データを開いていなければ、できない */
    canExtract: boolean;
    /** 抽出し直している最中 */
    extracting: boolean;
    /** エクスポートしているパート */
    exportingId: string | null;
    onwaypoint: (index: number) => void;
    onevent: (event: RideEvent) => void;
    onextract: () => void;
    onsettings: () => void;
    onaddepisode: () => void;
    onchangeepisode: (id: string, change: Partial<Omit<Episode, "id">>) => void;
    onremoveepisode: (id: string) => void;
    onexportepisode: (episode: Episode, name: string) => void;
  } = $props();

  const MODES: Record<SegmentMode, string> = { snap: "道に沿う", straight: "直線", manual: "直線（手修正）" };

  const lengths = $derived(segmentLengths(path));
  /** 経由点ごとの、始点からの距離（m） */
  const reached = $derived(
    path.waypoints.map((_, index) => lengths.slice(0, index).reduce((sum, length) => sum + length, 0)),
  );
  const last = $derived(path.waypoints.length - 1);
  const km = (m: number) => (m / 1000).toFixed(1);
</script>

<aside class="float">
  <header>
    <b>経由地 {path.waypoints.length}</b>
    <span class="space"></span>
    <b class="mono">{km(reached[last] ?? 0)} km</b>
  </header>
  <div class="waypoints">
    {#each path.waypoints as waypoint, index (index)}
      <button type="button" class="row waypoint" class:on={index === selected} onclick={() => onwaypoint(index)}>
        <span class="number mono" class:end={index === 0 || index === last}>{index + 1}</span>
        <span class="mono reached">{km(reached[index])} km</span>
        {#if index < last}
          <span class="mode">
            <span class="sample" class:dashed={waypoint.segment_mode !== "snap"}></span>{MODES[waypoint.segment_mode]}
          </span>
          <span class="mono length">{km(lengths[index])}</span>
        {:else}
          <span class="mode">{index === 0 ? "始まり" : "終わり"}</span>
          <span></span>
        {/if}
      </button>
    {:else}
      <p class="empty">地図をクリックして経由地を追加</p>
    {/each}
  </div>

  <EpisodeList
    {path}
    {events}
    {episodes}
    {spans}
    onadd={onaddepisode}
    onchange={onchangeepisode}
    onremove={onremoveepisode}
    onexport={onexportepisode}
    {exportingId}
  />

  <header class="events-head">
    <b>イベント {events.length} 件</b>
    <span class="space"></span>
    {#if canExtract}
      <button type="button" class="btn small" onclick={onextract} disabled={extracting}>
        {#if extracting}<Spinner />{/if}再抽出
      </button>
    {/if}
  </header>
  <div class="events">
    {#each events as event (event.id)}
      {@const hidden = event.hidden === true}
      <button
        type="button"
        class="row event"
        class:hidden
        class:on={event.id === focusedId}
        onclick={() => onevent(event)}
      >
        <span class="mono at">{km(event.route_m)}</span>
        <KindTag kind={event.kind} faded={hidden} />
        <span class="name"><EventName {event} /></span>
        {#if hidden}<span class="off"><StatusMark status={event.status} hidden /></span>{/if}
      </button>
    {:else}
      {#if !canExtract}
        <p class="empty">
          判定データを選ぶと、経路からイベントを抽出します。
          <button type="button" class="link" onclick={onsettings}>設定を開く</button>
        </p>
      {:else}
        <p class="empty">経由地を 2 つ追加すると、ここにイベントが出ます。</p>
      {/if}
    {/each}
  </div>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    width: 300px;
    max-height: 100%;
    overflow: hidden;
  }
  header {
    display: flex;
    flex: none;
    align-items: baseline;
    gap: 8px;
    box-sizing: border-box;
    height: 36px;
    padding: 10px 12px 0;
  }
  .events-head {
    align-items: center;
    height: 40px;
    padding: 0 8px 0 12px;
    border-top: 1px solid var(--line);
    white-space: nowrap;
  }
  .space {
    flex: 1;
  }
  .waypoints,
  .events {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow-y: auto;
  }
  /* 経由点が多くても、イベントの一覧に場所を残す。 */
  .waypoints {
    flex: 0 1 auto;
    max-height: 40%;
    padding-bottom: 6px;
  }
  .events {
    flex: 1 1 auto;
    padding-bottom: 8px;
  }
  .row {
    display: grid;
    flex: none;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    border: 0;
    background: none;
    text-align: left;
    white-space: nowrap;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.on {
    background: var(--primary-soft);
    box-shadow: inset 0 0 0 1px var(--primary);
  }
  .waypoint {
    grid-template-columns: 22px 56px minmax(0, 1fr) 40px;
    height: 28px;
  }
  .number {
    display: inline-grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--surface);
    box-shadow: inset 0 0 0 2px var(--route);
    font-size: 11px;
    font-weight: 700;
    line-height: 1;
  }
  .number.end {
    background: var(--ink);
    box-shadow: none;
    color: #fff;
  }
  .waypoint.on .number {
    background: var(--primary);
    box-shadow: none;
    color: #fff;
  }
  .reached,
  .length {
    text-align: right;
  }
  .waypoint.on .reached {
    font-weight: 700;
  }
  .mode {
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
  }
  .mode,
  .length {
    color: var(--ink-3);
  }
  .waypoint.on .mode,
  .waypoint.on .length {
    color: var(--ink);
  }
  /* 地図の線と同じ見本。道に沿う区間は実線、それ以外は破線。 */
  .sample {
    flex: none;
    width: 18px;
    height: 3px;
    border-radius: 2px;
    background: var(--route);
  }
  .sample.dashed {
    border-radius: 0;
    background: repeating-linear-gradient(90deg, var(--route) 0 5px, transparent 5px 8px);
  }
  .event {
    grid-template-columns: 34px 18px minmax(0, 1fr) auto;
    height: var(--row);
  }
  .at {
    color: var(--ink-3);
    text-align: right;
  }
  .name {
    display: flex;
    min-width: 0;
    overflow: hidden;
  }
  .event.hidden {
    color: var(--ink-4);
  }
  .event.hidden .at {
    color: inherit;
  }
  .event.hidden .name {
    text-decoration: line-through;
  }
  .off {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
  }
  .empty {
    margin: 0;
    padding: 4px 12px 8px;
    color: var(--ink-3);
    white-space: normal;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--primary);
    text-decoration: underline;
  }
</style>
