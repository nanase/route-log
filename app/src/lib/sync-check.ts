/** クラウドとの同期を、手元で動かした Worker に対して確かめる。 */

import { invoke } from "@tauri-apps/api/core";
import type { LonLat, RideDocument, RideSummary } from "./types";

interface MapHandle {
  jump(center: LonLat, zoom: number): void;
  click(at: LonLat): void;
  idle(timeoutMs?: number): Promise<boolean>;
}

interface PanelHandle {
  refresh(): Promise<void>;
  push(id: string, overwrite?: boolean): Promise<void>;
  pull(id: string): Promise<void>;
  remove(id: string, where: { local: boolean; cloud: boolean }): Promise<void>;
  snapshot(): { rows: string[]; message: string };
}

interface AppHandle {
  create(rideDate: string): Promise<void>;
  setTitle(title: string): Promise<void>;
  doc(): RideDocument | null;
  /** 走行の切り替えの板を開き、その口を返す。同期の操作は、この板にある */
  openRides(): Promise<PanelHandle>;
  /** 手元に保存してある走行 */
  list(): Promise<RideSummary[]>;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function runSyncCheck(apiUrl: string, map: MapHandle, app: AppHandle) {
  const steps: Record<string, unknown>[] = [];
  await invoke("set_sync_settings", { apiUrl, clientId: "", clientSecret: "" });

  // 日付を毎回変えて、前の確認が残したクラウドの走行と重ならないようにする。
  const day = String(1 + (Math.floor(Date.now() / 1000) % 28)).padStart(2, "0");
  const date = `2031-01-${day}`;
  await app.create(date);
  map.jump([138.66, 36.35], 12);
  await map.idle();
  map.click([138.62, 36.345]);
  await sleep(300);
  map.click([138.7, 36.36]);
  await sleep(1500);
  await app.setTitle("同期の確認");

  const id = app.doc()?.ride.id ?? "";
  const panel = await app.openRides();
  const mine = () => panel.snapshot().rows.filter((row) => row.startsWith(date));
  const note = (step: string, expected: string) =>
    steps.push({ step, expected, rows: mine(), message: panel.snapshot().message, title: app.doc()?.ride.title });

  await panel.refresh();
  note("作ったばかりの走行", "手元だけ（local_only）");

  await panel.push(id);
  note("送る", "同じ（same）");

  await app.setTitle("同期の確認（手元で直した）");
  await panel.refresh();
  note("手元で題を直す", "手元が新しい（local_newer）");

  await panel.push(id);
  note("もう一度送る", "同じ（same）");

  await app.setTitle("送っていない題");
  await panel.refresh();
  note("送らずに題を直す", "手元が新しい（local_newer）");

  await panel.pull(id);
  note("受け取る", "同じ（same）。題が、送ってあった「同期の確認（手元で直した）」に戻る");

  await app.setTitle("送っていない題");
  await panel.refresh();
  const captured = await invoke<string>("capture_check", { name: "sync" }).catch((e) => `写せない: ${e}`);
  const local = async () => (await app.list()).some((ride) => ride.id === id);

  await panel.remove(id, { local: false, cloud: true });
  steps.push({
    step: "クラウドからだけ消す",
    expected: "手元だけ（local_only）。手元の記録は残る",
    rows: mine(),
    message: panel.snapshot().message,
    local: await local(),
  });

  await panel.push(id);
  note("送り直す", "同じ（same）");

  await panel.remove(id, { local: true, cloud: true });
  steps.push({
    step: "手元からもクラウドからも消す",
    expected: "行が無くなる。手元の記録も無くなり、開いていた走行は閉じる",
    rows: mine(),
    message: panel.snapshot().message,
    local: await local(),
    open: app.doc()?.ride.id ?? null,
  });
  return { steps, captures: [captured] };
}
