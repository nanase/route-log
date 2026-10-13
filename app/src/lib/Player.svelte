<script lang="ts">
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { untrack } from "svelte";
  import { clock, hms } from "./time";
  import type { Position, Timeline } from "./types";

  /** 分割された動画を、実時刻の 1 本の軸として再生する。 */
  let { timeline, muted = false }: { timeline: Timeline; muted?: boolean } = $props();

  let video: HTMLVideoElement;
  /** いま読み込んでいるファイルの番号。まだ無ければ -1 */
  let file = $state(-1);
  let realMs = $state(0);
  let offsetMs = $state(0);
  let loadedPath = "";
  /** 読み込みが済んだら移る位置 */
  let pending: { offsetMs: number; play: boolean } | null = null;

  const startMs = $derived(timeline.spans[0]?.start_ms ?? 0);
  const endMs = $derived(timeline.spans.at(-1)?.end_ms ?? 0);
  const percent = (ms: number) => ((ms - startMs) / Math.max(endMs - startMs, 1)) * 100;

  // 走行が変われば先頭へ移る。時計の補正だけが変わったときは、位置を保って時刻を直す。
  $effect(() => {
    const index = untrack(() => file);
    if (timeline.files[index]?.path === loadedPath) {
      realMs = timeline.spans[index].start_ms + video.currentTime * 1000;
    } else if (timeline.files.length > 0) {
      show({ file: 0, offset_ms: 0 }, false);
    }
  });

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
    if (pending.play) void video.play();
    pending = null;
  }

  function next(play: boolean) {
    if (file + 1 < timeline.files.length) show({ file: file + 1, offset_ms: 0 }, play);
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

  export const play = () => void video.play();
  export const pause = () => video.pause();
  /** いまの状態。自動の確認が読む。 */
  export const snapshot = () => ({
    file: timeline.files[file]?.name ?? "",
    clock: clock(realMs),
    offsetSeconds: video.currentTime,
    paused: video.paused,
    readyState: video.readyState,
  });

  function onkeydown(event: KeyboardEvent) {
    const target = event.target;
    // 文字を打つ欄では何もしない。タイムラインのつまみは、こちらの動きに揃える。
    if (target instanceof HTMLInputElement && target.type !== "range") return;
    if (event.key === " ") {
      if (video.paused) void video.play();
      else video.pause();
    } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      const step = (event.shiftKey ? 10_000 : 1000) * (event.key === "ArrowLeft" ? -1 : 1);
      void seek(realMs + step);
    } else {
      return;
    }
    event.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<section>
  <!-- svelte-ignore a11y_media_has_caption -->
  <video bind:this={video} muted={muted} {onloadedmetadata} {ontimeupdate} onended={() => next(true)}></video>

  <p class="now">
    <strong>{clock(realMs)}</strong>
    <span>{hms(offsetMs)}</span>
    <span>{timeline.files[file]?.name ?? ""}</span>
  </p>

  <div class="bar">
    {#each timeline.spans as span (span.file)}
      <div
        class="span"
        class:current={span.file === file}
        style:left="{percent(span.start_ms)}%"
        style:width="{percent(span.end_ms) - percent(span.start_ms)}%"
      ></div>
    {/each}
    <div class="head" style:left="{percent(realMs)}%"></div>
    <input
      type="range"
      aria-label="再生位置"
      min={startMs}
      max={endMs}
      step="1000"
      value={realMs}
      oninput={(event) => seek(event.currentTarget.valueAsNumber)}
    />
  </div>
  <p class="ends"><span>{clock(startMs)}</span><span>{clock(endMs)}</span></p>

  <table>
    <tbody>
      {#each timeline.spans as span (span.file)}
        {@const item = timeline.files[span.file]}
        {@const gap = timeline.gaps.find((g) => g.start_ms === span.end_ms)}
        <tr class:current={span.file === file}>
          <td><button type="button" onclick={() => show({ file: span.file, offset_ms: 0 }, false)}>{item.name}</button></td>
          <td>{clock(span.start_ms)} – {clock(span.end_ms)}</td>
          <td>{hms(item.duration_ms)}</td>
          <td>{item.codec}</td>
        </tr>
        {#if gap}
          <tr class="gap"><td colspan="4">録画停止 {hms(gap.end_ms - gap.start_ms)}</td></tr>
        {/if}
      {/each}
    </tbody>
  </table>
</section>

<style>
  section {
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto auto auto minmax(0, 12rem);
    grid-template-columns: minmax(0, 1fr);
    gap: 0.25rem;
    height: 100%;
    min-height: 0;
  }
  video {
    width: 100%;
    height: 100%;
    min-height: 0;
    background: #000;
  }
  .now {
    display: flex;
    gap: 1rem;
    align-items: baseline;
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .now strong {
    font-size: 1.5rem;
  }
  .bar {
    position: relative;
    height: 1.5rem;
    background: repeating-linear-gradient(45deg, #ddd 0 4px, #f4f4f4 4px 8px);
  }
  .span {
    position: absolute;
    top: 0;
    bottom: 0;
    box-sizing: border-box;
    border-left: 1px solid #fff;
    background: #7a9cc6;
  }
  .span.current {
    background: #1f4e79;
  }
  .head {
    position: absolute;
    top: -2px;
    bottom: -2px;
    width: 2px;
    background: #d33;
  }
  .bar input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }
  .ends {
    display: flex;
    justify-content: space-between;
    margin: 0;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
  }
  table {
    display: block;
    overflow: auto;
    border-collapse: collapse;
    white-space: nowrap;
    font-size: 0.85rem;
    font-variant-numeric: tabular-nums;
  }
  td {
    padding: 0.1rem 0.75rem 0.1rem 0;
  }
  tr.current td {
    font-weight: bold;
  }
  tr.gap td {
    color: #777;
  }
  td button {
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: #1f4e79;
    cursor: pointer;
  }
</style>
