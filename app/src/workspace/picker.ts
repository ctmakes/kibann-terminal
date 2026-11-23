// The theme picker (View → Theme…): a searchable list over the workspace.
// Moving through it previews each theme live on the real panes; Enter (or a
// click) keeps it, Esc goes back to the one in use before.

import type { ThemeApi, ThemeEntry } from "./theme";
import "../ui.css";
import "./picker.css";

/** Wait this long on a row before previewing it, so holding an arrow key
 * doesn't recolor every pane for every row passed. */
const PREVIEW_MS = 60;

export class ThemePicker {
  readonly el: HTMLDivElement;
  visible = false;
  private readonly input: HTMLInputElement;
  private readonly list: HTMLDivElement;
  private all: ThemeEntry[] = [];
  private shown: ThemeEntry[] = [];
  private at = 0;
  private timer = 0;
  /** The theme in use when the picker opened. */
  private original = "";

  constructor(
    private readonly api: ThemeApi,
    /** Called after it closes, to put focus back in the terminal. */
    private readonly onClose: () => void,
  ) {
    this.el = document.createElement("div");
    this.el.className = "theme-picker";
    this.el.style.display = "none";
    // A click outside the panel closes it, as Esc does.
    this.el.addEventListener("mousedown", (e) => {
      if (e.target === this.el) {
        e.preventDefault();
        this.close(false);
      }
    });
    const panel = document.createElement("div");
    panel.className = "theme-picker-panel";
    this.input = document.createElement("input");
    this.input.className = "theme-picker-input";
    this.input.placeholder = "Search themes";
    this.input.spellcheck = false;
    this.input.addEventListener("input", () => this.filter());
    this.input.addEventListener("keydown", (e) => this.onKey(e));
    this.list = document.createElement("div");
    this.list.className = "theme-picker-list";
    const hint = document.createElement("div");
    hint.className = "theme-picker-hint";
    hint.textContent = "↑↓ preview   ↩ keep   esc cancel";
    panel.append(this.input, this.list, hint);
    this.el.append(panel);
  }

  async open(): Promise<void> {
    if (this.visible) return;
    this.visible = true;
    this.original = this.api.current();
    this.el.style.display = "flex";
    this.input.value = "";
    this.input.focus();
    this.all = await this.api.list();
    this.filter(this.original);
  }

  /** Close, keeping the selected theme or going back to the original. */
  private close(keep: boolean): void {
    if (!this.visible) return;
    this.visible = false;
    clearTimeout(this.timer);
    this.el.style.display = "none";
    const pick = this.shown[this.at]?.name;
    if (keep && pick) void this.api.choose(pick);
    else void this.api.preview(null);
    this.onClose();
  }

  /** Show the themes matching the search, selecting `select` if given. */
  private filter(select?: string): void {
    const q = this.input.value.trim().toLowerCase();
    const words = q.split(/\s+/).filter(Boolean);
    this.shown = this.all.filter((t) => {
      const hay = `${t.name} ${t.dark ? "dark" : "light"}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
    const i = select ? this.shown.findIndex((t) => t.name === select) : 0;
    this.at = Math.max(0, i);
    this.render();
    if (!select) this.previewSoon();
  }

  private render(): void {
    const rows = this.shown.map((t, i) => {
      const row = document.createElement("div");
      row.className = i === this.at ? "theme-picker-row current" : "theme-picker-row";
      row.dataset.name = t.name;
      const name = document.createElement("span");
      name.className = "theme-picker-name";
      name.textContent = t.name;
      const tag = document.createElement("span");
      tag.className = "theme-picker-tag";
      tag.textContent = (t.name === this.original ? "in use · " : "") + (t.dark ? "dark" : "light");
      row.append(name, tag);
      row.addEventListener("mousedown", (e) => {
        e.preventDefault();
        this.at = i;
        this.close(true);
      });
      return row;
    });
    if (rows.length === 0) {
      const none = document.createElement("div");
      none.className = "theme-picker-none";
      none.textContent = "No theme matches.";
      rows.push(none);
    }
    this.list.replaceChildren(...rows);
    rows[this.at]?.scrollIntoView({ block: "nearest" });
  }

  private onKey(e: KeyboardEvent): void {
    const move = (by: number) => {
      if (this.shown.length === 0) return;
      this.at = Math.max(0, Math.min(this.shown.length - 1, this.at + by));
      this.render();
      this.previewSoon();
    };
    switch (e.key) {
      case "ArrowDown":
        move(1);
        break;
      case "ArrowUp":
        move(-1);
        break;
      case "PageDown":
        move(10);
        break;
      case "PageUp":
        move(-10);
        break;
      case "Enter":
        this.close(true);
        break;
      case "Escape":
        this.close(false);
        break;
      default:
        return; // typing goes to the search
    }
    e.preventDefault();
  }

  private previewSoon(): void {
    clearTimeout(this.timer);
    const name = this.shown[this.at]?.name;
    if (!name) return;
    this.timer = window.setTimeout(() => void this.api.preview(name), PREVIEW_MS);
  }
}
