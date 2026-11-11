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
