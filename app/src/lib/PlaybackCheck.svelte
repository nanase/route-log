<script lang="ts">
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { runCheck } from "./playback-check";

  /** 計測する動画のパス。計測が済むと、報告を書いてアプリを終了する。 */
  let { target }: { target: string } = $props();

  let video: HTMLVideoElement;
  let lines = $state<string[]>([]);

  onMount(async () => {
    let report: unknown;
    try {
      await invoke("allow_video_file", { path: target });
      report = await runCheck(video, convertFileSrc(target), (line) => lines.push(line));
    } catch (e) {
      report = { error: String(e) };
    }
    await invoke("finish_check", { report: JSON.stringify(report, null, 2) });
  });
</script>

<!-- svelte-ignore a11y_media_has_caption -->
<video bind:this={video} controls muted crossorigin="anonymous"></video>
<pre>{lines.join("\n")}</pre>

<style>
  video {
    width: 100%;
    height: 70vh;
    background: #000;
  }
</style>
