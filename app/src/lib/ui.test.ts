import { describe, expect, test } from "bun:test";
import { startDate } from "./ui";

describe("起動のときに開く走行", () => {
  const dates = ["2026-10-04", "2026-10-03", "2026-09-20"];

  test("前に開いていた走行がまだあれば、それを開く", () => {
    expect(startDate("2026-10-03", dates)).toBe("2026-10-03");
  });

  test("前に開いていた走行が無くなっていれば、いちばん新しいものを開く", () => {
    expect(startDate("2026-08-01", dates)).toBe("2026-10-04");
  });

  test("初めての起動では、いちばん新しいものを開く", () => {
    expect(startDate(null, dates)).toBe("2026-10-04");
  });

  test("走行が 1 つも無ければ、何も開かない", () => {
    expect(startDate("2026-10-03", [])).toBeUndefined();
  });
});
