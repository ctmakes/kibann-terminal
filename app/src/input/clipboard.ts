import type { InputEvent } from "../proto";

/**
 * Paste arrives as a DOM paste event because the paste shortcuts are passed
 * through to the webview. In Tauri on macOS that requires an Edit menu with a
 * Paste item, which the default menu has.
 */
export function attachPaste(target: HTMLElement, send: (e: InputEvent) => void): () => void {
  const onPaste = (e: ClipboardEvent) => {
    e.preventDefault();
    const data = e.clipboardData?.getData("text/plain") ?? "";
    if (data) send({ kind: "paste", data });
  };
  target.addEventListener("paste", onPaste);
  return () => target.removeEventListener("paste", onPaste);
}

/**
 * Copy the terminal's selection. Cmd+C and the menu's Copy reach the webview
 * as a DOM copy event. WebKit only enables Copy when the page has a selection
 * or cancels `beforecopy`, so cancel it whenever the terminal has one.
 */
export function attachCopy(target: HTMLElement, selection: () => string): () => void {
  const onBeforeCopy = (e: Event) => {
    if (selection()) e.preventDefault();
  };
  const onCopy = (e: ClipboardEvent) => {
    const text = selection();
    if (!text || !e.clipboardData) return;
    e.preventDefault();
    e.clipboardData.setData("text/plain", text);
  };
  target.addEventListener("beforecopy", onBeforeCopy);
  target.addEventListener("copy", onCopy);
  return () => {
    target.removeEventListener("beforecopy", onBeforeCopy);
    target.removeEventListener("copy", onCopy);
  };
}

/** Copy shortcuts without a native menu item behind them (Ctrl+Shift+C off macOS). */
export function copyNow(): void {
  document.execCommand("copy");
}
