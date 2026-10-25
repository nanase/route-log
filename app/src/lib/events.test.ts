import { describe, expect, test } from "bun:test";
import {
  aheadAt,
  asideOf,
  confirmOnVideo,
  confirmTyped,
  customAt,
  defaultTarget,
  knotsOf,
  neighbor,
  nextConfirmable,
  patch,
  pinStop,
  positionAt,
  closeStop,
  insertByPosition,
  openStop,
  placeOf,
  retimeStop,
  setHidden,
  stopBetween,
  stopsOf,
  timeAt,
  unconfirm,
  unpinStop,
  waypointEvent,
  waypointEventAt,
} from "./events";
import type { EventStatus, RideEvent } from "./types";

function event(id: string, route_m: number, status: EventStatus, time_s: number | null = null): RideEvent {
  return { id, kind: "tunnel", label: id, route_m, time_ms: time_s === null ? null : time_s * 1000, status, detail: {} };
}

/** 字幕にしないイベント。 */
function silent(id: string, route_m: number, status: EventStatus = "estimated", time_s: number | null = null): RideEvent {
  return { ...event(id, route_m, status, time_s), hidden: true };
}

const ids = (events: (RideEvent | null)[]) => events.map((e) => e?.id ?? null);

describe("その位置を通る時刻", () => {
  const knot = (route_m: number, time_s: number) => ({ route_m, time_ms: time_s * 1000 });

  test("基準の間は距離で補間する", () => {
    const knots = [knot(0, 0), knot(1000, 100), knot(3000, 500)];
    expect(timeAt(knots, [], 500)).toBe(50_000);
    expect(timeAt(knots, [], 2000)).toBe(300_000);
    expect(timeAt(knots, [], 1000)).toBe(100_000);
  });

  test("基準の外は推定しない", () => {
    const knots = [knot(1000, 100), knot(3000, 500)];
    expect(timeAt(knots, [], 999)).toBeNull();
    expect(timeAt(knots, [], 3001)).toBeNull();
    expect(timeAt([knot(1000, 100)], [], 1000)).toBe(100_000);
    expect(timeAt([], [], 0)).toBeNull();
  });

  test("基準と末尾の桁だけ違う位置は、基準と同じ所とみなす", () => {
    const knots = [knot(0, 0), knot(3000, 500)];
    expect(timeAt(knots, [], 3000.0004)).toBe(500_000);
    expect(timeAt(knots, [], 3000.01)).toBeNull();
  });

  test("停車の間は進まず、停車した場所は着いた時刻にする", () => {
    // 0 秒に 0 m、1000 秒に 4000 m。その間の 200 秒から 800 秒まで停車していた。
    const knots = [knot(0, 0), knot(4000, 1000)];
    const stops = [{ start_ms: 200_000, end_ms: 800_000 }];
    expect(timeAt(knots, stops, 1000)).toBe(100_000);
    expect(timeAt(knots, stops, 2000)).toBe(200_000);
    expect(timeAt(knots, stops, 3000)).toBe(900_000);
  });
});

describe("いまの位置", () => {
  const knots = [
    { route_m: 0, time_ms: 0 },
    { route_m: 1000, time_ms: 100_000 },
    { route_m: 1000, time_ms: 400_000 },
    { route_m: 2000, time_ms: 500_000 },
  ];

  test("基準の間は時刻で補間する", () => {
    expect(positionAt(knots, [], 50_000)).toBe(500);
    expect(positionAt(knots, [], 450_000)).toBe(1500);
  });

  test("止まっていた間は同じ位置にいる", () => {
    expect(positionAt(knots, [], 250_000)).toBe(1000);
  });

  test("基準の外は分からない", () => {
    expect(positionAt(knots, [], -1)).toBeNull();
    expect(positionAt(knots, [], 500_001)).toBeNull();
    expect(positionAt([], [], 0)).toBeNull();
  });

  test("最後の基準より後は、直前の速さで進んだ仮の位置を出す", () => {
    // 最後の組の速さは 10 m/s。
    expect(aheadAt(knots, [], 510_000)).toBe(2100);
    expect(aheadAt(knots, [], 500_000)).toBeNull();
    expect(aheadAt(knots, [], 250_000)).toBeNull();
    // 最後の組が同じ位置なら、止まっていたものとして、その前の組の速さを使う。
    expect(aheadAt(knots.slice(0, 3), [], 410_000)).toBe(1100);
    // 停車の間と、始まりだけを入れた停車より後は進まない。
    expect(aheadAt(knots, [{ start_ms: 505_000, end_ms: 600_000 }], 610_000)).toBe(2150);
    expect(aheadAt(knots, [], 610_000, 505_000)).toBe(2050);
    // 速さの分かる組が無ければ出さない。
    expect(aheadAt([{ route_m: 0, time_ms: 0 }], [], 10_000)).toBeNull();
    expect(aheadAt([], [], 0)).toBeNull();
  });

  test("停車の間は、時刻が進んでも位置が変わらない", () => {
    // 0 秒に 0 m、1000 秒に 4000 m。その間の 200 秒から 800 秒まで停車していた。
    const ends = [
      { route_m: 0, time_ms: 0 },
      { route_m: 4000, time_ms: 1_000_000 },
    ];
    const stops = [{ start_ms: 200_000, end_ms: 800_000 }];
    const at = (seconds: number) => positionAt(ends, stops, seconds * 1000);
    expect([at(100), at(200), at(500), at(800), at(900)]).toEqual([1000, 2000, 2000, 2000, 3000]);
    // 重なる停車は、1 度だけ引く。
    const twice = [...stops, { start_ms: 300_000, end_ms: 800_000 }];
    expect(positionAt(ends, twice, 900_000)).toBe(3000);
  });

  test("基準になるのは、確定していて時刻から置いたものでないイベントだけ", () => {
    const memo: RideEvent = { ...event("memo", 300, "confirmed", 30), detail: { by_time: true } };
    const events = [event("d", 0, "confirmed", 0), memo, event("x", 500, "estimated", 50), silent("h", 600, "estimated", 60)];
    expect(knotsOf(events)).toEqual([{ route_m: 0, time_ms: 0 }]);
  });

  test("字幕にしないイベントも、確定していれば基準になる", () => {
    const events = [event("d", 0, "confirmed", 0), silent("w", 600, "confirmed", 60)];
    expect(knotsOf(events)).toEqual([
      { route_m: 0, time_ms: 0 },
      { route_m: 600, time_ms: 60_000 },
    ]);
  });

  // 0 秒に 0 m、1000 秒に 4000 m。その間の 200 秒から 800 秒まで停車していた。
  const ends = [event("d", 0, "confirmed", 0), event("a", 4000, "confirmed", 1000)];
  const stop = { ...stopBetween(200_000, 800_000, 0), id: "s", route_m: 2000 };

  test("地図で場所を決めた停車は、その場所に着いた時刻と出た時刻の基準になる", () => {
    const pinned = pinStop(stop, [0.009, 0], 1000);
    expect(pinned.detail.by_time).toBeUndefined();
    const events = [...ends, pinned];
    const at = (seconds: number) => positionAt(knotsOf(events), stopsOf(events), seconds * 1000);
    expect([at(100), at(200), at(800), at(900)]).toEqual([500, 1000, 1000, 2500]);
    // 推定に戻すと、走っていた時間の比から置く。
    const back = [...ends, unpinStop(pinned)];
    expect(unpinStop(pinned).detail.at).toBeUndefined();
    expect(positionAt(knotsOf(back), stopsOf(back), 800_000)).toBe(2000);
  });

  test("停車の間に確定した時刻は、最初のものが停車の場所を決め、離れた位置のものは基準にしない", () => {
    const events = [
      ...ends,
      stop,
      event("w", 1500, "confirmed", 300),
      event("near", 1510, "confirmed", 500),
      event("far", 1600, "confirmed", 800),
    ];
    const at = (ms: number) => positionAt(knotsOf(events), stopsOf(events), ms);
    // 停車の終わりで、1600 m へ飛ばない。
    expect([at(200_000), at(799_999), at(800_000), at(900_000)]).toEqual([1500, 1500, 1500, 2750]);
    expect(asideOf(events)).toEqual(["far"]);
  });
});

describe("次の候補", () => {
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

  test("字幕にしないものと、時刻から置いたものは、自動では候補にしない", () => {
    const memo: RideEvent = { ...event("memo", 100, "estimated"), detail: { by_time: true } };
    const events = [silent("h", 0), memo, event("x", 500, "estimated")];
    expect(defaultTarget(events, 0)?.id).toBe("x");
  });

  test("確定したら、その後ろの未確定のものへ進む", () => {
    const events = [event("d", 0, "confirmed", 0), silent("h", 100), event("x", 500, "estimated")];
    expect(nextConfirmable(events, "d")?.id).toBe("x");
    expect(nextConfirmable(events, "x")).toBeNull();
  });

  test("隣へ移るときは、字幕にしないものを飛ばす", () => {
    const events = [event("d", 0, "confirmed", 0), silent("h", 100), event("x", 500, "estimated")];
    expect(ids([neighbor(events, "d", 1), neighbor(events, "x", -1), neighbor(events, "x", 1)])).toEqual(["x", "d", null]);
    expect(ids([neighbor(events, null, 1), neighbor(events, null, -1)])).toEqual(["d", null]);
  });

  test("字幕にしないものを選んでいるときは、そこから隣へ移る", () => {
    const events = [event("d", 0, "confirmed", 0), silent("h", 100), event("x", 500, "estimated")];
    expect(ids([neighbor(events, "h", 1), neighbor(events, "h", -1)])).toEqual(["x", "d"]);
  });
});

describe("確定", () => {
  test("動画で確定すると、カメラの時計での時刻も残る", () => {
    const confirmed = confirmOnVideo(event("x", 500, "estimated"), 100_000, 30_000);
    expect(confirmed).toMatchObject({ status: "confirmed", time_ms: 100_000, detail: { camera_ms: 70_000 } });
  });

  test("動画の位置が小数のミリ秒でも、時刻は整数になる", () => {
    // 再生位置は小数の秒で返るので、実時刻に直すと端数が付く。Rust 側は整数しか受け付けない。
    const confirmed = confirmOnVideo(event("x", 500, "estimated"), 1_790_977_977_367.118, 30_000);
    expect(confirmed.time_ms).toBe(1_790_977_977_367);
    expect(confirmed.detail.camera_ms).toBe(1_790_977_947_367);
    const memo = customAt(1_790_977_977_367.9, 0, "メモ");
    expect([memo.time_ms, memo.detail.camera_ms]).toEqual([1_790_977_977_368, 1_790_977_977_368]);
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

  test("字幕にしないことにしても、確定した時刻は残る", () => {
    const confirmed = confirmOnVideo(event("x", 500, "estimated"), 100_000, 30_000);
    const hidden = setHidden(confirmed, true);
    expect(hidden).toMatchObject({ status: "confirmed", time_ms: 100_000, hidden: true, detail: { camera_ms: 70_000 } });
    const back = setHidden(hidden, false);
    expect("hidden" in back).toBe(false);
    expect(back).toEqual(confirmed);
  });

  test("経由点のイベントは、字幕にしないものとして作り、位置で見分ける", () => {
    const made = waypointEvent([138.5, 36.5], 2, 700);
    expect(made).toMatchObject({ kind: "waypoint", status: "estimated", hidden: true, label: "経由地 3", route_m: 700 });
    expect(made.detail).toEqual({ at: [138.5, 36.5], extracted_label: "経由地 3" });
    const events = [event("x", 500, "estimated"), made];
    expect(waypointEventAt(events, [138.5, 36.5])?.id).toBe(made.id);
    expect(waypointEventAt(events, [138.5, 36.6])).toBeNull();
  });

  test("停車のイベントは、始まりの時刻と長さを持ち、基準にはならない", () => {
    const stop = stopBetween(100_000.4, 400_000.2, 30_000);
    expect(stop).toMatchObject({ kind: "stop", status: "confirmed", hidden: true, time_ms: 100_000 });
    expect(stop.detail).toEqual({ by_time: true, camera_ms: 70_000, stay_ms: 300_000 });
    expect(stopsOf([event("x", 500, "estimated"), stop])).toEqual([{ start_ms: 100_000, end_ms: 400_000 }]);
    expect(knotsOf([stop])).toEqual([]);
  });

  test("始まりだけを入れた停車は、動画の位置で終わりを入れる", () => {
    const open = stopBetween(100_000, 100_000, 30_000);
    expect(openStop([event("x", 500, "estimated"), open])?.id).toBe(open.id);
    // 終わりが無いうちは、停車として数えない。
    expect(stopsOf([open])).toEqual([]);
    const closed = closeStop(open, 700_000.4, 30_000);
    expect([closed.time_ms, closed.detail.stay_ms, closed.detail.camera_ms]).toEqual([100_000, 600_000, 70_000]);
    expect(openStop([closed])).toBeNull();
    // 始まりより前の位置で押したら、そこを始まりにする。
    const backwards = closeStop(open, 40_000, 30_000);
    expect([backwards.time_ms, backwards.detail.stay_ms, backwards.detail.camera_ms]).toEqual([40_000, 60_000, 10_000]);
    expect(closeStop(open, 100_000, 30_000)).toBe(open);
  });

  test("足すイベントは、位置の順の所に入る", () => {
    const events = [event("d", 0, "confirmed", 0), event("x", 500, "confirmed", 50), event("y", 800, "estimated"), event("a", 1000, "estimated")];
    const order = (added: RideEvent) => insertByPosition(events, added).map((e) => e.id);
    expect(order(event("n", 600, "estimated"))).toEqual(["d", "x", "n", "y", "a"]);
    expect(order(event("n", 500, "estimated"))).toEqual(["d", "x", "n", "y", "a"]);
    expect(order(event("n", 2000, "estimated"))).toEqual(["d", "x", "y", "a", "n"]);
    // 時刻だけが分かっているものは、基準の間ならその位置、外なら時刻の近い基準の位置に置く。
    expect([placeOf(events, 25_000), placeOf(events, 90_000), placeOf(events, -5)]).toEqual([250, 500, 0]);
    expect(placeOf([], 0)).toBe(0);
  });

  test("停車の始まりと終わりは、もう一方を動かさずに直せる", () => {
    const stop = stopBetween(100_000, 400_000, 30_000);
    const later = retimeStop(stop, "end", 500_000);
    expect([later.time_ms, later.detail.stay_ms, later.detail.camera_ms]).toEqual([100_000, 400_000, 70_000]);
    const earlier = retimeStop(stop, "start", 40_000);
    expect([earlier.time_ms, earlier.detail.stay_ms]).toEqual([40_000, 360_000]);
    // 手で入れた始まりは、時計の補正値と一緒には動かさない。
    expect("camera_ms" in earlier.detail).toBe(false);
    // 始まりが終わりより後になる直し方は、受け付けない。
    expect(retimeStop(stop, "start", 400_000)).toBe(stop);
    expect(retimeStop(stop, "end", 100_000)).toBe(stop);
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
