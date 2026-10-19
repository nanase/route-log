/**
 * 走行に付けるラベル。「投稿済」のような目印で、打った文字がそのままラベルになる。
 *
 * 前もって定義はしない。どの走行かに付いているものが、候補と絞り込みの選択肢になる。
 */

/** 札の色の数。色は `app.css` の `--label-<番号>-soft` と `--label-<番号>-ink` にある */
export const LABEL_COLORS = 8;

/** 打った文字を、ラベルの名前にする。前後の空白を除き、続く空白を 1 つにする。 */
export function labelName(text: string): string {
  return text.trim().replace(/\s+/g, " ");
}

/** 名前から決まる色の番号（1 から）。同じ名前は、どの走行でも、どの PC でも同じ色になる。 */
export function labelColor(name: string): number {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.codePointAt(0)!) >>> 0;
  return (hash % LABEL_COLORS) + 1;
}

/** ラベルを足す。空の名前と、もう付いているものは足さない。 */
export function withLabel(labels: string[], text: string): string[] {
  const name = labelName(text);
  return name === "" || labels.includes(name) ? labels : [...labels, name];
}

export function withoutLabel(labels: string[], name: string): string[] {
  return labels.filter((label) => label !== name);
}

/** 使っているラベルと、付いている走行の数。多い順で、同じ数なら名前の順。 */
export function usedLabels(rides: string[][]): { name: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const labels of rides) for (const name of new Set(labels)) counts.set(name, (counts.get(name) ?? 0) + 1);
  return [...counts]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name, "ja"));
}

/** 絞り込み。ラベルごとに、付いている走行だけ（has）か、付いていない走行だけ（not）を出す */
export type LabelFilter = Record<string, "has" | "not">;

/** 絞り込みの札を押したとき。付いている走行だけ、付いていない走行だけ、外す、の順に変わる。 */
export function toggleFilter(filter: LabelFilter, name: string): LabelFilter {
  const { [name]: now, ...rest } = filter;
  if (now === undefined) return { ...rest, [name]: "has" };
  if (now === "has") return { ...rest, [name]: "not" };
  return rest;
}

/** 走行のラベルが、絞り込みのどれにも当てはまるか。 */
export function matchesFilter(labels: string[], filter: LabelFilter): boolean {
  return Object.entries(filter).every(([name, want]) => labels.includes(name) === (want === "has"));
}
