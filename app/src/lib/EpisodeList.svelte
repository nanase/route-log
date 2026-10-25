<script lang="ts">
  import { exportBlocker, extentOf, nameOf, RECORDED_LIMIT_MS, recordedOf } from "./episodes";
  import Icon from "./Icon.svelte";
  import { endpointsOf } from "./overview";
  import type { Path } from "./route";
  import type { Episode, RideEvent, Span } from "./types";

  /**
   * 回の一覧。経路入力のときに、経由点とイベントの一覧の間に置く。回ごとに名前と、始まりと
   * 終わりの経由点を選び、録画の長さを出す。
   */
  let {
    path,
    events,
    episodes,
    spans,
    onadd,
    onchange,
    onremove,
    onexport,
  }: {
    path: Path;
    events: RideEvent[];
    episodes: Episode[];
    /** タイムラインの範囲。動画が無ければ null */
    spans: Span[] | null;
    onadd: () => void;
    onchange: (id: string, change: Partial<Omit<Episode, "id">>) => void;
    onremove: (id: string) => void;
    /** Resolve へ書き出す。`name` は画面に出している回の名前 */
    onexport: (episode: Episode, name: string) => void;
  } = $props();

  const ends = $derived(endpointsOf({ ...path, events }));
  const canAdd = $derived(path.waypoints.length >= 2);
  const known = $derived(new Set(path.waypoints.map((waypoint) => waypoint.id)));
  const limitHours = RECORDED_LIMIT_MS / 3_600_000;

  /** 時:分。 */
  function clock(ms: number): string {
    const minutes = Math.floor(ms / 60_000);
    return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, "0")}`;
  }
</script>

{#snippet pick(episode: Episode, key: "from" | "to", label: string, bad: boolean)}
  <select
    class="field"
    class:bad
    value={episode[key]}
    aria-label={label}
    onchange={(e) => onchange(episode.id, { [key]: e.currentTarget.value })}
  >
    {#if !known.has(episode[key])}
      <option value={episode[key]} disabled>消えた経由地</option>
    {/if}
    {#each ends as point (point.index)}
      <option value={path.waypoints[point.index].id}>{point.index + 1}　{point.name}</option>
    {/each}
  </select>
{/snippet}

<header>
  <b>パート {episodes.length}</b>
  <span class="fine about">完成動画の範囲</span>
  <span class="space"></span>
  <button type="button" class="btn small" disabled={!canAdd} onclick={onadd}><Icon name="plus" size={14} />追加</button>
</header>
<div class="episodes">
  {#each episodes as episode, index (episode.id)}
    {@const extent = extentOf(path.waypoints, episode)}
    {@const recorded = spans === null ? null : recordedOf({ ...path, events }, spans, episode)}
    {@const blocker = exportBlocker(extent !== null, spans, recorded)}
    <div class="episode">
      <div class="top">
        <input
          class="field name"
          value={episode.title}
          placeholder={nameOf({ ...episode, title: "" }, index)}
          aria-label="パートの名前"
          onchange={(e) => onchange(episode.id, { title: e.currentTarget.value.trim() })}
        />
        {#if extent === null}
          <span class="length"></span>
        {:else if spans === null}
          <span class="length muted" title="この走行ログには動画が指定されていません">動画なし</span>
        {:else if recorded === null}
          <span class="length muted" title="始点と終点の経由地の通過時刻が推定されると録画時間が表示されます"
            >録画 不明</span
          >
        {:else if recorded > RECORDED_LIMIT_MS}
          <span class="length over" title="予想動画時間が {limitHours} 時間を超えます">
            <Icon name="warning" size={14} />録画 <b class="mono">{clock(recorded)}</b>
          </span>
        {:else}
          <span class="length" title="録画時間（録画停止と停車を除く）">
            録画 <b class="mono">{clock(recorded)}</b>
          </span>
        {/if}
        <button
          type="button"
          class="btn small icon-only ghost export"
          aria-label="{nameOf(episode, index)}を Resolve へ書き出す"
          title={blocker ?? "停車を除いたソースタイムライン（FCPXML）と字幕位置のマーカー（EDL）をエクスポートします"}
          disabled={blocker !== null}
          onclick={() => onexport(episode, nameOf(episode, index))}
        >
          <Icon name="export" size={14} />
        </button>
        <button
          type="button"
          class="btn small icon-only ghost trash"
          aria-label="{nameOf(episode, index)}を消す"
          title="パートを削除"
          onclick={() => onremove(episode.id)}
        >
          <Icon name="trash" size={14} />
        </button>
      </div>
      <div class="range">
        {@render pick(episode, "from", "始まりの経由地", extent === null)}
        <span class="arrow" aria-hidden="true">→</span>
        {@render pick(episode, "to", "終わりの経由地", extent === null)}
      </div>
      {#if extent === null}
        <p class="problem"><Icon name="warning" size={14} />終わりは、始まりより後の経由地にします</p>
      {/if}
    </div>
  {:else}
    <p class="empty">
      {canAdd
        ? "「追加」で、完成動画の範囲を、始まりと終わりの経由地で決めます。"
        : "経由地を 2 つ追加すると、パートを作れます。"}
    </p>
  {/each}
</div>

<style>
  header {
    display: flex;
    flex: none;
    align-items: center;
    gap: 8px;
    height: 40px;
    padding: 0 8px 0 12px;
    border-top: 1px solid var(--line);
    white-space: nowrap;
  }
  .about {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .space {
    flex: 1;
  }
  /*
   * 経由点とイベントが多くても縮めない。縮めてよいことにすると、中身の多い一覧と同じ割合で
   * 縮み、1 行ほどになって押せなくなる。回が 3 つを超えたら、この中で送る。
   */
  .episodes {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 6px;
    max-height: 240px;
    padding: 0 8px 8px;
    overflow-y: auto;
  }
  .episode {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 4px;
    padding: 6px;
    border: 1px solid var(--line-soft);
    border-radius: var(--radius-control);
    background: var(--surface-3);
  }
  .top,
  .range {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .name {
    flex: 1;
    min-width: 0;
    height: 26px;
  }
  .length {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 3px;
    color: var(--ink-3);
    font-size: 12px;
  }
  .length b {
    color: var(--ink);
    font-size: 13px;
  }
  .length.muted {
    color: var(--ink-4);
  }
  .length.over,
  .length.over b {
    color: var(--warn-ink);
  }
  .export,
  .trash {
    flex: none;
    color: var(--ink-3);
  }
  .trash:hover {
    color: var(--danger);
  }
  .range select {
    flex: 1;
    width: 0;
    min-width: 0;
    height: 26px;
    padding: 0 4px;
    font-size: 12px;
  }
  .range select.bad {
    border-color: var(--danger);
  }
  .arrow {
    flex: none;
    color: var(--ink-3);
  }
  .problem {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0;
    color: var(--danger);
    font-size: 12px;
  }
  .empty {
    margin: 0;
    padding: 0 4px;
    color: var(--ink-3);
    white-space: normal;
  }
</style>
