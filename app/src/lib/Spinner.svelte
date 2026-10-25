<script lang="ts">
  /**
   * 動いている最中であることを示す、回る輪。色は置いた場所の字に合わせる。
   */
  let { size = 12 }: { size?: number } = $props();
</script>

<span class="spinner" style:--size="{size}px" aria-hidden="true"></span>

<style>
  .spinner {
    display: inline-block;
    flex: none;
    box-sizing: border-box;
    width: var(--size);
    height: var(--size);
    border: 2px solid color-mix(in srgb, currentColor 25%, transparent);
    border-top-color: currentColor;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  /* 動きを減らす設定でも、止まって見えないよう、ゆっくり回す。 */
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 2.4s;
    }
  }
</style>
