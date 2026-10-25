<script lang="ts">
  import LabelTag from "./LabelTag.svelte";
  import { labelName, withLabel, withoutLabel } from "./labels";

  /**
   * 走行のラベルを付け外す欄。打って Enter で付け、札の × で外す。
   *
   * 前に使ったラベルを候補に出す。候補に無い名前は、新しいラベルとして付ける。
   */
  let {
    labels,
    known,
    onchange,
  }: {
    /** この走行に付いているラベル */
    labels: string[];
    /** どれかの走行に付いているラベルと、付いている走行の数。多い順 */
    known: { name: string; count: number }[];
    onchange: (labels: string[]) => void;
  } = $props();

  let draft = $state("");
  let editing = $state(false);
  /** 候補のうち、Enter で付けるもの */
  let active = $state(0);
  let input: HTMLInputElement;

  const options = $derived.by(() => {
    const typed = labelName(draft);
    const found = known
      .filter(({ name }) => !labels.includes(name) && name.includes(typed))
      .map(({ name, count }) => ({ name, count, fresh: false }));
    const fresh = typed !== "" && !labels.includes(typed) && !known.some(({ name }) => name === typed);
    return fresh ? [...found, { name: typed, count: 0, fresh: true }] : found;
  });

  $effect(() => {
    if (active >= options.length) active = Math.max(options.length - 1, 0);
  });

  function add(name: string) {
    const next = withLabel(labels, name);
    if (next !== labels) onchange(next);
    draft = "";
    active = 0;
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      add(options[active]?.name ?? draft);
    } else if (event.key === "ArrowDown" && options.length > 0) {
      event.preventDefault();
      active = (active + 1) % options.length;
    } else if (event.key === "ArrowUp" && options.length > 0) {
      event.preventDefault();
      active = (active - 1 + options.length) % options.length;
    } else if (event.key === "Backspace" && draft === "" && labels.length > 0) {
      onchange(labels.slice(0, -1));
    } else if (event.key === "Escape") {
      // 走行の一覧ごと閉じないよう、欄を離れるだけにする。
      event.preventDefault();
      event.stopPropagation();
      draft = "";
      input.blur();
    }
  }
</script>

<!-- 欄の中のどこを押しても、字を打てるようにする。 -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="editor" onclick={() => input.focus()}>
  {#each labels as name (name)}
    <LabelTag {name} onremove={() => onchange(withoutLabel(labels, name))} />
  {/each}
  <input
    bind:this={input}
    bind:value={draft}
    aria-label="ラベルを付ける"
    placeholder={labels.length === 0 ? "ラベルを付ける（例：投稿済）" : ""}
    autocomplete="off"
    {onkeydown}
    oninput={() => (active = 0)}
    onfocus={() => (editing = true)}
    onblur={() => (editing = false)}
  />
  {#if editing && options.length > 0}
    <!-- 候補を押す間に、欄からフォーカスが外れて候補が消えないようにする。 -->
    <div class="suggest" role="listbox" tabindex="-1" onmousedown={(event) => event.preventDefault()}>
      {#each options as option, index (option.name)}
        <button
          type="button"
          role="option"
          aria-selected={index === active}
          class:active={index === active}
          onclick={() => add(option.name)}
        >
          {#if option.fresh}
            <span class="fresh">「{option.name}」を新規追加</span>
          {:else}
            <LabelTag name={option.name} />
            <span class="count mono">{option.count} 件</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .editor {
    position: relative;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    box-sizing: border-box;
    min-height: var(--control);
    padding: 3px 6px;
    border: 1px solid var(--line-input);
    border-radius: var(--radius-control);
    background: var(--surface);
    cursor: text;
  }
  .editor:focus-within {
    outline: 2px solid var(--primary);
    outline-offset: 1px;
  }
  input {
    flex: 1;
    min-width: 120px;
    height: 20px;
    padding: 0 2px;
    border: 0;
    outline: none;
    background: none;
  }
  .suggest {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 2;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    max-height: 240px;
    overflow-y: auto;
    padding: 4px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    cursor: default;
  }
  .suggest button {
    display: flex;
    flex: none;
    align-items: center;
    gap: 8px;
    height: var(--control);
    padding: 0 8px;
    border: 0;
    border-radius: 5px;
    background: none;
    text-align: left;
  }
  .suggest button:hover,
  .suggest button.active {
    background: var(--surface-2);
  }
  .count {
    margin-left: auto;
    color: var(--ink-3);
    font-size: 11px;
  }
  .fresh {
    color: var(--primary-ink);
  }
</style>
