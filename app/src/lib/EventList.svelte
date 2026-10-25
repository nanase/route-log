<script lang="ts">
  import EventName from "./EventName.svelte";
  import { asideOf, confirmTyped, inStop, retimeStop, setHidden, stopsOf, unconfirm, unpinStop } from "./events";
  import Icon from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";
  import { KIND_ORDER, KINDS } from "./kinds";
  import { nameChoices, setName, shownName } from "./names";
  import KindTag from "./KindTag.svelte";
  import StatusMark from "./StatusMark.svelte";
  import { clock, jstToMs, short } from "./time";
  import type { EventKind, Gap, RideEvent } from "./types";
  import { remembered } from "./ui";

  /**
   * イベントの一覧。行を押すと、そのイベントが「次の候補」になり、名前・時刻・字幕に出すか
   * どうかを直す欄が開く。状態・種類・名前で絞り込める。
   *
   * 行の左のチェックでいくつか選ぶと、字幕に出すかどうかをまとめて切り替えられる。Shift を
   * 押しながらチェックすると、前にチェックした行からそこまでを選ぶ。
   */
  let {
    events,
    targetId,
    selectedId,
    conflicts,
    gaps,
    rideDate,
    placingId,
    onselect,
    onchange,
    onhide,
    onremove,
    onplace,
    onextract,
    extracting,
  }: {
    events: RideEvent[];
    /** 次の候補 */
    targetId: string | null;
    /** 直す欄を開いているイベント */
    selectedId: string | null;
    /** 位置が先なのに時刻が後になっている、確定済みのイベントの ID */
    conflicts: string[];
    /** 録画を止めていた時間。推定時刻がここに入るイベントに印を付ける */
    gaps: Gap[];
    /** 手で入れた時刻に付ける日付。`YYYY-MM-DD` */
    rideDate: string;
    /** 地図で場所を決めている停車 */
    placingId: string | null;
    onselect: (event: RideEvent) => void;
    onchange: (id: string, change: (event: RideEvent) => RideEvent) => void;
    /** 選んだイベントを、まとめて字幕なし（`hidden` が真）か字幕ありにする */
    onhide: (ids: string[], hidden: boolean) => void;
    onremove: (id: string) => void;
    /** 停車の場所を、地図で決め始める。null なら、やめる */
    onplace: (id: string | null) => void;
    /** 抽出し直す。判定用データを開いていなければ null */
    onextract: (() => void) | null;
    /** 抽出し直している最中 */
    extracting: boolean;
  } = $props();

  /** 状態での絞り込み。未確定は、字幕にするもののうち、まだ確定していないもの */
  const FILTERS = ["all", "pending", "confirmed", "hidden"] as const;
  type Filter = (typeof FILTERS)[number];
  const FILTER_NAMES: Record<Filter, string> = {
    all: "すべて",
    pending: "未確定",
    confirmed: "確定",
    hidden: "字幕なし",
  };
  /** 人が消せる種類。経路から抽出したものは、消しても次の抽出で戻るので、字幕なしにして隠す */
  const REMOVABLE: EventKind[] = ["custom", "waypoint", "stop"];
  const FILTER_KEY = "routelog.event-filter";

  let rows: HTMLDivElement;
  let filter = $state<Filter>(remembered(FILTER_KEY, FILTERS, "all"));
  /** 出す種類。空なら、どの種類も出す */
  let kinds = $state<EventKind[]>([]);
  let query = $state("");
  /** チェックを入れているイベントの ID */
  let checked = $state.raw<ReadonlySet<string>>(new Set());
  /** 最後にチェックを切り替えた行。Shift を押しながらのチェックは、ここからの範囲を選ぶ */
  let anchorId: string | null = null;

  const matches: Record<Filter, (event: RideEvent) => boolean> = {
    all: () => true,
    pending: (event) => event.status === "estimated" && !event.hidden,
    confirmed: (event) => event.status === "confirmed",
    hidden: (event) => event.hidden === true,
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
        (word === "" || `${shownName(event)}${KINDS[event.kind]}`.toLowerCase().includes(word)),
    );
  });
  const narrowed = $derived(filter !== "all" || kinds.length > 0 || query.trim() !== "");
  /** 停車にしてある時間。推定時刻がここに入るのは、止まった場所そのものなので、警告しない */
  const stays = $derived(stopsOf(events));
  /** 停車の間に確定したのに、停車の場所と位置が合わないイベント */
  const aside = $derived(asideOf(events));
  /** チェックを入れていて、いまもあるイベント。抽出し直して消えたものは数えない */
  const picked = $derived(events.filter((event) => checked.has(event.id)).map((event) => event.id));
  /** 出している行に、チェックがいくつ入っているか */
  const pickedShown = $derived(shown.filter((event) => checked.has(event.id)).length);

  /** 行のチェックを切り替える。Shift を押していれば、前に切り替えた行からここまでを同じにする。 */
  function check(event: RideEvent, on: boolean, range: boolean) {
    const ids = shown.map((row) => row.id);
    const [from, to] = [ids.indexOf(anchorId ?? ""), ids.indexOf(event.id)];
    const span = range && from >= 0 ? ids.slice(Math.min(from, to), Math.max(from, to) + 1) : [event.id];
    const next = new Set(checked);
    for (const id of span) {
      if (on) next.add(id);
      else next.delete(id);
    }
    checked = next;
    anchorId = event.id;
  }

  /** 出している行のすべてに、チェックを入れるか外す。 */
  function checkShown(on: boolean) {
    const next = new Set(checked);
    for (const event of shown) {
      if (on) next.add(event.id);
      else next.delete(event.id);
    }
    checked = next;
    anchorId = null;
  }

  function uncheckAll() {
    checked = new Set();
    anchorId = null;
  }

  /** チェックを入れたイベントを、まとめて字幕なしか字幕ありにする。済んだら、チェックを外す。 */
  function hidePicked(hidden: boolean) {
    onhide(picked, hidden);
    uncheckAll();
  }

  /** チェックの欄を押したあとも、Space が動画の再生に届くようにする。 */
  const release = (input: HTMLInputElement) => input.blur();

  function warning(event: RideEvent): { short: string; long: string } | null {
    if (aside.includes(event.id)) {
      return {
        short: "停車の場所と合いません",
        long: "停車中に確定した時刻ですが停車位置と離れています。時刻推定には使いません",
      };
    }
    if (conflicts.includes(event.id)) {
      return { short: "時刻の順が合いません", long: "位置の順と時刻の順が合いません" };
    }
    const time = event.time_ms;
    const stopped = time !== null && gaps.some((gap) => gap.start_ms <= time && time < gap.end_ms);
    return stopped && event.status === "estimated" && !event.hidden && !inStop(stays, time ?? 0)
      ? { short: "録画停止の間", long: "推定時刻が録画停止中です" }
      : null;
  }

  /** 停車の始まりか終わりの時刻を、手で直す。読めない値と、始まりが後になる値は、受け付けない。 */
  function typeStop(event: RideEvent, edge: "start" | "end", value: string) {
    const time = jstToMs(rideDate, value);
    if (time !== null) onchange(event.id, (current) => retimeStop(current, edge, time));
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

  // 次の候補が動画に合わせて進んでも、その行が見えているようにする。並び直しで行が動いたときも追う。
  const targetIndex = $derived(shown.findIndex((event) => event.id === targetId));
  $effect(() => {
    if (targetId === null || targetIndex < 0) return;
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
        aria-label="再抽出"
        title="経路からイベントを再抽出"
        disabled={extracting}
        onclick={onextract}
      >
        {#if extracting}<Spinner size={14} />{:else}<Icon name="refresh" />{/if}
      </button>
    {/if}
  </header>
  <div class="narrow">
    <input
      type="checkbox"
      class="pick"
      aria-label="出している行をすべて選ぶ"
      title="表示中の行をすべて選択"
      disabled={shown.length === 0}
      checked={shown.length > 0 && pickedShown === shown.length}
      indeterminate={pickedShown > 0 && pickedShown < shown.length}
      onchange={(e) => {
        checkShown(e.currentTarget.checked);
        release(e.currentTarget);
      }}
    />
    <label class="search">
      <Icon name="search" size={14} />
      <input type="search" placeholder="名前で絞り込み" aria-label="名前で絞り込み" bind:value={query} />
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
  {#if picked.length > 0}
    <div class="bulk" role="group" aria-label="選んだイベントをまとめて切り替える">
      <b>{picked.length} 件を選択</b>
      <span class="space"></span>
      <button type="button" class="btn small" onclick={() => hidePicked(false)}>字幕あり</button>
      <button type="button" class="btn small" onclick={() => hidePicked(true)}>
        <StatusMark status="estimated" hidden />字幕なし
      </button>
      <button type="button" class="btn small ghost" onclick={uncheckAll}>選択解除</button>
    </div>
  {/if}

  <div class="rows" bind:this={rows}>
    {#each shown as event (event.id)}
      {@const warn = warning(event)}
      {@const target = event.id === targetId}
      {@const selected = event.id === selectedId}
      {@const hidden = event.hidden === true}
      <div class="item" class:selected class:target>
        <div class="line">
        <input
          type="checkbox"
          class="pick"
          aria-label="{event.label || KINDS[event.kind]}を選ぶ"
          checked={checked.has(event.id)}
          onclick={(e) => {
            check(event, e.currentTarget.checked, e.shiftKey);
            release(e.currentTarget);
          }}
        />
        <button
          type="button"
          class="row {event.status}"
          class:hidden
          data-id={event.id}
          aria-expanded={selected}
          onclick={() => onselect(event)}
        >
          <StatusMark status={event.status} {hidden} />
          <span class="mono time">{event.time_ms === null ? "--:--:--" : clock(event.time_ms)}</span>
          <span class="mono km">{(event.route_m / 1000).toFixed(1)}</span>
          <KindTag kind={event.kind} faded={hidden} />
          <span class="name"><EventName {event} /></span>
          <span class="tail">
            {#if warn !== null}
              <span class="warn" title={warn.long}><Icon name="warning" size={13} />{warn.short}</span>
            {/if}
            {#if event.kind === "stop"}
              {@const stay = event.detail.stay_ms ?? 0}
              <span class="stay mono">{stay > 0 ? short(stay) : "終わり未入力"}</span>
            {/if}
            {#if target && event.status !== "confirmed"}<span class="next">次</span>{/if}
          </span>
        </button>
        </div>
        {#if selected}
          <div class="edit">
            <label>
              名前
              <input
                class="field label"
                value={event.label}
                onchange={(e) => {
                  const label = e.currentTarget.value.trim();
                  onchange(event.id, (current) => setName(current, label));
                }}
              />
            </label>
            {#if nameChoices(event).length > 0}
              <!-- OSM とデータベースで名前が違う。選ばなければ、マーカーには両方を並べる。 -->
              <div class="choices" role="group" aria-label="名前の候補">
                {#each nameChoices(event) as choice (choice)}
                  <button
                    type="button"
                    class="btn small"
                    class:on={event.detail.named_by_hand === true && event.label === choice}
                    onclick={() => onchange(event.id, (current) => setName(current, choice))}
                  >
                    {choice}
                  </button>
                {/each}
              </div>
            {/if}
            {#if event.kind === "stop" && event.time_ms !== null}
              <!-- 停車は、始まりと終わりの時刻を持つ。その間は、進んでいないものとして推定する。 -->
              {#each [["start", "始まり", event.time_ms], ["end", "終わり", event.time_ms + (event.detail.stay_ms ?? 0)]] as const as [edge, name, time] (edge)}
                <label>
                  {name}
                  <input
                    class="field mono"
                    type="time"
                    step="1"
                    aria-label="停車の{name}の時刻"
                    value={clock(time)}
                    onchange={(e) => typeStop(event, edge, e.currentTarget.value)}
                  />
                </label>
              {/each}
              <div class="place">
                場所
                {#if placingId === event.id}
                  <span class="fine">地図で、経路の上を押します</span>
                  <button type="button" class="btn" onclick={() => onplace(null)}>やめる</button>
                {:else}
                  <span class="fine">{event.detail.at === undefined ? "前後の時刻から推定" : "地図で決めた所"}</span>
                  <button type="button" class="btn" onclick={() => onplace(event.id)}>
                    {event.detail.at === undefined ? "地図で指定" : "再指定"}
                  </button>
                  {#if event.detail.at !== undefined}
                    <button type="button" class="btn" onclick={() => onchange(event.id, unpinStop)}>推定に戻す</button>
                  {/if}
                {/if}
              </div>
            {:else}
            <label>
              時刻
              <input
                class="field mono"
                type="time"
                step="1"
                aria-label="通過時刻"
                title="入力した時刻で確定します"
                value={event.status === "confirmed" && event.time_ms !== null ? clock(event.time_ms) : ""}
                onchange={(e) => typeTime(event, e.currentTarget.value)}
              />
            </label>
            {/if}
            <div class="buttons">
            {#if event.status === "confirmed" && event.kind !== "stop"}
              <button type="button" class="btn" onclick={() => onchange(event.id, unconfirm)}>確定を解除</button>
            {/if}
            <!-- 字幕なしにしても、確定した時刻は残り、時刻推定の基準になる。 -->
            <div class="seg" role="group" aria-label="字幕に出すか">
              <button
                type="button"
                class:on={!hidden}
                aria-pressed={!hidden}
                onclick={() => onchange(event.id, (c) => setHidden(c, false))}
              >
                字幕あり
              </button>
              <button
                type="button"
                class:on={hidden}
                aria-pressed={hidden}
                title="字幕に出さない（確定した時刻は推定に使用）"
                onclick={() => onchange(event.id, (c) => setHidden(c, true))}
              >
                字幕なし
              </button>
            </div>
            {#if REMOVABLE.includes(event.kind)}
              <button type="button" class="btn remove" onclick={() => onremove(event.id)}>
                <Icon name="trash" size={14} />削除
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
            ? "判定データ未選択"
            : "イベントなし"}
        {:else}
          該当なし
          {#if narrowed}<button type="button" class="link" onclick={clear}>絞り込み解除</button>{/if}
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
    gap: 8px;
    padding: 0 12px 6px 10px;
  }
  /* 行を選ぶチェック */
  .pick {
    flex: none;
    width: 14px;
    height: 14px;
    margin: 0;
  }
  /* 選んだイベントをまとめて切り替える帯 */
  .bulk {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    height: 34px;
    padding: 0 8px 0 12px;
    border-top: 1px solid var(--primary-line);
    border-bottom: 1px solid var(--primary-line);
    background: var(--primary-softer);
    white-space: nowrap;
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
  .line {
    display: flex;
    align-items: center;
    padding-left: 10px;
  }
  .line:hover {
    background: var(--surface-2);
  }
  .item.target .line,
  .item.selected .line {
    background: var(--primary-soft);
  }
  .item.target .line {
    box-shadow: inset 0 0 0 1px var(--primary);
  }
  .row {
    display: grid;
    flex: 1;
    grid-template-columns: 11px 62px 34px 18px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: var(--row);
    padding: 0 12px 0 8px;
    border: 0;
    background: none;
    text-align: left;
    white-space: nowrap;
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
  .item.target .row.estimated .time {
    color: var(--ink);
  }
  .name {
    display: flex;
    min-width: 0;
    overflow: hidden;
  }
  .item.target .name,
  .item.selected .name {
    font-weight: 700;
  }
  /* 字幕なしのものは、行を灰色にして、名前に取り消し線を引く。確定した時刻は、濃いまま残す。 */
  .row.hidden,
  .row.hidden.estimated .time,
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
  /* 停車していた長さ */
  .stay {
    color: var(--ink-3);
    font-size: 12px;
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
  /* 名前の候補。選んだものは、操作の主色で囲む。 */
  .choices {
    display: flex;
    gap: 4px;
  }
  .choices .on {
    border-color: var(--primary);
    color: var(--primary);
  }
  .place {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-2);
    font-size: 12px;
  }
  .place .fine {
    color: var(--ink-3);
  }
  .buttons {
    display: flex;
    gap: 6px;
    margin-left: auto;
  }
  .buttons .seg button {
    padding: 0 10px;
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
