<script lang="ts">
  import Icon from "./Icon.svelte";

  /**
   * 経路入力のときの、操作の手がかり。地図の左下に浮かせ、? キーで出し入れする。
   */
  let {
    shown,
    ontoggle,
    width = $bindable(0),
  }: {
    shown: boolean;
    ontoggle: () => void;
    /** 出している板かボタンの幅。地図の縮尺を、その右へ逃がす */
    width?: number;
  } = $props();

  /** 押し方と、それで起きること。押し方は、キーの形で並べて出す。 */
  const HINTS: [string[], string][] = [
    [["クリック"], "新たな経由地を追加"],
    [["線の上をクリック"], "途中に経由地を追加"],
    [["ドラッグ"], "経由地を移動"],
    [["右クリック"], "経由地を削除"],
    [["Shift", "クリック"], "連結方法を変更"],
    [["小さな丸をドラッグ"], "直線区間を曲げる"],
  ];
</script>

{#if shown}
  <section class="float" aria-label="操作方法" bind:clientWidth={width}>
    <header>
      <b>操作方法</b>
      <span class="space"></span>
      <button type="button" class="toggle" onclick={ontoggle} aria-label="操作方法を閉じる" title="閉じる">
        <kbd>?</kbd><Icon name="down" />
      </button>
    </header>
    <dl>
      {#each HINTS as [keys, effect] (effect)}
        <dt>{#each keys as key (key)}<kbd>{key}</kbd>{/each}</dt>
        <dd>{effect}</dd>
      {/each}
    </dl>
  </section>
{:else}
  <button type="button" class="btn float closed" onclick={ontoggle} title="操作方法を開く" bind:clientWidth={width}>
    <Icon name="help" />操作方法<kbd>?</kbd><Icon name="up" />
  </button>
{/if}

<style>
  section {
    width: 344px;
    padding: 10px 12px 12px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .space {
    flex: 1;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0;
    border: 0;
    background: none;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 6px 10px;
    margin: 0;
  }
  dt {
    display: flex;
    gap: 3px;
  }
  dd {
    margin: 0;
  }
  .closed {
    height: 30px;
    border-color: var(--line);
    border-radius: var(--radius-tile);
  }
</style>
