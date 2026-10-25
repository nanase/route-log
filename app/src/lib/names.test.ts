import { expect, test } from "bun:test";
import { setHidden } from "./events";
import { nameChoices, sameName, setName, shownName } from "./names";
import type { RideEvent } from "./types";

function structure(label: string, detail: Partial<RideEvent["detail"]> = {}): RideEvent {
  return {
    id: "a",
    kind: "tunnel",
    label,
    route_m: 0,
    time_ms: null,
    status: "estimated",
    detail: { extracted_label: label, ...detail },
  };
}

test("橋・トンネルは、公称の長さを添えて出す", () => {
  expect(shownName(structure("犬戻トンネル", { official_name: "犬戻トンネル", official_length_m: 737 }))).toBe(
    "犬戻トンネル 737m",
  );
  expect(shownName(structure("大安寺橋", { official_name: "大安寺橋", official_length_m: 291.3 }))).toBe(
    "大安寺橋 291.3m",
  );
});

test("名前の無いトンネルは、公称の名前を使う", () => {
  expect(shownName(structure("", { official_name: "池沢隧道", official_length_m: 379 }))).toBe("池沢隧道 379m");
});

test("名前が違えば、選ぶまで両方を出す", () => {
  const both = structure("新鳥居トンネル", { official_name: "鳥居トンネル", official_length_m: 1738 });
  expect(shownName(both)).toBe("新鳥居トンネル／鳥居トンネル 1738m");
  expect(nameChoices(both)).toEqual(["新鳥居トンネル", "鳥居トンネル"]);
  const chosen = setName(both, "鳥居トンネル");
  expect(chosen.detail.named_by_hand).toBe(true);
  expect(shownName(chosen)).toBe("鳥居トンネル 1738m");
  expect(shownName(setName(both, "新鳥居トンネル"))).toBe("新鳥居トンネル 1738m");
});

test("全角と半角、ケとヶの違いは同じ名前とみなす", () => {
  expect(sameName("角ケ平トンネル", "角ヶ平トンネル")).toBe(true);
  expect(sameName("大白川第１トンネル", "大白川第1トンネル")).toBe(true);
  expect(sameName("角ケ平隧道", "角ヶ平トンネル")).toBe(false);
  const same = structure("角ケ平トンネル", { official_name: "角ヶ平トンネル" });
  expect(shownName(same)).toBe("角ヶ平トンネル");
  expect(nameChoices(same)).toEqual([]);
});

test("長さで終わる名前には、長さを足さない", () => {
  const typed = structure("犬戻トンネル 737m", { extracted_label: "犬戻トンネル", official_length_m: 737 });
  expect(shownName(typed)).toBe("犬戻トンネル 737m");
});

test("橋・トンネルでなければ、名前のまま", () => {
  expect(shownName({ ...structure("国道19号", { official_length_m: 10 }), kind: "road" })).toBe("国道19号");
});

test("字幕の有無を切り替えると、人が決めた印が付く", () => {
  expect(setHidden(structure("明治橋"), true).detail.hidden_by_hand).toBe(true);
  expect(setHidden(structure("明治橋"), false).detail.hidden_by_hand).toBe(true);
});
