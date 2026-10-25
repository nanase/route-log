<script lang="ts">
  import Icon from "./Icon.svelte";

  /**
   * 地図の道具。経路入力のときに、地図の左上に浮かせる。
   */
  let {
    canUndo,
    canRedo,
    snap,
    canSnap,
    canFit,
    onundo,
    onredo,
    onsnap,
    onfit,
    onsettings,
  }: {
    canUndo: boolean;
    canRedo: boolean;
    /** これから足す区間を、道に沿わせるか */
    snap: boolean;
    /** 道に沿わせることができるか。判定用データを開いていなければ、できない */
    canSnap: boolean;
    /** 経路があるか */
    canFit: boolean;
    onundo: () => void;
    onredo: () => void;
    onsnap: (snap: boolean) => void;
    onfit: () => void;
    onsettings: () => void;
  } = $props();
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
    <b>連結方法</b>
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
    {#if !canSnap}
      <span class="fine">
        「道に沿わせる」には判定データが必要です。
        <button type="button" class="link" onclick={onsettings}>設定を開く</button>
      </span>
    {/if}
  </div>

  <button type="button" class="btn fit" onclick={onfit} disabled={!canFit}>
    <Icon name="fit" />全経路表示
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
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--primary);
    text-decoration: underline;
  }
</style>
