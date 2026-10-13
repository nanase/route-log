/** キーの操作を、どこで受け取るかの決まり。 */

/**
 * そのキーを、画面の操作（再生、確定、取り消しなど）として受け取ってよいか。
 *
 * 字を打つ欄とダイアログの中では受け取らない。ボタンの上の Enter と Space は、ボタンを押す
 * ために使われるので、これも受け取らない。
 */
export function forShortcut(event: KeyboardEvent): boolean {
  const from = event.target;
  if (!(from instanceof Element)) return true;
  if (from.closest("dialog") !== null) return false;
  if (from instanceof HTMLTextAreaElement || from instanceof HTMLSelectElement) return false;
  const pressing = event.key === "Enter" || event.key === " ";
  if (from instanceof HTMLInputElement) {
    // チェックの欄では字を打たない。Space だけは、チェックの切り替えに使われる。
    return (from.type === "checkbox" || from.type === "radio") && event.key !== " ";
  }
  return !(pressing && from.closest("button") !== null);
}

/**
 * ボタンをマウスで押しても、キーの行き先をボタンに移さない。
 *
 * 押したあとも Space や Enter が、ボタンではなく動画と確定に届く。字を打つ欄にいたなら、
 * そこを離れて、打った内容を確定させる。
 */
export function keepKeys(event: MouseEvent) {
  const button = event.target instanceof Element ? event.target.closest("button") : null;
  if (button === null || button.closest("dialog") !== null) return;
  event.preventDefault();
  if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
}
