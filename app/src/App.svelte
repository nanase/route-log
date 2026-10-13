<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import PlaybackCheck from "./lib/PlaybackCheck.svelte";
  import Player from "./lib/Player.svelte";
  import { hms } from "./lib/time";
  import { runTimelineCheck } from "./lib/timeline-check";
  import type { RideVideos, Scan, Timeline } from "./lib/types";

  /** 自動の確認の対象。`undefined` は問い合わせ中 */
  let check = $state<{ file: string | null; dir: string | null } | undefined>(undefined);
  let player = $state<Player>();
  let dir = $state("");
  let message = $state("");
  let rides = $state<RideVideos[]>([]);
  let selected = $state<RideVideos | null>(null);
  let clockOffsetSeconds = $state(0);
  let timeline = $state<Timeline | null>(null);

  onMount(async () => {
    check = await invoke("check_target");
    if (check?.dir) await checkTimeline(check.dir);
  });

  /** フォルダを読み、最初の走行でファイルをまたぐ再生を確かめて、報告を書いて終了する。 */
  async function checkTimeline(target: string) {
    let report: unknown;
    try {
      await load(target);
      while (timeline === null || player === undefined) await new Promise((r) => setTimeout(r, 100));
      report = await runTimelineCheck(timeline, player, async (seconds) => {
        clockOffsetSeconds = seconds;
      });
    } catch (e) {
      report = { error: String(e), message };
    }
    await invoke("finish_check", { report: JSON.stringify(report, null, 2) });
  }

  async function openFolder() {
    const picked = await open({ directory: true, title: "動画フォルダを選ぶ" });
    if (picked !== null) await load(picked);
  }

  async function load(picked: string) {
    dir = picked;
    message = "読み込み中…";
    rides = [];
    selected = null;
    try {
      const scan = await invoke<Scan>("scan_videos", { dir });
      rides = scan.rides;
      selected = rides[0] ?? null;
      const count = rides.reduce((n, ride) => n + ride.files.length, 0);
      message = `${count} 本`;
      if (scan.skipped.length > 0) {
        message += `、読めないファイル ${scan.skipped.length} 本（${scan.skipped[0].reason}）`;
      }
    } catch (e) {
      message = String(e);
    }
  }

  $effect(() => {
    const ride = selected;
    const clockOffsetMs = Math.round((clockOffsetSeconds || 0) * 1000);
    if (ride === null) {
      timeline = null;
      return;
    }
    void invoke<Timeline>("build_timeline", { files: ride.files, clockOffsetMs }).then((built) => {
      timeline = built;
    });
  });

  const totalMs = (ride: RideVideos) => ride.files.reduce((sum, file) => sum + file.duration_ms, 0);
</script>

{#if check?.file}
  <PlaybackCheck target={check.file} />
{:else if check !== undefined}
  <main>
    <header>
      <button type="button" onclick={openFolder}>動画フォルダを開く</button>
      <span class="dir">{dir}</span>
      <span>{message}</span>
      <label>
        時計の補正（秒）
        <input type="number" step="1" bind:value={clockOffsetSeconds} />
      </label>
    </header>
    <nav>
      {#each rides as ride (ride.date)}
        <button type="button" class:selected={ride === selected} onclick={() => (selected = ride)}>
          <strong>{ride.date}</strong>
          <span>{ride.files.length} 本・{hms(totalMs(ride))}</span>
        </button>
      {/each}
    </nav>
    <div class="player">
      {#if timeline !== null}
        <Player bind:this={player} {timeline} muted={check?.dir != null} />
      {/if}
    </div>
  </main>
{/if}

<style>
  :global(body) {
    margin: 0;
    font-family: system-ui, sans-serif;
  }
  main {
    display: grid;
    grid-template: auto minmax(0, 1fr) / 12rem minmax(0, 1fr);
    gap: 0.5rem;
    height: 100vh;
    box-sizing: border-box;
    padding: 0.5rem;
  }
  header {
    grid-column: 1 / -1;
    display: flex;
    gap: 0.75rem;
    align-items: center;
    font-size: 0.9rem;
  }
  .dir {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #555;
  }
  label input {
    width: 5rem;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    overflow: auto;
  }
  nav button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 0.4rem 0.5rem;
    border: 1px solid #ccc;
    background: #fff;
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  nav button.selected {
    border-color: #1f4e79;
    background: #e6eef7;
  }
  .player {
    min-height: 0;
  }
</style>
