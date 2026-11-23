// Short messages in the window's corner: the first-run hint, a theme that
// can't be read, a part's error.

import "../ui.css";
import "./notices.css";

const HINT_KEY = "kibann.hint.shortcuts";

export class Notices {
  readonly el: HTMLDivElement;

  constructor() {
    this.el = document.createElement("div");
    this.el.className = "notices";
  }

  /** Show `text` for `ms`; with `action`, until it's used or dismissed. */
  show(text: string, ms = 8000, action?: { label: string; run(): void }): void {
    const n = document.createElement("div");
    n.className = "notice";
    n.textContent = text;
    if (action) {
      const b = document.createElement("button");
      b.className = "notice-action";
      b.textContent = action.label;
      b.addEventListener("click", () => {
        n.remove();
        action.run();
      });
      const x = document.createElement("button");
      x.className = "notice-close";
      x.textContent = "✕";
      x.setAttribute("aria-label", "Dismiss");
      x.addEventListener("click", () => n.remove());
      n.append(b, x);
      n.style.pointerEvents = "auto";
    }
    this.el.append(n);
    if (!action) window.setTimeout(() => n.remove(), ms);
  }

  /** Say once, ever, that `key` shows the shortcuts. */
  hintOnce(key: string): void {
    try {
      if (localStorage.getItem(HINT_KEY)) return;
      localStorage.setItem(HINT_KEY, "1");
    } catch {
      return; // can't remember it was shown: don't show it every time
    }
    this.show(`Press ${key} to see every keyboard shortcut.`, 10000);
  }
}
