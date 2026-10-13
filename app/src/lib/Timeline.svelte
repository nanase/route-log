<script lang="ts">
  import Icon from "./Icon.svelte";
  import { KINDS } from "./kinds";
  import KindTag from "./KindTag.svelte";
  import { clock, short } from "./time";
  import type { Gap, RideEvent, Timeline } from "./types";
  import type { ExcludedClip } from "./ui";

  /**
   * タイムライン。動画ファイルの範囲、録画停止、イベントの印、再生位置を、実時刻の軸に並べる。
   *
   * 上の細い帯は全体で、太い帯はその一部を広げたものである。細い帯の枠が、広げている範囲を示す。
   *
   * 動画の帯を右クリックすると、その動画を走行から外せる。外した動画は薄く残り、押すと戻せる。
   */
  let {
    timeline,
    excluded,
    events,
    conflicts,
    nowMs,
    targetId,
    onseek,
    onselect,
    onexclude,
  }: {
    /** 開いている走行に使う動画。無ければ null */
    timeline: Timeline | null;
    /** 走行から外している動画 */
    excluded: ExcludedClip[];
    events: RideEvent[];
    /** 位置が先なのに時刻が後になっている、確定済みのイベントの ID */
    conflicts: string[];
    /** 動画の再生位置の実時刻 */
    nowMs: number;
    /** 次に確定する相手 */
    targetId: string | null;
    onseek: (realMs: number) => void;
    onselect: (event: RideEvent) => void;
    /** 動画を走行から外す。`excluded` が偽なら、外した動画を戻す */
    onexclude: (name: string, excluded: boolean) => void;
  } = $props();

  const JST_MS = 9 * 3_600_000;
  /** これより狭くは広げない */
  const MIN_SPAN_MS = 60_000;
  /** 目盛りの間隔の候補（ms）。目盛りが多すぎない、いちばん細かいものを使う */
  const STEPS_MS = [10, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600].map((s) => s * 1000);
  /** 目盛りの数字 1 つに要る幅（px） */
  const TICK_PX = 110;
  /** 名前を書ける、動画の帯と録画停止の幅（px） */
  const CLIP_LABEL_PX = 48;
  const GAP_LABEL_PX = 62;
  /** 動画 1 本の操作を出す板の幅（px） */
  const MENU_PX = 300;

  /** 時刻の軸を置く箱。幅が、軸の長さになる */
  let box: HTMLDivElement;
  let width = $state(0);
  /** 広げている範囲。全体を出しているときは null */
  let view = $state<{ start: number; end: number } | null>(null);
  /** 右クリックで開いた、動画 1 本の操作の板。開いていなければ null */
  let menu = $state<{ clip: ExcludedClip; label: string; out: boolean; left: number; bottom: number } | null>(null);
  let menuElement = $state<HTMLDivElement>();

  const spans = $derived(timeline?.spans ?? []);
  const gaps = $derived(timeline?.gaps ?? []);
  /** 軸に並べるものがあるか。走行に使う動画が無くても、外した動画があれば、戻せるように並べる */
  const shown = $derived(spans.length > 0 || excluded.length > 0);
  // 軸の両端。外した動画の範囲も含める。
  const startMs = $derived(
    shown ? Math.min(...spans.map((clip) => clip.start_ms), ...excluded.map((clip) => clip.start_ms)) : 0,
  );
  const endMs = $derived(
    shown ? Math.max(...spans.map((clip) => clip.end_ms), ...excluded.map((clip) => clip.end_ms)) : 0,
  );
  const from = $derived(view?.start ?? startMs);
  const to = $derived(view?.end ?? endMs);
  const span = $derived(Math.max(to - from, 1));
  /** 時刻を、太い帯の上の位置（px）にする */
  const x = (ms: number) => ((ms - from) / span) * width;
  /** 時刻を、細い帯の上の位置（px）にする */
  const overviewX = (ms: number) => ((ms - startMs) / Math.max(endMs - startMs, 1)) * width;

  const ticks = $derived.by(() => {
    if (width === 0 || !shown) return [];
    const step = STEPS_MS.find((ms) => span / ms <= width / TICK_PX) ?? STEPS_MS[STEPS_MS.length - 1];
    // 日本時間の、きりのよい時刻に目盛りを置く。
    const first = Math.ceil((from + JST_MS) / step) * step - JST_MS;
    const list: { ms: number; label: string }[] = [];
    for (let ms = first; ms <= to; ms += step) {
      list.push({ ms, label: step < 60_000 ? clock(ms) : clock(ms).slice(0, 5) });
    }
    return list;
  });

  const inGap = (time: number, list: Gap[]) => list.some((gap) => gap.start_ms <= time && time < gap.end_ms);
  const marks = $derived(
    events.flatMap((event) => {
      const time = event.time_ms;
      if (time === null || time < startMs || time > endMs) return [];
      const warned = conflicts.includes(event.id) || (event.status === "estimated" && inGap(time, gaps));
      return [{ event, time, warned }];
    }),
  );

  /** 録画停止のうち、外した動画の帯に隠れない所。「停止」と長さは、ここに書く */
  const stops = $derived.by(() => {
    let rest: Gap[] = gaps;
    for (const clip of excluded) {
      rest = rest.flatMap((gap) =>
        [
          { start_ms: gap.start_ms, end_ms: Math.min(gap.end_ms, clip.start_ms) },
          { start_ms: Math.max(gap.start_ms, clip.end_ms), end_ms: gap.end_ms },
        ].filter((part) => part.end_ms > part.start_ms),
      );
    }
    return rest;
  });

  // 走行や時計の補正が変わって軸が動いたら、全体に戻す。
  $effect(() => {
    void [startMs, endMs];
    view = null;
  });

  // 広げているときに再生位置が範囲を出たら、範囲を送る。再生位置を、左から 1 割の所に置く。
  // 走行に使う動画が無ければ、再生位置も無いので、送らない。
  $effect(() => {
    if (timeline === null || view === null || (nowMs >= view.start && nowMs <= view.end)) return;
    const length = view.end - view.start;
    show(nowMs - length * 0.1, length);
  });

  // 走行を切り替えたり、動画を外したり戻したりして帯が変わったら、開いている板は閉じる。
  $effect(() => {
    void [timeline, excluded];
    menu = null;
  });

  /** `start` から `length` の範囲を広げて出す。全体からはみ出さないようにする。 */
  function show(start: number, length: number) {
    const whole = endMs - startMs;
    if (length >= whole) {
      view = null;
      return;
    }
    const clamped = Math.min(Math.max(start, startMs), endMs - length);
    view = { start: clamped, end: clamped + length };
  }

  /** `factor` 倍の長さにする。`around` の時刻の、画面の上での位置を保つ。 */
  function zoom(factor: number, around = nowMs) {
    const center = Math.min(Math.max(around, from), to);
    const length = Math.max(span * factor, Math.min(MIN_SPAN_MS, endMs - startMs));
    show(center - ((center - from) / span) * length, length);
  }

  /** 押している間、その位置へ再生位置を動かす。 */
  function scrub(event: PointerEvent, toMs: (px: number) => number) {
    // 右のボタンは、動画 1 本の操作を出すのに使う。
    if (event.button !== 0) return;
    const track = event.currentTarget as HTMLElement;
    const seekAt = (e: PointerEvent) => onseek(toMs(e.clientX - track.getBoundingClientRect().left));
    track.setPointerCapture(event.pointerId);
    seekAt(event);
    const stop = () => {
      track.removeEventListener("pointermove", seekAt);
      track.removeEventListener("pointerup", stop);
      track.removeEventListener("pointercancel", stop);
    };
    track.addEventListener("pointermove", seekAt);
    track.addEventListener("pointerup", stop);
    track.addEventListener("pointercancel", stop);
  }

  /** 動画 1 本の操作の板を、その帯のすぐ上に出す。 */
  function openMenu(event: MouseEvent, clip: ExcludedClip, label: string, out: boolean) {
    event.preventDefault();
    const band = (event.currentTarget as HTMLElement).getBoundingClientRect();
    menu = {
      clip,
      label,
      out,
      left: Math.max(Math.min(event.clientX - 24, window.innerWidth - MENU_PX - 12), 12),
      bottom: window.innerHeight - band.top + 6,
    };
  }

  function chooseMenu() {
    if (menu === null) return;
    onexclude(menu.clip.name, !menu.out);
    menu = null;
  }

  /** 板の外を押すか、Esc を押したら、板を閉じる。 */
  function onpointerdown(event: PointerEvent) {
    if (menu !== null && !(event.target instanceof Node && menuElement?.contains(event.target))) menu = null;
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape") menu = null;
  }

  function onwheel(event: WheelEvent) {
    if (!shown) return;
    menu = null;
    event.preventDefault();
    const at = from + ((event.clientX - box.getBoundingClientRect().left) / Math.max(width, 1)) * span;
    zoom(event.deltaY < 0 ? 0.5 : 2, at);
  }

  /** いまの状態。自動の確認が読む。 */
  export const snapshot = () => ({
    view: view === null ? "全体" : `${clock(view.start)}-${clock(view.end)}`,
    clips: spans.length,
    excluded: excluded.map((clip) => `${clock(clip.start_ms)}-${clock(clip.end_ms)}`),
    stops: stops.map((gap) => `${clock(gap.start_ms)}-${clock(gap.end_ms)}`),
    menu: menu === null ? null : `${menu.label} ${menu.out ? "走行に戻す" : "走行から外す"}`,
    marks: marks.map((mark) => `${clock(mark.time)} ${mark.event.kind}${mark.warned ? " 警告" : ""}`),
    head: clock(nowMs),
  });
  export const zoomIn = () => zoom(0.5);
  export const zoomOut = () => zoom(2);

  /** 自動の確認が使う口。その名前の動画の帯を右クリックしたことにする。帯が出ていなければ false。 */
  export function rightClick(name: string): boolean {
    const bands = [...box.querySelectorAll<HTMLElement>("[data-file]")];
    const band = bands.find((element) => element.dataset.file === name);
    if (band === undefined) return false;
    const at = band.getBoundingClientRect();
    const [clientX, clientY] = [at.left + at.width / 2, at.top + at.height / 2];
    band.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX, clientY, button: 2 }));
    return true;
  }

  /** 開いている板のボタンを押したことにする。 */
  export function pressMenu() {
    menuElement?.querySelector("button")?.click();
  }
</script>

<svelte:window {onpointerdown} {onkeydown} />

<section>
  <div class="side">
    <b>タイムライン</b>
    <span class="mono fine">
      {#if shown}{clock(from).slice(0, 5)} – {clock(to).slice(0, 5)}{/if}
    </span>
    <div class="buttons">
      <button
        type="button"
        class="btn small icon-only"
        aria-label="縮める"
        title="広い範囲を出す"
        disabled={view === null}
        onclick={() => zoom(2)}
      >
        <Icon name="minus" />
      </button>
      <button
        type="button"
        class="btn small icon-only"
        aria-label="広げる"
        title="再生位置のまわりを広げる"
        disabled={!shown || span <= MIN_SPAN_MS}
        onclick={() => zoom(0.5)}
      >
        <Icon name="plus" />
      </button>
      <button type="button" class="btn small" disabled={view === null} onclick={() => (view = null)}>全体</button>
    </div>
  </div>

  <div class="lane" {onwheel}>
    {#if !shown}
      <p class="empty">
        この日付の動画はありません。時刻は、イベントの一覧の行を押して手で入れられます。<br />
        設定で動画フォルダを選ぶと、ここに動画とイベントが並びます。
      </p>
    {/if}
    <!-- 軸の両端に余白を取る。端にあるイベントの札が切れない。 -->
    <div class="axis-box" bind:this={box} bind:clientWidth={width}>
    {#if shown}
      <!-- 全体。枠が、下の太い帯に広げている範囲を示す。 -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="overview" onpointerdown={(event) => scrub(event, (px) => startMs + (px / width) * (endMs - startMs))}>
        {#each spans as clip (clip.file)}
          <div
            class="piece"
            style:left="{overviewX(clip.start_ms)}px"
            style:width="{Math.max(overviewX(clip.end_ms) - overviewX(clip.start_ms) - 1, 1)}px"
          ></div>
        {/each}
        {#each excluded as clip (clip.name)}
          <div
            class="piece out"
            style:left="{overviewX(clip.start_ms)}px"
            style:width="{Math.max(overviewX(clip.end_ms) - overviewX(clip.start_ms) - 1, 1)}px"
          ></div>
        {/each}
        <div
          class="window"
          style:left="{overviewX(from)}px"
          style:width="{Math.max(overviewX(to) - overviewX(from), 6)}px"
        ></div>
        {#if timeline !== null}<div class="pin" style:left="{overviewX(nowMs)}px"></div>{/if}
      </div>

      <div class="axis mono">
        {#each ticks as tick (tick.ms)}
          <span style:left="{x(tick.ms)}px">{tick.label}</span>
        {/each}
      </div>

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="track"
        onpointerdown={(event) => scrub(event, (px) => from + (px / width) * span)}
        oncontextmenu={(event) => event.preventDefault()}
      >
        {#each spans as clip (clip.file)}
          {@const file = timeline?.files[clip.file]}
          {@const left = Math.max(x(clip.start_ms), 0)}
          {@const right = Math.min(x(clip.end_ms), width)}
          {#if file !== undefined && right > left}
            {@const label = `動画 ${clip.file + 1}`}
            <div
              class="clip"
              class:current={nowMs >= clip.start_ms && nowMs < clip.end_ms}
              data-file={file.name}
              style:left="{left}px"
              style:width="{Math.max(right - left - 1, 1)}px"
              title="{file.name}　{clock(clip.start_ms)} – {clock(clip.end_ms)}　{file.codec}　右クリックで走行から外す"
              oncontextmenu={(event) => openMenu(event, { name: file.name, start_ms: clip.start_ms, end_ms: clip.end_ms }, label, false)}
            >
              {#if right - left >= CLIP_LABEL_PX}{label}{/if}
            </div>
          {/if}
        {/each}
        <!-- 走行から外した動画。薄く出しておき、押すと戻す操作が出る。 -->
        {#each excluded as clip (clip.name)}
          {@const left = Math.max(x(clip.start_ms), 0)}
          {@const right = Math.min(x(clip.end_ms), width)}
          {#if right > left}
            <button
              type="button"
              class="clip out"
              data-file={clip.name}
              style:left="{left}px"
              style:width="{Math.max(right - left - 1, 1)}px"
              title="{clip.name}　{clock(clip.start_ms)} – {clock(clip.end_ms)}　走行から外している。押すと戻せる"
              onpointerdown={(event) => event.stopPropagation()}
              onclick={(event) => openMenu(event, clip, "外した動画", true)}
              oncontextmenu={(event) => openMenu(event, clip, "外した動画", true)}
            >
              {#if right - left >= CLIP_LABEL_PX}外した{/if}
            </button>
          {/if}
        {/each}
        {#each stops as gap (gap.start_ms)}
          {@const left = Math.max(x(gap.start_ms), 0)}
          {@const right = Math.min(x(gap.end_ms), width)}
          {#if right - left >= GAP_LABEL_PX}
            <div class="stop" style:left="{left}px" style:width="{right - left}px">
              停止 {short(gap.end_ms - gap.start_ms)}
            </div>
          {/if}
        {/each}
      </div>

      <!-- イベントの印。線が時刻を指し、札が種類を示す。 -->
      {#each marks as mark (mark.event.id)}
        {@const at = x(mark.time)}
        {#if at >= 0 && at <= width}
          {@const event = mark.event}
          <div class="stem {event.status}" class:warned={mark.warned} style:left="{at}px"></div>
          <button
            type="button"
            class="mark {event.status}"
            class:target={event.id === targetId}
            class:warned={mark.warned}
            style:left="{at}px"
            title="{event.label || KINDS[event.kind]}　{clock(mark.time)}"
            onclick={() => onselect(event)}
          >
            <KindTag kind={event.kind} />
          </button>
        {/if}
      {/each}

      {#if timeline !== null && x(nowMs) >= 0 && x(nowMs) <= width}
        <div class="head" style:left="{x(nowMs)}px"></div>
        <div class="head-time mono" style:left="{Math.min(Math.max(x(nowMs), 19), width - 19)}px">{clock(nowMs)}</div>
      {/if}
    {/if}
    </div>
  </div>

  {#if menu !== null}
    <div
      class="menu"
      bind:this={menuElement}
      style:left="{menu.left}px"
      style:bottom="{menu.bottom}px"
      style:width="{MENU_PX}px"
    >
      <div class="about">
        <b>{menu.label}</b>
        <span class="mono">{clock(menu.clip.start_ms)} – {clock(menu.clip.end_ms)}</span>
      </div>
      <div class="mono fine name">{menu.clip.name}</div>
      <div class="act">
        <span class="fine">
          {menu.out ? "戻すと、再生と時刻にまた使います。" : "外すと、再生にも時刻にも使いません。あとで戻せます。"}
        </span>
        <button type="button" class="btn" onclick={chooseMenu}>{menu.out ? "走行に戻す" : "走行から外す"}</button>
      </div>
    </div>
  {/if}
</section>

<style>
  section {
    display: flex;
    box-sizing: border-box;
    height: 100%;
    border-top: 1px solid var(--line-strong);
    background: var(--surface-2);
  }
  .side {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 6px;
    box-sizing: border-box;
    width: 132px;
    padding: 10px 12px 0;
  }
  .side .fine {
    min-height: 15px;
  }
  .buttons {
    display: flex;
    gap: 4px;
  }
  .lane {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }
  .empty {
    margin: 0;
    padding: 28px 12px 0;
    color: var(--ink-3);
  }
  .axis-box {
    position: absolute;
    inset: 0 var(--gutter);
  }
  .axis-box > * {
    position: absolute;
  }

  /* 録画していない時間は、斜線の地で出す。 */
  .overview,
  .track {
    right: 0;
    left: 0;
    cursor: pointer;
  }
  .overview {
    top: 5px;
    height: 14px;
  }
  .overview::before {
    position: absolute;
    inset: 3px 0;
    border-radius: 2px;
    background: repeating-linear-gradient(135deg, var(--hatch) 0 3px, var(--hatch-gap) 3px 6px);
    content: "";
  }
  .piece {
    position: absolute;
    top: 3px;
    height: 8px;
    background: var(--clip);
  }
  .piece.out {
    opacity: 0.35;
  }
  .window {
    position: absolute;
    top: 0;
    box-sizing: border-box;
    height: 14px;
    border: 2px solid var(--ink);
    border-radius: 3px;
  }
  .pin {
    position: absolute;
    top: 1px;
    width: 2px;
    height: 12px;
    margin-left: -1px;
    background: var(--route);
  }
  .axis {
    top: 22px;
    right: 0;
    left: 0;
    height: 14px;
    color: var(--ink-3);
    font-size: 11px;
    pointer-events: none;
  }
  .axis span {
    position: absolute;
    white-space: nowrap;
  }
  .track {
    top: 40px;
    height: 22px;
    border-radius: 3px;
    background: repeating-linear-gradient(135deg, var(--hatch) 0 4px, var(--hatch-gap) 4px 8px);
  }
  .clip {
    position: absolute;
    top: 0;
    box-sizing: border-box;
    height: 22px;
    padding-left: 8px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--clip);
    color: #fff;
    font-size: 11px;
    line-height: 22px;
    white-space: nowrap;
  }
  .clip.current {
    background: var(--clip-current);
  }
  /* 走行から外した動画は、破線の枠と薄い地で出す。 */
  .clip.out {
    padding: 0 0 0 7px;
    border: 1px dashed var(--ink-4);
    background: rgba(127, 151, 184, 0.22);
    color: var(--ink-3);
    line-height: 20px;
    text-align: left;
  }
  .clip.out:hover {
    background: rgba(127, 151, 184, 0.4);
    color: var(--ink);
  }

  /* 動画 1 本の操作。帯のすぐ上に、地図や一覧に重ねて出す。 */
  .menu {
    position: fixed;
    z-index: 3;
    display: flex;
    flex-direction: column;
    gap: 4px;
    box-sizing: border-box;
    padding: 8px 10px 10px 12px;
    border: 1px solid var(--primary);
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
  }
  .menu .about {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .menu .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .menu .act {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 2px;
  }
  .menu .act .fine {
    flex: 1;
  }
  .stop {
    position: absolute;
    top: 0;
    height: 22px;
    color: var(--ink-2);
    font-size: 10px;
    line-height: 22px;
    text-align: center;
    white-space: nowrap;
    pointer-events: none;
  }

  /* イベントの時刻を指す線。確定は実線、推定は破線にする。 */
  .stem {
    top: 40px;
    width: 0;
    height: 28px;
    margin-left: -1px;
    border-left: 2px dashed #fff;
    pointer-events: none;
  }
  .stem.confirmed {
    border-left: 2px solid var(--ink);
  }
  .stem.hidden {
    opacity: 0.4;
  }
  .stem.warned {
    border-left-color: var(--warn-ink);
  }
  .mark {
    top: 68px;
    display: grid;
    margin-left: -9px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: none;
  }
  /* 状態は、札のまわりの輪でも分ける。確定は緑、警告は橙、次の相手は白と青の二重。 */
  .mark.confirmed {
    box-shadow: 0 0 0 2px var(--ok);
  }
  .mark.hidden {
    opacity: 0.35;
  }
  .mark.warned {
    box-shadow: 0 0 0 2px var(--warn-ring);
  }
  .mark.target {
    z-index: 1;
    box-shadow:
      0 0 0 2px #fff,
      0 0 0 4px var(--primary);
  }
  .mark:hover {
    z-index: 2;
  }

  .head {
    top: 20px;
    bottom: 4px;
    width: 2px;
    margin-left: -1px;
    background: var(--route);
    pointer-events: none;
  }
  .head-time {
    top: 19px;
    width: 58px;
    height: 16px;
    margin-left: -29px;
    border-radius: 3px;
    background: var(--route);
    color: #fff;
    font-size: 11px;
    font-weight: 700;
    line-height: 16px;
    text-align: center;
    pointer-events: none;
  }
</style>
