<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { Notice } from "./ui";

  /**
   * 上の帯のすぐ下に出す知らせ。エラーは赤、注意は橙、済んだことは緑の帯で、直すか閉じるまで残る。
   */
  let { notices, onclose }: { notices: Notice[]; onclose: (key: string) => void } = $props();
</script>

{#each notices as notice (notice.key)}
  <div class="notice float {notice.level}" role={notice.level === "error" ? "alert" : "status"}>
    <Icon name={notice.level === "done" ? "check" : notice.level} />
    <span class="text"><b>{notice.title}</b>{notice.detail}</span>
    {#if notice.action}
      <button type="button" class="btn" onclick={notice.action.run}>{notice.action.label}</button>
    {/if}
    <button type="button" class="btn icon-only ghost" aria-label="閉じる" onclick={() => onclose(notice.key)}>
      <Icon name="close" />
    </button>
  </div>
{/each}

<style>
  .notice {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
    padding: 6px 8px 6px 12px;
    border-radius: var(--radius-control);
  }
  .error {
    border-color: var(--danger-line);
    background: var(--danger-soft);
    color: var(--danger-ink);
  }
  .warning {
    border-color: var(--warn-line);
    background: var(--warn-soft);
    color: var(--warn-text);
  }
  .done {
    border-color: var(--ok);
    background: var(--ok-soft);
    color: var(--ok-ink);
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .btn:not(.ghost) {
    color: var(--ink);
  }
</style>
