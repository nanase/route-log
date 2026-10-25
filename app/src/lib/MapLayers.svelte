<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { Basemap } from "./ui";

  /**
   * 背景地図と道路の線。拡大・縮小のボタンの下に置き、押したときだけ板を開く。経路入力と時刻入力の
   * どちらの画面でも使う。
   */
  let {
    basemap,
    hasRoads,
    showRoads,
    onbasemap,
    onroads,
    onsettings,
  }: {
    basemap: Basemap;
    /** 道路データを開いてあるか */
    hasRoads: boolean;
    showRoads: boolean;
    onbasemap: (basemap: Basemap) => void;
    onroads: (shown: boolean) => void;
    onsettings: () => void;
  } = $props();

  const BASEMAPS: { id: Basemap; name: string }[] = [
    { id: "pale", name: "淡色" },
    { id: "std", name: "標準" },
    { id: "photo", name: "写真" },
  ];

  let open = $state(false);
  let root: HTMLDivElement;

  /** 板の外を押すか Esc で閉じる */
  function closeOutside(event: PointerEvent) {
    if (open && !root.contains(event.target as Node)) open = false;
  }
  function closeOnEscape(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      open = false;
      event.stopPropagation();
    }
  }
</script>

<svelte:window onpointerdown={closeOutside} onkeydown={closeOnEscape} />

<div class="layers" bind:this={root}>
  <button
    type="button"
    class="toggle"
    class:on={open}
    aria-expanded={open}
    aria-label="地図の表示"
    title="地図の表示"
    onclick={() => (open = !open)}
  >
    <Icon name="layers" />
  </button>
  {#if open}
    <section class="float panel" aria-label="地図の表示">
      <div class="group">
        <b>背景地図</b>
        <div class="tiles">
          {#each BASEMAPS as item (item.id)}
            <button
              type="button"
              class="tile"
              class:on={basemap === item.id}
              aria-pressed={basemap === item.id}
              onclick={() => onbasemap(item.id)}
            >
              <span class="swatch {item.id}"></span>{item.name}
            </button>
          {/each}
        </div>
      </div>
      <div class="group">
        <label class:off={!hasRoads}>
          <input
            type="checkbox"
            checked={showRoads && hasRoads}
            disabled={!hasRoads}
            onchange={(event) => onroads(event.currentTarget.checked)}
          />道路の線を表示
        </label>
        {#if hasRoads}
          <div class="legend">
            <span class="band national"></span>国道
            <span class="band prefectural"></span>都道府県道
          </div>
        {:else}
          <span class="fine">
            道路データが必要です。
            <button type="button" class="link" onclick={onsettings}>設定を開く</button>
          </span>
        {/if}
      </div>
    </section>
  {/if}
</div>

<style>
  .layers {
    position: relative;
  }
  .toggle {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow-float);
  }
  .toggle:hover {
    background: var(--surface-2);
  }
  .toggle.on {
    color: var(--primary);
  }
  .panel {
    position: absolute;
    top: 0;
    right: calc(100% + 8px);
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: 224px;
    padding: 12px;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 6px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    padding: 4px 4px 5px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-tile);
    background: var(--surface);
    font-size: 12px;
  }
  .tile:hover:not(.on) {
    background: var(--surface-2);
  }
  /* 選んだボタンは枠が 1px 太い。そのぶん内側を詰めて、大きさを変えない。 */
  .tile.on {
    padding: 3px 3px 4px;
    border: 2px solid var(--primary);
    background: var(--primary-soft);
    color: var(--primary);
    font-weight: 700;
  }
  .swatch {
    width: 100%;
    height: 26px;
    border-radius: 4px;
  }
  /* 背景地図の見本。地の色に、道を 1 本通す。 */
  .swatch.pale {
    background: linear-gradient(160deg, #f7f6f1 46%, #b9bdc3 46% 54%, #f7f6f1 54%);
  }
  .swatch.std {
    background: linear-gradient(160deg, #e4efd4 46%, #f0b24a 46% 54%, #e4efd4 54%);
  }
  .swatch.photo {
    background: linear-gradient(160deg, #55684c 46%, #8d9483 46% 54%, #3f5140 54%);
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  label.off {
    color: var(--ink-4);
  }
  label input {
    width: 14px;
    height: 14px;
    margin: 0;
  }
  .legend {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-3);
    font-size: 12px;
  }
  .band {
    width: 22px;
    height: 5px;
    border-radius: 3px;
  }
  .band.national {
    background: var(--road-national);
    opacity: 0.45;
  }
  .band.prefectural {
    margin-left: 8px;
    background: var(--road-prefectural);
    opacity: 0.5;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--primary);
    text-decoration: underline;
  }
</style>
