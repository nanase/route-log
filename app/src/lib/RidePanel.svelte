<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "./Icon.svelte";
  import { hms } from "./time";
  import type { RideSummary, RideVideos } from "./types";
  import type { SyncRow, SyncState } from "./ui";

  /**
   * 走行の切り替え。上の帯の日付と題を押すと開く。
   *
   * 走行を選ぶ・作る・消すことと、クラウドとの同期をここで行う。同期は、手元とクラウドの走行を
   * 突き合わせ、押したものだけを送り受けする。
   */
  let {
    date,
    title,
    videos,
    stored,
    onselect,
    oncreate,
    ontitle,
    onremove,
    onchanged,
    onsettings,
    onclose,
  }: {
    /** 開いている走行の日付 */
    date: string | null;
    /** 開いている走行の題 */
    title: string;
    videos: RideVideos[];
    stored: RideSummary[];
    onselect: (date: string) => void | Promise<void>;
    oncreate: (date: string) => void | Promise<void>;
    ontitle: (title: string) => void;
    /** 手元の走行の記録を消す */
    onremove: (id: string) => Promise<void>;
    /** 手元の走行が受け取りで変わったときに呼ぶ */
    onchanged: () => void | Promise<void>;
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
    videos: RideVideos | undefined;
    summary: RideSummary | undefined;
    sync: SyncRow | undefined;
  }

  const STATES: Record<SyncState, { name: string; tone: string }> = {
    same: { name: "クラウドと同じ", tone: "ok" },
    local_newer: { name: "手元が新しい", tone: "warn" },
    remote_newer: { name: "クラウドが新しい", tone: "info" },
    local_only: { name: "手元だけ", tone: "warn" },
    remote_only: { name: "クラウドだけ", tone: "info" },
    conflict: { name: "両方で変更", tone: "danger" },
  };

  let dialog: HTMLDialogElement;
  let newDate = $state("");
  /** 同期の接続先が設定してあるか。まだ調べていなければ undefined */
  let configured = $state<boolean | undefined>(undefined);
  let synced = $state<SyncRow[]>([]);
  /** クラウドの一覧を取れているか */
  let listed = $state(false);
  let message = $state("");
  let busy = $state(false);
  /** 消すかどうかを聞いている行 */
  let asking = $state<string | null>(null);
  let alsoCloud = $state(false);

  const rows = $derived.by(() => {
    const syncOf = (id: string) => synced.find((row) => row.id === id);
    const videosOf = (rideDate: string) => videos.find((ride) => ride.date === rideDate);
    const list: Row[] = stored.map((summary) => ({
      key: summary.id,
      date: summary.ride_date,
      id: summary.id,
      title: summary.title,
      videos: videosOf(summary.ride_date),
      summary,
      sync: syncOf(summary.id),
    }));
    for (const row of synced) {
      if (row.state !== "remote_only" || list.some((known) => known.id === row.id)) continue;
      list.push({
        key: row.id,
        date: row.ride_date,
        id: row.id,
        title: row.title,
        videos: videosOf(row.ride_date),
        summary: undefined,
        sync: row,
      });
    }
    for (const ride of videos) {
      if (list.some((known) => known.date === ride.date)) continue;
      const none = { id: null, summary: undefined, sync: undefined };
      list.push({ key: ride.date, date: ride.date, title: ride.date, videos: ride, ...none });
    }
    return list.sort((a, b) => (a.date === b.date ? 0 : a.date < b.date ? 1 : -1));
  });
  const differing = $derived(synced.filter((row) => row.state !== "same").length);

  $effect(() => {
    dialog.showModal();
    // 開いたときに、最初の欄やボタンを勝手に選ばない。
    dialog.focus();
    void load();
  });

  async function load() {
    const settings = await invoke<{ api_url: string | null; has_token: boolean }>("get_sync_settings");
    configured = Boolean(settings.api_url) && settings.has_token;
    if (configured) await refresh();
  }

  /** 画面の操作を 1 つ行う。失敗は 1 行で出す。 */
  async function run(work: () => Promise<void>) {
    busy = true;
    message = "";
    try {
      await work();
    } catch (e) {
      message = String(e);
    }
    busy = false;
  }

  async function list() {
    synced = await invoke<SyncRow[]>("sync_list");
    listed = true;
  }

  export const refresh = () => run(list);

  export const push = (id: string, overwrite = false) =>
    run(async () => {
      await invoke("sync_push", { id, overwrite });
      await list();
    });

  export const pull = (id: string) =>
    run(async () => {
      await invoke("sync_pull", { id });
      await onchanged();
      await list();
    });

  /** どちらを残すか迷わないものを、まとめて送り受けする。両方で変わったものは残す。 */
  const syncAll = () =>
    run(async () => {
      for (const row of synced) {
        if (row.state === "local_newer" || row.state === "local_only") {
          await invoke("sync_push", { id: row.id, overwrite: false });
        } else if (row.state === "remote_newer" || row.state === "remote_only") {
          await invoke("sync_pull", { id: row.id });
        }
      }
      await onchanged();
      await list();
    });

  /**
   * 走行の記録を消す。手元とクラウドの、どちらから消すかを選べる。
   *
   * クラウドを先に消す。繋がらずに失敗したら手元も残るので、もう一度選び直せる。
   */
  export const remove = (id: string, where: { local: boolean; cloud: boolean }) =>
    run(async () => {
      if (where.cloud) await invoke("sync_delete", { id });
      if (where.local) await onremove(id);
      asking = null;
      if (listed) await list();
    });

  /** いまの一覧。自動の確認が読む。 */
  export const snapshot = () => ({
    rows: synced.map((row) => `${row.ride_date} ${row.title}: ${row.state}`),
    message,
  });

  /** その走行がクラウドにもあるか。一覧を取れていなければ、分からないので無いものとして扱う。 */
  const onCloud = (row: Row) => row.sync !== undefined && row.sync.state !== "local_only";

  function ask(row: Row) {
    asking = row.key;
    alsoCloud = false;
  }

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

    <div class="cloud" class:failed={message !== ""}>
      <Icon name={message === "" ? "cloud" : "error"} />
      {#if message !== ""}
        <span class="text">{message}</span>
      {:else if configured === false}
        <span class="text">クラウド同期の接続先が、まだ設定されていません</span>
      {:else if !listed}
        <span class="text">{busy ? "クラウドの一覧を取っています…" : "クラウドの一覧をまだ取っていません"}</span>
      {:else if differing > 0}
        <span class="text">クラウドと違う走行が {differing} 件あります</span>
      {:else}
        <span class="text">どの走行も、クラウドと同じです</span>
      {/if}
      {#if configured === false}
        <button type="button" class="btn sized" onclick={onsettings}>設定を開く</button>
      {:else if configured}
        <button type="button" class="btn sized" onclick={refresh} disabled={busy}>一覧を取り直す</button>
        <button type="button" class="btn sized" onclick={syncAll} disabled={busy || !listed || differing === 0}>
          まとめて同期
        </button>
      {/if}
    </div>

    <div class="rows">
      {#if rows.length === 0}
        <p class="empty">
          走行がまだありません。上の欄に日付を入れて作るか、設定で動画フォルダを選んでください。
        </p>
      {/if}
      {#each rows as row (row.key)}
        {@const open = row.date === date && row.sync?.state !== "remote_only"}
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
                <b>この走行の記録を消しますか。</b>経路とイベントが消えます。動画のファイルは消えません。
              {:else}
                <b>クラウドにある、この走行の記録を消しますか。</b>手元には記録がありません。動画のファイルは消えません。
              {/if}
            </span>
            <div class="buttons">
              {#if local && onCloud(row)}
                <label><input type="checkbox" bind:checked={alsoCloud} />クラウドからも消す</label>
              {/if}
              <span class="space"></span>
              <button type="button" class="btn" onclick={() => (asking = null)}>やめる</button>
              <button
                type="button"
                class="btn danger"
                disabled={busy}
                onclick={() => remove(id, { local, cloud: local ? alsoCloud : true })}
              >
                記録を消す
              </button>
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
                <span class="mono detail">{summaryOf(row)}</span>
              </div>
            {:else if row.sync?.state === "remote_only"}
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
                </span>
                <span class="mono detail">{summaryOf(row)}</span>
              </button>
            {/if}
            <span class="sync">
              {#if row.sync !== undefined && row.id !== null}
                {@const id = row.id}
                {@const state = row.sync.state}
                <span class="pill {STATES[state].tone}">
                  {#if state === "same"}<Icon name="check" size={13} />{/if}{STATES[state].name}
                </span>
                {#if state === "local_newer" || state === "local_only"}
                  <button type="button" class="btn small" onclick={() => push(id)} disabled={busy}>送る</button>
                {:else if state === "remote_newer" || state === "remote_only"}
                  <button type="button" class="btn small" onclick={() => pull(id)} disabled={busy}>受け取る</button>
                {:else if state === "conflict"}
                  <button type="button" class="btn small" onclick={() => push(id, true)} disabled={busy}>
                    手元で上書き
                  </button>
                  <button type="button" class="btn small" onclick={() => pull(id)} disabled={busy}>
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
                onclick={() => ask(row)}
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
    align-items: center;
    gap: 10px;
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
    gap: 8px;
  }
  .buttons label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .buttons input {
    width: 14px;
    height: 14px;
    margin: 0;
  }
  .space {
    flex: 1;
  }
</style>
