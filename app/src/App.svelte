<script lang="ts">
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { runCheck } from "./lib/playback-check";

  let video: HTMLVideoElement;
  let path = $state("");
  let lines = $state<string[]>([]);
  let busy = $state(false);

  const log = (line: string) => lines.push(line);

  async function open(): Promise<string | null> {
    try {
      const file = await invoke<{ path: string; size: number }>("allow_video_file", { path });
      log(`${file.path}（${(file.size / 2 ** 30).toFixed(1)} GB）`);
      return convertFileSrc(file.path);
    } catch (e) {
      log(`開けない: ${e}`);
      return null;
    }
  }

  async function play() {
    const source = await open();
    if (source !== null) video.src = source;
  }

  async function check() {
    busy = true;
    const source = await open();
    const report = source === null ? null : await runCheck(video, source, log);
    if (report?.error) log(`失敗: ${report.error}`);
    busy = false;
    return report;
  }

  onMount(async () => {
    const target = await invoke<string | null>("check_target");
    if (target === null) return;
    path = target;
    const report = await check();
    await invoke("finish_check", { report: JSON.stringify(report ?? { error: "開けない", lines }, null, 2) });
  });
</script>

<main>
  <form onsubmit={(e) => (e.preventDefault(), play())}>
    <input bind:value={path} placeholder="動画ファイルのパス" spellcheck="false" />
    <button type="submit" disabled={busy}>開く</button>
    <button type="button" onclick={check} disabled={busy}>計測</button>
  </form>
  <!-- svelte-ignore a11y_media_has_caption -->
  <video bind:this={video} controls muted crossorigin="anonymous"></video>
  <pre>{lines.join("\n")}</pre>
</main>

<style>
  main {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) 12rem;
    gap: 0.5rem;
    height: 100vh;
    box-sizing: border-box;
    padding: 0.5rem;
    font-family: system-ui, sans-serif;
  }
  form {
    display: flex;
    gap: 0.5rem;
  }
  input {
    flex: 1;
  }
  video {
    width: 100%;
    height: 100%;
    background: #000;
  }
  pre {
    margin: 0;
    overflow: auto;
    font-size: 0.8rem;
  }
  :global(body) {
    margin: 0;
  }
</style>
