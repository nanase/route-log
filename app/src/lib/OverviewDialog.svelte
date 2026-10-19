<script lang="ts">
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { save } from "@tauri-apps/plugin-dialog";
  import { onDestroy, onMount, untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import {
    clampMotion,
    creditImages,
    type CreditImages,
    endpointsOf,
    fileName,
    rememberedMotion,
    rememberMotion,
  } from "./overview";
  import { line, part } from "./route";
  import type { RideDocument } from "./types";

  /**
   * 経路の概略図の動画を書き出す。範囲を経由点で選び、止まった所の見本を見てから書き出す。
   *
   * 動画は 4K・60 fps の MP4。背景は国土地理院の写真で、右下に出典を入れる。
   */
  let { doc, onclose }: { doc: RideDocument; onclose: () => void } = $props();

  type Progress = { phase: "tiles"; done: number; total: number } | { phase: "frames"; done: number; total: number };

  /** 見本を頼むのを、選び終えるまで待つ時間（ms） */
  const PREVIEW_DELAY_MS = 400;

  let dialog: HTMLDialogElement;
  const ends = $derived(endpointsOf(doc));
  let from = $state(0);
  let to = $state(untrack(() => Math.max(doc.waypoints.length - 1, 0)));
  let motion = $state(rememberedMotion());
  let credits: CreditImages | undefined;
  /** 見本の画像の URL。まだ無ければ空 */
  let previewUrl = $state("");
  let previewing = $state(false);
  let previewError = $state("");
  /** 書き出しの進み具合。書き出していなければ null */
  let progress = $state<Progress | null>(null);
  let message = $state<{ tone: "ok" | "error"; text: string } | null>(null);
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  /** 最後に頼んだ見本の番号。古い見本の答えは捨てる */
  let previewSeq = 0;

  const rendering = $derived(progress !== null);
  const valid = $derived(from < to);
  const routeLine = $derived(valid ? line(part(doc, from, to)) : []);
  const lengthKm = $derived(valid ? ends[to].km - ends[from].km : 0);
  const seconds = $derived(motion.zoomSeconds + motion.holdSeconds);
  const percent = $derived(progress === null || progress.total === 0 ? 0 : (progress.done / progress.total) * 100);

  onMount(() => {
    dialog.showModal();
    dialog.focus();
  });
  onDestroy(() => {
    clearTimeout(previewTimer);
    if (previewUrl !== "") URL.revokeObjectURL(previewUrl);
  });

  // 範囲が変わったら、少し待ってから見本を描き直す。
  $effect(() => {
    const points = routeLine;
    clearTimeout(previewTimer);
    if (points.length < 2) return;
    previewTimer = setTimeout(() => void preview(points), PREVIEW_DELAY_MS);
  });

  async function loadCredits(): Promise<CreditImages> {
    credits ??= await creditImages();
    return credits;
  }

  async function preview(points: [number, number][]) {
    const seq = ++previewSeq;
    previewing = true;
    previewError = "";
    try {
      const png = await invoke<ArrayBuffer>("overview_preview", { line: points, credits: await loadCredits() });
      if (seq !== previewSeq) return;
      if (previewUrl !== "") URL.revokeObjectURL(previewUrl);
      previewUrl = URL.createObjectURL(new Blob([png], { type: "image/png" }));
    } catch (e) {
      if (seq === previewSeq) previewError = String(e);
    } finally {
      if (seq === previewSeq) previewing = false;
    }
  }

  function setMotion(key: keyof typeof motion, value: number) {
    if (!Number.isFinite(value)) return;
    motion = clampMotion({ ...motion, [key]: value });
    rememberMotion(motion);
  }

  async function render() {
    const path = await save({
      title: "概略図の動画を書き出す",
      defaultPath: fileName(doc.ride.title, from, to, doc.waypoints.length),
      filters: [{ name: "MP4", extensions: ["mp4"] }],
    });
    if (path === null) return;
    message = null;
    progress = { phase: "tiles", done: 0, total: 0 };
    const onProgress = new Channel<Progress>();
    onProgress.onmessage = (next) => (progress = next);
    try {
      await invoke("render_overview", {
        line: routeLine,
        motion: $state.snapshot(motion),
        credits: await loadCredits(),
        path,
        onProgress,
      });
      const name = path.split(/[\\/]/).at(-1) ?? path;
      message = { tone: "ok", text: `${name} を書き出しました` };
    } catch (e) {
      const text = String(e);
      message = { tone: "error", text: text.includes("やめました") ? "書き出しをやめました" : text };
    }
    progress = null;
  }

  // 書き出している間は閉じない。Esc でも閉じない。
  function oncancel(event: Event) {
    if (rendering) event.preventDefault();
  }
</script>

<dialog bind:this={dialog} class="card" tabindex="-1" aria-labelledby="overview-title" {onclose} {oncancel}>
  <header>
    <h2 id="overview-title">概略図の動画</h2>
    <button
      type="button"
      class="btn icon-only ghost"
      aria-label="閉じる"
      disabled={rendering}
      onclick={() => dialog.close()}
    >
      <Icon name="close" />
    </button>
  </header>

  <div class="body">
    <div class="preview">
      {#if previewUrl !== ""}<img src={previewUrl} alt="止まった所の見本" />{/if}
      {#if !valid}
        <span class="state">始点より後の経由点を、終点に選んでください</span>
      {:else if previewError !== ""}
        <span class="state error"><Icon name="error" />{previewError}</span>
      {:else if previewing}
        <span class="state busy">写真を読み込み中…</span>
      {/if}
    </div>
    <p class="fine caption">止まった所の見本。最初は引いた所から、ゆっくり寄ってここで止まります</p>

    <div class="range">
      <label>
        <span class="sub">始点</span>
        <select class="field" bind:value={from} disabled={rendering}>
          {#each ends.slice(0, -1) as end (end.index)}
            <option value={end.index}>{end.index + 1}　{end.name}（{end.km.toFixed(1)} km）</option>
          {/each}
        </select>
      </label>
      <span class="arrow" aria-hidden="true">→</span>
      <label>
        <span class="sub">終点</span>
        <select class="field" bind:value={to} disabled={rendering}>
          {#each ends.slice(1) as end (end.index)}
            <option value={end.index}>{end.index + 1}　{end.name}（{end.km.toFixed(1)} km）</option>
          {/each}
        </select>
      </label>
      <span class="length mono">{valid ? `${lengthKm.toFixed(1)} km` : ""}</span>
    </div>

    <div class="motion">
      <label>
        <span class="sub">寄る時間</span>
        <span class="unit">
          <input
            class="field mono"
            type="number"
            min="0"
            max="30"
            step="0.5"
            value={motion.zoomSeconds}
            disabled={rendering}
            onchange={(e) => setMotion("zoomSeconds", e.currentTarget.valueAsNumber)}
          />秒
        </span>
      </label>
      <label>
        <span class="sub">止めて見せる時間</span>
        <span class="unit">
          <input
            class="field mono"
            type="number"
            min="0"
            max="120"
            step="1"
            value={motion.holdSeconds}
            disabled={rendering}
            onchange={(e) => setMotion("holdSeconds", e.currentTarget.valueAsNumber)}
          />秒
        </span>
      </label>
      <label>
        <span class="sub">最初の大きさ</span>
        <span class="unit">
          <input
            class="field mono"
            type="number"
            min="10"
            max="100"
            step="5"
            value={Math.round(motion.startScale * 100)}
            disabled={rendering}
            onchange={(e) => setMotion("startScale", e.currentTarget.valueAsNumber / 100)}
          />%
        </span>
      </label>
      <span class="sub total">全体で <span class="mono">{seconds}</span> 秒</span>
    </div>
    <p class="fine">
      4K・60 fps の MP4 に書き出します。音は入りません。背景は国土地理院の写真で、右下に出典を入れます。
    </p>
  </div>

  <footer>
    {#if progress !== null}
      <div class="progress">
        <span class="sub">
          {progress.phase === "tiles" ? "写真を取得中" : "動画を書いています"}
          <span class="mono">{progress.done} / {progress.total}</span>
        </span>
        <div class="bar"><div style:width="{percent}%"></div></div>
      </div>
      <button type="button" class="btn" onclick={() => void invoke("cancel_overview")}>やめる</button>
    {:else}
      {#if message !== null}
        <span class="message" class:error={message.tone === "error"}>
          <Icon name={message.tone === "ok" ? "check" : "error"} />{message.text}
        </span>
      {/if}
      <button type="button" class="btn" onclick={() => dialog.close()}>閉じる</button>
      <button type="button" class="btn primary" disabled={!valid} onclick={render}>
        <Icon name="export" />書き出す
      </button>
    {/if}
  </footer>
</dialog>

<style>
  dialog {
    width: min(720px, 94vw);
    max-height: 94vh;
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
  .body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px 20px;
  }
  .preview {
    position: relative;
    display: grid;
    place-items: center;
    aspect-ratio: 16 / 9;
    overflow: hidden;
    border-radius: var(--radius-tile);
    background: #2b3540;
  }
  .preview img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .state {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 80%;
    padding: 6px 12px;
    border-radius: var(--radius-control);
    background: rgba(26, 29, 33, 0.72);
    color: #fff;
  }
  .state.busy {
    position: absolute;
    top: 10px;
    right: 10px;
  }
  .caption {
    margin: -6px 0 0;
  }
  .range,
  .motion {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 8px 12px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .range select {
    width: 250px;
  }
  .arrow {
    padding-bottom: 6px;
    color: var(--ink-3);
  }
  .length {
    padding-bottom: 6px;
    color: var(--ink-2);
  }
  .unit {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .unit input {
    width: 72px;
    text-align: right;
  }
  .total {
    padding-bottom: 6px;
  }
  p {
    margin: 0;
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    min-height: 32px;
    padding: 12px 20px;
    border-top: 1px solid var(--line);
    border-radius: 0 0 var(--radius-card) var(--radius-card);
    background: var(--surface-3);
  }
  .progress {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 4px;
  }
  .bar {
    height: 6px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--line-soft);
  }
  .bar div {
    height: 100%;
    background: var(--primary);
  }
  .message {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--ok-ink);
  }
  .message.error {
    color: var(--danger-ink);
  }
</style>
