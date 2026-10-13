<script lang="ts">
  import { confirmTyped, unconfirm } from "./events";
  import { KINDS } from "./kinds";
  import { clock, jstToMs } from "./time";
  import type { Gap, RideEvent } from "./types";

  /**
   * イベントの一覧。行を押すと、そのイベントが「次に確定する相手」になり、名前・時刻・
   * 字幕に出すかどうかを直す欄が開く。
   */
  let {
    events,
    targetId,
    conflicts,
    gaps,
    rideDate,
    onselect,
    onchange,
    onremove,
    onextract,
  }: {
    events: RideEvent[];
    /** 次に確定する相手 */
    targetId: string | null;
    /** 位置が先なのに時刻が後になっている、確定済みのイベントの ID */
    conflicts: string[];
    /** 録画を止めていた時間。推定時刻がここに入るイベントに印を付ける */
    gaps: Gap[];
    /** 手で入れた時刻に付ける日付。`YYYY-MM-DD` */
    rideDate: string;
    onselect: (event: RideEvent) => void;
    onchange: (id: string, change: (event: RideEvent) => RideEvent) => void;
    onremove: (id: string) => void;
    /** 抽出し直す。判定用データを開いていなければ null */
    onextract: (() => void) | null;
  } = $props();

  let showHidden = $state(false);
  const hidden = $derived(events.filter((event) => event.status === "hidden").length);
  const shown = $derived(showHidden ? events : events.filter((event) => event.status !== "hidden"));

  function warning(event: RideEvent): string {
    if (conflicts.includes(event.id)) return "位置の順と時刻の順が合いません";
    const time = event.time_ms;
    const stopped = time !== null && gaps.some((gap) => gap.start_ms <= time && time < gap.end_ms);
    return stopped && event.status === "estimated" ? "推定時刻が録画停止の間にあります" : "";
  }

  function typeTime(event: RideEvent, value: string) {
    const time = jstToMs(rideDate, value);
    onchange(event.id, (current) => (time === null ? unconfirm(current) : confirmTyped(current, time)));
  }
</script>

<section>
  <header>
    <strong>イベント {events.length - hidden} 件</strong>
    <label><input type="checkbox" bind:checked={showHidden} />字幕にしない {hidden} 件も出す</label>
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
          {@const warn = warning(event)}
          <tr class={event.status} class:target={event.id === targetId}>
            <td class="time" title={warn}>
              {event.time_ms === null ? "—" : clock(event.time_ms)}{warn ? " ⚠" : ""}
            </td>
            <td class="km">{(event.route_m / 1000).toFixed(1)} km</td>
            <td>{KINDS[event.kind]}</td>
            <td><button type="button" onclick={() => onselect(event)}>{event.label || "（名前なし）"}</button></td>
          </tr>
          {#if event.id === targetId}
            <tr class="edit">
              <td colspan="4">
                <div>
                <input
                  class="label"
                  aria-label="名前"
                  value={event.label}
                  onchange={(e) => {
                    const label = e.currentTarget.value.trim();
                    onchange(event.id, (current) => ({ ...current, label }));
                  }}
                />
                <input
                  type="time"
                  step="1"
                  aria-label="通過時刻"
                  value={event.status === "confirmed" && event.time_ms !== null ? clock(event.time_ms) : ""}
                  onchange={(e) => typeTime(event, e.currentTarget.value)}
                />
                {#if event.status === "confirmed"}
                  <button type="button" onclick={() => onchange(event.id, unconfirm)}>確定を外す</button>
                {/if}
                {#if event.status === "hidden"}
                  <button type="button" onclick={() => onchange(event.id, (c) => ({ ...c, status: "estimated" }))}>
                    字幕に出す
                  </button>
                {:else}
                  <button
                    type="button"
                    onclick={() => onchange(event.id, (c) => ({ ...unconfirm(c), status: "hidden" }))}
                  >
                    字幕にしない
                  </button>
                {/if}
                {#if event.kind === "custom"}
                  <button type="button" onclick={() => onremove(event.id)}>消す</button>
                {/if}
                </div>
              </td>
            </tr>
          {/if}
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
    width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  td {
    padding: 0.1rem 0.75rem 0.1rem 0;
    white-space: nowrap;
  }
  td button {
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }
  tr:hover,
  tr.target {
    background: #e6eef7;
  }
  tr.confirmed .time {
    font-weight: bold;
  }
  tr.estimated .time {
    color: #777;
  }
  tr.hidden {
    color: #999;
  }
  .km {
    text-align: right;
  }
  /* 名前の列に残りの幅を渡し、時刻・距離・種類の列を詰める。 */
  td:last-child {
    width: 100%;
  }
  /* 直す欄は表の幅を広げず、収まらなければ折り返す。 */
  tr.edit td {
    max-width: 0;
    padding: 0.2rem 0 0.4rem 1rem;
    background: #f3f6fa;
  }
  tr.edit div {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }
  tr.edit .label {
    width: 12rem;
  }
  tr.edit button {
    padding: 0.05rem 0.4rem;
    border: 1px solid #999;
    border-radius: 3px;
    background: #fff;
  }
</style>
