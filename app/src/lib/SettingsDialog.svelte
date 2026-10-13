<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "./Icon.svelte";
  import type { Config } from "./types";
  import type { Folder, FolderState } from "./ui";

  /**
   * 設定。初回に 1 度選べば済むフォルダ 3 つ、走行ごとの時計の補正、クラウド同期の接続先をまとめる。
   */
  let {
    config,
    videoState,
    roadState,
    geoState,
    clockOffsetMs,
    rideDate,
    onpick,
    onclock,
    onsynced,
    onclose,
  }: {
    /** 選んであるフォルダの場所 */
    config: Config;
    videoState: FolderState;
    roadState: FolderState;
    geoState: FolderState;
    clockOffsetMs: number;
    /** 時計の補正を直す相手の走行。開いていなければ null */
    rideDate: string | null;
    onpick: (folder: Folder) => void;
    onclock: (clockOffsetMs: number) => void;
    /** 同期の接続先を保存した。呼ばれた側は、新しい接続先でクラウドと突き合わせ直す */
    onsynced: () => void;
    onclose: () => void;
  } = $props();

  let dialog: HTMLDialogElement;
  let apiUrl = $state("");
  let clientId = $state("");
  let clientSecret = $state("");
  let hasToken = $state(false);
  /** 保存してある接続先。まだ読んでいなければ undefined */
  let savedUrl = $state<string | undefined>(undefined);
  let editingSync = $state(false);
  let syncMessage = $state("");
  let busy = $state(false);

  const folders = $derived([
    {
      id: "video" as const,
      name: "動画フォルダ",
      note: "ドラレコの動画を置いた場所",
      state: videoState,
      dir: config.video_dir,
    },
    {
      id: "road" as const,
      name: "道路データ",
      note: "地図に重ねる国道・都道府県道の線。選ばなくても使えます",
      state: roadState,
      dir: config.road_data_dir,
    },
    {
      id: "geo" as const,
      name: "判定データ",
      note: "イベントの抽出と「道に沿わせる」に使います",
      state: geoState,
      dir: config.geo_data_dir,
    },
  ]);
  const syncReady = $derived(Boolean(savedUrl) && hasToken);
  const seconds = $derived(Math.round(clockOffsetMs / 1000));

  $effect(() => {
    dialog.showModal();
    // 開いたときに、最初の欄やボタンを勝手に選ばない。
    dialog.focus();
    void loadSync();
  });

  async function loadSync() {
    const settings = await invoke<{ api_url: string | null; has_token: boolean }>("get_sync_settings");
    savedUrl = settings.api_url ?? "";
    apiUrl = savedUrl;
    hasToken = settings.has_token;
  }

  async function saveSync() {
    busy = true;
    syncMessage = "";
    try {
      await invoke("set_sync_settings", { apiUrl, clientId, clientSecret });
      clientId = "";
      clientSecret = "";
      await loadSync();
      editingSync = false;
      onsynced();
    } catch (e) {
      syncMessage = String(e);
    }
    busy = false;
  }
</script>

<dialog bind:this={dialog} class="card" tabindex="-1" aria-labelledby="settings-title" {onclose}>
  <header>
    <h2 id="settings-title">設定</h2>
    <button type="button" class="btn icon-only ghost" aria-label="閉じる" onclick={() => dialog.close()}>
      <Icon name="close" />
    </button>
  </header>

  <section class="folders">
    <div class="lead"><b>フォルダ</b></div>
    {#each folders as folder, i (folder.id)}
      {#if i > 0}<div class="rule"></div>{/if}
      <div class="folder">
        <span class="name">
          <b>{folder.name}</b>
          {#if folder.state === "ready"}
            <span class="pill ok"><Icon name="check" size={13} />読み込み済み</span>
          {:else if folder.state === "loading"}
            <span class="pill muted">読み込み中…</span>
          {:else if folder.state === "error"}
            <span class="pill warn"><Icon name="warning" size={13} />開けません</span>
          {:else}
            <span class="pill warn"><Icon name="warning" size={13} />まだ選んでいません</span>
          {/if}
        </span>
        <button
          type="button"
          class="btn"
          class:primary={folder.state === "none"}
          disabled={folder.state === "loading"}
          onclick={() => onpick(folder.id)}
        >
          {folder.dir ? "選び直す" : "フォルダを選ぶ"}
        </button>
        <span class="sub note">
          {folder.note}{#if folder.dir}<span class="mono dir" title={folder.dir}>{folder.dir}</span>{/if}
        </span>
      </div>
    {/each}
  </section>

  <section class="line">
    <div class="text">
      <b>時計の補正</b>
      <span class="sub">
        {#if rideDate === null}
          ドラレコの時計のずれを、秒で直します。走行を開くと直せます
        {:else}
          ドラレコの時計のずれを、秒で直します。<span class="mono">{rideDate}</span> の走行に効きます
        {/if}
      </span>
    </div>
    <div class="stepper">
      <button
        type="button"
        class="btn icon-only"
        aria-label="1 秒減らす"
        disabled={rideDate === null}
        onclick={() => onclock((seconds - 1) * 1000)}
      >
        <Icon name="minus" />
      </button>
      <input
        class="field mono"
        type="number"
        step="1"
        aria-label="時計の補正（秒）"
        disabled={rideDate === null}
        value={seconds}
        onchange={(event) => onclock(Math.round(event.currentTarget.valueAsNumber || 0) * 1000)}
      />
      <button
        type="button"
        class="btn icon-only"
        aria-label="1 秒増やす"
        disabled={rideDate === null}
        onclick={() => onclock((seconds + 1) * 1000)}
      >
        <Icon name="plus" />
      </button>
      <span>秒</span>
    </div>
  </section>

  <section class="line">
    <div class="text">
      <b>クラウド同期の接続先</b>
      <span class="sub">設定すると、走行を自動でクラウドと同期します。保存済みの鍵は出しません</span>
    </div>
    {#if savedUrl !== undefined}
      {#if syncReady}
        <span class="pill ok"><Icon name="check" size={13} />設定済み</span>
      {:else}
        <span class="pill warn"><Icon name="warning" size={13} />まだ設定していません</span>
      {/if}
      {#if !editingSync}
        <button type="button" class="btn" class:primary={!syncReady} onclick={() => (editingSync = true)}>
          {syncReady ? "変える" : "設定する"}
        </button>
      {/if}
    {/if}
  </section>
  {#if editingSync}
    <form class="sync" onsubmit={(event) => (event.preventDefault(), saveSync())}>
      <label class="wide">
        API の URL
        <input class="field" bind:value={apiUrl} placeholder="https://…" spellcheck="false" />
      </label>
      <label>
        Client ID
        <input
          class="field"
          bind:value={clientId}
          placeholder={hasToken ? "保存済み。替えるときだけ入れる" : ""}
          spellcheck="false"
        />
      </label>
      <label>
        Client Secret
        <input
          class="field"
          type="password"
          bind:value={clientSecret}
          placeholder={hasToken ? "保存済み。替えるときだけ入れる" : ""}
        />
      </label>
      <div class="wide buttons">
        {#if syncMessage !== ""}<span class="failed"><Icon name="error" />{syncMessage}</span>{/if}
        <button type="button" class="btn" onclick={() => (editingSync = false)}>やめる</button>
        <button type="submit" class="btn primary" disabled={busy}>接続先を保存</button>
      </div>
    </form>
  {/if}

  <footer>
    <button type="button" class="btn close" onclick={() => dialog.close()}>閉じる</button>
  </footer>
</dialog>

<style>
  dialog {
    width: min(560px, 92vw);
    max-height: 92vh;
  }
  header {
    display: flex;
    align-items: center;
    height: 48px;
    padding: 0 10px 0 20px;
    border-bottom: 1px solid var(--line);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 16px;
  }
  .folders {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px 20px;
  }
  .lead {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .rule {
    height: 1px;
    background: var(--line-soft);
  }
  .folder {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 4px 12px;
  }
  .folder .name {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .folder .btn {
    grid-row: span 2;
  }
  .note {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  /* 長い場所は、頭を省いて末尾（フォルダの名前）を残す。 */
  .dir {
    overflow: hidden;
    direction: rtl;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 20px;
    border-top: 1px solid var(--line);
  }
  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
  }
  .stepper {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .stepper input {
    width: 72px;
    font-size: 14px;
    text-align: right;
  }
  .sync {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px 12px;
    padding: 0 20px 14px;
  }
  .sync label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--ink-2);
    font-size: 12px;
  }
  .sync input {
    color: var(--ink);
    font-size: 13px;
  }
  .wide {
    grid-column: 1 / -1;
  }
  .buttons {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }
  .failed {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 6px;
    color: var(--danger-ink);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    padding: 12px 20px;
    border-top: 1px solid var(--line);
    border-radius: 0 0 var(--radius-card) var(--radius-card);
    background: var(--surface-3);
  }
  .close {
    height: 32px;
    padding: 0 20px;
  }
</style>
