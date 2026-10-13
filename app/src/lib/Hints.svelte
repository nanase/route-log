<script lang="ts">
  import Icon from "./Icon.svelte";

  /**
   * 経路を入れるときの、操作の手がかり。地図の左下に浮かせ、? キーで出し入れする。
   */
  let { shown, ontoggle }: { shown: boolean; ontoggle: () => void } = $props();

  /** 押し方と、それで起きること。押し方は、キーの形で並べて出す。 */
  const HINTS: [string[], string][] = [
    [["クリック"], "終わりに点を足す"],
    [["線の上をクリック"], "途中に点を足す"],
    [["ドラッグ"], "点を動かす"],
    [["右クリック"], "点を消す"],
    [["Shift", "クリック"], "次の点までの結び方を切り替える"],
    [["小さな丸をドラッグ"], "直線の区間を曲げる"],
  ];
</script>

{#if shown}
  <section class="float" aria-label="操作の手がかり">
    <header>
      <b>操作</b>
      <span class="space"></span>
      <button type="button" class="toggle" onclick={ontoggle} title="操作の手がかりをしまう">
        <span class="fine">出し入れ</span><kbd>?</kbd>
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
  <button type="button" class="btn float closed" onclick={ontoggle} title="操作の手がかりを出す">
    <Icon name="help" />操作<kbd>?</kbd>
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
