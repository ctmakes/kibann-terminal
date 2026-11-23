// The theme picker (View → Theme…): a searchable list over the workspace.
// Moving through it previews each theme live on the real panes; Enter (or a
// click) keeps it, Esc goes back to the one in use before.

import type { ThemeApi, ThemeEntry } from "./theme";

/** Wait this long on a row before previewing it, so holding an arrow key
 * doesn't recolor every pane for every row passed. */
const PREVIEW_MS = 60;
const ROW_H = 26;

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
    Object.assign(this.el.style, {
      position: "absolute",
      inset: "0",
      zIndex: "30",
      display: "none",
      justifyContent: "center",
      alignItems: "flex-start",
      background: "var(--k-scrim)",
      font: "12px -apple-system, system-ui, sans-serif",
      color: "var(--k-strong)",
    });
    // A click outside the panel closes it, as Esc does.
    this.el.addEventListener("mousedown", (e) => {
      if (e.target === this.el) {
        e.preventDefault();
        this.close(false);
      }
    });
    const panel = document.createElement("div");
    Object.assign(panel.style, {
      marginTop: "10vh",
      width: "min(440px, calc(100% - 32px))",
      background: "var(--k-panel)",
      border: "1px solid var(--k-divider)",
      borderRadius: "10px",
      boxShadow: "0 12px 40px var(--k-shadow)",
      overflow: "hidden",
    });
    this.input = document.createElement("input");
    this.input.className = "theme-picker-input";
    this.input.placeholder = "Search themes";
    this.input.spellcheck = false;
    Object.assign(this.input.style, {
      display: "block",
      width: "100%",
      boxSizing: "border-box",
      padding: "10px 12px",
      font: "13px -apple-system, system-ui, sans-serif",
      color: "var(--k-bright)",
      background: "transparent",
      border: "none",
      borderBottom: "1px solid var(--k-divider)",
      outline: "none",
    });
    this.input.addEventListener("input", () => this.filter());
    this.input.addEventListener("keydown", (e) => this.onKey(e));
    this.list = document.createElement("div");
    this.list.className = "theme-picker-list";
    Object.assign(this.list.style, { maxHeight: `${ROW_H * 12}px`, overflowY: "auto", padding: "4px 0" });
    const hint = document.createElement("div");
    hint.textContent = "↑↓ preview   ↩ keep   esc cancel";
    Object.assign(hint.style, {
      padding: "6px 12px",
      color: "var(--k-muted)",
      borderTop: "1px solid var(--k-divider)",
      fontSize: "11px",
    });
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
      row.className = "theme-picker-row";
      row.dataset.name = t.name;
      Object.assign(row.style, {
        display: "flex",
        alignItems: "center",
        gap: "8px",
        height: `${ROW_H}px`,
        padding: "0 12px",
        cursor: "default",
        background: i === this.at ? "var(--k-current)" : "",
        color: i === this.at ? "var(--k-current-text)" : "",
      });
      const name = document.createElement("span");
      name.textContent = t.name;
      Object.assign(name.style, { flex: "1", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" });
      const tag = document.createElement("span");
      tag.textContent = (t.name === this.original ? "in use · " : "") + (t.dark ? "dark" : "light");
      Object.assign(tag.style, { color: "var(--k-muted)", fontSize: "11px" });
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
      none.textContent = "No theme matches.";
      Object.assign(none.style, { padding: "6px 12px", color: "var(--k-muted)" });
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
