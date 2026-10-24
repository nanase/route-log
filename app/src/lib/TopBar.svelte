<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { BarState } from "./sync";
  import type { Mode } from "./ui";

  /**
   * 上の帯。開いている走行、いまの作業（経路／時刻）、保存と同期の状態と、停車を探す・書き出し・設定の入口を
   * 並べる。
   */
  let {
    date,
    title,
    videoSummary,
    mode,
    status,
    failure,
    compact,
    canExport,
    canOverview,
    canFindStops,
    stopProgress,
    onrides,
    onmode,
    onexport,
    onoverview,
    onfindstops,
    oncancelstops,
    onsettings,
    onretry,
  }: {
    /** 開いている走行の日付。開いていなければ null */
    date: string | null;
    title: string;
    /** 動画の本数と長さ。読み込み中はその旨 */
    videoSummary: string;
    mode: Mode;
    /** 保存と同期の状態。出すものが無ければ null */
    status: BarState | null;
    /** 同期できなかった理由。同期できていれば空 */
    failure: string;
    /** 幅が狭いとき。ボタンの字を省き、動画の要約を出さない */
    compact: boolean;
    canExport: boolean;
    /** 概略図の動画を作れるか。経由点が 2 つ以上あれば作れる */
    canOverview: boolean;
    /** 停車を探せるか。走行に使う動画があれば探せる */
    canFindStops: boolean;
    /** 停車を探している間の進み具合（0〜1）。探していなければ null */
    stopProgress: number | null;
    onrides: () => void;
    onmode: (mode: Mode) => void;
    onexport: () => void;
    onoverview: () => void;
    onfindstops: () => void;
    oncancelstops: () => void;
    onsettings: () => void;
    /** 保存をやり直す */
    onretry: () => void;
  } = $props();
</script>

<header class="float" class:compact>
  <button type="button" class="btn ghost ride" onclick={onrides} aria-haspopup="dialog" title="走行を切り替える">
    {#if date === null}
      <strong>走行を選ぶ</strong>
    {:else}
      <strong class="mono">{date}</strong>
      {#if title !== date}<strong class="title">{title}</strong>{/if}
      {#if !compact && videoSummary !== ""}<span class="videos">{videoSummary}</span>{/if}
    {/if}
    <Icon name="down" />
  </button>
  <div class="rule"></div>
  <div class="seg" role="group" aria-label="作業の切り替え">
    <button type="button" class:on={mode === "route"} aria-pressed={mode === "route"} onclick={() => onmode("route")}>
      <span class="mono">1</span>経路入力
    </button>
    <button
      type="button"
      class:on={mode === "confirm"}
      aria-pressed={mode === "confirm"}
      onclick={() => onmode("confirm")}
    >
      <span class="mono">2</span>時刻入力
    </button>
  </div>
  <div class="space"></div>

  <!-- 保存と同期の状態は、まとめて 1 つだけ出す。同期を設定していなければ、保存のことだけを出す。 -->
  <div class="save" role="status">
    {#if status === "saved"}
      <span class="state done"><Icon name="check" />保存済み</span>
    {:else if status === "saving"}
      <span class="state busy"><span class="spinner"></span>保存中…</span>
    {:else if status === "syncing"}
      <span class="state busy" title="クラウドと同期しています"><span class="spinner"></span>同期中…</span>
    {:else if status === "synced"}
      <span class="state done" title="手元とクラウドが同じです"><Icon name="check" />同期済み</span>
    {:else if status === "unsynced"}
      <span
        class="state behind"
        title="手元には保存してあります。クラウドと同期できなかったので、次の保存か、少し置いてからもう一度試します。{failure}"
      >
        <span class="dot"></span>未同期
      </span>
    {:else if status === "conflict"}
      <button
        type="button"
        class="btn small danger"
        onclick={onrides}
        aria-haspopup="dialog"
        title="手元とクラウドの両方で変更された走行があります。押して、どちらを残すかを選びます"
      >
        <Icon name="warning" size={13} />両方で変更
      </button>
    {:else if status === "save_error"}
      <span class="pill danger"><Icon name="error" size={13} />保存できません</span>
      <button type="button" class="btn small" onclick={onretry}>もう一度</button>
    {/if}
  </div>

  {#if stopProgress === null}
    <button
      type="button"
      class="btn"
      class:icon-only={compact}
      onclick={onfindstops}
      disabled={!canFindStops}
      aria-label="停車を探す"
      title="信号待ちを含む停車の候補を、動画の音と映像の動きから探す。探してある動画は飛ばす"
    >
      <Icon name="search" />{#if !compact}停車を探す{/if}
    </button>
  {:else}
    <!-- 探している間は、やめるボタンにして、進み具合を添える。 -->
    <button
      type="button"
      class="btn"
      onclick={oncancelstops}
      aria-label="停車を探すのをやめる"
      title="停車を探すのをやめる。探し終えた動画の候補は残る"
    >
      <Icon name="close" />{compact ? "" : "やめる "}<span class="mono">{Math.floor(stopProgress * 100)}%</span>
    </button>
  {/if}
  <button
    type="button"
    class="btn"
    class:icon-only={compact}
    onclick={onoverview}
    disabled={!canOverview}
    aria-haspopup="dialog"
    aria-label="概略図の動画"
    title="動画の冒頭に入れる、経路の概略図の動画を書き出す"
  >
    <Icon name="movie" />{#if !compact}概略図の動画{/if}
  </button>
  <button
    type="button"
    class="btn"
    class:icon-only={compact}
    onclick={onexport}
    disabled={!canExport}
    aria-label="CSV を書き出す"
    title="通過時刻の一覧を CSV に書き出す"
  >
    <Icon name="export" />{#if !compact}CSV を書き出す{/if}
  </button>
  <button type="button" class="btn icon-only" onclick={onsettings} aria-label="設定" title="設定">
    <Icon name="settings" />
  </button>
</header>

<style>
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    height: var(--bar);
    padding: 0 8px 0 6px;
    white-space: nowrap;
  }
  header.compact {
    gap: 10px;
  }
  .ride {
    gap: 8px;
    min-width: 0;
    height: 32px;
  }
  .title {
    overflow: hidden;
    font-size: 14px;
    text-overflow: ellipsis;
  }
  .videos {
    color: var(--ink-3);
  }
  .rule {
    flex: none;
    width: 1px;
    height: 22px;
    background: var(--line);
  }
  .seg {
    flex: none;
  }
  .space {
    flex: 1;
  }
  .save {
    display: flex;
    flex: none;
    align-items: center;
    gap: 8px;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .done {
    color: var(--ok);
  }
  .busy {
    color: var(--ink-2);
  }
  .behind {
    color: var(--warn-ink);
  }
  .spinner {
    box-sizing: border-box;
    width: 12px;
    height: 12px;
    border: 2px solid var(--line-strong);
    border-top-color: var(--ink-2);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--warn);
  }
  .btn {
    flex: none;
  }
  .btn.ride {
    flex: 0 1 auto;
  }
</style>
