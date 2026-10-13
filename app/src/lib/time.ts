const JST_MS = 9 * 3_600_000;

/** 実時刻を `hh:mm:ss`（JST）にする。 */
export const clock = (unixMs: number) => new Date(unixMs + JST_MS).toISOString().slice(11, 19);

/** 長さを `h:mm:ss` にする。 */
export function hms(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${Math.floor(seconds / 3600)}:${pad(Math.floor(seconds / 60) % 60)}:${pad(seconds % 60)}`;
}
