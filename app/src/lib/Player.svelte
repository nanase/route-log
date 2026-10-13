<script lang="ts">
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { onDestroy, untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import { forShortcut } from "./keys";
  import { clock, hms } from "./time";
  import type { Position, Timeline } from "./types";

  /** 分割された動画を、実時刻の 1 本の軸として再生する。 */
  let {
    timeline,
    muted = false,
    active = true,
    ontime,
  }: {
    timeline: Timeline;
    muted?: boolean;
    /** キーの操作を受け取るか。動画を出していない間は受け取らない */
    active?: boolean;
    /** 再生位置の実時刻が変わるたびに呼ぶ */
    ontime?: (realMs: number) => void;
  } = $props();

  /** 逆再生で 1 回に戻る間隔。負の再生速度が使えないので、シークを繰り返す */
  const REVERSE_STEP_MS = 300;
  /** 続けて押したときに上げる倍率の上限 */
  const MAX_RATE = 4;

  let video: HTMLVideoElement;
  /** いま読み込んでいるファイルの番号。まだ無ければ -1 */
  let file = $state(-1);
  let realMs = $state(0);
  let offsetMs = $state(0);
  /** 再生の倍率。負なら逆再生、0 なら停止 */
  let rate = $state(0);
  let reverseTimer: ReturnType<typeof setInterval> | undefined;
  let loadedPath = "";
  /** 読み込みが済んだら移る位置 */
  let pending: { offsetMs: number; play: boolean } | null = null;

  const startMs = $derived(timeline.spans[0]?.start_ms ?? 0);
  const endMs = $derived(timeline.spans.at(-1)?.end_ms ?? 0);
  const current = $derived(timeline.files[file]);

  // 走行が変われば先頭へ移る。時計の補正が変わったときと、ほかの動画を外したり戻したりした
  // ときは、位置を保って、番号と時刻を直す。
  $effect(() => {
    const index = timeline.files.findIndex((candidate) => candidate.path === loadedPath);
    if (index >= 0) {
      file = index;
      realMs = timeline.spans[index].start_ms + video.currentTime * 1000;
      return;
    }
    if (timeline.files.length === 0) return;
    // 見ていた動画を外したときは、その時刻より後で、最初に録画のある所へ移る。
    const at = untrack(() => realMs);
    if (loadedPath !== "" && at >= startMs && at < endMs) void seek(at);
    else show({ file: 0, offset_ms: 0 }, false);
  });

  $effect(() => ontime?.(realMs));
  onDestroy(() => clearInterval(reverseTimer));

  function show(position: Position, play: boolean) {
    const target = timeline.files[position.file];
    file = position.file;
    offsetMs = position.offset_ms;
    realMs = timeline.spans[position.file].start_ms + position.offset_ms;
    if (target.path === loadedPath) {
      video.currentTime = position.offset_ms / 1000;
      if (play) void video.play();
    } else {
      loadedPath = target.path;
      pending = { offsetMs: position.offset_ms, play };
      video.src = convertFileSrc(target.path);
    }
  }

  function onloadedmetadata() {
    if (pending === null) return;
    video.currentTime = pending.offsetMs / 1000;
    // ファイルを替えると倍率が 1 に戻るので、掛け直す。
    video.playbackRate = rate > 0 ? rate : 1;
    if (pending.play) void video.play();
    pending = null;
  }

  function next(play: boolean) {
    if (file + 1 < timeline.files.length) show({ file: file + 1, offset_ms: 0 }, play);
    else if (rate > 0) rate = 0;
  }

  function ontimeupdate() {
    if (pending !== null || file < 0) return;
    const span = timeline.spans[file];
    const now = span.start_ms + video.currentTime * 1000;
    // 次のファイルと重なる末尾は再生せず、次へ移る。
    if (now >= span.end_ms && file + 1 < timeline.files.length) {
      next(!video.paused);
    } else {
      offsetMs = video.currentTime * 1000;
      realMs = now;
    }
  }

  /** 実時刻へ移る。録画していない時刻は、次に録画が始まる位置にする。 */
  export async function seek(ms: number) {
    const clamped = Math.round(Math.min(Math.max(ms, startMs), endMs - 1));
    const position = await invoke<Position | null>("locate", { realMs: clamped });
    if (position !== null) show(position, !video.paused);
  }

  /** 再生の倍率を変える。負なら逆再生、0 なら停止。 */
  function setRate(next: number) {
    clearInterval(reverseTimer);
    reverseTimer = undefined;
    rate = next;
    if (next > 0) {
      video.playbackRate = next;
      void video.play();
    } else {
      video.pause();
      video.playbackRate = 1;
      if (next < 0) reverseTimer = setInterval(() => void seek(now() + REVERSE_STEP_MS * rate), REVERSE_STEP_MS);
    }
  }

  export const play = () => setRate(1);
  export const pause = () => setRate(0);
  /** 再生位置の実時刻。画面の表示より新しい */
  export function now(): number {
    return file < 0 ? realMs : timeline.spans[file].start_ms + video.currentTime * 1000;
  }
  /** いまの状態。自動の確認が読む。 */
  export const snapshot = () => ({
    file: current?.name ?? "",
    clock: clock(realMs),
    offsetSeconds: video.currentTime,
    paused: video.paused,
    readyState: video.readyState,
  });

  const toggle = () => setRate(video.paused && rate >= 0 ? 1 : 0);
  // 続けて押すと 2 倍、4 倍になる。
  const forward = () => setRate(rate > 0 ? Math.min(rate * 2, MAX_RATE) : 1);
  const reverse = () => setRate(rate < 0 ? Math.max(rate * 2, -MAX_RATE) : -1);
  const step = (ms: number) => void seek(now() + ms);

  function onkeydown(event: KeyboardEvent) {
    if (!active || !forShortcut(event)) return;
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const key = event.key.toLowerCase();
    if (key === " ") toggle();
    else if (key === "k") setRate(0);
    else if (key === "l") forward();
    else if (key === "j") reverse();
    else if (key === "arrowleft") step(event.shiftKey ? -10_000 : -1000);
    else if (key === "arrowright") step(event.shiftKey ? 10_000 : 1000);
    else return;
    event.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<section>
  <div class="screen">
    <!-- svelte-ignore a11y_media_has_caption -->
    <video bind:this={video} {muted} {onloadedmetadata} {ontimeupdate} onended={() => next(rate > 0)}></video>
    <div class="chip now">
      <span class="mono clock">{clock(realMs)}</span>
      <span class="mono where" title={current?.name ?? ""}>
        動画 {file + 1}/{timeline.files.length}・{hms(offsetMs)}
      </span>
    </div>
    <div class="chip mono rate" class:moving={rate !== 0}>
      {#if rate === 0}停止{:else if rate > 0}×{rate}{:else}逆 ×{-rate}{/if}
    </div>
  </div>

  <div class="transport">
    <div class="keys">
      <button type="button" class="key" class:on={rate < 0} aria-label="逆再生" title="逆再生" onclick={reverse}>
        <Icon name="reverse" solid /><kbd>J</kbd>
      </button>
      <button type="button" class="key" aria-label="停止" title="停止" onclick={() => setRate(0)}>
        <Icon name="pause" weight={3} /><kbd>K</kbd>
      </button>
      <button type="button" class="key" class:on={rate > 0} aria-label="再生" title="再生" onclick={forward}>
        <Icon name="play" solid /><kbd>L</kbd>
      </button>
    </div>
    <button type="button" class="space" aria-label="再生／一時停止" title="再生／一時停止" onclick={toggle}>
      {#if rate === 0}<Icon name="play" solid />{:else}<Icon name="pause" weight={3} />{/if}<kbd>Space</kbd>
    </button>
    <div class="keys">
      <button type="button" class="key wide" onclick={() => step(-10_000)}>
        <span class="mono">−10 秒</span><span class="pair"><kbd>Shift</kbd><kbd>←</kbd></span>
      </button>
      <button type="button" class="key" onclick={() => step(-1000)}>
        <span class="mono">−1 秒</span><kbd>←</kbd>
      </button>
      <button type="button" class="key" onclick={() => step(1000)}>
        <span class="mono">+1 秒</span><kbd>→</kbd>
      </button>
      <button type="button" class="key wide" onclick={() => step(10_000)}>
        <span class="mono">+10 秒</span><span class="pair"><kbd>Shift</kbd><kbd>→</kbd></span>
      </button>
    </div>
    <span class="fill"></span>
    <span class="fine hint">J・L 連打で<br />2 倍、4 倍</span>
  </div>
</section>

<style>
  section {
    display: flex;
    flex: none;
    flex-direction: column;
  }
  .screen {
    position: relative;
    aspect-ratio: 16 / 9;
    background: linear-gradient(180deg, #3a4552 0%, #262d36 62%, var(--video-ground) 100%);
  }
  video {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  /* 動画に重ねる表示。実時刻と、ファイルの中の位置、再生の倍率を出す。 */
  .chip {
    position: absolute;
    top: 10px;
    padding: 4px 8px;
    border-radius: var(--radius-control);
    background: var(--video-chip);
    color: #fff;
  }
  .now {
    left: 10px;
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .clock {
    font-size: 22px;
    font-weight: 700;
    line-height: 1;
  }
  .where {
    color: #d5dbe2;
    font-size: 12px;
  }
  .rate {
    right: 10px;
    color: #d5dbe2;
    font-weight: 700;
  }
  .rate.moving {
    color: #fff;
  }

  .transport {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    height: 52px;
    padding: 0 12px;
    border-bottom: 1px solid var(--line);
  }
  .keys {
    display: flex;
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-tile);
  }
  .key {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    width: 40px;
    height: 40px;
    padding: 0;
    border: 0;
    border-right: 1px solid var(--line);
    background: var(--surface);
    font-size: 12px;
    white-space: nowrap;
  }
  .key:last-child {
    border-right: 0;
  }
  .key.wide {
    width: 60px;
  }
  .key:hover {
    background: var(--surface-2);
  }
  .key.on {
    background: var(--primary-soft);
    color: var(--primary);
  }
  .pair {
    display: flex;
    gap: 2px;
  }
  .space {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    width: 72px;
    height: 42px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-tile);
    background: var(--ink);
    color: #fff;
  }
  .space:hover {
    background: var(--ink-2);
  }
  .fill {
    flex: 1;
  }
  .hint {
    line-height: 1.3;
    text-align: right;
    white-space: nowrap;
  }
</style>
