import { expect, test } from "bun:test";
import { clock, hms, jstToMs, short } from "./time";

test("日付と時刻を日本時間として読む", () => {
  // 2000-01-01T00:00:00Z は Unix 秒で 946684800。
  expect(jstToMs("2000-01-01", "09:00:00")).toBe(946_684_800_000);
  expect(jstToMs("2000-01-01", "09:00")).toBe(946_684_800_000);
  expect(jstToMs("2000-01-01", "00:00:00")).toBe(946_684_800_000 - 9 * 3_600_000);
});

test("読めないものは null", () => {
  expect(jstToMs("2000-01-01", "")).toBeNull();
  expect(jstToMs("", "09:00:00")).toBeNull();
  expect(jstToMs("2000/01/01", "09:00:00")).toBeNull();
});

test("表示は日本時間で、読んだものと往復する", () => {
  expect(clock(946_684_800_000)).toBe("09:00:00");
  expect(clock(jstToMs("2026-10-03", "05:05:00") as number)).toBe("05:05:00");
  expect(hms(3_723_000)).toBe("1:02:03");
});

test("短い書き方は、1 時間に満たなければ時を省く", () => {
  expect(short(25_000)).toBe("0:25");
  expect(short(754_000)).toBe("12:34");
  expect(short(3_723_000)).toBe("1:02:03");
});
