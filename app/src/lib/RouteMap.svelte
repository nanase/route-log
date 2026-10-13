<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import type { Feature } from "geojson";
  import maplibregl from "maplibre-gl";
  import "maplibre-gl/dist/maplibre-gl.css";
  import { Protocol } from "pmtiles";
  import { onMount, untrack } from "svelte";
  import { distanceM } from "./geo";
  import {
    appendWaypoint,
    insertVertex,
    line,
    moveVertex,
    moveWaypoint,
    type Path,
    pieces,
    removeVertex,
    removeWaypoint,
    toggleSnap,
  } from "./route";
  import type { LonLat, RoadData } from "./types";

  /**
   * 地図の上で経路を編集する。
   *
   * - 地図をクリックすると、終わりに経由点を足す
   * - 経由点と頂点はドラッグで動かし、右クリックで消す
   * - 線の途中の小さな丸をドラッグすると、そこに頂点を足す
   * - Shift を押しながら経由点をクリックすると、次の経由点までの結び方を吸着と直線で切り替える
   */
  let {
    path,
    rideId,
    roads = null,
    marks = [],
    here = null,
    snap = false,
    onsnap,
    onchange,
  }: {
    path: Path;
    /** 走行が変わったら、経路の全体が見えるように地図を動かす */
    rideId: string | null;
    roads?: RoadData | null;
    /** イベントの位置。地図に点で出す */
    marks?: { at: LonLat; label: string; hidden: boolean }[];
    /** 新しく足す区間を、道に沿わせるか */
    snap?: boolean;
    /** 点を、`radiusM` 以内でいちばん近い道の上へ寄せる。近くに道が無ければ null を返す */
    onsnap?: (at: LonLat, radiusM: number) => Promise<LonLat | null>;
    /** 動画のいまの時刻にいると推定される位置 */
    here?: LonLat | null;
    onchange: (path: Path) => void;
  } = $props();

  const VIEW_KEY = "routelog.map-view";
  /** これより短い区切りには、頂点を足す丸を出さない（画面上の px） */
  const MIN_PIECE_PX = 48;
  const GSI = "https://cyberjapandata.gsi.go.jp/xyz/pale/{z}/{x}/{y}.png";

  let container: HTMLDivElement;
  let map: maplibregl.Map | undefined;
  let ready = $state(false);
  let markers: maplibregl.Marker[] = [];
  const errors: string[] = [];

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

  /** 点を、画面上で近い道の上へ寄せる。寄せる先が無ければ、そのままにする。 */
  async function onRoad(at: LonLat): Promise<LonLat> {
    if (map === undefined || onsnap === undefined) return at;
    const pixel = map.project(at);
    const edge = map.unproject([pixel.x + SNAP_PX, pixel.y]).toArray();
    return (await onsnap(at, distanceM(at, edge))) ?? at;
  }

  const lineData = (points: LonLat[]): Feature => ({
    type: "Feature",
    properties: {},
    geometry: { type: "LineString", coordinates: points },
  });

  function draw(next: Path) {
    (map?.getSource("route") as maplibregl.GeoJSONSource | undefined)?.setData(lineData(line(next)));
  }

  function handle(kind: "waypoint" | "waypoint snap" | "vertex" | "middle", at: LonLat, label = "") {
    const element = document.createElement("div");
    element.className = `handle ${kind}`;
    element.textContent = label;
    // 丸の上のクリックで、地図に経由点を足さない。
    element.addEventListener("click", (event) => {
      event.stopPropagation();
      if (event.shiftKey) onshiftclick?.();
    });
    let onshiftclick: (() => void) | undefined;
    const marker = new maplibregl.Marker({ element, draggable: true }).setLngLat(at).addTo(map!);
    markers.push(marker);
    return {
      /** ドラッグの間は線だけを描き直し、離したときに確定する */
      /** `settle` があれば、離した位置をそれで直してから確定する */
      ondrag(moved: (to: LonLat) => Path, settle?: (to: LonLat) => Promise<LonLat>) {
        const to = (): LonLat => marker.getLngLat().toArray();
        marker.on("drag", () => draw(moved(to())));
        marker.on("dragend", async () => onchange(moved(settle ? await settle(to()) : to())));
      },
      onshiftclick(changed: () => Path) {
        onshiftclick = () => onchange(changed());
      },
      onremove(removed: () => Path) {
        element.addEventListener("contextmenu", (event) => {
          event.preventDefault();
          event.stopPropagation();
          onchange(removed());
        });
      },
    };
  }

  function place(current: Path) {
    for (const marker of markers) marker.remove();
    markers = [];
    if (map === undefined) return;
    // 吸着の区間の線は道が決めるので、頂点も、頂点を足す丸も出さない。
    const snapped = (segment: number) => current.waypoints[segment].segment_mode === "snap";
    for (const piece of pieces(current)) {
      if (snapped(piece.segment)) continue;
      const [from, to] = [map.project(piece.from), map.project(piece.to)];
      if (from.dist(to) < MIN_PIECE_PX) continue;
      const middle = map.unproject(from.add(to).div(2)).toArray();
      handle("middle", middle).ondrag((at) => insertVertex(current, piece.segment, piece.index, at));
    }
    current.route.segments.forEach((vertices, segment) => {
      if (snapped(segment)) return;
      vertices.forEach((vertex, index) => {
        const vertexHandle = handle("vertex", vertex);
        vertexHandle.ondrag((to) => moveVertex(current, segment, index, to));
        vertexHandle.onremove(() => removeVertex(current, segment, index));
      });
    });
    current.waypoints.forEach((waypoint, index) => {
      const kind = waypoint.segment_mode === "snap" ? "waypoint snap" : "waypoint";
      const waypointHandle = handle(kind, [waypoint.lon, waypoint.lat], String(index + 1));
      waypointHandle.onshiftclick(() => toggleSnap(current, index));
      // 前後のどちらかが吸着の区間なら、離した位置を道の上へ寄せる。
      const beside = snapped(index) || (index > 0 && snapped(index - 1));
      waypointHandle.ondrag((to) => moveWaypoint(current, index, to), beside ? onRoad : undefined);
      waypointHandle.onremove(() => removeWaypoint(current, index));
    });
  }

  function fit(current: Path) {
    const points = line(current);
    if (map === undefined || points.length === 0) return;
    if (points.length === 1) {
      map.jumpTo({ center: points[0] });
      return;
    }
    const bounds = points.reduce((box, p) => box.extend(p), new maplibregl.LngLatBounds(points[0], points[0]));
    map.fitBounds(bounds, { padding: 60, maxZoom: 15, animate: false });
  }

  onMount(() => {
    const protocol = new Protocol();
    maplibregl.addProtocol("pmtiles", protocol.tile);
    const created = new maplibregl.Map({
      container,
      style: {
        version: 8,
        sources: {
          base: {
            type: "raster",
            tiles: [GSI],
            tileSize: 256,
            minzoom: 5,
            maxzoom: 18,
            attribution: '<a href="https://maps.gsi.go.jp/development/ichiran.html">地理院タイル</a>',
          },
        },
        layers: [{ id: "base", type: "raster", source: "base" }],
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
    created.addControl(new maplibregl.NavigationControl({ showCompass: false }));
    created.addControl(new maplibregl.ScaleControl());
    created.on("error", (event) => errors.push(String(event.error?.message ?? event.error)));
    created.on("load", () => {
      created.addSource("route", { type: "geojson", data: lineData([]) });
      const round = { "line-join": "round", "line-cap": "round" } as const;
      created.addLayer({
        id: "route-casing",
        type: "line",
        source: "route",
        layout: round,
        paint: { "line-color": "#fff", "line-width": 7 },
      });
      created.addLayer({
        id: "route",
        type: "line",
        source: "route",
        layout: round,
        paint: { "line-color": "#e03131", "line-width": 4 },
      });
      created.addSource("marks", { type: "geojson", data: { type: "FeatureCollection", features: [] } });
      created.addLayer({
        id: "marks",
        type: "circle",
        source: "marks",
        paint: {
          "circle-radius": 5,
          "circle-color": ["case", ["get", "hidden"], "#adb5bd", "#f59f00"],
          "circle-stroke-color": "#fff",
          "circle-stroke-width": 1.5,
        },
      });
      ready = true;
    });
    created.on("click", async (event) => {
      const clicked = event.lngLat.toArray();
      if (!snap) {
        onchange(appendWaypoint(path, clicked));
        return;
      }
      const at = await onRoad(clicked);
      onchange(appendWaypoint(path, at, "snap"));
    });
    created.on("moveend", () => {
      localStorage.setItem(VIEW_KEY, JSON.stringify({ center: created.getCenter().toArray(), zoom: created.getZoom() }));
    });
    // 頂点を足す丸は画面上の長さで出し分けるので、縮尺が変わったら置き直す。
    created.on("zoomend", () => place(path));
    return () => {
      created.remove();
      maplibregl.removeProtocol("pmtiles");
    };
  });

  $effect(() => {
    if (!ready) return;
    draw(path);
    place(path);
  });

  $effect(() => {
    void rideId;
    if (ready) untrack(() => fit(path));
  });

  $effect(() => {
    if (!ready) return;
    const features: Feature[] = marks.map((mark) => ({
      type: "Feature",
      properties: { label: mark.label, hidden: mark.hidden },
      geometry: { type: "Point", coordinates: mark.at },
    }));
    (map?.getSource("marks") as maplibregl.GeoJSONSource | undefined)?.setData({ type: "FeatureCollection", features });
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
  });

  // 道路の線は経路の下に重ねる。
  $effect(() => {
    if (!ready || roads === null || map === undefined || map.getSource("national")) return;
    const overlay = (id: "national" | "prefectural", file: string, color: string, width: number) => {
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
          layout: { "line-join": "round", "line-cap": "round" },
          paint: { "line-color": color, "line-width": width, "line-opacity": 0.55 },
        },
        "route-casing",
      );
    };
    overlay("prefectural", roads.prefectural, "#2f9e44", 2);
    overlay("national", roads.national, "#1c7ed6", 3);
  });

  let popup: maplibregl.Popup | undefined;

  /** その位置を地図のまん中に出し、名前を添える。 */
  export function focus(at: LonLat, label: string) {
    if (map === undefined) return;
    popup?.remove();
    popup = new maplibregl.Popup({ closeButton: false, offset: 10 }).setLngLat(at).setText(label).addTo(map);
    map.easeTo({ center: at, zoom: Math.max(map.getZoom(), 13) });
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

  /** `from` にいちばん近い丸を `to` までドラッグしたことにする。 */
  export function drag(from: LonLat, to: LonLat) {
    if (map === undefined || markers.length === 0) return;
    const canvas = map.getCanvasContainer();
    const middle: LonLat = [(from[0] + to[0]) / 2, (from[1] + to[1]) / 2];
    mouse("mousedown", from, nearest(from).getElement());
    mouse("mousemove", middle, canvas);
    mouse("mousemove", to, canvas);
    mouse("mouseup", to, canvas);
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
    return {
      zoom: map?.getZoom() ?? null,
      center: map?.getCenter().toArray() ?? null,
      tilesLoaded: map?.areTilesLoaded() ?? false,
      handles: {
        waypoint: container.querySelectorAll(".handle.waypoint").length,
        vertex: container.querySelectorAll(".handle.vertex").length,
        middle: container.querySelectorAll(".handle.middle").length,
      },
      roads: { national: count("national"), prefectural: count("prefectural") },
      here: hereMarker?.getLngLat().toArray() ?? null,
      errors: [...errors],
    };
  }
</script>

<div class="map" bind:this={container} oncontextmenu={(event) => event.preventDefault()} role="application"></div>

<style>
  .map {
    width: 100%;
    height: 100%;
  }
  .map :global(.handle) {
    box-sizing: border-box;
    border-radius: 50%;
    cursor: grab;
  }
  .map :global(.handle.waypoint) {
    width: 22px;
    height: 22px;
    border: 2px solid #fff;
    background: #e03131;
    box-shadow: 0 0 2px #000;
    color: #fff;
    font: bold 11px/18px system-ui, sans-serif;
    text-align: center;
  }
  .map :global(.handle.waypoint.snap) {
    background: #1c7ed6;
  }
  .map :global(.handle.vertex) {
    width: 14px;
    height: 14px;
    border: 2px solid #e03131;
    background: #fff;
  }
  .map :global(.here) {
    width: 16px;
    height: 16px;
    box-sizing: border-box;
    border: 3px solid #fff;
    border-radius: 50%;
    background: #1c7ed6;
    box-shadow: 0 0 0 2px #1c7ed6;
    pointer-events: none;
  }
  .map :global(.handle.middle) {
    width: 10px;
    height: 10px;
    border: 1px solid #e03131;
    background: #fff;
    opacity: 0.6;
  }
</style>
