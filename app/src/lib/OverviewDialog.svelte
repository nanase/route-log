<script lang="ts">
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { save } from "@tauri-apps/plugin-dialog";
  import { onDestroy, onMount, untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";
  import {
    clampMotion,
    creditImages,
    type CreditImages,
    type Drawn,
    drawnEpisode,
    drawnWaypoints,
    endpointsOf,
    episodeChoices,
    episodeFileName,
    fileName,
    FRAME_RATES,
    type FrameRate,
    motionInput,
    othersOf,
    rememberedMotion,
    rememberMotion,
  } from "./overview";
  import { line, part } from "./route";
  import type { LonLat, RideDocument } from "./types";

  /**
   * 経路の概略図の動画を書き出す。範囲を回か経由点で選び、見本を見てから書き出す。回なら、全部の
   * 回を合わせた経路を見せてから、この回へ寄る。他の回は、寄るにつれて薄くする。
   *
   * 動画は 4K の MP4。フレームレートは選べ、既定は 59.94 fps。背景は国土地理院の写真で、右下に出典を入れる。
   */
  let { doc, onclose }: { doc: RideDocument; onclose: () => void } = $props();

  type Progress = { phase: "tiles"; done: number; total: number } | { phase: "frames"; done: number; total: number };
  /** 見本にする所。最初の見え方なら、最初の大きさを添える */
  type StillAt = { at: "start"; scale: number } | { at: "stop" };

  /** 見本を頼むのを、選び終えるまで待つ時間（ms） */
  const PREVIEW_DELAY_MS = 400;
  /** 範囲を経由点で選ぶときの、範囲の選択肢の値。回の ID とは重ならない */
  const BY_WAYPOINTS = "waypoints";

  let dialog: HTMLDialogElement;
  const ends = $derived(endpointsOf(doc));
  const choices = $derived(episodeChoices(doc.waypoints, doc.episodes));
  /** 範囲。回の ID か、BY_WAYPOINTS。回があれば最初の回にしておく */
  let scope = $state(untrack(() => choices[0]?.id ?? BY_WAYPOINTS));
  let from = $state(0);
  let to = $state(untrack(() => Math.max(doc.waypoints.length - 1, 0)));
  /** 見本に出す所。最初の見え方か、止まった所 */
  let still = $state<"start" | "stop">("stop");
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
  const episode = $derived(choices.find((choice) => choice.id === scope) ?? null);
  /** 描く所。終点が始点より後でなければ null */
  const drawn = $derived<Drawn | null>(
    scope === BY_WAYPOINTS ? drawnWaypoints(from, to) : drawnEpisode(choices, scope),
  );
  const valid = $derived(drawn !== null);
  const routeLine = $derived(drawn === null ? [] : line(part(doc, drawn.from, drawn.to)));
  const others = $derived(drawn === null ? [] : othersOf(doc, drawn));
  const lengthKm = $derived(drawn === null ? 0 : ends[drawn.to].km - ends[drawn.from].km);
  /** Rust 側に渡す動きの設定。薄く描く経路が無ければ、全経路を見せる時間は 0 になる */
  const input = $derived(motionInput(motion, others.length > 0));
  const seconds = $derived(input.leadSeconds + input.zoomSeconds + input.holdSeconds);
  const percent = $derived(progress === null || progress.total === 0 ? 0 : (progress.done / progress.total) * 100);

  onMount(() => {
    dialog.showModal();
    dialog.focus();
  });
  onDestroy(() => {
    clearTimeout(previewTimer);
    if (previewUrl !== "") URL.revokeObjectURL(previewUrl);
  });

  // 範囲か見本に出す所が変わったら、少し待ってから見本を描き直す。最初の見え方は、最初の
  // 大きさでも変わる。
  $effect(() => {
    const route = { line: routeLine, others };
    const at: StillAt = still === "start" ? { at: "start", scale: motion.startScale } : { at: "stop" };
    clearTimeout(previewTimer);
    if (route.line.length < 2) return;
    previewTimer = setTimeout(() => void preview(route, at), PREVIEW_DELAY_MS);
  });

  async function loadCredits(): Promise<CreditImages> {
    credits ??= await creditImages();
    return credits;
  }

  async function preview(route: { line: LonLat[]; others: LonLat[][] }, at: StillAt) {
    const seq = ++previewSeq;
    previewing = true;
    previewError = "";
    try {
      const png = await invoke<ArrayBuffer>("overview_preview", { route, still: at, credits: await loadCredits() });
      if (seq !== previewSeq) return;
      if (previewUrl !== "") URL.revokeObjectURL(previewUrl);
      previewUrl = URL.createObjectURL(new Blob([png], { type: "image/png" }));
    } catch (e) {
      if (seq === previewSeq) previewError = String(e);
    } finally {
      if (seq === previewSeq) previewing = false;
    }
  }

  function setMotion(key: Exclude<keyof typeof motion, "fps">, value: number) {
    if (!Number.isFinite(value)) return;
    remember({ ...motion, [key]: value });
  }

  function setFps(fps: string) {
    remember({ ...motion, fps: fps as FrameRate });
  }

  function remember(next: typeof motion) {
    motion = clampMotion(next);
    rememberMotion(motion);
  }

  async function render() {
    const path = await save({
      title: "概略図生成",
      defaultPath:
        episode === null
          ? fileName(doc.ride.title, from, to, doc.waypoints.length)
          : episodeFileName(doc.ride.title, episode),
      filters: [{ name: "MP4", extensions: ["mp4"] }],
    });
    if (path === null) return;
    message = null;
    progress = { phase: "tiles", done: 0, total: 0 };
    const onProgress = new Channel<Progress>();
    onProgress.onmessage = (next) => (progress = next);
    try {
      await invoke("render_overview", {
        route: { line: routeLine, others },
        motion: input,
        credits: await loadCredits(),
        path,
        onProgress,
      });
      const name = path.split(/[\\/]/).at(-1) ?? path;
      message = { tone: "ok", text: `${name} を生成` };
    } catch (e) {
      const text = String(e);
      message = { tone: "error", text: text.includes("やめました") ? "生成を中止" : text };
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
    <h2 id="overview-title">概略図生成</h2>
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
      {#if previewUrl !== ""}
        <img src={previewUrl} alt={still === "start" ? "最初の見本" : "止まった所の見本"} />
      {/if}
      {#if !valid}
        <span class="state">終点は始点より後の経由地を選択</span>
      {:else if previewError !== ""}
        <span class="state error"><Icon name="error" />{previewError}</span>
      {:else if previewing}
        <span class="state busy"><Spinner />写真を読み込み中…</span>
      {/if}
    </div>
    <div class="caption">
      <div class="seg" role="group" aria-label="見本に出す所">
        {#each [["start", "最初"], ["stop", "ストップ"]] as const as [at, label] (at)}
          <button type="button" class:on={still === at} aria-pressed={still === at} onclick={() => (still = at)}>
            {label}
          </button>
        {/each}
      </div>
    </div>

    <div class="range">
      {#if choices.length > 0}
        <label>
          <span class="sub">範囲</span>
          <select class="field scope" bind:value={scope} disabled={rendering}>
            {#each choices as choice (choice.id)}
              <option value={choice.id}>{choice.name}</option>
            {/each}
            <option value={BY_WAYPOINTS}>経由地で選ぶ</option>
          </select>
        </label>
      {/if}
      {#if scope === BY_WAYPOINTS}
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
      {:else if drawn !== null}
        <span class="ends">
          {drawn.from + 1}　{ends[drawn.from].name}
          <span class="arrow" aria-hidden="true">→</span>
          {drawn.to + 1}　{ends[drawn.to].name}
        </span>
      {/if}
      <span class="length mono">{valid ? `${lengthKm.toFixed(1)} km` : ""}</span>
    </div>

    <div class="motion">
      <label>
        <span class="sub">フレームレート</span>
        <span class="unit">
          <select
            class="field mono"
            value={motion.fps}
            disabled={rendering}
            onchange={(e) => setFps(e.currentTarget.value)}
          >
            {#each Object.keys(FRAME_RATES) as fps (fps)}
              <option value={fps}>{fps}</option>
            {/each}
          </select>fps
        </span>
      </label>
      {#if others.length > 0}
        <label>
          <span class="sub">全経路の時間</span>
          <span class="unit">
            <input
              class="field mono"
              type="number"
              min="0"
              max="30"
              step="0.5"
              value={motion.leadSeconds}
              disabled={rendering}
              onchange={(e) => setMotion("leadSeconds", e.currentTarget.valueAsNumber)}
            />秒
          </span>
        </label>
      {/if}
      <label>
        <span class="sub">ズーム時間</span>
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
        <span class="sub">ストップ時間</span>
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
        <span class="sub">開始時の縮尺</span>
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
  </div>

  <footer>
    {#if progress !== null}
      <div class="progress">
        <span class="sub">
          <Spinner />{progress.phase === "tiles" ? "写真を取得中" : "動画を生成中"}
          <span class="mono">{progress.done} / {progress.total}</span>
        </span>
        <div class="bar"><div style:width="{percent}%"></div></div>
      </div>
      <button type="button" class="btn" onclick={() => void invoke("cancel_overview")}>中止</button>
    {:else}
      {#if message !== null}
        <span class="message" class:error={message.tone === "error"}>
          <Icon name={message.tone === "ok" ? "check" : "error"} />{message.text}
        </span>
      {/if}
      <button type="button" class="btn" onclick={() => dialog.close()}>閉じる</button>
      <button type="button" class="btn primary" disabled={!valid} onclick={render}>
        <Icon name="export" />生成
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
    display: flex;
    align-items: center;
    gap: 12px;
    margin: -4px 0 0;
  }
  .caption .seg button {
    height: 26px;
    padding: 0 10px;
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
  .range select.scope {
    width: 180px;
  }
  .arrow {
    padding-bottom: 6px;
    color: var(--ink-3);
  }
  .ends {
    display: flex;
    gap: 8px;
    padding-bottom: 6px;
  }
  .ends .arrow {
    padding: 0;
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
