/**
 * UUIDv7。先頭の 48 ビットがミリ秒の時刻なので、作った順に並ぶ。Rust 側の `Uuid::now_v7` と同じ形。
 */
export function newId(nowMs = Date.now()): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  for (let i = 0; i < 6; i++) bytes[i] = Math.floor(nowMs / 2 ** (8 * (5 - i))) % 256;
  bytes[6] = 0x70 | (bytes[6] & 0x0f);
  bytes[8] = 0x80 | (bytes[8] & 0x3f);
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return [hex.slice(0, 8), hex.slice(8, 12), hex.slice(12, 16), hex.slice(16, 20), hex.slice(20)].join("-");
}
