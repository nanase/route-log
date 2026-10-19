import { describe, expect, test } from "bun:test";
import {
  LABEL_COLORS,
  labelColor,
  labelName,
  matchesFilter,
  toggleFilter,
  usedLabels,
  withLabel,
  withoutLabel,
} from "./labels";

describe("ラベルの付け外し", () => {
  test("打った文字の前後の空白を除き、続く空白を 1 つにする", () => {
    expect(labelName("  友人と　 2 人 ")).toBe("友人と 2 人");
  });

  test("空の名前と、もう付いているものは足さない", () => {
    const labels = ["投稿済"];
    expect(withLabel(labels, "  ")).toBe(labels);
    expect(withLabel(labels, " 投稿済 ")).toBe(labels);
    expect(withLabel(labels, "峠")).toEqual(["投稿済", "峠"]);
  });

  test("外すと、ほかは付けた順のまま残る", () => {
    expect(withoutLabel(["投稿済", "峠", "雨"], "峠")).toEqual(["投稿済", "雨"]);
  });
});

test("色は名前で決まり、決まった数の中に収まる", () => {
  expect(labelColor("投稿済")).toBe(labelColor("投稿済"));
  for (const name of ["投稿済", "編集中", "峠", "雨", "友人と", "a", ""]) {
    expect(labelColor(name)).toBeGreaterThanOrEqual(1);
    expect(labelColor(name)).toBeLessThanOrEqual(LABEL_COLORS);
  }
});

test("使っているラベルは、多い順で、同じ数なら名前の順に並ぶ", () => {
  const used = usedLabels([["投稿済", "峠"], ["峠", "雨"], ["峠"], ["投稿済"], []]);
  expect(used).toEqual([
    { name: "峠", count: 3 },
    { name: "投稿済", count: 2 },
    { name: "雨", count: 1 },
  ]);
});

describe("絞り込み", () => {
  test("押すたびに、付いている走行だけ、付いていない走行だけ、外す、と変わる", () => {
    const has = toggleFilter({}, "投稿済");
    expect(has).toEqual({ 投稿済: "has" });
    const not = toggleFilter(has, "投稿済");
    expect(not).toEqual({ 投稿済: "not" });
    expect(toggleFilter(not, "投稿済")).toEqual({});
  });

  test("選んだすべてに当てはまる走行だけを出す", () => {
    const filter = { 峠: "has", 投稿済: "not" } as const;
    expect(matchesFilter(["峠"], filter)).toBe(true);
    expect(matchesFilter(["峠", "投稿済"], filter)).toBe(false);
    expect(matchesFilter(["雨"], filter)).toBe(false);
    expect(matchesFilter([], {})).toBe(true);
  });
});
