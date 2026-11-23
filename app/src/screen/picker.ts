// View → Screen…: the screens to pick from. Moving through the list shows
// each one at once; Enter keeps it (the config's `screen`), Esc goes back.

import type { ScreenEntry, ScreenNow } from "../proto";
import "../ui.css";
import "./picker.css";

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
    this.el.style.display = "none";
    const panel = document.createElement("div");
    panel.className = "screen-picker-panel";
    this.input = document.createElement("input");
    this.input.className = "screen-picker-input";
    this.input.spellcheck = false;
    this.input.placeholder = "Screens";
    this.input.addEventListener("input", () => this.filter());
    this.input.addEventListener("keydown", (e) => this.onKey(e));
    this.input.addEventListener("blur", () => {
      // Clicking away keeps nothing: back to where it was.
      if (this.visible) window.setTimeout(() => this.visible && document.activeElement !== this.input && this.cancel(), 0);
    });
    this.list = document.createElement("div");
    this.list.className = "screen-picker-list";
    const hint = document.createElement("div");
    hint.className = "screen-picker-hint";
    hint.textContent = "↑↓ to try · Enter to keep · Esc to go back · kibann skill show screens to make one";
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
        const top = document.createElement("div");
        top.className = "screen-picker-top";
        const name = document.createElement("span");
        name.className = "screen-picker-name";
        name.textContent = s.name;
        const origin = document.createElement("span");
        origin.className = "screen-picker-origin";
        origin.textContent = s.builtin ? "built in" : "yours";
        top.append(name, origin);
        const about = document.createElement("div");
        about.className = "screen-picker-about";
        about.textContent = s.about ?? "";
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
