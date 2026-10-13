<script lang="ts">
  import type { EventStatus } from "./types";

  /**
   * イベントの状態の印。色だけに頼らず、形でも分ける。確定は塗りの丸、推定は破線の輪にする。
   * 字幕なしは斜線の輪で、時刻を確定してあれば、輪と斜線を確定の色にする。
   */
  let { status, hidden = false }: { status: EventStatus; hidden?: boolean } = $props();

  const NAMES: Record<EventStatus, string> = { confirmed: "確定", estimated: "推定" };
  const name = $derived(hidden ? (status === "confirmed" ? "字幕なし（時刻は確定）" : "字幕なし") : NAMES[status]);
</script>

<span class="status {status}" class:hidden role="img" aria-label={name} title={name}></span>

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
    --mark: var(--mark-muted);
    border: 1.5px solid var(--mark);
    background: linear-gradient(135deg, transparent 42%, var(--mark) 42% 58%, transparent 58%);
  }
  .hidden.confirmed {
    --mark: var(--ok);
  }
</style>
