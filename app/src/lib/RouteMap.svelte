<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import type { Feature, FeatureCollection } from "geojson";
  import maplibregl from "maplibre-gl";
  import "maplibre-gl/dist/maplibre-gl.css";
  import { Protocol } from "pmtiles";
  import { onMount, untrack } from "svelte";
  import { distanceM, nearestOn } from "./geo";
  import Icon from "./Icon.svelte";
  import { KIND_MARKS, KIND_ORDER } from "./kinds";
  import KindTag from "./KindTag.svelte";
  import {
    appendWaypoint,
    insertVertex,
    insertWaypoint,
    line,
    moveVertex,
    moveWaypoint,
    onSegment,
    onWaypoint,
    type Path,
    type PathEdit,
    pieces,
    removeVertex,
    removeWaypoint,
    toggleSnap,
  } from "./route";
  import type { EventKind, LonLat, RoadData } from "./types";
  import type { Basemap } from "./ui";

  /**
   * 地図の上で経路を編集する。
   *
   * - 地図をクリックすると、終わりに経由点を足す
   * - 経路の線の上をクリックすると、そこに経由点を足して区間を 2 つに分ける
   * - 経由点と頂点はドラッグで動かし、右クリックで消す
   * - 線の途中の小さな丸をドラッグすると、そこに頂点を足す
   * - Shift を押しながら経由点をクリックすると、次の経由点までの結び方を吸着と直線で切り替える
   * - 経由点をクリックすると選び、結び方の切り替えと削除のボタンをそばに出す
   *
   * `editable` でなければ、経路は見るだけになる。時刻入力のときに使う。経由点の丸は小さく出し、
   * 押すと `onwaypoint` に知らせる。`placing` があれば、経路の上を押して停車の場所を決める。
   *
   * 操作の結果は、変えた後の経路ではなく、経路に当てる編集（`PathEdit`）として `onedit` に渡す。
   * 点を道へ寄せるのを待つ間に、次の操作や保存の応答で経路が変わるからである。
   */
  let {
    path,
    rideId,
    editable = true,
    roads = null,
    showRoads = true,
    basemap = "pale",
    marks = [],
    callout = null,
    here = null,
    guessed = false,
    follow = false,
    snap = false,
    canSnap = false,
    selected = null,
    timed = [],
    placing = null,
    pad = { top: 60, right: 60, bottom: 60, left: 60 },
    onsnap,
    onselect,
    onwaypoint,
    onplace,
    onedit,
  }: {
    path: Path;
    /** 走行が変わったら、経路の全体が見えるように地図を動かす */
    rideId: string | null;
    editable?: boolean;
    roads?: RoadData | null;
    /** 道路の線を重ねるか */
    showRoads?: boolean;
    basemap?: Basemap;
    /** イベントの位置。地図に、種類の色の点で出す */
    marks?: { at: LonLat; kind: EventKind; hidden: boolean }[];
    /** 名前を添えて目立たせるイベント。次の候補か、一覧で選んだもの */
    callout?: { at: LonLat; kind: EventKind; label: string; note: string } | null;
    /** 動画のいまの時刻にいると推定される位置 */
    here?: LonLat | null;
    /** `here` が、最後の確定より後を直前の速さから置いた、仮の位置か */
    guessed?: boolean;
    /** 現在位置が画面の端へ寄ったら、地図を動かして追う */
    follow?: boolean;
    /** 新しく足す区間を、道に沿わせるか */
    snap?: boolean;
    /** 道に沿わせることができるか。判定用データを開いていなければ、できない */
    canSnap?: boolean;
    /** 選んでいる経由点の番号 */
    selected?: number | null;
    /** 通過時刻を確定してある経由点の番号。見るだけのときに、丸を確定の色にする */
    timed?: number[];
    /**
     * 停車の場所を決めているときの、その停車のいまの位置（m）。決めていなければ null。
     * 同じ道を行って戻る経路で、どちらの通過かを選ぶ目安にする。
     */
    placing?: number | null;
    /** 浮かせた板に隠れる幅（px）。経路の全体を出すときに、ここを避ける */
    pad?: { top: number; right: number; bottom: number; left: number };
    /** 点を、`radiusM` 以内でいちばん近い道の上へ寄せる。近くに道が無ければ null を返す */
    onsnap?: (at: LonLat, radiusM: number) => Promise<LonLat | null>;
    onselect?: (index: number | null) => void;
    /** 見るだけのときに、経由点の丸を押した */
    onwaypoint?: (index: number) => void;
    /** 停車の場所を決めた。`at` は経路の上の点、`routeM` はその始点からの距離 */
    onplace?: (at: LonLat, routeM: number) => void;
    /**
     * 経路を変える操作を、した順に 1 つずつ経路に入れてもらう。
     *
     * `make` は、前の操作が経路に入った後で呼ばれ、いまの経路に当てる編集を返す。当てるものが
     * 無ければ null を返す。返る約束は、その操作が経路に入り終えたときに果たされる。
     */
    onedit: (make: () => PathEdit | null | Promise<PathEdit | null>) => Promise<void>;
  } = $props();

  const VIEW_KEY = "routelog.map-view";
  /** これより短い区切りには、頂点を足す丸を出さない（画面上の px） */
  const MIN_PIECE_PX = 48;
  /** 現在位置を追うとき、画面の縁からこの割合より外へ出たら地図を動かす */
  const FOLLOW_MARGIN = 0.2;
  const GSI = "https://cyberjapandata.gsi.go.jp/xyz";
  const GSI_CREDIT = '<a href="https://maps.gsi.go.jp/development/ichiran.html">地理院タイル</a>';
  /** 背景地図。どれも地理院タイルで、縮尺 5 から 18 まである */
  const BASEMAPS: Record<Basemap, { tiles: string; attribution: string }> = {
    pale: { tiles: `${GSI}/pale/{z}/{x}/{y}.png`, attribution: GSI_CREDIT },
    std: { tiles: `${GSI}/std/{z}/{x}/{y}.png`, attribution: GSI_CREDIT },
    // 写真は、縮尺の小さいところで衛星画像を使っている。その出典も添える。
    photo: {
      tiles: `${GSI}/seamlessphoto/{z}/{x}/{y}.jpg`,
      attribution: `${GSI_CREDIT}（Landsat8 画像: GSI, TSIC, GEO Grid/AIST, courtesy of the U.S. Geological Survey。海底地形: GEBCO）`,
    },
  };

  let container: HTMLDivElement;
  let calloutElement: HTMLDivElement;
  let popoverElement: HTMLDivElement;
  let map: maplibregl.Map | undefined;
  let ready = $state(false);
  let markers: maplibregl.Marker[] = [];
  const errors: string[] = [];

  /** `app.css` で決めた色。地図の線と点にも同じ色を使う。 */
  const token = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

  function savedView(): { center: LonLat; zoom: number } {
    try {
      const view = JSON.parse(localStorage.getItem(VIEW_KEY) ?? "");
      if (Array.isArray(view.center) && typeof view.zoom === "number") return view;
    } catch {
      // 保存が無いか壊れている。既定の位置から始める。
    }
    return { center: [137.7, 36.5], zoom: 6 };
  }

  /** 道へ寄せるときに探す範囲（画面上の px）。縮尺を引いていても、近くの道に届く */
  const SNAP_PX = 24;

  /** 道へ寄せるときに探す範囲（m）。画面上の `SNAP_PX` を、その場所での距離に直す。 */
  function snapRadiusM(at: LonLat): number {
    if (map === undefined) return 0;
    const pixel = map.project(at);
    return distanceM(at, map.unproject([pixel.x + SNAP_PX, pixel.y]).toArray());
  }

  /**
   * 点を、`radiusM` 以内でいちばん近い道の上へ寄せる。寄せる先が無ければ、そのままにする。
   *
   * 判定データを読めないなどで寄せられなかったときも、そのままにする。読めない理由は、経路を
   * 保存するときに知らせが出る。
   */
  async function onRoad(at: LonLat, radiusM: number): Promise<LonLat> {
    if (onsnap === undefined) return at;
    try {
      return (await onsnap(at, radiusM)) ?? at;
    } catch {
      return at;
    }
  }

  /** 線の上とみなす、線からの距離（画面上の px） */
  const ON_LINE_PX = 8;
  /** 停車の場所として受け付ける、押した所から経路までの距離（画面上の px） */
  const PLACE_PX = 24;

  /** 画面上の点にいちばん近い、経路の上の点。経路から離れすぎていれば null。 */
  function placeNear(point: maplibregl.Point) {
    if (map === undefined || placing === null) return null;
    const found = nearestOn(line(path), map.unproject(point).toArray(), placing);
    return found !== null && map.project(found.at).dist(point) <= PLACE_PX ? found : null;
  }

  let previewMarker: maplibregl.Marker | undefined;

  /** 押せば停車の場所になる点を、カーソルのそばの経路の上に出す。 */
  function preview(found: { at: LonLat } | null) {
    if (map === undefined) return;
    map.getCanvas().style.cursor = found === null ? "" : "crosshair";
    if (found === null) {
      previewMarker?.remove();
      previewMarker = undefined;
      return;
    }
    if (previewMarker === undefined) {
      const element = document.createElement("div");
      element.className = "place-preview";
      previewMarker = new maplibregl.Marker({ element }).setLngLat(found.at).addTo(map);
    } else {
      previewMarker.setLngLat(found.at);
    }
  }

  /** クリックした位置が経路の線の上なら、その線の区切り。 */
  function pieceAt(at: LonLat) {
    if (map === undefined) return null;
    const p = map.project(at);
    let best: { piece: ReturnType<typeof pieces>[number]; distance: number } | null = null;
    for (const piece of pieces(path)) {
      const [a, b] = [map.project(piece.from), map.project(piece.to)];
      const length2 = a.dist(b) ** 2;
      const t = length2 === 0 ? 0 : Math.min(1, Math.max(0, ((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / length2));
      const distance = Math.hypot(p.x - (a.x + (b.x - a.x) * t), p.y - (a.y + (b.y - a.y) * t));
      if (distance <= ON_LINE_PX && (best === null || distance < best.distance)) best = { piece, distance };
    }
    return best?.piece ?? null;
  }

  /** 経路の線。区間ごとに分け、道に沿わせた区間は実線、それ以外は破線で描けるようにする。 */
  function routeData(next: Path): FeatureCollection {
    const features: Feature[] = next.route.segments.map((vertices, i) => {
      const [from, to] = [next.waypoints[i], next.waypoints[i + 1]];
      return {
        type: "Feature",
        properties: { snap: from.segment_mode === "snap" },
        geometry: { type: "LineString", coordinates: [[from.lon, from.lat], ...vertices, [to.lon, to.lat]] },
      };
    });
    return { type: "FeatureCollection", features };
  }

  function draw(next: Path) {
    (map?.getSource("route") as maplibregl.GeoJSONSource | undefined)?.setData(routeData(next));
  }

  /** 丸を押しているか */
  let pressed = false;
  /** 丸をドラッグしている最中か */
  let dragging = false;
  /** 離した位置を経路に入れている最中の、ドラッグの数 */
  let settling = 0;
  /** 丸の置き直しを、後回しにしたか */
  let overdue = false;

  /**
   * 経路の線と丸を、いまの経路に合わせる。丸を押している間と、離した位置が経路に入るまでの
   * 間は、後回しにする。
   *
   * 丸は、経路が変わるたびに作り直す。保存の応答で経路が入れ替わったときに作り直すと、
   * つかんでいる丸が消えて、ドラッグが途切れる。
   */
  function render() {
    if (pressed || settling > 0) {
      overdue = true;
      return;
    }
    overdue = false;
    draw(path);
    place(path);
  }

  const press = () => (pressed = true);

  /** 後回しにした置き直しを、できるようになっていれば行う。 */
  function catchUp() {
    if (overdue) render();
  }

  /** 押していたボタンか指を離した。 */
  function lifted() {
    if (!pressed) return;
    pressed = false;
    // ドラッグを地図の外で離すと、地図には伝わらず、離した位置が経路に入らない。丸を経路の上の
    // 位置へ戻して、ドラッグを取りやめる。
    if (dragging) overdue = true;
    dragging = false;
    catchUp();
  }

  function handle(kind: string, at: LonLat, label = "", title = "") {
    const element = document.createElement("div");
    element.className = `handle ${kind}`;
    element.textContent = label;
    element.title = title;
    let onshiftclick: (() => void) | undefined;
    let onplainclick: (() => void) | undefined;
    // 丸の上のクリックで、地図に経由点を足さない。
    element.addEventListener("click", (event) => {
      event.stopPropagation();
      if (event.shiftKey) onshiftclick?.();
      else onplainclick?.();
    });
    const marker = new maplibregl.Marker({ element, draggable: editable }).setLngLat(at).addTo(map!);
    markers.push(marker);
    if (editable) {
      element.addEventListener("mousedown", press);
      element.addEventListener("touchstart", press, { passive: true });
    }
    return {
      /**
       * ドラッグの間は線だけを描き直し、離したときに経路に入れる。`toRoad` なら、離した位置を
       * 道の上へ寄せてから入れる。
       */
      ondrag(moved: (to: LonLat) => PathEdit, toRoad = false) {
        const to = (): LonLat => marker.getLngLat().toArray();
        marker.on("dragstart", () => (dragging = true));
        marker.on("drag", () => draw(moved(to())(path)));
        marker.on("dragend", () => {
          const dropped = to();
          const radiusM = snapRadiusM(dropped);
          dragging = false;
          settling += 1;
          const done = () => {
            settling -= 1;
            // 何も変わらなかったときも、丸を経路の上の位置へ戻す。
            overdue = true;
            catchUp();
          };
          void onedit(async () => moved(toRoad ? await onRoad(dropped, radiusM) : dropped)).then(done, done);
        });
      },
      onclick(clicked: () => void) {
        onplainclick = clicked;
      },
      onshiftclick(changed: () => PathEdit) {
        onshiftclick = () => void onedit(changed);
      },
      onremove(removed: () => PathEdit) {
        element.addEventListener("contextmenu", (event) => {
          event.preventDefault();
          event.stopPropagation();
          void onedit(removed);
        });
      },
    };
  }

  /**
   * クリックした所に経由点を足す編集。経路の線の上なら、そこで区間を 2 つに分ける。そうで
   * なければ、終わりに足す。前のクリックが経路に入った後で呼ぶ。
   */
  async function added(clicked: LonLat, radiusM: number, snapping: boolean): Promise<PathEdit> {
    const shown = path;
    const piece = pieceAt(clicked);
    if (piece !== null) {
      const snapped = shown.waypoints[piece.segment].segment_mode === "snap";
      const at = snapped ? await onRoad(clicked, radiusM) : clicked;
      return onSegment(shown, piece.segment, (latest) => insertWaypoint(latest, piece.segment, piece.index, at));
    }
    if (!snapping) return (latest) => appendWaypoint(latest, clicked);
    const at = await onRoad(clicked, radiusM);
    return (latest) => appendWaypoint(latest, at, "snap");
  }

  /** 選んだ経由点の、次の点までの結び方を切り替える。 */
  function toggle(index: number) {
    const shown = path;
    void onedit(() => onWaypoint(shown, index, (latest) => toggleSnap(latest, index)));
  }

  /** 選んだ経由点を消す。 */
  function remove(index: number) {
    const shown = path;
    void onedit(() => onWaypoint(shown, index, (latest) => removeWaypoint(latest, index)));
  }

  function place(current: Path) {
    for (const marker of markers) marker.remove();
    markers = [];
    if (map === undefined) return;
    const last = current.waypoints.length - 1;
    const waypointClass = (index: number) =>
      [
        "waypoint",
        current.waypoints[index].segment_mode === "snap" ? "snap" : "",
        index === 0 || index === last ? "end" : "",
        index === selected ? "selected" : "",
      ]
        .filter(Boolean)
        .join(" ");
    if (!editable) {
      // 見るだけのときは、経由点を小さく出す。つかめない。押すと、そこを通った時刻を入れる相手になる。
      current.waypoints.forEach((waypoint, index) => {
        const state = timed.includes(index) ? "fixed timed" : "fixed";
        const title = `経由地 ${index + 1}。押すと、ここを通った時刻を入力できます`;
        const fixed = handle(`${waypointClass(index)} ${state}`, [waypoint.lon, waypoint.lat], String(index + 1), title);
        fixed.onclick(() => {
          if (placing === null) {
            onwaypoint?.(index);
            return;
          }
          const found = nearestOn(line(current), [waypoint.lon, waypoint.lat], placing);
          if (found !== null) onplace?.(found.at, found.routeM);
        });
      });
      return;
    }
    // 吸着の区間の線は道が決めるので、頂点も、頂点を足す丸も出さない。
    const snapped = (segment: number) => current.waypoints[segment].segment_mode === "snap";
    for (const piece of pieces(current)) {
      if (snapped(piece.segment)) continue;
      const [from, to] = [map.project(piece.from), map.project(piece.to)];
      if (from.dist(to) < MIN_PIECE_PX) continue;
      const middle = map.unproject(from.add(to).div(2)).toArray();
      handle("middle", middle).ondrag((at) =>
        onSegment(current, piece.segment, (latest) => insertVertex(latest, piece.segment, piece.index, at)),
      );
    }
    current.route.segments.forEach((vertices, segment) => {
      if (snapped(segment)) return;
      vertices.forEach((vertex, index) => {
        const vertexHandle = handle("vertex", vertex);
        vertexHandle.ondrag((to) => onSegment(current, segment, (latest) => moveVertex(latest, segment, index, to)));
        vertexHandle.onremove(() => onSegment(current, segment, (latest) => removeVertex(latest, segment, index)));
      });
    });
    current.waypoints.forEach((waypoint, index) => {
      const waypointHandle = handle(waypointClass(index), [waypoint.lon, waypoint.lat], String(index + 1));
      waypointHandle.onclick(() => onselect?.(index === selected ? null : index));
      waypointHandle.onshiftclick(() => onWaypoint(current, index, (latest) => toggleSnap(latest, index)));
      // 前後のどちらかが吸着の区間なら、離した位置を道の上へ寄せる。
      const beside = snapped(index) || (index > 0 && snapped(index - 1));
      waypointHandle.ondrag((to) => onWaypoint(current, index, (latest) => moveWaypoint(latest, index, to)), beside);
      waypointHandle.onremove(() => onWaypoint(current, index, (latest) => removeWaypoint(latest, index)));
    });
  }

  /** 浮かせた板に隠れない所の、まん中からのずれ。 */
  const padOffset = (): [number, number] => [(pad.left - pad.right) / 2, (pad.top - pad.bottom) / 2];

  function fit(current: Path) {
    const points = line(current);
    if (map === undefined || points.length === 0) return;
    if (points.length === 1) {
      map.jumpTo({ center: points[0] });
      return;
    }
    const bounds = points.reduce((box, p) => box.extend(p), new maplibregl.LngLatBounds(points[0], points[0]));
    const margin = 40;
    map.fitBounds(bounds, {
      padding: {
        top: pad.top + margin,
        right: pad.right + margin,
        bottom: pad.bottom + margin,
        left: pad.left + margin,
      },
      maxZoom: 15,
      animate: false,
    });
  }

  /** イベントの点に使う、種類の色の小さな四角。 */
  function markImage(color: string, outline: boolean) {
    const [size, ratio] = [14, 2];
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = size * ratio;
    const context = canvas.getContext("2d")!;
    context.scale(ratio, ratio);
    context.beginPath();
    context.roundRect(1.5, 1.5, size - 3, size - 3, 3);
    context.fillStyle = outline ? "#fff" : color;
    context.fill();
    context.lineWidth = outline ? 2 : 1.5;
    context.strokeStyle = outline ? color : "#fff";
    context.stroke();
    return context.getImageData(0, 0, canvas.width, canvas.height);
  }

  onMount(() => {
    const protocol = new Protocol();
    maplibregl.addProtocol("pmtiles", protocol.tile);
    const names = Object.keys(BASEMAPS) as Basemap[];
    const created = new maplibregl.Map({
      container,
      style: {
        version: 8,
        sources: Object.fromEntries(
          names.map((name) => [
            `base-${name}`,
            { type: "raster", tileSize: 256, minzoom: 5, maxzoom: 18, ...BASEMAPS[name], tiles: [BASEMAPS[name].tiles] },
          ]),
        ),
        // 選んでいない背景は隠しておく。隠した層のタイルは読まれない。
        layers: names.map((name) => ({
          id: `base-${name}`,
          type: "raster",
          source: `base-${name}`,
          layout: { visibility: name === untrack(() => basemap) ? "visible" : "none" },
        })),
      },
      ...savedView(),
      minZoom: 5,
      maxBounds: [
        [118, 18],
        [158, 48],
      ],
      dragRotate: false,
      pitchWithRotate: false,
      touchPitch: false,
    });
    map = created;
    created.touchZoomRotate.disableRotation();
    created.keyboard.disableRotation();
    created.addControl(new maplibregl.ScaleControl());
    created.on("error", (event) => errors.push(String(event.error?.message ?? event.error)));
    created.on("load", () => {
      created.addSource("route", { type: "geojson", data: routeData({ waypoints: [], route: { segments: [] } }) });
      const round = { "line-join": "round", "line-cap": "round" } as const;
      created.addLayer({
        id: "route-casing",
        type: "line",
        source: "route",
        layout: round,
        paint: { "line-color": "#fff", "line-width": 8 },
      });
      created.addLayer({
        id: "route-snap",
        type: "line",
        source: "route",
        filter: ["get", "snap"],
        layout: round,
        paint: { "line-color": token("--route"), "line-width": 4.5 },
      });
      // 道に沿わせていない区間は、破線にして見分ける。
      created.addLayer({
        id: "route-straight",
        type: "line",
        source: "route",
        filter: ["!", ["get", "snap"]],
        layout: { "line-join": "round" },
        paint: { "line-color": token("--route"), "line-width": 4.5, "line-dasharray": [2.4, 1.5] },
      });
      for (const kind of KIND_ORDER) {
        const mark = KIND_MARKS[kind];
        created.addImage(`kind-${kind}`, markImage(token(mark.color), mark.outline === true), { pixelRatio: 2 });
      }
      const empty: FeatureCollection = { type: "FeatureCollection", features: [] };
      created.addSource("callout", { type: "geojson", data: empty });
      created.addLayer({
        id: "callout",
        type: "circle",
        source: "callout",
        paint: {
          "circle-radius": 13,
          "circle-color": "rgba(255, 255, 255, 0.35)",
          "circle-stroke-color": token("--primary"),
          "circle-stroke-width": 2,
        },
      });
      created.addSource("marks", { type: "geojson", data: empty });
      created.addLayer({
        id: "marks",
        type: "symbol",
        source: "marks",
        layout: {
          "icon-image": ["concat", "kind-", ["get", "kind"]],
          "icon-allow-overlap": true,
          "icon-ignore-placement": true,
        },
        paint: { "icon-opacity": ["case", ["get", "hidden"], 0.45, 1] },
      });
      ready = true;
    });
    created.on("click", (event) => {
      if (!editable) {
        const found = placeNear(event.point);
        if (found !== null) onplace?.(found.at, found.routeM);
        return;
      }
      // 押した所と、そのときの寄せる範囲と結び方を覚えておき、順番が来たら経路に入れる。
      const clicked = event.lngLat.toArray();
      const [radiusM, snapping] = [snapRadiusM(clicked), snap];
      void onedit(() => added(clicked, radiusM, snapping));
    });
    // 線の上では、押すと経由点が足されることをカーソルで知らせる。
    // 動かすたびに線の区切りを総当たりすると、長い経路で重い。描かれた線に当たるかだけを見る。
    created.on("mousemove", (event) => {
      if (!editable && placing !== null) {
        preview(placeNear(event.point));
        return;
      }
      const { x, y } = event.point;
      const box: [[number, number], [number, number]] = [
        [x - ON_LINE_PX, y - ON_LINE_PX],
        [x + ON_LINE_PX, y + ON_LINE_PX],
      ];
      const layers = ["route-snap", "route-straight"];
      const onLine = ready && editable && created.queryRenderedFeatures(box, { layers }).length > 0;
      created.getCanvas().style.cursor = onLine ? "copy" : "";
    });
    created.on("moveend", () => {
      localStorage.setItem(VIEW_KEY, JSON.stringify({ center: created.getCenter().toArray(), zoom: created.getZoom() }));
    });
    // 頂点を足す丸は画面上の長さで出し分けるので、縮尺が変わったら置き直す。
    created.on("zoomend", render);
    const LIFTS = ["mouseup", "touchend", "touchcancel"] as const;
    for (const type of LIFTS) window.addEventListener(type, lifted);
    return () => {
      for (const type of LIFTS) window.removeEventListener(type, lifted);
      created.remove();
      maplibregl.removeProtocol("pmtiles");
    };
  });

  $effect(() => {
    if (placing === null) untrack(() => preview(null));
  });

  // 時刻を確定してある経由点の並び。配列は保存のたびに作り直されるので、中身で比べる。
  const timedKey = $derived(timed.join(","));
  $effect(() => {
    if (!ready) return;
    // 経路のほか、選んでいる経由点、編集できるかどうか、時刻を確定してあるかでも、丸の見た目が変わる。
    void [path, selected, editable, timedKey];
    untrack(render);
  });

  $effect(() => {
    void rideId;
    if (ready) untrack(() => fit(path));
  });

  // 見るだけのときは、矢印キーを動画の送り戻しに使う。地図がキーで動かないようにする。
  $effect(() => {
    if (!ready) return;
    if (editable) map?.keyboard.enable();
    else map?.keyboard.disable();
  });

  $effect(() => {
    if (!ready) return;
    for (const name of Object.keys(BASEMAPS)) {
      map?.setLayoutProperty(`base-${name}`, "visibility", name === basemap ? "visible" : "none");
    }
  });

  $effect(() => {
    if (!ready) return;
    const features: Feature[] = marks.map((mark) => ({
      type: "Feature",
      properties: { kind: mark.kind, hidden: mark.hidden },
      geometry: { type: "Point", coordinates: mark.at },
    }));
    (map?.getSource("marks") as maplibregl.GeoJSONSource | undefined)?.setData({ type: "FeatureCollection", features });
  });

  // 名前を添えるイベント。丸で囲み、吹き出しをそばに出す。吹き出しの中身は下のマークアップが描く。
  let calloutMarker: maplibregl.Marker | undefined;
  const calloutAt = $derived(callout === null ? null : `${callout.at[0]},${callout.at[1]}`);
  $effect(() => {
    if (!ready || map === undefined) return;
    const at = calloutAt === null ? null : (calloutAt.split(",").map(Number) as LonLat);
    const features: Feature[] =
      at === null ? [] : [{ type: "Feature", properties: {}, geometry: { type: "Point", coordinates: at } }];
    const source = map.getSource("callout") as maplibregl.GeoJSONSource | undefined;
    source?.setData({ type: "FeatureCollection", features });
    if (at === null) {
      calloutMarker?.remove();
      return;
    }
    calloutMarker ??= new maplibregl.Marker({ element: calloutElement, anchor: "left", offset: [20, -18] });
    calloutMarker.setLngLat(at).addTo(map);
  });

  // 選んだ経由点のそばに出す、結び方の切り替えと削除のボタン。
  let popoverMarker: maplibregl.Marker | undefined;
  const chosen = $derived(editable && selected !== null ? (path.waypoints[selected] ?? null) : null);
  $effect(() => {
    if (!ready || map === undefined) return;
    if (chosen === null) {
      popoverMarker?.remove();
      return;
    }
    popoverMarker ??= new maplibregl.Marker({ element: popoverElement, anchor: "top-left", offset: [-40, 20] });
    popoverMarker.setLngLat([chosen.lon, chosen.lat]).addTo(map);
  });

  let hereMarker: maplibregl.Marker | undefined;
  $effect(() => {
    if (!ready || map === undefined) return;
    if (here === null) {
      hereMarker?.remove();
      hereMarker = undefined;
      return;
    }
    if (hereMarker === undefined) {
      const element = document.createElement("div");
      element.className = "here";
      hereMarker = new maplibregl.Marker({ element }).setLngLat(here).addTo(map);
    } else {
      hereMarker.setLngLat(here);
    }
    hereMarker.getElement().classList.toggle("guessed", guessed);
    if (!follow) return;
    // 現在位置が、浮かせた板を除いた画面の縁へ寄ったら、まん中へ戻す。
    const { x, y } = map.project(here);
    const { clientWidth: width, clientHeight: height } = container;
    const [w, h] = [width - pad.left - pad.right, height - pad.top - pad.bottom];
    const inside =
      x > pad.left + w * FOLLOW_MARGIN &&
      x < pad.left + w * (1 - FOLLOW_MARGIN) &&
      y > pad.top + h * FOLLOW_MARGIN &&
      y < pad.top + h * (1 - FOLLOW_MARGIN);
    if (!inside && !map.isMoving()) map.easeTo({ center: here, offset: padOffset() });
  });

  // 道路の線は経路の下に重ねる。
  $effect(() => {
    if (!ready || roads === null || map === undefined) return;
    const visibility = showRoads ? "visible" : "none";
    if (map.getSource("national")) {
      for (const id of ["national", "prefectural"]) map.setLayoutProperty(id, "visibility", visibility);
      return;
    }
    const overlay = (id: "national" | "prefectural", file: string, color: string, opacity: number) => {
      map!.addSource(id, {
        type: "vector",
        url: `pmtiles://${convertFileSrc(file)}`,
        attribution: '© <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors',
      });
      map!.addLayer(
        {
          id,
          type: "line",
          source: id,
          "source-layer": "routes",
          filter: ["==", ["get", "kind"], "road"],
          layout: { "line-join": "round", "line-cap": "round", visibility },
          // 背景地図の道をなぞる、太く透けた帯にする。
          paint: {
            "line-color": color,
            "line-width": ["interpolate", ["linear"], ["zoom"], 7, 2.5, 11, 5, 15, 9],
            "line-opacity": opacity,
          },
        },
        "route-casing",
      );
    };
    overlay("prefectural", roads.prefectural, token("--road-prefectural"), 0.34);
    overlay("national", roads.national, token("--road-national"), 0.3);
  });

  /** その位置を、浮かせた板に隠れない所のまん中に出す。 */
  export function focus(at: LonLat) {
    map?.easeTo({ center: at, zoom: Math.max(map.getZoom(), 13), offset: padOffset() });
  }

  /** 経路の全体が見えるように、地図を動かす。 */
  export function fitRoute() {
    fit(path);
  }

  /** 地図を 1 段、拡大（正）または縮小（負）する。 */
  export function zoomBy(step: 1 | -1) {
    if (step > 0) map?.zoomIn();
    else map?.zoomOut();
  }

  /** 自動の確認が使う口。 */
  export function jump(center: LonLat, zoom: number) {
    map?.jumpTo({ center, zoom });
  }

  /** 地図をクリックしたことにする。 */
  export function click(at: LonLat) {
    if (map === undefined) return;
    const canvas = map.getCanvas();
    const box = canvas.getBoundingClientRect();
    const { x, y } = map.project(at);
    const init = { bubbles: true, clientX: box.left + x, clientY: box.top + y, button: 0 };
    for (const type of ["mousedown", "mouseup", "click"]) canvas.dispatchEvent(new MouseEvent(type, init));
  }

  /** その位置にいちばん近い丸と、地図の中での画面座標。 */
  function nearest(at: LonLat) {
    const target = map!.project(at);
    const distance = (marker: maplibregl.Marker) => map!.project(marker.getLngLat()).dist(target);
    return markers.reduce((best, marker) => (distance(marker) < distance(best) ? marker : best));
  }

  function mouse(type: string, at: LonLat, element: Element) {
    const box = map!.getCanvas().getBoundingClientRect();
    const { x, y } = map!.project(at);
    const init = { bubbles: true, cancelable: true, clientX: box.left + x, clientY: box.top + y, button: 0, buttons: 1 };
    element.dispatchEvent(new MouseEvent(type, init));
  }

  /**
   * `from` にいちばん近い丸を `to` までドラッグしたことにする。`holdMs` を渡すと、つかんだまま
   * 途中でその間だけ止まる。
   */
  export async function drag(from: LonLat, to: LonLat, holdMs = 0) {
    if (map === undefined || markers.length === 0) return;
    const canvas = map.getCanvasContainer();
    const middle: LonLat = [(from[0] + to[0]) / 2, (from[1] + to[1]) / 2];
    mouse("mousedown", from, nearest(from).getElement());
    mouse("mousemove", middle, canvas);
    if (holdMs > 0) await new Promise((resolve) => setTimeout(resolve, holdMs));
    mouse("mousemove", to, canvas);
    mouse("mouseup", to, canvas);
  }

  /**
   * `from` にいちばん近い丸をつかんで `to` まで動かし、地図の外で離したことにする。離す直前に
   * 丸があった位置を返す。
   */
  export function dragAway(from: LonLat, to: LonLat): LonLat | null {
    if (map === undefined || markers.length === 0) return null;
    const marker = nearest(from);
    mouse("mousedown", from, marker.getElement());
    mouse("mousemove", to, map.getCanvasContainer());
    const carried = marker.getLngLat().toArray();
    mouse("mouseup", to, document.body);
    return carried;
  }

  /** 経由点の丸が、いま出ている位置。 */
  export function waypointHandles(): LonLat[] {
    const waypoints = markers.filter((marker) => marker.getElement().classList.contains("waypoint"));
    return waypoints.map((marker) => marker.getLngLat().toArray());
  }

  /** 地図の `at` の所を押したことにする。経由点の丸の上でも、丸ではなく地図を押す。 */
  export function clickMap(at: LonLat) {
    if (map === undefined) return;
    const point = map.project(at);
    map.fire("click", { point, lngLat: map.unproject(point), originalEvent: new MouseEvent("click") });
  }

  /** `at` にいちばん近い丸をクリックしたことにする。 */
  export function clickHandle(at: LonLat) {
    if (map === undefined || markers.length === 0) return;
    nearest(at).getElement().dispatchEvent(new MouseEvent("click", { bubbles: true }));
  }

  /** `at` にいちばん近い丸を、Shift を押しながらクリックしたことにする。 */
  export function shiftClick(at: LonLat) {
    if (map === undefined || markers.length === 0) return;
    nearest(at).getElement().dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: true }));
  }

  /** `at` にいちばん近い丸を右クリックしたことにする。 */
  export function rightClick(at: LonLat) {
    if (map === undefined || markers.length === 0) return;
    mouse("contextmenu", at, nearest(at).getElement());
  }

  /** 読み込みが落ち着くまで待つ。待ちきれなければ false。 */
  export async function idle(timeoutMs = 20_000): Promise<boolean> {
    const deadline = performance.now() + timeoutMs;
    while (performance.now() < deadline) {
      if (map !== undefined && ready && map.loaded() && map.areTilesLoaded()) return true;
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    return false;
  }

  export function snapshot() {
    const count = (id: string) =>
      map?.getSource(id) ? map.querySourceFeatures(id, { sourceLayer: "routes" }).length : null;
    const shown = (id: string) => map?.getLayer(id) !== undefined && map.getLayoutProperty(id, "visibility") !== "none";
    return {
      zoom: map?.getZoom() ?? null,
      center: map?.getCenter().toArray() ?? null,
      tilesLoaded: map?.areTilesLoaded() ?? false,
      basemap: Object.keys(BASEMAPS).filter((name) => shown(`base-${name}`)),
      handles: {
        waypoint: container.querySelectorAll(".handle.waypoint").length,
        vertex: container.querySelectorAll(".handle.vertex").length,
        middle: container.querySelectorAll(".handle.middle").length,
      },
      roads: { national: count("national"), prefectural: count("prefectural"), shown: shown("national") },
      selected,
      here: hereMarker?.getLngLat().toArray() ?? null,
      guessed: hereMarker?.getElement().classList.contains("guessed") ?? false,
      errors: [...errors],
    };
  }

  /** 板の上の操作を、下の地図に渡さない。地図が動いたり、経由点が足されたりしないようにする。 */
  const hold = (event: Event) => event.stopPropagation();
</script>

<div class="map" bind:this={container} oncontextmenu={(event) => event.preventDefault()} role="application"></div>

<!-- 地図の上の吹き出し。置く場所は地図が決め、中身はここで描く。 -->
<div class="parked">
  <div class="callout" bind:this={calloutElement}>
    {#if callout !== null}
      <KindTag kind={callout.kind} />
      <b>{callout.label}</b>
      {#if callout.note !== ""}<span class="mono note">{callout.note}</span>{/if}
    {/if}
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div
    class="popover"
    bind:this={popoverElement}
    onclick={hold}
    onmousedown={hold}
    ondblclick={hold}
    oncontextmenu={hold}
    onwheel={hold}
  >
    {#if chosen !== null && selected !== null}
      {@const index = selected}
      <b>経由地 {index + 1}</b>
      {#if index < path.waypoints.length - 1}
        <span class="to">次の点まで</span>
        <div class="seg">
          <button
            type="button"
            class:on={chosen.segment_mode === "snap"}
            disabled={!canSnap && chosen.segment_mode !== "snap"}
            title={canSnap ? "" : "判定データを選ぶと使えます"}
            onclick={() => chosen.segment_mode !== "snap" && toggle(index)}
          >
            道に沿わせる
          </button>
          <button
            type="button"
            class:on={chosen.segment_mode !== "snap"}
            onclick={() => chosen.segment_mode === "snap" && toggle(index)}
          >
            直線
          </button>
        </div>
      {:else}
        <span class="to">終わり</span>
      {/if}
      <button
        type="button"
        class="btn icon-only remove"
        aria-label="経由地を削除"
        title="経由地を削除"
        onclick={() => remove(index)}
      >
        <Icon name="trash" />
      </button>
    {/if}
  </div>
</div>

<style>
  .map {
    width: 100%;
    height: 100%;
    font-family: var(--font-ui);
  }
  /* 吹き出しは、地図に渡すまでここに置いておく。 */
  .parked {
    display: none;
  }
  .map :global(.handle) {
    box-sizing: border-box;
    border-radius: 50%;
    cursor: grab;
  }
  /* 途中の経由点は白地に経路の赤の輪、始点と終点は黒、選んだ点は青にする。 */
  .map :global(.handle.waypoint) {
    width: 26px;
    height: 26px;
    border: 3px solid var(--route);
    background: #fff;
    box-shadow: 0 1px 3px rgba(26, 29, 33, 0.35);
    color: var(--ink);
    font: 700 13px/20px var(--font-mono);
    text-align: center;
  }
  .map :global(.handle.waypoint.end) {
    border: 2.5px solid #fff;
    background: var(--ink);
    color: #fff;
    line-height: 21px;
  }
  .map :global(.handle.waypoint.selected) {
    width: 28px;
    height: 28px;
    border: 2.5px solid #fff;
    background: var(--primary);
    box-shadow:
      0 0 0 7px rgba(31, 79, 163, 0.2),
      0 1px 3px rgba(26, 29, 33, 0.35);
    color: #fff;
    line-height: 23px;
  }
  .map :global(.handle.waypoint.fixed) {
    width: 18px;
    height: 18px;
    border-width: 2px;
    font-size: 10px;
    line-height: 14px;
    cursor: pointer;
  }
  .map :global(.handle.waypoint.fixed:hover) {
    box-shadow:
      0 0 0 4px rgba(31, 79, 163, 0.25),
      0 1px 3px rgba(26, 29, 33, 0.35);
  }
  /* 通過時刻を確定してある経由点は、確定の緑にする。 */
  .map :global(.handle.waypoint.fixed.timed) {
    border-color: #fff;
    background: var(--ok);
    color: #fff;
  }
  .map :global(.handle.vertex) {
    width: 14px;
    height: 14px;
    border: 2.5px solid var(--route);
    background: #fff;
  }
  .map :global(.handle.middle) {
    width: 12px;
    height: 12px;
    border: 2px solid var(--route);
    background: #fff;
    opacity: 0.75;
  }
  .map :global(.handle.middle:hover) {
    opacity: 1;
  }
  .map :global(.place-preview) {
    box-sizing: border-box;
    width: 16px;
    height: 16px;
    border: 3px solid var(--kind-stop);
    border-radius: 50%;
    background: #fff;
    pointer-events: none;
  }
  .map :global(.here) {
    box-sizing: border-box;
    width: 14px;
    height: 14px;
    border: 2.5px solid #fff;
    border-radius: 50%;
    background: var(--primary);
    box-shadow: 0 0 0 8px rgba(31, 79, 163, 0.18);
    pointer-events: none;
  }
  /* 仮の位置は、推定のイベントと同じく破線の輪にする。 */
  .map :global(.here.guessed) {
    width: 16px;
    height: 16px;
    border: 3px dashed var(--primary);
    background: #fff;
    box-shadow: 0 0 0 7px rgba(31, 79, 163, 0.12);
  }

  .callout {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 8px 0 4px;
    border: 1px solid var(--primary);
    border-radius: var(--radius-control);
    background: var(--surface);
    box-shadow: 0 2px 8px rgba(26, 29, 33, 0.16);
    color: var(--ink);
    font: 13px/1.4 var(--font-ui);
    white-space: nowrap;
    pointer-events: none;
  }
  .callout .note {
    color: var(--ink-3);
  }
  .popover {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    height: 40px;
    padding: 0 8px 0 12px;
    border: 1px solid var(--primary);
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    color: var(--ink);
    font: 13px/1.4 var(--font-ui);
    white-space: nowrap;
    cursor: default;
  }
  .popover .to {
    color: var(--ink-3);
  }
  .popover .seg button {
    height: 26px;
    padding: 0 10px;
  }
  .popover .remove {
    width: 26px;
    height: 26px;
    color: var(--danger);
  }

  /* 縮尺と出典は、浮かせた板を避けて置く。避ける幅は、地図を置く側が変数で渡す。 */
  .map :global(.maplibregl-ctrl-bottom-left) {
    bottom: 2px;
    left: var(--map-left, 0);
  }
  .map :global(.maplibregl-ctrl-bottom-right) {
    right: var(--map-right, 0);
    bottom: 2px;
  }
  .map :global(.maplibregl-ctrl-scale) {
    border-color: var(--ink-3);
    background: rgba(255, 255, 255, 0.7);
    color: var(--ink-3);
    font: 11px/1.2 var(--font-mono);
  }
  .map :global(.maplibregl-ctrl-attrib) {
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.85);
    color: var(--ink-3);
    font: 11px/1.6 var(--font-ui);
  }
  .map :global(.maplibregl-ctrl-attrib a) {
    color: inherit;
  }
</style>
