import { describe, expect, test } from "bun:test";
import {
  confirmOnVideo,
  confirmTyped,
  customAt,
  defaultTarget,
  knotsOf,
  neighbor,
  nextConfirmable,
  patch,
  positionAt,
  unconfirm,
} from "./events";
import type { EventStatus, RideEvent } from "./types";

function event(id: string, route_m: number, status: EventStatus, time_s: number | null = null): RideEvent {
  return { id, kind: "tunnel", label: id, route_m, time_ms: time_s === null ? null : time_s * 1000, status, detail: {} };
}

const ids = (events: (RideEvent | null)[]) => events.map((e) => e?.id ?? null);

describe("いまの位置", () => {
  const knots = [
    { route_m: 0, time_ms: 0 },
    { route_m: 1000, time_ms: 100_000 },
    { route_m: 1000, time_ms: 400_000 },
    { route_m: 2000, time_ms: 500_000 },
  ];

  test("基準の間は時刻で補間する", () => {
    expect(positionAt(knots, 50_000)).toBe(500);
    expect(positionAt(knots, 450_000)).toBe(1500);
  });

  test("止まっていた間は同じ位置にいる", () => {
    expect(positionAt(knots, 250_000)).toBe(1000);
  });

  test("基準の外は分からない", () => {
    expect(positionAt(knots, -1)).toBeNull();
    expect(positionAt(knots, 500_001)).toBeNull();
    expect(positionAt([], 0)).toBeNull();
  });

  test("基準になるのは、確定していて時刻から置いたものでないイベントだけ", () => {
    const memo: RideEvent = { ...event("memo", 300, "confirmed", 30), detail: { by_time: true } };
    const events = [event("d", 0, "confirmed", 0), memo, event("x", 500, "estimated", 50), event("h", 600, "hidden", 60)];
    expect(knotsOf(events)).toEqual([{ route_m: 0, time_ms: 0 }]);
  });
});

describe("次に確定する相手", () => {
  test("推定の無いうちは、位置の順で最初の未確定のもの", () => {
    const events = [event("d", 0, "estimated"), event("x", 500, "estimated")];
    expect(defaultTarget(events, 0)?.id).toBe("d");
    const started = [event("d", 0, "confirmed", 10), event("x", 500, "estimated")];
    expect(defaultTarget(started, 20_000)?.id).toBe("x");
  });

  test("推定があれば、いまより後の最初のもの", () => {
    const events = [
      event("d", 0, "confirmed", 0),
      event("x", 500, "estimated", 50),
      event("y", 800, "estimated", 80),
      event("a", 1000, "confirmed", 100),
    ];
    expect(defaultTarget(events, 60_000)?.id).toBe("y");
    expect(defaultTarget(events, 90_000)).toBeNull();
  });

  test("字幕にしないものと、時刻から置いたものは相手にしない", () => {
    const memo: RideEvent = { ...event("memo", 100, "estimated"), detail: { by_time: true } };
    const events = [event("h", 0, "hidden"), memo, event("x", 500, "estimated")];
    expect(defaultTarget(events, 0)?.id).toBe("x");
  });

  test("確定したら、その後ろの未確定のものへ進む", () => {
    const events = [event("d", 0, "confirmed", 0), event("h", 100, "hidden"), event("x", 500, "estimated")];
    expect(nextConfirmable(events, "d")?.id).toBe("x");
    expect(nextConfirmable(events, "x")).toBeNull();
  });

  test("隣へ移るときは、字幕にしないものを飛ばす", () => {
    const events = [event("d", 0, "confirmed", 0), event("h", 100, "hidden"), event("x", 500, "estimated")];
    expect(ids([neighbor(events, "d", 1), neighbor(events, "x", -1), neighbor(events, "x", 1)])).toEqual(["x", "d", null]);
    expect(ids([neighbor(events, null, 1), neighbor(events, null, -1)])).toEqual(["d", null]);
  });
});

describe("確定", () => {
  test("動画で確定すると、カメラの時計での時刻も残る", () => {
    const confirmed = confirmOnVideo(event("x", 500, "estimated"), 100_000, 30_000);
    expect(confirmed).toMatchObject({ status: "confirmed", time_ms: 100_000, detail: { camera_ms: 70_000 } });
  });

  test("手で入れた時刻は、カメラの時刻を持たない", () => {
    const onVideo = confirmOnVideo(event("x", 500, "estimated"), 100_000, 30_000);
    const typed = confirmTyped(onVideo, 200_000);
    expect(typed).toMatchObject({ status: "confirmed", time_ms: 200_000 });
    expect("camera_ms" in typed.detail).toBe(false);
  });

  test("確定を外すと、時刻もカメラの時刻も消える", () => {
    const back = unconfirm(confirmOnVideo(event("x", 500, "estimated"), 100_000, 0));
    expect(back).toMatchObject({ status: "estimated", time_ms: null });
    expect("camera_ms" in back.detail).toBe(false);
  });

  test("手動のイベントは、時刻から置く印を持つ", () => {
    const memo = customAt(100_000, 30_000, "メモ");
    expect(memo).toMatchObject({ kind: "custom", status: "confirmed", time_ms: 100_000 });
    expect(memo.detail).toEqual({ by_time: true, camera_ms: 70_000 });
    expect(memo.id).not.toBe("");
  });

  test("書き換えるのは名指しした 1 件だけ", () => {
    const events = [event("d", 0, "estimated"), event("x", 500, "estimated")];
    const patched = patch(events, "x", (e) => ({ ...e, label: "白地トンネル" }));
    expect(patched.map((e) => e.label)).toEqual(["d", "白地トンネル"]);
    expect(events[1].label).toBe("x");
  });
});
