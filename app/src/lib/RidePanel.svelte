<script lang="ts">
  import Icon from "./Icon.svelte";
  import LabelEditor from "./LabelEditor.svelte";
  import LabelTag from "./LabelTag.svelte";
  import { type LabelFilter, matchesFilter, toggleFilter, usedLabels } from "./labels";
  import { barState, type RideSync, type SyncView } from "./sync";
  import { hms } from "./time";
  import type { RideSummary, RideVideos } from "./types";

  /**
   * 走行の切り替え。上の帯の日付と題を押すと開く。
   *
   * 走行を選ぶ・作る・消すことをここで行う。クラウドとの同期は自動なので、ここには走行ごとの
   * 同期の状態を出すだけである。両方で変わった走行だけは、どちらを残すかをここで選ぶ。
   */
  let {
    date,
    title,
    labels,
    filter = $bindable(),
    videos,
    stored,
    sync,
    onselect,
    oncreate,
    ontitle,
    onlabels,
    onremove,
    onresolve,
    onsettings,
    onclose,
  }: {
    /** 開いている走行の日付 */
    date: string | null;
    /** 開いている走行の題 */
    title: string;
    /** 開いている走行のラベル */
    labels: string[];
    /** ラベルでの絞り込み。一覧を閉じても覚えておくよう、開く側が持つ */
    filter: LabelFilter;
    videos: RideVideos[];
    stored: RideSummary[];
    /** クラウドとの同期の状態 */
    sync: SyncView;
    onselect: (date: string) => void | Promise<void>;
    oncreate: (date: string) => void | Promise<void>;
    ontitle: (title: string) => void;
    onlabels: (labels: string[]) => void;
    /** 走行の記録を消す。同期を設定していれば、クラウドからも消える */
    onremove: (id: string) => Promise<void>;
    /** 両方で変わった走行を、選んだほうで揃える */
    onresolve: (id: string, keep: "local" | "cloud") => Promise<void>;
    onsettings: () => void;
    onclose: () => void;
  } = $props();

  /** 一覧の 1 行。動画だけがある日付、手元の走行、クラウドにだけある走行のどれか。 */
  interface Row {
    key: string;
    date: string;
    /** 走行の ID。まだ記録の無い日付では null */
    id: string | null;
    title: string;
    /** 付いているラベル。記録の無い日付と、まだ受け取っていない走行では空 */
    labels: string[];
    videos: RideVideos | undefined;
    summary: RideSummary | undefined;
    /** クラウドにだけあって、まだ受け取れていない走行か */
    remote: boolean;
    /** 同期の状態。同期を設定していないか、まだ突き合わせていなければ undefined */
    state: RideSync | undefined;
  }

  const STATES: Record<RideSync, { name: string; tone: string }> = {
    synced: { name: "同期済み", tone: "ok" },
    waiting: { name: "同期中…", tone: "muted" },
    sending: { name: "同期中…", tone: "muted" },
    unsent: { name: "未同期", tone: "warn" },
    conflict: { name: "両方で変更", tone: "danger" },
  };

  let dialog: HTMLDialogElement;
  let newDate = $state("");
  /** 操作に失敗した理由 */
  let message = $state("");
  let busy = $state(false);
  /** 消すかどうかを聞いている行 */
  let asking = $state<string | null>(null);

  const rows = $derived.by(() => {
    const videosOf = (rideDate: string) => videos.find((ride) => ride.date === rideDate);
    const list: Row[] = stored.map((summary) => ({
      key: summary.id,
      date: summary.ride_date,
      id: summary.id,
      title: summary.title,
      // 開いている走行は、保存を待たずに、いまのラベルを出す。
      labels: summary.ride_date === date ? labels : summary.labels,
      videos: videosOf(summary.ride_date),
      summary,
      remote: false,
      state: sync.rides[summary.id],
    }));
    for (const row of sync.remote) {
      if (list.some((known) => known.id === row.id)) continue;
      list.push({
        key: row.id,
        date: row.ride_date,
        id: row.id,
        title: row.title,
        labels: [],
        videos: videosOf(row.ride_date),
        summary: undefined,
        remote: true,
        // 突き合わせの最中なら、これから受け取る。済んでも残っているのは、受け取れなかった走行である。
        state: sync.rides[row.id] ?? (sync.checking ? "sending" : "unsent"),
      });
    }
    for (const ride of videos) {
      if (list.some((known) => known.date === ride.date)) continue;
      const none = { id: null, labels: [], summary: undefined, remote: false, state: undefined };
      list.push({ key: ride.date, date: ride.date, title: ride.date, videos: ride, ...none });
    }
    return list.sort((a, b) => (a.date === b.date ? 0 : a.date < b.date ? 1 : -1));
  });
  /** どれかの走行に付いているラベル。多い順 */
  const known = $derived(usedLabels(rows.map((row) => row.labels)));
  /** 絞り込みのうち、まだどれかの走行に付いているラベルのもの。外したラベルで一覧が空のままにならない */
  const narrowing: LabelFilter = $derived(
    Object.fromEntries(Object.entries(filter).filter(([name]) => known.some((label) => label.name === name))),
  );
  const shown = $derived(rows.filter((row) => matchesFilter(row.labels, narrowing)));
  const narrowed = $derived(Object.keys(narrowing).length > 0);
  const FILTER_TIPS = { has: "付いている走行だけを出しています", not: "付いていない走行だけを出しています" };
  const FILTER_NAMES = { has: "（付いている走行だけ）", not: "（付いていない走行だけ）" };

  /** 同期の全体の状態。上の帯と同じ決め方で、保存のことは見ない */
  const overall = $derived(barState("saved", sync));
  const conflicts = $derived(Object.values(sync.rides).filter((state) => state === "conflict").length);

  $effect(() => {
    dialog.showModal();
    // 開いたときに、最初の欄やボタンを勝手に選ばない。
    dialog.focus();
  });

  /** 画面の操作を 1 つ行う。失敗は 1 行で出す。 */
  async function run(work: () => Promise<void>) {
    busy = true;
    message = "";
    try {
      await work();
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    }
    busy = false;
  }

  /** 両方で変わった走行を、手元かクラウドの、選んだほうで揃える。 */
  export const resolve = (id: string, keep: "local" | "cloud") => run(() => onresolve(id, keep));

  /**
   * 走行の記録を消す。同期を設定していれば、クラウドからも消える。
   *
   * クラウドから消せなかったときは、手元の記録も残る。理由を出して、聞いている行はそのままにする。
   */
  export const remove = (id: string) =>
    run(async () => {
      await onremove(id);
      asking = null;
    });

  /** いまの一覧。自動の確認が読む。 */
  export const snapshot = () => ({
    rows: rows.filter((row) => row.id !== null).map((row) => `${row.date} ${row.title}: ${row.state ?? "-"}`),
    message,
  });

  async function create() {
    if (newDate === "") return;
    await oncreate(newDate);
    newDate = "";
    dialog.close();
  }

  async function choose(row: Row) {
    await onselect(row.date);
    dialog.close();
  }

  const summaryOf = (row: Row) => {
    const video = row.videos
      ? `動画 ${row.videos.files.length} 本・${hms(row.videos.files.reduce((sum, file) => sum + file.duration_ms, 0))}`
      : "動画なし";
    const route = row.summary ? `経路 ${(row.summary.length_m / 1000).toFixed(1)} km` : "記録なし";
    return `${video}　${route}`;
  };
</script>

<!-- 板の外（背景）を押したら閉じる。背景を押すと、押された相手は dialog そのものになる。 -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog
  bind:this={dialog}
  class="card" tabindex="-1"
  aria-labelledby="rides-title"
  {onclose}
  onclick={(event) => event.target === dialog && dialog.close()}
>
  <div class="board">
    <header>
      <h2 id="rides-title">走行</h2>
      <form onsubmit={(event) => (event.preventDefault(), create())}>
        <input class="field mono" type="date" aria-label="新しい走行の日付" bind:value={newDate} />
        <button type="submit" class="btn" disabled={newDate === ""}>この日付で作る</button>
      </form>
    </header>

    <!-- 同期の全体の状態を 1 行で出す。同期は自動なので、押して送り受けするボタンは置かない。 -->
    <div class="cloud" class:failed={message !== ""}>
      <Icon name={message === "" ? "cloud" : "error"} />
      {#if message !== ""}
        <span class="text">{message}</span>
      {:else if !sync.configured}
        <span class="text">クラウド同期の接続先が、まだ設定されていません</span>
        <button type="button" class="btn sized" onclick={onsettings}>設定を開く</button>
      {:else if overall === "syncing"}
        <span class="text">クラウドと同期しています…</span>
      {:else if conflicts > 0}
        <span class="text">両方で変更された走行が {conflicts} 件あります。どちらを残すかを選んでください</span>
      {:else if overall === "unsynced"}
        <span class="text" title={sync.failure}>クラウドと同期できていません。少し置いて、もう一度試します</span>
      {:else}
        <span class="text">すべての走行ログが同期済み</span>
      {/if}
    </div>

    {#if known.length > 0}
      <!-- 押すたびに、付いている走行だけ、付いていない走行だけ、外す、と変わる。 -->
      <div class="filter" role="group" aria-label="ラベルで絞り込む">
        <span class="name">ラベル</span>
        {#each known as label (label.name)}
          {@const state = narrowing[label.name]}
          <button
            type="button"
            class="choice"
            class:has={state === "has"}
            class:not={state === "not"}
            aria-label={label.name + (state === undefined ? "" : FILTER_NAMES[state])}
            title={state === undefined ? "押すと、付いている走行だけを出します" : FILTER_TIPS[state]}
            onclick={() => (filter = toggleFilter(narrowing, label.name))}
          >
            {#if state !== undefined}<span class="mark" aria-hidden="true">{state === "has" ? "✓" : "−"}</span>{/if}
            <span class="text">{label.name}</span>
          </button>
        {/each}
        <span class="count mono">{rows.length} 件中 {shown.length} 件</span>
        {#if narrowed}
          <button type="button" class="clear" onclick={() => (filter = {})}>外す</button>
        {/if}
      </div>
    {/if}

    <div class="rows">
      {#if rows.length === 0}
        <p class="empty">
          走行がまだありません。上の欄に日付を入れて作るか、設定で動画フォルダを選んでください。
        </p>
      {:else if shown.length === 0}
        <p class="empty">当てはまる走行がありません。絞り込みを外すと、すべての走行が出ます。</p>
      {/if}
      {#each shown as row (row.key)}
        {@const open = row.date === date && !row.remote}
        {#if asking === row.key && row.id !== null}
          {@const id = row.id}
          {@const local = row.summary !== undefined}
          <div class="asking">
            <span class="head">
              <span class="mono date">{row.date}</span>
              {#if row.title !== row.date}<b>{row.title}</b>{:else}<span class="untitled">（題なし）</span>{/if}
            </span>
            <span>
              {#if local}
                <b>この走行の記録を消しますか。</b>経路とイベントが消えます。{#if sync.configured}クラウドからも消えます。{/if}動画のファイルは消えません。
              {:else}
                <b>クラウドにある、この走行の記録を消しますか。</b>手元には記録がありません。動画のファイルは消えません。
              {/if}
            </span>
            <div class="buttons">
              <button type="button" class="btn" onclick={() => (asking = null)}>やめる</button>
              <button type="button" class="btn danger" disabled={busy} onclick={() => remove(id)}>記録を消す</button>
            </div>
          </div>
        {:else}
          <div class="ride" class:open>
            {#if open}
              <div class="main">
                <span class="head">
                  <span class="mono date">{row.date}</span>
                  <input
                    class="field title"
                    aria-label="走行の題"
                    placeholder="題を付ける"
                    value={title === row.date ? "" : title}
                    onchange={(event) => ontitle(event.currentTarget.value.trim() || row.date)}
                  />
                  <span class="pill here">開いている</span>
                </span>
                {#if row.id !== null}
                  <LabelEditor labels={row.labels} {known} onchange={onlabels} />
                {/if}
                <span class="mono detail">{summaryOf(row)}</span>
              </div>
            {:else if row.remote}
              <div class="main">
                <span class="head">
                  <span class="mono date">{row.date}</span>
                  {#if row.title !== row.date}<b>{row.title}</b>{:else}<span class="untitled">（題なし）</span>{/if}
                </span>
                <span class="mono detail">{summaryOf(row)}</span>
              </div>
            {:else}
              <button type="button" class="main" onclick={() => choose(row)} title="この走行を開く">
                <span class="head">
                  <span class="mono date">{row.date}</span>
                  {#if row.title !== row.date}<b>{row.title}</b>{:else}<span class="untitled">（題なし）</span>{/if}
                  {#each row.labels as name (name)}<LabelTag {name} />{/each}
                </span>
                <span class="mono detail">{summaryOf(row)}</span>
              </button>
            {/if}
            <span class="sync">
              {#if row.state !== undefined && row.id !== null}
                {@const id = row.id}
                <span class="pill {STATES[row.state].tone}">
                  {#if row.state === "synced"}<Icon name="check" size={13} />{/if}{STATES[row.state].name}
                </span>
                {#if row.state === "conflict"}
                  <button type="button" class="btn small" onclick={() => resolve(id, "local")} disabled={busy}>
                    手元で上書き
                  </button>
                  <button type="button" class="btn small" onclick={() => resolve(id, "cloud")} disabled={busy}>
                    クラウドで上書き
                  </button>
                {/if}
              {/if}
            </span>
            {#if row.id !== null}
              <button
                type="button"
                class="btn small icon-only ghost trash"
                aria-label="{row.date} の記録を消す"
                title="記録を消す"
                onclick={() => (asking = row.key)}
              >
                <Icon name="trash" size={14} />
              </button>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  </div>
</dialog>

<style>
  /* 上の帯の、日付と題のすぐ下に出す。 */
  dialog {
    top: calc(var(--gutter) + var(--bar) + 6px);
    left: var(--gutter);
    width: min(600px, calc(100vw - 2 * var(--gutter)));
    max-height: calc(100vh - var(--bar) - 3 * var(--gutter));
    margin: 0;
    overflow: hidden;
  }
  dialog::backdrop {
    background: rgba(26, 29, 33, 0.12);
  }
  .board {
    display: flex;
    flex-direction: column;
    max-height: inherit;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 12px 16px;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
  }
  header form {
    display: flex;
    gap: 8px;
  }
  header input {
    width: 136px;
  }
  .cloud {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    padding: 8px 12px 8px 16px;
    border-top: 1px solid var(--line-soft);
    background: var(--surface-2);
    color: var(--ink);
  }
  .cloud :global(.icon) {
    color: var(--ink-3);
  }
  .cloud.failed {
    background: var(--danger-soft);
    color: var(--danger-ink);
  }
  .cloud.failed :global(.icon) {
    color: var(--danger);
  }
  .cloud .text {
    flex: 1;
  }
  .sized {
    height: 26px;
  }
  .filter {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 8px 12px 8px 16px;
    border-top: 1px solid var(--line-soft);
  }
  .filter .name {
    margin-right: 2px;
    color: var(--ink-3);
    font-size: 12px;
  }
  .filter .count {
    margin-left: auto;
    color: var(--ink-3);
    font-size: 12px;
    white-space: nowrap;
  }
  .clear {
    padding: 0;
    border: 0;
    background: none;
    color: var(--primary);
    font-size: 12px;
    text-decoration: underline;
  }
  .choice {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--line-strong);
    border-radius: 4px;
    background: var(--surface);
    font-size: 12px;
    white-space: nowrap;
  }
  .choice:hover:not(.has):not(.not) {
    background: var(--surface-2);
  }
  .choice .mark {
    width: 12px;
    font-weight: 700;
    text-align: center;
  }
  /* 付いている走行だけは青、付いていない走行だけは赤に塗る。赤には、名前に線も引く。 */
  .choice.has {
    border-color: var(--primary);
    background: var(--primary);
    color: #fff;
    font-weight: 700;
  }
  .choice.not {
    border-color: var(--danger);
    background: var(--danger);
    color: #fff;
    font-weight: 700;
  }
  .choice.not .text {
    text-decoration: line-through;
  }
  .rows {
    min-height: 0;
    overflow-y: auto;
  }
  .empty {
    margin: 0;
    padding: 16px;
    border-top: 1px solid var(--line-soft);
    color: var(--ink-3);
  }
  .ride {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 4px 12px;
    padding: 9px 12px 9px 16px;
    border-top: 1px solid var(--line-soft);
  }
  .ride.open {
    background: var(--primary-soft);
    box-shadow: inset 0 0 0 1px var(--primary);
  }
  .main {
    display: flex;
    grid-row: span 2;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
  }
  button.main:hover .date {
    text-decoration: underline;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
    min-width: 0;
  }
  .head b {
    overflow: hidden;
    font-size: 14px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .date {
    flex: none;
    font-weight: 700;
  }
  .untitled {
    color: var(--ink-3);
  }
  .title {
    flex: 1;
    min-width: 0;
    height: 26px;
    font-size: 14px;
    font-weight: 700;
  }
  .here {
    flex: none;
    height: 18px;
    background: var(--primary);
    color: #fff;
    font-size: 11px;
    font-weight: 700;
  }
  .detail {
    color: var(--ink-2);
    font-size: 12px;
    white-space: nowrap;
  }
  .sync {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
    min-height: 24px;
  }
  .trash {
    grid-column: 2;
    justify-self: end;
    color: var(--ink-3);
  }
  .trash:hover {
    color: var(--danger);
  }
  .asking {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px 12px 16px;
    border-top: 1px solid var(--line-soft);
    background: var(--danger-soft);
  }
  .buttons {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
