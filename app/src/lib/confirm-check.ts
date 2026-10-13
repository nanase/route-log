/** 動画を見ながらの時刻の確定を、画面を人が見なくても確かめる。キーは実際のキーイベントで送る。 */

import { invoke } from "@tauri-apps/api/core";
import { confirmTyped } from "./events";
import { clock, jstToMs } from "./time";
import type { LonLat, RideEvent, Timeline } from "./types";

interface PlayerHandle {
  seek(realMs: number): Promise<void>;
  now(): number;
}

interface MapHandle {
  jump(center: LonLat, zoom: number): void;
  click(at: LonLat): void;
  idle(timeoutMs?: number): Promise<boolean>;
  snapshot(): Record<string, unknown>;
}

interface AppHandle {
  events(): RideEvent[];
  target(): string | null;
  choose(id: string): void;
  setClockOffsetSeconds(seconds: number): Promise<void>;
  changeEvent(id: string, change: (event: RideEvent) => RideEvent): Promise<void>;
  /** 保存と時刻の推定が済むまで待つ */
  settled(): Promise<void>;
  rideDate(): string;
  message(): string;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const key = (name: string) => window.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true }));

export async function runConfirmCheck(timeline: Timeline, player: PlayerHandle, map: MapHandle, app: AppHandle) {
  const steps: Record<string, unknown>[] = [];
  const shown = () =>
    app
      .events()
      .filter((e) => e.status !== "hidden")
      .map((e) => `${e.time_ms === null ? "--:--:--" : clock(e.time_ms)} ${(e.route_m / 1000).toFixed(2)}km ${e.kind} ${e.label} ${e.status}`);
  const note = (step: string, expected: string, extra: Record<string, unknown> = {}) =>
    steps.push({ step, expected, events: shown(), target: targetLabel(), ...extra });
  const targetLabel = () => app.events().find((e) => e.id === app.target())?.label ?? null;
  const find = (kind: string) => app.events().find((e) => e.kind === kind);

  // 碓氷峠。軽井沢町から安中市へ、県境をまたぐ線を引く。
  map.jump([138.66, 36.35], 12);
  await map.idle();
  map.click([138.62, 36.345]);
  await sleep(300);
  map.click([138.7, 36.36]);
  await sleep(2500);
  note("線を引く", "出発・県境・市区町村境・到着が並び、どれも時刻が無い。次の相手は出発");

  const start = timeline.spans[0].start_ms;
  await player.seek(start + 60_000);
  await sleep(1500);
  key("Enter");
  await app.settled();
  note("1 分の所で Enter", `出発が ${clock(start + 60_000)} で確定し、次の相手が県境に移る`);

  const arrival = find("arrival");
  if (arrival) app.choose(arrival.id);
  await player.seek(start + 60_000 + 1_200_000);
  await sleep(1500);
  key("Enter");
  await app.settled();
  const length = find("arrival")?.route_m ?? 0;
  const boundary = find("pref_boundary");
  const expectedBoundary = boundary ? start + 60_000 + (1_200_000 * boundary.route_m) / length : 0;
  note("到着を選び、21 分の所で Enter", `到着が確定し、県境の推定が ${clock(expectedBoundary)} になる`, {
    here: map.snapshot().here,
  });

  await player.seek(start + 60_000 + 300_000);
  await sleep(1500);
  key("a");
  await app.settled();
  note("6 分の所で A", `手動のイベントが ${clock(start + 360_000)} にでき、位置は全体の 4 分の 1（${(length / 4000).toFixed(2)} km）`, {
    here: map.snapshot().here,
  });

  key("p");
  await sleep(1500);
  note("P", "次の相手が 1 つ前のイベントに移り、動画がその時刻へ動く", { now: clock(player.now()) });

  await app.setClockOffsetSeconds(90);
  await app.settled();
  await sleep(600);
  note("時計の補正を +90 秒", "動画で確定した出発・到着・手動と、その間の推定が、どれも 90 秒進む");

  const typed = jstToMs(app.rideDate(), "12:00:00");
  if (boundary && typed !== null) await app.changeEvent(boundary.id, (event) => confirmTyped(event, typed));
  await app.settled();
  await app.setClockOffsetSeconds(0);
  await app.settled();
  await sleep(600);
  note("県境に 12:00:00 を手で入れ、補正を 0 に戻す", "県境は 12:00:00 のまま。出発と到着は 90 秒戻る。順序が合わないので警告が出る");

  const captured = await invoke<string>("capture_check", { name: "confirm" }).catch((e) => `写せない: ${e}`);

  key("Backspace");
  await app.settled();
  note("Backspace", "最後に Enter で確定した到着の確定が外れる");

  // 再生のキー。動いた量を、動画の時刻の差で見る。
  const moved = async (keys: string[], waitMs: number) => {
    await player.seek(start + 600_000);
    await sleep(1500);
    const from = player.now();
    for (const name of keys) key(name);
    await sleep(waitMs);
    const seconds = (player.now() - from) / 1000;
    key("k");
    await sleep(300);
    return Number(seconds.toFixed(1));
  };
  const transport = {
    "L を 1 回、3 秒（3 秒ほど進む）": await moved(["l"], 3000),
    "L を 2 回、3 秒（6 秒ほど進む）": await moved(["l", "l"], 3000),
    "J を 1 回、3 秒（3 秒ほど戻る）": await moved(["j"], 3000),
    "J を 2 回、3 秒（6 秒ほど戻る）": await moved(["j", "j"], 3000),
    "→ を 1 回（1 秒進む）": await moved(["ArrowRight"], 1500),
    "K のあと 2 秒（動かない）": await moved(["l", "k"], 2000),
  };

  return { steps, transport, captures: [captured], message: app.message() };
}
