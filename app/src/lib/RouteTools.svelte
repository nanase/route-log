<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { Basemap } from "./ui";

  /**
   * 地図の道具。経路を入れるときに、地図の左上に浮かせる。
   */
  let {
    canUndo,
    canRedo,
    snap,
    canSnap,
    basemap,
    hasRoads,
    showRoads,
    canFit,
    onundo,
    onredo,
    onsnap,
    onbasemap,
    onroads,
    onfit,
    onsettings,
  }: {
    canUndo: boolean;
    canRedo: boolean;
    /** これから足す区間を、道に沿わせるか */
    snap: boolean;
    /** 道に沿わせることができるか。判定用データを開いていなければ、できない */
    canSnap: boolean;
    basemap: Basemap;
    /** 道路データを開いてあるか */
    hasRoads: boolean;
    showRoads: boolean;
    /** 経路があるか */
    canFit: boolean;
    onundo: () => void;
    onredo: () => void;
    onsnap: (snap: boolean) => void;
    onbasemap: (basemap: Basemap) => void;
    onroads: (shown: boolean) => void;
    onfit: () => void;
    onsettings: () => void;
  } = $props();

  const BASEMAPS: { id: Basemap; name: string }[] = [
    { id: "pale", name: "淡色" },
    { id: "std", name: "標準" },
    { id: "photo", name: "写真" },
  ];
</script>

<section class="float" aria-label="地図の道具">
  <div class="group">
    <button type="button" class="btn wide" onclick={onundo} disabled={!canUndo}>
      <Icon name="undo" />取り消し<span class="space"></span><kbd>Ctrl</kbd><kbd>Z</kbd>
    </button>
    <button type="button" class="btn wide" onclick={onredo} disabled={!canRedo}>
      <Icon name="redo" />やり直し<span class="space"></span><kbd>Ctrl</kbd><kbd>Y</kbd>
    </button>
  </div>

  <div class="group">
    <b>点の結び方</b>
    <div class="tiles two">
      <button
        type="button"
        class="tile join"
        class:on={snap}
        aria-pressed={snap}
        disabled={!canSnap}
        onclick={() => onsnap(true)}
      >
        <svg viewBox="0 0 44 14" width="44" height="14" aria-hidden="true">
          <path d="M3 10C12 10 12 3 22 4S32 11 41 6" />
        </svg>
        道に沿わせる
      </button>
      <button type="button" class="tile join" class:on={!snap} aria-pressed={!snap} onclick={() => onsnap(false)}>
        <svg viewBox="0 0 44 14" width="44" height="14" aria-hidden="true">
          <path d="M3 10L41 4" stroke-dasharray="7 5" />
        </svg>
        直線
      </button>
    </div>
    {#if canSnap}
      <span class="fine">これから足す区間に使います</span>
    {:else}
      <span class="fine">
        「道に沿わせる」は、判定データを選ぶと使えます。
        <button type="button" class="link" onclick={onsettings}>設定を開く</button>
      </span>
    {/if}
  </div>

  <div class="group">
    <b>背景地図</b>
    <div class="tiles three">
      {#each BASEMAPS as item (item.id)}
        <button
          type="button"
          class="tile base"
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
      />道路の線を重ねる
    </label>
    {#if hasRoads}
      <div class="legend">
        <span class="band national"></span>国道
        <span class="band prefectural"></span>都道府県道
      </div>
    {:else}
      <span class="fine">
        道路データを選ぶと重なります。
        <button type="button" class="link" onclick={onsettings}>設定を開く</button>
      </span>
    {/if}
  </div>

  <button type="button" class="btn fit" onclick={onfit} disabled={!canFit}>
    <Icon name="fit" />経路の全体を表示
  </button>
</section>

<style>
  section {
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
  .wide {
    justify-content: flex-start;
  }
  .space {
    flex: 1;
  }
  .fit {
    justify-content: center;
  }
  .tiles {
    display: grid;
    gap: 6px;
  }
  .two {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .three {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
  .tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-tile);
    background: var(--surface);
  }
  .tile:hover:not(:disabled):not(.on) {
    background: var(--surface-2);
  }
  .tile.on {
    border: 2px solid var(--primary);
    background: var(--primary-soft);
    color: var(--primary);
    font-weight: 700;
  }
  .tile:disabled {
    color: var(--ink-4);
    opacity: 0.6;
  }
  .join {
    justify-content: center;
    gap: 6px;
    height: 56px;
    padding: 0;
  }
  .join svg {
    fill: none;
    stroke: var(--route);
    stroke-width: 3;
    stroke-linecap: round;
  }
  .base {
    gap: 3px;
    padding: 4px 4px 5px;
    font-size: 12px;
  }
  /* 選んだボタンは枠が 1px 太い。そのぶん内側を詰めて、大きさを変えない。 */
  .base.on {
    padding: 3px 3px 4px;
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
