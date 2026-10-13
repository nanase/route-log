<script lang="ts">
  import type { RouteRef } from "./types";

  /**
   * 路線の標識。青地に白の数字で、国道は逆三角（おにぎり形）、都道府県道は六角形にする。
   */
  let { route }: { route: RouteRef } = $props();

  const national = $derived(route.class === "national");
  const text = $derived(String(route.number));
  // 3 桁からは字を小さくして、枠に収める。
  const small = $derived(text.length >= 3);
  const baseline = $derived(national ? (small ? 10 : 10.5) : small ? 12.5 : 13);
</script>

<svg
  viewBox="0 0 22 20"
  width="20"
  height="18"
  role="img"
  aria-label="{national ? '国道' : '都道府県道'}の標識 {text}"
>
  {#if national}
    <path d="M3.2 1.5H18.8Q21.6 1.5 20.2 4L12.7 17.4Q11 20.2 9.3 17.4L1.8 4Q.4 1.5 3.2 1.5Z" />
  {:else}
    <path d="M6.5 1.5H15.5L20.6 10L15.5 18.5H6.5L1.4 10Z" />
  {/if}
  <text x="11" y={baseline} class:small>{text}</text>
</svg>

<style>
  svg {
    flex: none;
  }
  path {
    fill: var(--kind-road);
  }
  text {
    fill: #fff;
    font: 700 8.5px var(--font-mono);
    text-anchor: middle;
  }
  text.small {
    font-size: 6.8px;
  }
</style>
