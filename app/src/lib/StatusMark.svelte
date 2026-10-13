<script lang="ts">
  import type { EventStatus } from "./types";

  /**
   * イベントの状態の印。色だけに頼らず、形でも分ける。確定は塗りの丸、推定は破線の輪、
   * 字幕にしないものは斜線の輪にする。
   */
  let { status }: { status: EventStatus } = $props();

  const NAMES: Record<EventStatus, string> = { confirmed: "確定", estimated: "推定", hidden: "字幕にしない" };
</script>

<span class="status {status}" role="img" aria-label={NAMES[status]} title={NAMES[status]}></span>

<style>
  .status {
    display: inline-block;
    flex: none;
    box-sizing: border-box;
    width: 11px;
    height: 11px;
    border-radius: 50%;
  }
  .confirmed {
    background: var(--ok);
  }
  .estimated {
    border: 1.5px dashed var(--ink-4);
  }
  .hidden {
    border: 1.5px solid var(--mark-muted);
    background: linear-gradient(135deg, transparent 42%, var(--mark-muted) 42% 58%, transparent 58%);
  }
</style>
