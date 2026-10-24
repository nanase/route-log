const JST_MS = 9 * 3_600_000;

/** 実時刻を `hh:mm:ss`（JST）にする。 */
export const clock = (unixMs: number) => new Date(unixMs + JST_MS).toISOString().slice(11, 19);

/** `YYYY-MM-DD` と `hh:mm[:ss]`（JST）を Unix ミリ秒にする。読めなければ null。 */
export function jstToMs(date: string, time: string): number | null {
  const d = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  const t = /^(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(time);
  if (d === null || t === null) return null;
  return Date.UTC(+d[1], +d[2] - 1, +d[3], +t[1], +t[2], +(t[3] ?? 0)) - JST_MS;
}

/** 長さを短く書く。1 時間に満たなければ `m:ss`、それ以上は `h:mm:ss`。 */
export function short(ms: number): string {
  if (ms >= 3_600_000) return hms(ms);
  const seconds = Math.floor(ms / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/** 長さを「n 分 m 秒」と書く。1 分に満たなければ秒だけ。 */
export function minutesSeconds(ms: number): string {
  const seconds = Math.round(ms / 1000);
  return seconds < 60 ? `${seconds} 秒` : `${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒`;
}

/** 長さを `h:mm:ss` にする。 */
export function hms(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${Math.floor(seconds / 3600)}:${pad(Math.floor(seconds / 60) % 60)}:${pad(seconds % 60)}`;
}
