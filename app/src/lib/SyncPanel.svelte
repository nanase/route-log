<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  /**
   * クラウドとの同期。手元とクラウドの走行を突き合わせ、押したものだけを送り受けする。
   */
  let {
    onchanged,
    onclose,
  }: {
    /** 手元の走行が受け取りで変わったときに呼ぶ */
    onchanged: () => void | Promise<void>;
    onclose: () => void;
  } = $props();

  type State = "same" | "local_newer" | "remote_newer" | "local_only" | "remote_only" | "conflict";
  interface Row {
    id: string;
    title: string;
    ride_date: string;
    local_updated_at: string | null;
    remote_updated_at: string | null;
    state: State;
  }

  const STATES: Record<State, string> = {
    same: "同じ",
    local_newer: "手元が新しい",
    remote_newer: "クラウドが新しい",
    local_only: "手元だけ",
    remote_only: "クラウドだけ",
    conflict: "両方で変更",
  };

  let dialog: HTMLDialogElement;
  let apiUrl = $state("");
  let clientId = $state("");
  let clientSecret = $state("");
  let hasToken = $state(false);
  let rows = $state<Row[]>([]);
  let message = $state("");
  let busy = $state(false);

  $effect(() => {
    dialog.showModal();
    void load();
  });

  async function load() {
    const settings = await invoke<{ api_url: string | null; has_token: boolean }>("get_sync_settings");
    apiUrl = settings.api_url ?? "";
    hasToken = settings.has_token;
    if (apiUrl !== "" && hasToken) await refresh();
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

  export const refresh = () =>
    run(async () => {
      rows = await invoke<Row[]>("sync_list");
    });

  const saveSettings = () =>
    run(async () => {
      await invoke("set_sync_settings", { apiUrl, clientId, clientSecret });
      clientId = "";
      clientSecret = "";
      await load();
    });

  export const push = (id: string, overwrite = false) =>
    run(async () => {
      await invoke("sync_push", { id, overwrite });
      rows = await invoke<Row[]>("sync_list");
    });

  export const pull = (id: string) =>
    run(async () => {
      await invoke("sync_pull", { id });
      await onchanged();
      rows = await invoke<Row[]>("sync_list");
    });

  /** どちらを残すか迷わないものを、まとめて送り受けする。両方で変わったものは残す。 */
  const syncAll = () =>
    run(async () => {
      for (const row of rows) {
        if (row.state === "local_newer" || row.state === "local_only") {
          await invoke("sync_push", { id: row.id, overwrite: false });
        } else if (row.state === "remote_newer" || row.state === "remote_only") {
          await invoke("sync_pull", { id: row.id });
        }
      }
      await onchanged();
      rows = await invoke<Row[]>("sync_list");
    });

  /** いまの一覧。自動の確認が読む。 */
  export const snapshot = () => ({ rows: rows.map((row) => `${row.ride_date} ${row.title}: ${row.state}`), message });
</script>

<dialog bind:this={dialog} onclose={onclose}>
  <header>
    <strong>クラウドと同期</strong>
    <button type="button" onclick={() => dialog.close()}>閉じる</button>
  </header>

  <form onsubmit={(event) => (event.preventDefault(), saveSettings())}>
    <label>API の URL <input bind:value={apiUrl} placeholder="https://…" spellcheck="false" /></label>
    <label>
      Client ID <input bind:value={clientId} placeholder={hasToken ? "保存済み。替えるときだけ入れる" : ""} spellcheck="false" />
    </label>
    <label>
      Client Secret
      <input type="password" bind:value={clientSecret} placeholder={hasToken ? "保存済み。替えるときだけ入れる" : ""} />
    </label>
    <button type="submit" disabled={busy}>設定を保存</button>
  </form>

  <p class="actions">
    <button type="button" onclick={refresh} disabled={busy}>一覧を取り直す</button>
    <button type="button" onclick={syncAll} disabled={busy || rows.length === 0}>まとめて同期</button>
    <span class="message">{message}</span>
  </p>

  {#if rows.length === 0 && !busy && message === ""}
    <p class="empty">
      走行がまだありません。画面を閉じて、左上の日付の欄から走行を作り、地図に経路を引いてください。ここに行が出て、右端に「送る」のボタンが付きます。
    </p>
  {/if}
  <table>
    <tbody>
      {#each rows as row (row.id)}
        <tr>
          <td>{row.ride_date}</td>
          <td>{row.title}</td>
          <td class={row.state}>{STATES[row.state]}</td>
          <td>
            {#if row.state === "local_newer" || row.state === "local_only"}
              <button type="button" onclick={() => push(row.id)} disabled={busy}>送る</button>
            {:else if row.state === "remote_newer" || row.state === "remote_only"}
              <button type="button" onclick={() => pull(row.id)} disabled={busy}>受け取る</button>
            {:else if row.state === "conflict"}
              <button type="button" onclick={() => push(row.id, true)} disabled={busy}>手元で上書き</button>
              <button type="button" onclick={() => pull(row.id)} disabled={busy}>クラウドで上書き</button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</dialog>

<style>
  dialog {
    width: min(46rem, 90vw);
    max-height: 80vh;
    padding: 1rem;
    border: 1px solid #999;
    border-radius: 6px;
    font-size: 0.9rem;
  }
  header {
    display: flex;
    justify-content: space-between;
    margin-bottom: 0.75rem;
  }
  form {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.4rem 0.75rem;
    margin-bottom: 0.75rem;
  }
  form label {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  form label:first-child {
    grid-column: 1 / -1;
  }
  form button {
    justify-self: start;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    margin: 0 0 0.5rem;
  }
  .message {
    color: #c92a2a;
  }
  .empty {
    margin: 0;
    color: #555;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  td {
    padding: 0.2rem 0.75rem 0.2rem 0;
    white-space: nowrap;
  }
  td.conflict {
    color: #c92a2a;
    font-weight: bold;
  }
  td.same {
    color: #777;
  }
</style>
