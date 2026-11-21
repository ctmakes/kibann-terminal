// View → Screen…: the screens to pick from. Moving through the list shows
// each one at once; Enter keeps it (the config's `screen`), Esc goes back.

import type { ScreenEntry, ScreenNow } from "../proto";

export interface ScreenApi {
  /** The screen the config names, ready to show. When it can't be read,
   * the last good one comes back with `problem` saying why. */
  current(): Promise<ScreenNow>;
  list(): Promise<ScreenEntry[]>;
  /** The screen `name`, ready to show (for trying it). Fails with why. */
  load(name: string): Promise<ScreenNow>;
  /** Keep `name` (the config's `screen`). */
  choose(name: string): Promise<void>;
  /** Run `program` for part `part`. */
  exec(
    part: string,
    program: string,
    args: string[],
    cwd: string | null,
    timeout: number | null,
  ): Promise<{ code: number | null; stdout: string; stderr: string }>;
}

const ROW_H = 44;

export class ScreenPicker {
  readonly el: HTMLDivElement;
  visible = false;
  private readonly input: HTMLInputElement;
  private readonly list: HTMLDivElement;
  private all: ScreenEntry[] = [];
  private shown: ScreenEntry[] = [];
  private at = 0;
  /** The screen showing when it opened, to go back to. */
  private from = "";
  private trying = "";

  constructor(
    private readonly api: ScreenApi,
    private readonly opts: {
      /** The screen shown now. */
      current(): string;
      /** Show `now` for a moment (null: back to the kept one). */
      preview(now: ScreenNow | null): void;
      onClose(): void;
      notice(text: string): void;
    },
  ) {
    this.el = document.createElement("div");
    this.el.className = "screen-picker";
    Object.assign(this.el.style, {
      position: "absolute",
      inset: "0",
      zIndex: "40",
      display: "none",
      justifyContent: "center",
      alignItems: "flex-start",
      background: "transparent",
      font: "12px -apple-system, system-ui, sans-serif",
      color: "var(--k-strong)",
      pointerEvents: "none",
    });
    const panel = document.createElement("div");
    Object.assign(panel.style, {
      marginTop: "8vh",
      width: "min(460px, calc(100% - 32px))",
      background: "var(--k-panel)",
      border: "1px solid var(--k-divider)",
      borderRadius: "10px",
      boxShadow: "0 16px 50px var(--k-shadow)",
      overflow: "hidden",
      pointerEvents: "auto",
    });
    this.input = document.createElement("input");
    this.input.className = "screen-picker-input";
    this.input.spellcheck = false;
    this.input.placeholder = "Screens";
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
    this.input.addEventListener("blur", () => {
      // Clicking away keeps nothing: back to where it was.
      if (this.visible) window.setTimeout(() => this.visible && document.activeElement !== this.input && this.cancel(), 0);
    });
    this.list = document.createElement("div");
    this.list.className = "screen-picker-list";
    Object.assign(this.list.style, { maxHeight: `${ROW_H * 8}px`, overflowY: "auto", padding: "4px 0" });
    const hint = document.createElement("div");
    hint.textContent = "↑↓ to try · Enter to keep · Esc to go back · kibann skill show screens to make one";
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
    this.from = this.trying = this.opts.current();
    this.el.style.display = "flex";
    this.input.value = "";
    this.input.focus();
    try {
      this.all = await this.api.list();
    } catch (e) {
      this.opts.notice(`Couldn't list the screens: ${e}`);
      this.all = [];
    }
    this.filter();
    const i = this.shown.findIndex((s) => s.name === this.from);
    this.select(Math.max(0, i), false);
  }

  /** Step to the next screen (Cmd+Shift+L) without opening the list. */
  async cycle(step: 1 | -1): Promise<void> {
    const all = await this.api.list();
    if (all.length === 0) return;
    const i = all.findIndex((s) => s.name === this.opts.current());
    const next = all[(i + step + all.length) % all.length];
    await this.api.choose(next.name).catch((e) => this.opts.notice(`Couldn't use ${next.name}: ${e}`));
  }

  private filter(): void {
    const q = this.input.value.trim().toLowerCase();
    this.shown = this.all.filter((s) => !q || s.name.includes(q) || (s.about ?? "").toLowerCase().includes(q));
    this.render();
    this.select(0, q !== "");
  }

  private render(): void {
    this.list.replaceChildren(
      ...this.shown.map((s, i) => {
        const row = document.createElement("div");
        row.className = "screen-picker-row";
        row.dataset.name = s.name;
        Object.assign(row.style, {
          height: `${ROW_H}px`,
          boxSizing: "border-box",
          padding: "6px 12px",
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          gap: "2px",
          cursor: "default",
        });
        const top = document.createElement("div");
        Object.assign(top.style, { display: "flex", gap: "8px", alignItems: "baseline" });
        const name = document.createElement("span");
        name.textContent = s.name;
        Object.assign(name.style, { color: "var(--k-bright)", fontWeight: "600" });
        const origin = document.createElement("span");
        origin.textContent = s.builtin ? "built in" : "yours";
        Object.assign(origin.style, { color: "var(--k-muted)", fontSize: "11px", marginLeft: "auto" });
        top.append(name, origin);
        const about = document.createElement("div");
        about.textContent = s.about ?? "";
        Object.assign(about.style, {
          color: "var(--k-text)",
          fontSize: "11px",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        });
        row.append(top, about);
        row.addEventListener("mousedown", (e) => {
          e.preventDefault();
          this.select(i, true);
        });
        row.addEventListener("dblclick", () => void this.keep());
        return row;
      }),
    );
  }

  private select(i: number, show: boolean): void {
    if (this.shown.length === 0) return;
    this.at = Math.max(0, Math.min(this.shown.length - 1, i));
    [...this.list.children].forEach((r, j) => {
      (r as HTMLElement).style.background = j === this.at ? "var(--k-current)" : "";
    });
    (this.list.children[this.at] as HTMLElement | undefined)?.scrollIntoView({ block: "nearest" });
    if (show) void this.tryOn(this.shown[this.at].name);
  }

  private async tryOn(name: string): Promise<void> {
    if (name === this.trying) return;
    this.trying = name;
    try {
      const now = await this.api.load(name);
      if (this.trying === name && this.visible) this.opts.preview(now);
    } catch (e) {
      if (this.trying === name) this.opts.notice(String(e));
    }
  }

  private onKey(e: KeyboardEvent): void {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      this.select(this.at + (e.key === "ArrowDown" ? 1 : -1), true);
    } else if (e.key === "Enter") {
      e.preventDefault();
      void this.keep();
    } else if (e.key === "Escape") {
      e.preventDefault();
      this.cancel();
    }
  }

  private async keep(): Promise<void> {
    const s = this.shown[this.at];
    this.close();
    if (!s) return;
    try {
      await this.api.choose(s.name);
    } catch (e) {
      this.opts.notice(`Couldn't use ${s.name}: ${e}`);
      this.opts.preview(null);
    }
  }

  private cancel(): void {
    const changed = this.trying !== this.from;
    this.close();
    if (changed) this.opts.preview(null);
  }

  private close(): void {
    this.visible = false;
    this.el.style.display = "none";
    this.opts.onClose();
  }
}
