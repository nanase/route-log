<script lang="ts">
  import EventName from "./EventName.svelte";
  import { confirmTyped, unconfirm } from "./events";
  import Icon from "./Icon.svelte";
  import { KIND_ORDER, KINDS } from "./kinds";
  import KindTag from "./KindTag.svelte";
  import StatusMark from "./StatusMark.svelte";
  import { clock, jstToMs } from "./time";
  import type { EventKind, Gap, RideEvent } from "./types";
  import { remembered } from "./ui";

  /**
   * イベントの一覧。行を押すと、そのイベントが「次に確定する相手」になり、名前・時刻・
   * 字幕に出すかどうかを直す欄が開く。状態・種類・名前で絞り込める。
   */
  let {
    events,
    targetId,
    selectedId,
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
    /** 直す欄を開いているイベント */
    selectedId: string | null;
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

  /** 状態での絞り込み。未確定は、字幕にするもののうち、まだ確定していないもの */
  const FILTERS = ["all", "pending", "confirmed", "hidden"] as const;
  type Filter = (typeof FILTERS)[number];
  const FILTER_NAMES: Record<Filter, string> = {
    all: "すべて",
    pending: "未確定",
    confirmed: "確定",
    hidden: "字幕にしない",
  };
  const FILTER_KEY = "routelog.event-filter";

  let rows: HTMLDivElement;
  let filter = $state<Filter>(remembered(FILTER_KEY, FILTERS, "all"));
  /** 出す種類。空なら、どの種類も出す */
  let kinds = $state<EventKind[]>([]);
  let query = $state("");

  const matches: Record<Filter, (event: RideEvent) => boolean> = {
    all: () => true,
    pending: (event) => event.status === "estimated",
    confirmed: (event) => event.status === "confirmed",
    hidden: (event) => event.status === "hidden",
  };
  const counts = $derived(
    Object.fromEntries(FILTERS.map((name) => [name, events.filter(matches[name]).length])) as Record<Filter, number>,
  );
  /** いまの一覧にある種類。絞り込みのボタンに並べる */
  const present = $derived(KIND_ORDER.filter((kind) => events.some((event) => event.kind === kind)));
  const shown = $derived.by(() => {
    const word = query.trim().toLowerCase();
    return events.filter(
      (event) =>
        matches[filter](event) &&
        (kinds.length === 0 || kinds.includes(event.kind)) &&
        (word === "" || `${event.label}${KINDS[event.kind]}`.toLowerCase().includes(word)),
    );
  });
  const narrowed = $derived(filter !== "all" || kinds.length > 0 || query.trim() !== "");

  function warning(event: RideEvent): { short: string; long: string } | null {
    if (conflicts.includes(event.id)) {
      return { short: "時刻の順が合いません", long: "位置の順と時刻の順が合いません" };
    }
    const time = event.time_ms;
    const stopped = time !== null && gaps.some((gap) => gap.start_ms <= time && time < gap.end_ms);
    return stopped && event.status === "estimated"
      ? { short: "録画停止の間", long: "推定時刻が録画停止の間にあります" }
      : null;
  }

  function typeTime(event: RideEvent, value: string) {
    const time = jstToMs(rideDate, value);
    onchange(event.id, (current) => (time === null ? unconfirm(current) : confirmTyped(current, time)));
  }

  function setFilter(next: Filter) {
    filter = next;
    localStorage.setItem(FILTER_KEY, next);
  }

  function toggleKind(kind: EventKind) {
    kinds = kinds.includes(kind) ? kinds.filter((other) => other !== kind) : [...kinds, kind];
  }

  function clear() {
    setFilter("all");
    kinds = [];
    query = "";
  }

  // 次の相手が動画に合わせて進んでも、その行が見えているようにする。
  $effect(() => {
    if (targetId === null) return;
    rows.querySelector(`[data-id="${CSS.escape(targetId)}"]`)?.scrollIntoView({ block: "nearest" });
  });
</script>

<section>
  <header>
    <b class="count">イベント {events.length} 件</b>
    {#each FILTERS as name (name)}
      <button
        type="button"
        class="chip"
        class:on={filter === name}
        aria-pressed={filter === name}
        onclick={() => setFilter(name)}
      >
        {FILTER_NAMES[name]}{#if name !== "all"}&nbsp;{counts[name]}{/if}
      </button>
    {/each}
    <span class="space"></span>
    {#if onextract !== null}
      <button
        type="button"
        class="btn small icon-only"
        aria-label="抽出し直す"
        title="経路からイベントを抽出し直す"
        onclick={onextract}
      >
        <Icon name="refresh" />
      </button>
    {/if}
  </header>
  <div class="narrow">
    <label class="search">
      <Icon name="search" size={14} />
      <input type="search" placeholder="名前で探す" aria-label="名前で探す" bind:value={query} />
    </label>
    <div class="kinds" role="group" aria-label="種類で絞り込む">
      {#each present as kind (kind)}
        <button
          type="button"
          class="kind"
          class:on={kinds.includes(kind)}
          class:off={kinds.length > 0 && !kinds.includes(kind)}
          aria-pressed={kinds.includes(kind)}
          aria-label="{KINDS[kind]}だけを出す"
          onclick={() => toggleKind(kind)}
        >
          <KindTag {kind} />
        </button>
      {/each}
    </div>
  </div>

  <div class="rows" bind:this={rows}>
    {#each shown as event (event.id)}
      {@const warn = warning(event)}
      {@const target = event.id === targetId}
      {@const selected = event.id === selectedId}
      <div class="item" class:selected>
        <button
          type="button"
          class="row {event.status}"
          class:target
          data-id={event.id}
          aria-expanded={selected}
          onclick={() => onselect(event)}
        >
          <StatusMark status={event.status} />
          <span class="mono time">{event.time_ms === null ? "--:--:--" : clock(event.time_ms)}</span>
          <span class="mono km">{(event.route_m / 1000).toFixed(1)}</span>
          <KindTag kind={event.kind} faded={event.status === "hidden"} />
          <span class="name"><EventName {event} /></span>
          <span class="tail">
            {#if warn !== null}
              <span class="warn" title={warn.long}><Icon name="warning" size={13} />{warn.short}</span>
            {/if}
            {#if target && event.status !== "confirmed"}<span class="next">次</span>{/if}
          </span>
        </button>
        {#if selected}
          <div class="edit">
            <label>
              名前
              <input
                class="field label"
                value={event.label}
                onchange={(e) => {
                  const label = e.currentTarget.value.trim();
                  onchange(event.id, (current) => ({ ...current, label }));
                }}
              />
            </label>
            <label>
              時刻
              <input
                class="field mono"
                type="time"
                step="1"
                aria-label="通過時刻"
                title="時刻を手で入れると、その時刻で確定します"
                value={event.status === "confirmed" && event.time_ms !== null ? clock(event.time_ms) : ""}
                onchange={(e) => typeTime(event, e.currentTarget.value)}
              />
            </label>
            <div class="buttons">
            {#if event.status === "confirmed"}
              <button type="button" class="btn" onclick={() => onchange(event.id, unconfirm)}>確定を外す</button>
            {/if}
            {#if event.status === "hidden"}
              <button
                type="button"
                class="btn"
                onclick={() => onchange(event.id, (c) => ({ ...c, status: "estimated" }))}
              >
                字幕に出す
              </button>
            {:else}
              <button
                type="button"
                class="btn"
                onclick={() => onchange(event.id, (c) => ({ ...unconfirm(c), status: "hidden" }))}
              >
                <StatusMark status="hidden" />字幕にしない
              </button>
            {/if}
            {#if event.kind === "custom"}
              <button type="button" class="btn remove" onclick={() => onremove(event.id)}>
                <Icon name="trash" size={14} />消す
              </button>
            {/if}
            </div>
          </div>
        {/if}
      </div>
    {:else}
      <p class="empty">
        {#if events.length === 0}
          {onextract === null
            ? "判定データを選ぶと、経路からイベントを抽出します。"
            : "経路を入れると、ここにイベントが出ます。"}
        {:else}
          当てはまるイベントはありません。
          {#if narrowed}<button type="button" class="link" onclick={clear}>絞り込みをやめる</button>{/if}
        {/if}
      </p>
    {/each}
  </div>
</section>

<style>
  section {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }
  header {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    height: 32px;
    padding: 0 8px 0 12px;
    white-space: nowrap;
  }
  .count {
    margin-right: 4px;
  }
  .space {
    flex: 1;
  }
  /* 状態で絞り込むボタン */
  .chip {
    height: 22px;
    padding: 0 8px;
    border: 1px solid var(--line-strong);
    border-radius: 11px;
    background: var(--surface);
    font-size: 12px;
    white-space: nowrap;
  }
  .chip:hover:not(.on) {
    background: var(--surface-2);
  }
  .chip.on {
    border-color: var(--ink);
    background: var(--ink);
    color: #fff;
    font-weight: 700;
  }
  .narrow {
    display: flex;
    flex: none;
    align-items: center;
    gap: 10px;
    padding: 0 12px 6px;
  }
  .search {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    min-width: 0;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-control);
    color: var(--ink-3);
  }
  .search:focus-within {
    outline: 2px solid var(--primary);
    outline-offset: 1px;
  }
  .search input {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    outline: 0;
    background: none;
    color: var(--ink);
    font-size: 12px;
  }
  .kinds {
    display: flex;
    flex: none;
    gap: 3px;
  }
  .kind {
    display: grid;
    place-items: center;
    padding: 2px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: none;
  }
  .kind:hover {
    background: var(--surface-2);
  }
  .kind.on {
    border-color: var(--primary);
    background: var(--primary-soft);
  }
  .kind.off {
    opacity: 0.4;
  }

  .rows {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .item.selected {
    box-shadow: inset 0 0 0 1px var(--primary);
  }
  .row {
    display: grid;
    grid-template-columns: 11px 62px 34px 18px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: var(--row);
    padding: 0 12px;
    border: 0;
    background: none;
    text-align: left;
    white-space: nowrap;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.target,
  .item.selected .row {
    background: var(--primary-soft);
  }
  .row.target {
    box-shadow: inset 0 0 0 1px var(--primary);
  }
  .time,
  .km {
    color: var(--ink-3);
  }
  .km {
    text-align: right;
  }
  .row.confirmed .time {
    color: var(--ink);
    font-weight: 700;
  }
  .row.target.estimated .time {
    color: var(--ink);
  }
  .name {
    display: flex;
    min-width: 0;
    overflow: hidden;
  }
  .row.target .name,
  .item.selected .name {
    font-weight: 700;
  }
  /* 字幕にしないものは、行を灰色にして、名前に取り消し線を引く。 */
  .row.hidden,
  .row.hidden .time,
  .row.hidden .km {
    color: var(--ink-4);
  }
  .row.hidden .name {
    text-decoration: line-through;
  }
  .tail {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .next {
    padding: 1px 6px;
    border-radius: 9px;
    background: var(--primary);
    color: #fff;
    font-size: 11px;
    font-weight: 700;
  }
  .warn {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--warn-soft);
    color: var(--warn-ink);
    font-size: 11px;
    font-weight: 700;
  }

  /* 直す欄。幅が足りなければ折り返す。 */
  .edit {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
    padding: 8px 12px;
    border-top: 1px solid var(--primary-line);
    background: var(--primary-softer);
    white-space: nowrap;
  }
  .edit label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-2);
    font-size: 12px;
  }
  .edit input {
    color: var(--ink);
    font-size: 13px;
  }
  .edit .label {
    width: 132px;
  }
  .buttons {
    display: flex;
    gap: 6px;
    margin-left: auto;
  }
  .remove {
    color: var(--danger);
  }
  .empty {
    margin: 0;
    padding: 12px;
    color: var(--ink-3);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--primary);
    text-decoration: underline;
  }
</style>
