<script lang="ts" module>
  /** 24×24 の線画。線の太さと色は、置いた場所の字に合わせる。 */
  const PATHS = {
    down: '<path d="M6 9l6 6 6-6"/>',
    check: '<path d="M5 12l5 5 9-10"/>',
    export: '<path d="M12 4v11M7 11l5 5 5-5M5 20h14"/>',
    cloud: '<path d="M7 18a4 4 0 0 1-.5-8A6 6 0 0 1 18 9.5 4.5 4.5 0 0 1 17.5 18Z"/>',
    settings: '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',
    undo: '<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>',
    redo: '<path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 0 0 0 12h3"/>',
    fit: '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>',
    trash: '<path d="M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    minus: '<path d="M5 12h14"/>',
    close: '<path d="M6 6l12 12M18 6L6 18"/>',
    warning: '<path d="M12 4l9 16H3zM12 10v4M12 17v.5"/>',
    error: '<path d="M12 7v6M12 16.5v.5"/><circle cx="12" cy="12" r="9"/>',
    refresh: '<path d="M20 12a8 8 0 1 1-2.6-5.9M20 4v5h-5"/>',
    play: '<path d="M7 6v12l10-6z"/>',
    reverse: '<path d="M17 6v12L7 12z"/>',
    pause: '<path d="M9 6v12M15 6v12"/>',
    search: '<circle cx="11" cy="11" r="6"/><path d="M16 16l4 4"/>',
    help: '<path d="M9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.7.4-1 .9-1 1.7M12 16.5v.5"/><circle cx="12" cy="12" r="9"/>',
  } as const;

  export type IconName = keyof typeof PATHS;
</script>

<script lang="ts">
  let {
    name,
    size = 16,
    weight = 2,
    solid = false,
  }: {
    name: IconName;
    size?: number;
    /** 線の太さ */
    weight?: number;
    /** 塗りつぶす。再生の三角に使う */
    solid?: boolean;
  } = $props();
</script>

<svg class="icon" class:solid viewBox="0 0 24 24" width={size} height={size} stroke-width={weight} aria-hidden="true">
  {@html PATHS[name]}
</svg>
