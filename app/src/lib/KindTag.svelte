<script lang="ts">
  import { KIND_MARKS, KINDS } from "./kinds";
  import type { EventKind } from "./types";

  /** イベントの種類を、漢字 1 字の札で出す。 */
  let {
    kind,
    faded = false,
  }: {
    kind: EventKind;
    /** 字幕なしのものは薄くする */
    faded?: boolean;
  } = $props();

  const mark = $derived(KIND_MARKS[kind]);
</script>

<span class="kind" class:outline={mark.outline} class:faded style:--kind="var({mark.color})" title={KINDS[kind]}>
  {mark.mark}
</span>

<style>
  .kind {
    display: inline-grid;
    flex: none;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 4px;
    background: var(--kind);
    color: #fff;
    font: 700 11px/1 var(--font-ui);
  }
  .kind.outline {
    background: var(--surface);
    box-shadow: inset 0 0 0 1.5px var(--kind);
    color: var(--kind);
  }
  .kind.faded {
    opacity: 0.45;
  }
</style>
