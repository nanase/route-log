<script lang="ts">
  import { shownName } from "./names";
  import RouteBadge from "./RouteBadge.svelte";
  import type { RideEvent } from "./types";

  /**
   * イベントの名前。路線のイベントでは、名前の前に標識を置く。名前の字も残す。橋・トンネルは、
   * 公称の名前と長さを添える。
   */
  let { event }: { event: RideEvent } = $props();

  const routes = $derived(event.kind === "road" ? (event.detail.routes ?? []) : []);
  // 抽出した名前は、路線の名前を「・」で繋いである。数が合えば、標識を 1 つずつ前に置く。
  const parts = $derived(event.label.split("・"));
  const paired = $derived(routes.length > 0 && parts.length === routes.length);
</script>

{#if paired}
  <span class="name">
    {#each routes as route, i (i)}
      {#if i > 0}<span class="dot">・</span>{/if}
      <RouteBadge {route} />{parts[i]}
    {/each}
  </span>
{:else}
  <span class="name">
    {#each routes as route, i (i)}<RouteBadge {route} />{/each}
    {shownName(event) || "（名前なし）"}
  </span>
{/if}

<style>
  .name {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    color: var(--ink-4);
  }
</style>
