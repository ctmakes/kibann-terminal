// The find bar: a search field at the top right of a pane, over its
// terminal. The daemon searches (screen and scrollback, as output arrives);
// the bar sends what's typed and shows "3 of 17".

import type { Find } from "../proto";
import "../ui.css";
import "./find.css";

export interface FindBarOptions {
  /** The text to find changed ("" finds nothing). */
  onNeedle(needle: string): void;
  /** Go to the next match: older (up the scrollback), or newer. */
  onStep(older: boolean): void;
  /** The bar closed (Esc or its button); the terminal takes focus back. */
  onClose(): void;
}

/** What the count says. */
export function countText(needle: string, find: Find | null): string {
  if (!needle || !find) return "";
  const more = find.searching ? "+" : "";
  if (find.total === 0) return find.searching ? "…" : "No matches";
  if (find.index === null) return `${find.total}${more} found`;
  return `${find.index + 1} of ${find.total}${more}`;
}

export class FindBar {
  readonly el: HTMLDivElement;
  readonly input: HTMLInputElement;
  private readonly count: HTMLSpanElement;
  private result: Find | null = null;
  visible = false;

  constructor(private readonly opts: FindBarOptions) {
    this.el = document.createElement("div");
    this.el.className = "find-bar";
    this.el.style.display = "none";
    // Clicks in the bar stay in the bar, not the terminal under it.
    this.el.addEventListener("mousedown", (e) => e.stopPropagation());

    this.input = document.createElement("input");
    this.input.className = "find-input";
    this.input.type = "text";
    this.input.placeholder = "Find";
    this.input.setAttribute("aria-label", "Find");
    for (const [k, v] of Object.entries({ autocapitalize: "off", autocomplete: "off", autocorrect: "off", spellcheck: "false" })) {
      this.input.setAttribute(k, v);
    }
    this.input.addEventListener("input", () => {
      this.opts.onNeedle(this.input.value);
      this.render();
    });
    this.input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        this.opts.onStep(!e.shiftKey);
      } else if (e.key === "Escape") {
        e.preventDefault();
        this.close();
      }
    });

    this.count = document.createElement("span");
    this.count.className = "find-count";

    const button = (cls: string, text: string, label: string, run: () => void) => {
      const b = document.createElement("button");
      b.className = `find-button ${cls}`;
      b.textContent = text;
      b.title = label;
      b.setAttribute("aria-label", label);
      b.addEventListener("mousedown", (e) => {
        e.preventDefault(); // keep focus in the field
        run();
      });
      return b;
    };
    this.el.append(
      this.input,
      this.count,
      button("find-older", "↑", "Next match (older)", () => this.opts.onStep(true)),
      button("find-newer", "↓", "Previous match (newer)", () => this.opts.onStep(false)),
      button("find-close", "✕", "Close", () => this.close()),
    );
  }

  /** Show the bar with its text selected, and search for it again. */
  open(): void {
    const was = this.visible;
    this.visible = true;
    this.el.style.display = "flex";
    this.input.focus();
    this.input.select();
    if (!was) this.opts.onNeedle(this.input.value);
    this.render();
  }

  close(): void {
    if (!this.visible) return;
    this.visible = false;
    this.el.style.display = "none";
    this.result = null;
    this.opts.onClose();
  }

  /** The daemon's results. */
  show(find: Find | null): void {
    this.result = find;
    this.render();
  }

  private render(): void {
    this.count.textContent = countText(this.input.value, this.result);
  }
}
