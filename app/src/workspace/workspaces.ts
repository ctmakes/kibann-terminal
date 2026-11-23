// Workspace files (View → Open Workspace… / Save Workspace…): a searchable list of
// the project's, the user's and the built-in workspaces, and a name field to
// save the window as one. The files themselves are read and written by the
// app's backend (`files::workspaces`); see README "Workspace files".

import type { PaneId } from "./layout";

import type { WorkspaceEntry } from "../proto";
export type { WorkspaceEntry };

export interface WorkspaceApi {
  /** The workspaces for the directory `pane` is in. */
  list(pane: PaneId | null): Promise<WorkspaceEntry[]>;
  /** Open it (after asking, if it runs commands from someone else).
   * False when the user said no. */
  open(entry: WorkspaceEntry, pane: PaneId | null): Promise<boolean>;
  /** Save the window as `name` in the user's workspaces; the file's path.
   * Fails with a message containing "exists" unless `force`. */
  save(name: string, force: boolean): Promise<string>;
}

const ROW_H = 30;

const ORIGIN: Record<WorkspaceEntry["origin"], string> = {
  project: "this project",
  user: "yours",
  builtin: "built in",
  file: "file",
};

export class WorkspacePicker {
  readonly el: HTMLDivElement;
  visible = false;
  private mode: "open" | "save" = "open";
  private readonly input: HTMLInputElement;
  private readonly list: HTMLDivElement;
  private readonly hint: HTMLDivElement;
  private all: WorkspaceEntry[] = [];
  private shown: WorkspaceEntry[] = [];
  private at = 0;
  private pane: PaneId | null = null;
  /** In save mode: the name already asked to replace. */
  private replacing: string | null = null;

  constructor(
    private readonly api: WorkspaceApi,
    private readonly opts: {
      /** Called after it closes, to put focus back in the terminal. */
      onClose(): void;
      notice(text: string): void;
    },
  ) {
    this.el = document.createElement("div");
    this.el.className = "workspace-picker";
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
    this.el.addEventListener("mousedown", (e) => {
      if (e.target === this.el) {
        e.preventDefault();
        this.close();
      }
    });
    const panel = document.createElement("div");
    Object.assign(panel.style, {
      marginTop: "10vh",
      width: "min(480px, calc(100% - 32px))",
      background: "var(--k-panel)",
      border: "1px solid var(--k-divider)",
      borderRadius: "10px",
      boxShadow: "0 12px 40px var(--k-shadow)",
      overflow: "hidden",
    });
    this.input = document.createElement("input");
    this.input.className = "workspace-picker-input";
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
    this.input.addEventListener("input", () => {
      this.replacing = null;
      this.filter();
    });
    this.input.addEventListener("keydown", (e) => this.onKey(e));
    this.list = document.createElement("div");
    this.list.className = "workspace-picker-list";
    Object.assign(this.list.style, {
      maxHeight: `${ROW_H * 10}px`,
      overflowY: "auto",
      padding: "4px 0",
    });
    this.hint = document.createElement("div");
    Object.assign(this.hint.style, {
      padding: "6px 12px",
      color: "var(--k-muted)",
      borderTop: "1px solid var(--k-divider)",
      fontSize: "11px",
    });
    panel.append(this.input, this.list, this.hint);
    this.el.append(panel);
  }

  /** Open to pick a workspace to open, or to name the window to save. */
  async open(mode: "open" | "save", pane: PaneId | null): Promise<void> {
    if (this.visible) {
      if (this.mode === mode) return;
      this.close();
    }
    this.visible = true;
    this.mode = mode;
    this.pane = pane;
    this.replacing = null;
    this.el.style.display = "flex";
    this.input.value = "";
    this.input.placeholder =
      mode === "open"
        ? "Search workspaces"
        : "Name the workspace (saved to your workspaces)";
    this.input.focus();
    this.setHint();
    this.all = [];
    this.render();
    try {
      this.all = await this.api.list(pane);
    } catch (e) {
      this.opts.notice(`Couldn't list the workspaces: ${e}`);
    }
    if (this.visible) this.filter();
  }

  private setHint(text?: string): void {
    this.hint.textContent =
      text ??
      (this.mode === "open"
        ? "↑↓ choose   ↩ open   esc cancel"
        : "↩ save   esc cancel");
  }

  close(): void {
    if (!this.visible) return;
    this.visible = false;
    this.el.style.display = "none";
    this.opts.onClose();
  }

  private filter(): void {
    const words = this.input.value
      .trim()
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean);
    const pool =
      this.mode === "open"
        ? this.all
        : this.all.filter((l) => l.origin === "user");
    this.shown = pool.filter((l) => {
      const hay = `${l.name} ${l.about ?? ""} ${
        ORIGIN[l.origin]
      }`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
    this.at = 0;
    this.render();
  }

  private render(): void {
    const rows = this.shown.map((l, i) => {
      const row = document.createElement("div");
      row.className = "workspace-picker-row";
      row.dataset.name = l.name;
      const on = this.mode === "open" && i === this.at;
      Object.assign(row.style, {
        display: "flex",
        alignItems: "center",
        gap: "8px",
        minHeight: `${ROW_H}px`,
        padding: "0 12px",
        cursor: "default",
        background: on ? "var(--k-current)" : "",
        color: on ? "var(--k-current-text)" : "",
      });
      const text = document.createElement("div");
      Object.assign(text.style, { flex: "1", minWidth: "0" });
      const name = document.createElement("div");
      name.textContent = l.name;
      const about = document.createElement("div");
      about.textContent = l.about ?? "";
      for (const s of [name, about]) {
        Object.assign(s.style, {
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        });
      }
      Object.assign(about.style, { fontSize: "11px", opacity: "0.65" });
      text.append(name);
      if (l.about) text.append(about);
      const tag = document.createElement("span");
      tag.textContent = ORIGIN[l.origin];
      Object.assign(tag.style, {
        color: "var(--k-muted)",
        fontSize: "11px",
        flex: "0 0 auto",
      });
      row.append(text, tag);
      row.addEventListener("mousedown", (e) => {
        e.preventDefault();
        if (this.mode === "open") {
          this.at = i;
          void this.choose();
        } else {
          this.input.value = l.name;
          this.input.focus();
        }
      });
      return row;
    });
    if (rows.length === 0 && this.mode === "open") {
      const none = document.createElement("div");
      none.textContent = this.all.length ? "No workspace matches." : "";
      Object.assign(none.style, {
        padding: "6px 12px",
        color: "var(--k-muted)",
      });
      rows.push(none);
    }
    this.list.replaceChildren(...rows);
    rows[this.at]?.scrollIntoView({ block: "nearest" });
  }

  private async choose(): Promise<void> {
    const entry = this.shown[this.at];
    if (!entry) return;
    const pane = this.pane;
    this.close();
    try {
      await this.api.open(entry, pane);
    } catch (e) {
      this.opts.notice(`Couldn't open ${entry.name}: ${e}`);
    }
  }

  private async save(): Promise<void> {
    const name = this.input.value.trim();
    if (!name) return;
    if (!/^[^/\\.][^/\\]*$/.test(name)) {
      this.setHint("A name can't start with a dot or hold a slash.");
      return;
    }
    try {
      const path = await this.api.save(name, this.replacing === name);
      this.close();
      this.opts.notice(`Saved the workspace as ${name} (${path})`);
    } catch (e) {
      const msg = String(e);
      if (msg.includes("exists") && this.replacing !== name) {
        this.replacing = name;
        this.setHint(`${name} exists. ↩ again replaces it.`);
        return;
      }
      this.close();
      this.opts.notice(`Couldn't save the workspace: ${msg}`);
    }
  }

  private onKey(e: KeyboardEvent): void {
    const move = (by: number) => {
      if (this.mode !== "open" || this.shown.length === 0) return;
      this.at = Math.max(0, Math.min(this.shown.length - 1, this.at + by));
      this.render();
    };
    switch (e.key) {
      case "ArrowDown":
        move(1);
        break;
      case "ArrowUp":
        move(-1);
        break;
      case "Enter":
        void (this.mode === "open" ? this.choose() : this.save());
        break;
      case "Escape":
        this.close();
        break;
      default:
        return; // typing goes to the search or the name
    }
    e.preventDefault();
  }
}
