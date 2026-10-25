<script lang="ts">
  import { labelColor } from "./labels";

  /**
   * 走行のラベルの札。色は名前から決まる。同期の状態の丸い札と見分けるため、角を小さくする。
   */
  let {
    name,
    onremove,
  }: {
    name: string;
    /** あれば、札に外すボタンを付ける */
    onremove?: () => void;
  } = $props();

  const color = $derived(labelColor(name));
</script>

<span class="tag" style:--soft="var(--label-{color}-soft)" style:--ink="var(--label-{color}-ink)">
  {name}
  {#if onremove}
    <button type="button" class="drop" aria-label="ラベル「{name}」を外す" title="ラベルを外す" onclick={onremove}>×</button>
  {/if}
</span>

<style>
  .tag {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 4px;
    box-sizing: border-box;
    height: 20px;
    padding: 0 7px;
    border-radius: 4px;
    background: var(--soft);
    color: var(--ink);
    font-size: 12px;
    font-weight: 400;
    white-space: nowrap;
  }
  .drop {
    display: inline-grid;
    place-items: center;
    width: 14px;
    height: 14px;
    margin-right: -3px;
    padding: 0;
    border: 0;
    border-radius: 3px;
    background: none;
    color: inherit;
    font-size: 13px;
    line-height: 1;
  }
  .drop:hover {
    background: rgba(26, 29, 33, 0.12);
  }
</style>
