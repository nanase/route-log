<script lang="ts">
  import EventName from "./EventName.svelte";
  import { KINDS } from "./kinds";
  import KindTag from "./KindTag.svelte";
  import { clock, short } from "./time";
  import type { RideEvent } from "./types";

  /**
   * 次に確定するイベントの案内と、確定の操作。動画のすぐ下に置く。
   */
  let {
    target,
    count,
    nowMs,
    hasVideo,
    canUndo,
    onconfirm,
    onmove,
    onundo,
    onadd,
  }: {
    /** 次に確定する相手。無ければ null */
    target: RideEvent | null;
    /** イベントの数 */
    count: number;
    /** 動画の再生位置の実時刻 */
    nowMs: number;
    /** 動画があるか。無ければ、動画の位置では確定できない */
    hasVideo: boolean;
    /** 取り消せる確定があるか */
    canUndo: boolean;
    onconfirm: () => void;
    onmove: (step: 1 | -1) => void;
    onundo: () => void;
    onadd: () => void;
  } = $props();

  const confirmed = $derived(target?.status === "confirmed");
  // 出発と到着は、名前を直していなければ種類と同じ語になる。重ねて出さない。
  const showKind = $derived(target !== null && target.kind !== "road" && target.label !== KINDS[target.kind]);
  const when = $derived.by(() => {
    if (target === null || target.time_ms === null) return "";
    const rest = target.time_ms - nowMs;
    return rest >= 0 ? `あと ${short(rest)}` : `${short(-rest)} 前`;
  });
</script>

<section class:done={confirmed}>
  <div class="what">
    {#if target === null}
      <span class="lead">次に確定する</span>
      {#if count === 0}
        <span class="name none">イベントがまだありません</span>
        <span class="time">「経路を入れる」で経路を引くと、ここに出ます</span>
      {:else}
        <span class="name none">確定を待つイベントはありません</span>
        <span class="time">一覧で選ぶと、確定し直せます</span>
      {/if}
    {:else}
      <span class="lead">{confirmed ? "選んでいる（確定済み）" : "次に確定する"}</span>
      <span class="name">
        <KindTag kind={target.kind} />
        {#if showKind}<span>{KINDS[target.kind]}</span>{/if}
        <EventName event={target} />
      </span>
      <span class="mono time">
        {#if target.time_ms === null}
          推定なし。前後を確定すると出ます
        {:else if confirmed}
          確定 {clock(target.time_ms)}
        {:else}
          推定 {clock(target.time_ms)}　<b>{when}</b>
        {/if}
      </span>
    {/if}
  </div>
  <div class="actions">
    <button type="button" class="btn primary confirm" onclick={onconfirm} disabled={target === null || !hasVideo}>
      {confirmed ? "いまの位置で確定し直す" : "いまの位置で確定"}<kbd>Enter</kbd>
    </button>
    <div class="more">
      <button type="button" class="btn" onclick={() => onmove(-1)} title="前のイベントへ">前<kbd>P</kbd></button>
      <button type="button" class="btn" onclick={() => onmove(1)} title="次のイベントへ">次<kbd>N</kbd></button>
      <button
        type="button"
        class="btn"
        onclick={onundo}
        disabled={!canUndo}
        aria-label="直前の確定を取り消す"
        title="直前の確定を取り消す"
      >
        取り消し<kbd>BS</kbd>
      </button>
      <button
        type="button"
        class="btn"
        onclick={onadd}
        disabled={!hasVideo}
        aria-label="いまの位置に手動のイベントを足す"
        title="いまの位置に手動のイベントを足す"
      >
        追加<kbd>A</kbd>
      </button>
    </div>
  </div>
</section>

<style>
  section {
    display: flex;
    flex: none;
    align-items: center;
    gap: 12px;
    box-sizing: border-box;
    height: 68px;
    padding: 0 12px;
    border-bottom: 1px solid var(--primary-line);
    background: var(--primary-soft);
  }
  section.done {
    border-bottom-color: var(--line);
    background: var(--surface-2);
  }
  .what {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .lead {
    color: var(--primary);
    font-size: 11px;
    font-weight: 700;
  }
  .done .lead {
    color: var(--ink-2);
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    font-size: 15px;
    font-weight: 700;
    white-space: nowrap;
  }
  .name > span {
    flex: none;
  }
  .name.none {
    color: var(--ink-2);
    font-weight: 400;
  }
  .time {
    color: var(--ink-2);
    font-size: 12px;
    white-space: nowrap;
  }
  .time b {
    color: var(--ink);
  }
  .actions {
    display: flex;
    flex: none;
    flex-direction: column;
    align-items: flex-end;
    gap: 5px;
  }
  .confirm {
    height: 30px;
    padding: 0 12px;
  }
  .more {
    display: flex;
    gap: 4px;
    font-size: 12px;
  }
  .more .btn {
    height: 22px;
    padding: 0 6px;
  }
</style>
