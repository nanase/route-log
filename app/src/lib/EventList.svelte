<script lang="ts">
  import type { EventKind, RideEvent } from "./types";

  /** 経路から抽出したイベントの一覧。行を押すと、地図がその位置へ動く。 */
  let {
    events,
    onselect,
    onextract,
  }: {
    events: RideEvent[];
    onselect: (event: RideEvent) => void;
    /** 抽出し直す。判定用データを開いていなければ null */
    onextract: (() => void) | null;
  } = $props();

  const KINDS: Record<EventKind, string> = {
    departure: "出発",
    arrival: "到着",
    pref_boundary: "県境",
    muni_boundary: "市区町村境",
    road: "路線",
    bridge: "橋",
    tunnel: "トンネル",
    michi_no_eki: "道の駅",
    custom: "手動",
  };

  let showHidden = $state(false);
  const hidden = $derived(events.filter((event) => event.status === "hidden").length);
  const shown = $derived(showHidden ? events : events.filter((event) => event.status !== "hidden"));
</script>

<section>
  <header>
    <strong>イベント {events.length - hidden} 件</strong>
    <label><input type="checkbox" bind:checked={showHidden} />非表示の {hidden} 件も出す</label>
    {#if onextract !== null}
      <button type="button" onclick={onextract}>抽出し直す</button>
    {:else}
      <span class="hint">判定データを開くと抽出します</span>
    {/if}
  </header>
  <div class="rows">
    <table>
      <tbody>
        {#each shown as event (event.id)}
          <tr class:hidden={event.status === "hidden"}>
            <td class="km">{(event.route_m / 1000).toFixed(1)} km</td>
            <td>{KINDS[event.kind]}</td>
            <td><button type="button" onclick={() => onselect(event)}>{event.label || "（名前なし）"}</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>

<style>
  section {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    height: 100%;
    min-height: 0;
    font-size: 0.85rem;
  }
  header {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    padding: 0.25rem 0;
    white-space: nowrap;
  }
  .hint {
    color: #777;
  }
  .rows {
    overflow: auto;
  }
  table {
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  td button {
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }
  tr:hover {
    background: #e6eef7;
  }
  tr.hidden {
    color: #999;
  }
  td {
    padding: 0.1rem 0.75rem 0.1rem 0;
    white-space: nowrap;
  }
  .km {
    text-align: right;
  }
</style>
