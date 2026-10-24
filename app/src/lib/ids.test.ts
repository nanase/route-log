import { expect, test } from "bun:test";
import { newId } from "./ids";

test("UUIDv7 の形で、時刻を先頭に持つ", () => {
  const id = newId(Date.UTC(2026, 9, 24, 3, 0, 0));
  expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  expect(Number.parseInt(id.replace("-", "").slice(0, 12), 16)).toBe(Date.UTC(2026, 9, 24, 3, 0, 0));
});

test("作るたびに違い、後に作ったものが後に並ぶ", () => {
  const [a, b] = [newId(1_790_000_000_000), newId(1_790_000_000_001)];
  expect(a).not.toBe(newId(1_790_000_000_000));
  expect(a < b).toBe(true);
});
