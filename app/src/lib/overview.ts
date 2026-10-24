/**
 * 経路の概略図の動画。範囲の選び方と、動きの設定、右下に入れる出典。
 *
 * 絵は Rust 側（routelog-overview）が描く。出典の文字だけは、アプリと同じ字で描けるよう、ここで
 * 画像にして渡す。
 */

import { waypointEventAt } from "./events";
import { segmentLengths } from "./route";
import type { RideDocument } from "./types";

/**
 * 選べるフレームレート。半端な値も正しく持てるよう、分数で持つ。既定の 59.94 は、Resolve の
 * タイムラインと、ドラレコの動画に合わせた値。
 */
export const FRAME_RATES = {
  "23.976": [24000, 1001],
  "24": [24, 1],
  "29.97": [30000, 1001],
  "30": [30, 1],
  "59.94": [60000, 1001],
  "60": [60, 1],
} as const satisfies Record<string, readonly [number, number]>;
export type FrameRate = keyof typeof FRAME_RATES;

/** 動きの設定。次に開いたときも使う。 */
export interface Motion {
  /** フレームレート。FRAME_RATES の名前 */
  fps: FrameRate;
  /** 寄っていく時間（秒） */
  zoomSeconds: number;
  /** 止まってから見せる時間（秒） */
  holdSeconds: number;
  /** 最初の見え方の大きさ。止まった所に対する比 */
  startScale: number;
}

export const MOTION_DEFAULT: Motion = { fps: "59.94", zoomSeconds: 5, holdSeconds: 5, startScale: 0.25 };
// 既定の値を変えたときに、前の既定の値が覚えたものとして残らないよう、名前を変えた。
const MOTION_KEY = "routelog.overview-motion-2";

/** 覚えている動きの設定。無いか壊れていれば既定の値。 */
export function rememberedMotion(): Motion {
  try {
    const saved = JSON.parse(localStorage.getItem(MOTION_KEY) ?? "");
    // フレームレートは後から足した。覚えていなければ、既定の値にする。
    const ok = (key: Exclude<keyof Motion, "fps">) => typeof saved[key] === "number" && Number.isFinite(saved[key]);
    if (ok("zoomSeconds") && ok("holdSeconds") && ok("startScale")) return clampMotion(saved);
  } catch {
    // 覚えていないか、壊れている。
  }
  return MOTION_DEFAULT;
}

export function rememberMotion(motion: Motion) {
  localStorage.setItem(MOTION_KEY, JSON.stringify(motion));
}

/** 受け付ける範囲に収める。Rust 側も同じ範囲に収める。 */
export function clampMotion(motion: Motion): Motion {
  const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);
  return {
    fps: Object.hasOwn(FRAME_RATES, motion.fps) ? motion.fps : MOTION_DEFAULT.fps,
    zoomSeconds: clamp(motion.zoomSeconds, 0, 30),
    holdSeconds: clamp(motion.holdSeconds, 0, 120),
    startScale: clamp(motion.startScale, 0.1, 1),
  };
}

/** Rust 側に渡す形。フレームレートは分子と分母にする。 */
export function motionInput(motion: Motion) {
  return { ...motion, fps: FRAME_RATES[motion.fps] };
}

/** 範囲の端に選べる経由点。名前と、始点からの距離（km）。 */
export interface Endpoint {
  index: number;
  name: string;
  km: number;
}

/**
 * 経由点の一覧。最初と最後は出発と到着の名前、途中は経由点のイベントに付けた名前を使う。
 * イベントが無ければ「経由点 N」。番号は地図の丸の番号と同じ。
 */
export function endpointsOf(doc: RideDocument): Endpoint[] {
  const lengths = segmentLengths(doc);
  let walked = 0;
  return doc.waypoints.map((waypoint, index) => {
    const km = walked / 1000;
    walked += lengths[index] ?? 0;
    const last = doc.waypoints.length - 1;
    const kind = index === 0 ? "departure" : index === last ? "arrival" : null;
    const event =
      kind === null
        ? waypointEventAt(doc.events, [waypoint.lon, waypoint.lat])
        : (doc.events.find((e) => e.kind === kind) ?? null);
    const fallback = index === 0 ? "出発" : index === last ? "到着" : `経由点 ${index + 1}`;
    return { index, name: event?.label || fallback, km };
  });
}

/** 書き出すファイルの名前。経路の一部なら、経由点の番号を添える。 */
export function fileName(title: string, from: number, to: number, count: number): string {
  const whole = from === 0 && to === count - 1;
  return whole ? `${title} 概略図.mp4` : `${title} 概略図 ${from + 1}-${to + 1}.mp4`;
}

/** 出典。全国最新写真だけを使ったとき */
const PHOTO_CREDIT = ["地理院タイル（全国最新写真（シームレス））を加工して作成"];
/** 縮尺が小さく、衛星画像（Landsat 8）のタイルも使ったとき。国土地理院が求める書き方に従う */
const LANDSAT_CREDIT = [
  ...PHOTO_CREDIT,
  "データソース：Landsat8画像（GSI,TSIC,GEO Grid/AIST）, Landsat8画像（courtesy of the U.S. Geological Survey）, 海底地形（GEBCO）",
];

/** 出典の画像を作った画面の幅（px）。Rust 側は、この幅に対する比で縮めて置く */
const BASE_WIDTH = 3840;
const FONT_PX = 30;
const LINE_PX = 42;
const PAD = [12, 18] as const;
const FONT = `${FONT_PX}px "BIZ UDPGothic", sans-serif`;

/** 出典の 2 とおりの画像（PNG）。Rust 側が、使ったタイルに合わせて選ぶ。 */
export interface CreditImages {
  photo: number[];
  landsat: number[];
}

export async function creditImages(): Promise<CreditImages> {
  await document.fonts.load(FONT);
  const [photo, landsat] = await Promise.all([creditPng(PHOTO_CREDIT), creditPng(LANDSAT_CREDIT)]);
  return { photo, landsat };
}

/** 半透明の黒い板に、白い字で右寄せに書く。 */
async function creditPng(lines: string[]): Promise<number[]> {
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (context === null) throw new Error("出典を描けません");
  context.font = FONT;
  const width = Math.min(Math.ceil(Math.max(...lines.map((t) => context.measureText(t).width))) + PAD[1] * 2, BASE_WIDTH);
  canvas.width = width;
  canvas.height = lines.length * LINE_PX + PAD[0] * 2;
  context.fillStyle = "rgba(0, 0, 0, 0.5)";
  context.beginPath();
  context.roundRect(0, 0, canvas.width, canvas.height, 8);
  context.fill();
  context.font = FONT;
  context.fillStyle = "#fff";
  context.textAlign = "right";
  context.textBaseline = "middle";
  lines.forEach((text, i) => context.fillText(text, width - PAD[1], PAD[0] + LINE_PX * (i + 0.5)));
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (blob === null) throw new Error("出典を描けません");
  return Array.from(new Uint8Array(await blob.arrayBuffer()));
}
