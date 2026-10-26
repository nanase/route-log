<script lang="ts">
  import Icon from "./Icon.svelte";
  import LabelEditor from "./LabelEditor.svelte";
  import LabelTag from "./LabelTag.svelte";
  import { type LabelFilter, matchesFilter, toggleFilter, usedLabels } from "./labels";
  import { barState, type RideSync, type SyncView } from "./sync";
  import { hms } from "./time";
  import type { RideSummary, VideoFolder, VideoSummary } from "./types";

  /**
   * 走行の切り替え。上の帯の日付と題を押すと開く。
   *
   * 走行を選ぶ・作る・消すことと、開いている走行の動画フォルダを選ぶことをここで行う。クラウドとの
   * 同期は自動なので、ここには走行ごとの同期の状態を出すだけである。両方で変わった走行だけは、
   * どちらを残すかをここで選ぶ。
   */
  let {
    date,
    title,
    labels,
    filter = $bindable(),
    summaries,
    folders,
    videoBusy,
    stored,
    sync,
    onselect,
    oncreate,
    ontitle,
    onlabels,
    onremove,
    onresolve,
    onaddfolder,
    onreloadfolders,
    onremovefolder,
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
    /** 日付ごとの動画の要約 */
    summaries: Record<string, VideoSummary>;
    /** 開いている走行の動画フォルダ */
    folders: VideoFolder[];
    /** 動画フォルダを読んでいる間。フォルダを足す・読み直す・外すボタンを止める */
    videoBusy: boolean;
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
    /** 開いている走行に動画フォルダを足す。足すフォルダは、呼ばれた側が選ばせる */
    onaddfolder: () => void;
    /** 開いている走行の動画フォルダを読み直す */
    onreloadfolders: () => void;
    /** 開いている走行から動画フォルダを外す */
    onremovefolder: (dir: string) => void;
    onsettings: () => void;
    onclose: () => void;
  } = $props();

  /** 一覧の 1 行。手元の走行か、クラウドにだけある走行のどちらか。 */
  interface Row {
    key: string;
    date: string;
    id: string;
    title: string;
    /** 付いているラベル。まだ受け取っていない走行では空 */
    labels: string[];
    videos: VideoSummary | undefined;
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
  /** 外すかどうかを聞いている動画フォルダ */
  let removing = $state<string | null>(null);

  const rows = $derived.by(() => {
    const videosOf = (rideDate: string) => summaries[rideDate];
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
  const FILTER_TIPS = { has: "このラベルの付いた走行だけを表示中", not: "このラベルの付いていない走行だけを表示中" };
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
    rows: rows.map((row) => `${row.date} ${row.title}: ${row.state ?? "-"}`),
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

  /** 行に出す動画の要約。動画が無く、見つからないフォルダがあれば、見つからないと出す */
  const videoOf = (row: Row) => {
    const videos = row.videos;
    if (videos === undefined || videos.count === 0) {
      return videos?.missing ? { text: "動画が見つかりません", lost: true } : { text: "動画なし", lost: false };
    }
    return { text: `動画 ${videos.count} 本・${hms(videos.duration_ms)}`, lost: false };
  };
</script>

{#snippet detail(row: Row)}
  {@const video = videoOf(row)}
  <span class="mono detail">
    <span class="video" class:lost={video.lost}>
      {#if video.lost}<Icon name="warning" size={13} />{/if}{video.text}
    </span>　{row.summary ? `経路 ${(row.summary.length_m / 1000).toFixed(1)} km` : "記録なし"}
  </span>
{/snippet}

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
        <button type="submit" class="btn" disabled={newDate === ""}>この日付で新規作成</button>
      </form>
    </header>

    <!-- 同期の全体の状態を 1 行で出す。同期は自動なので、押して送り受けするボタンは置かない。 -->
    <div class="cloud" class:failed={message !== ""}>
      <Icon name={message === "" ? "cloud" : "error"} />
      {#if message !== ""}
        <span class="text">{message}</span>
      {:else if !sync.configured}
        <span class="text">同期設定が設定されていません</span>
        <button type="button" class="btn sized" onclick={onsettings}>設定を開く</button>
      {:else if overall === "syncing"}
        <span class="text">同期中…</span>
      {:else if conflicts > 0}
        <span class="text">両方で変更された走行が {conflicts} 件あります。どちらを残すかを選んでください</span>
      {:else if overall === "unsynced"}
        <span class="text" title={sync.failure}>同期できていません。しばらくしてから再試行します</span>
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
            title={state === undefined ? "このラベルの付いた走行だけを表示" : FILTER_TIPS[state]}
            onclick={() => (filter = toggleFilter(narrowing, label.name))}
          >
            {#if state !== undefined}<span class="mark" aria-hidden="true">{state === "has" ? "✓" : "−"}</span>{/if}
            <span class="text">{label.name}</span>
          </button>
        {/each}
        <span class="count mono">{rows.length} 件中 {shown.length} 件</span>
        {#if narrowed}
          <button type="button" class="clear" onclick={() => (filter = {})}>解除</button>
        {/if}
      </div>
    {/if}

    <div class="rows">
      {#if rows.length === 0}
        <p class="empty">
          走行なし
        </p>
      {:else if shown.length === 0}
        <p class="empty">該当する走行なし</p>
      {/if}
      {#each shown as row (row.key)}
        {@const open = row.date === date && !row.remote}
        {#if asking === row.key}
          {@const id = row.id}
          {@const local = row.summary !== undefined}
          <div class="asking">
            <span class="head">
              <span class="mono date">{row.date}</span>
              {#if row.title !== row.date}<b>{row.title}</b>{:else}<span class="untitled">（題なし）</span>{/if}
            </span>
            <span>
              {#if local}
                <b>この走行の記録を削除しますか</b>経路とイベントを削除します。{#if sync.configured}同期先からも削除します。{/if}動画ファイルは残ります。
              {:else}
                <b>同期先にあるこの走行の記録を削除しますか</b>手元に記録はありません。動画ファイルは残ります。
              {/if}
            </span>
            <div class="buttons">
              <button type="button" class="btn" onclick={() => (asking = null)}>キャンセル</button>
              <button type="button" class="btn danger" disabled={busy} onclick={() => remove(id)}>記録を削除</button>
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
                    placeholder="題名"
                    value={title === row.date ? "" : title}
                    onchange={(event) => ontitle(event.currentTarget.value.trim() || row.date)}
                  />
                  <span class="pill here">表示中</span>
                </span>
                <LabelEditor labels={row.labels} {known} onchange={onlabels} />
                {@render detail(row)}
                <div class="folders">
                  <span class="caption">動画フォルダ</span>
                  {#each folders as folder (folder.dir)}
                    {#if removing === folder.dir}
                      <div class="removing">
                        <span class="folder">
                          <Icon name="folder" size={15} /><span class="mono dir" title={folder.dir}>{folder.dir}</span>
                        </span>
                        <span><b>このフォルダを除外しますか</b>　動画ファイルは削除されません</span>
                        <div class="buttons">
                          <button type="button" class="btn small" onclick={() => (removing = null)}>キャンセル</button>
                          <button
                            type="button"
                            class="btn small danger"
                            disabled={videoBusy}
                            onclick={() => {
                              removing = null;
                              onremovefolder(folder.dir);
                            }}
                          >
                            除外
                          </button>
                        </div>
                      </div>
                    {:else}
                      <div class="folder" class:lost={!folder.found}>
                        <Icon name={folder.found ? "folder" : "warning"} size={15} />
                        <span class="mono dir" title={folder.dir}>{folder.dir}</span>
                        <span class="count">{folder.found ? `${folder.count} 本` : "見つかりません"}</span>
                        <button
                          type="button"
                          class="btn small icon-only"
                          aria-label="再読み込み"
                          title="再読み込み"
                          disabled={videoBusy}
                          onclick={onreloadfolders}
                        >
                          <Icon name="refresh" size={14} />
                        </button>
                        <button
                          type="button"
                          class="btn small icon-only ghost"
                          aria-label="このフォルダを除外"
                          title="このフォルダを除外"
                          onclick={() => (removing = folder.dir)}
                        >
                          <Icon name="close" size={14} />
                        </button>
                      </div>
                    {/if}
                  {/each}
                  <div>
                    <button type="button" class="btn small" disabled={videoBusy} onclick={onaddfolder}>
                      <Icon name="plus" size={13} />フォルダを追加
                    </button>
                  </div>
                </div>
              </div>
            {:else if row.remote}
              <div class="main">
                <span class="head">
                  <span class="mono date">{row.date}</span>
                  {#if row.title !== row.date}<b>{row.title}</b>{:else}<span class="untitled">（題なし）</span>{/if}
                </span>
                {@render detail(row)}
              </div>
            {:else}
              <button type="button" class="main" onclick={() => choose(row)} title="この走行を開く">
                <span class="head">
                  <span class="mono date">{row.date}</span>
                  {#if row.title !== row.date}<b>{row.title}</b>{:else}<span class="untitled">（題なし）</span>{/if}
                  {#each row.labels as name (name)}<LabelTag {name} />{/each}
                </span>
                {@render detail(row)}
              </button>
            {/if}
            <span class="sync">
              {#if row.state !== undefined}
                {@const id = row.id}
                <span class="pill {STATES[row.state].tone}">
                  {#if row.state === "synced"}<Icon name="check" size={13} />{/if}{STATES[row.state].name}
                </span>
                {#if row.state === "conflict"}
                  <button type="button" class="btn small" onclick={() => resolve(id, "local")} disabled={busy}>
                    手元で上書き
                  </button>
                  <button type="button" class="btn small" onclick={() => resolve(id, "cloud")} disabled={busy}>
                    同期先で上書き
                  </button>
                {/if}
              {/if}
            </span>
            <button
              type="button"
              class="btn small icon-only ghost trash"
              aria-label="{row.date} の記録を消す"
              title="記録を削除"
              onclick={() => (asking = row.key)}
            >
              <Icon name="trash" size={14} />
            </button>
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
  .detail .video {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    vertical-align: bottom;
  }
  .lost {
    color: var(--warn-ink);
  }
  .folders {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 4px;
    padding: 8px 10px;
    border: 1px solid var(--primary-line);
    border-radius: var(--radius-tile);
    background: var(--surface);
  }
  .caption {
    color: var(--ink-2);
    font-size: 12px;
    font-weight: 700;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .folder :global(.icon) {
    color: var(--ink-3);
  }
  .folder.lost > :global(.icon:first-child) {
    color: var(--warn);
  }
  .folder.lost .dir {
    color: var(--ink-3);
  }
  /* 長い場所は、頭を省いて末尾（フォルダの名前）を残す。 */
  .dir {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    direction: rtl;
    font-size: 12px;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    flex: none;
    color: var(--ink-3);
    font-size: 12px;
  }
  .folder.lost .count {
    color: var(--warn-ink);
  }
  .removing {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--radius-control);
    background: var(--danger-soft);
    color: var(--danger-ink);
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
