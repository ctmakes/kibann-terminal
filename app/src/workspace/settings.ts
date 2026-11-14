// Settings (Cmd+,): a modal over the workspace with tabs down the left, as
// in Note67. The config file stays the source of truth: a change here is
// written to it, and comes back through the same path as an edit by hand.

import type { Platform } from "../input/keyboard";
import type { Settings } from "../proto";
import iconUrl from "../../src-tauri/icons/128x128@2x.png";
import { changesView } from "./changes";
import { sheetGrid } from "./shortcuts";

export type SettingsTab = "general" | "appearance" | "shortcuts" | "about" | "updates";

/** Where updating stands (update.rs's `Status`). */
export interface UpdateStatus {
  state: "idle" | "checking" | "downloading" | "ready" | "latest" | "error" | "dev";
  version: string | null;
  error: string | null;
  /** When the last check ended, in ms since 1970. */
  checked: number | null;
}

/** The config file: where, its problems, the themes it chooses. */
export interface ConfigInfo {
  path: string;
  problems: string[];
  light: string;
  dark: string;
}

/** What Settings needs from the app. */
export interface SettingsApi {
  /** Write `key = value` lines to the config file (none: the default). */
  set(key: string, values: string[]): Promise<void>;
  info(): Promise<ConfigInfo>;
  /** Open the config file in the text editor, or show it in the file manager. */
  openConfig(reveal: boolean): Promise<void>;
  /** The installed monospaced fonts, or null where they can't be listed. */
  fonts(): Promise<string[] | null>;
  updateStatus(): Promise<UpdateStatus>;
  checkUpdates(): Promise<void>;
  installUpdate(): Promise<void>;
  /** CHANGELOG.md and the running version. */
  changelog(): Promise<[string, string]>;
  openUrl(url: string): void;
}

export interface SettingsHooks {
  platform: Platform;
  /** The size panes use without a `font-size` line. */
  defaultFontSize: number;
  /** Open the theme picker (Settings closes first). */
  chooseTheme(): void;
  /** Settings closed: focus goes back to the terminal. */
  onClose(): void;
}

const REPO = "https://github.com/ctmakes/kibann-terminal";
const STORE = "kibann.settings.tab";

const svg = (body: string) =>
  `<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;

/** A gear: the sidebar's button. */
export const GEAR_ICON = svg(
  `<circle cx="8" cy="8" r="2.2"/><path d="M8 1.5v1.6M8 12.9v1.6M1.5 8h1.6M12.9 8h1.6M3.4 3.4l1.15 1.15M11.45 11.45l1.15 1.15M3.4 12.6l1.15-1.15M11.45 4.55l1.15-1.15"/><circle cx="8" cy="8" r="4.6"/>`,
);

const TABS: Array<{ group: string; tabs: Array<{ id: SettingsTab; label: string; icon: string }> }> = [
  {
    group: "General",
    tabs: [
      { id: "general", label: "General", icon: GEAR_ICON },
      {
        id: "appearance",
        label: "Appearance",
        icon: svg(`<circle cx="8" cy="8" r="5.75"/><path d="M8 2.25a5.75 5.75 0 0 1 0 11.5z" fill="currentColor" stroke="none"/>`),
      },
      {
        id: "shortcuts",
        label: "Shortcuts",
        icon: svg(`<rect x="1.5" y="4" width="13" height="8.5" rx="1.75"/><path d="M4 6.75h.01M6.67 6.75h.01M9.33 6.75h.01M12 6.75h.01M5 9.75h6"/>`),
      },
    ],
  },
  {
    group: "Help & Info",
    tabs: [
      { id: "about", label: "About", icon: svg(`<circle cx="8" cy="8" r="6"/><path d="M8 7.25v4M8 4.9h.01"/>`) },
      {
        id: "updates",
        label: "Updates",
        icon: svg(`<path d="M13.5 8a5.5 5.5 0 0 1-9.6 3.65M2.5 8a5.5 5.5 0 0 1 9.6-3.65"/><path d="M12.4 1.9v2.6H9.8M3.6 14.1v-2.6h2.6"/>`),
      },
    ],
  },
];

const css = (el: HTMLElement, style: Partial<CSSStyleDeclaration>) => Object.assign(el.style, style);

function div(cls?: string, text?: string): HTMLDivElement {
  const d = document.createElement("div");
  if (cls) d.className = cls;
  if (text !== undefined) d.textContent = text;
  return d;
}

/** A button: `primary` is the tab's main action, solid in the accent
 * blue; the others are outlined. */
function button(text: string, run: () => void, primary = false): HTMLButtonElement {
  const b = document.createElement("button");
  b.className = primary ? "settings-button settings-primary" : "settings-button";
  b.textContent = text;
  const rest = primary ? "var(--k-accent)" : "transparent";
  css(b, {
    flex: "0 0 auto",
    font: `${primary ? "500 " : ""}12px -apple-system, system-ui, sans-serif`,
    padding: "4px 12px",
    borderRadius: "6px",
    border: primary ? "1px solid var(--k-accent)" : "1px solid var(--k-muted)",
    background: rest,
    color: primary ? "var(--k-accent-text)" : "var(--k-bright)",
    cursor: "default",
  });
  b.addEventListener("mouseenter", () => {
    if (b.disabled) return;
    if (primary) b.style.filter = "brightness(1.1)";
    else b.style.background = "var(--k-tab)";
  });
  b.addEventListener("mouseleave", () => {
    b.style.filter = "";
    b.style.background = rest;
  });
  b.addEventListener("click", run);
  return b;
}

function link(text: string, run: () => void): HTMLAnchorElement {
  const a = document.createElement("a");
  a.textContent = text;
  a.href = "#";
  css(a, { color: "var(--k-accent)", textDecoration: "none", cursor: "pointer" });
  a.addEventListener("click", (e) => {
    e.preventDefault();
    run();
  });
  return a;
}

/** "just now", "5 min ago", "2 h ago", or the date. */
export function ago(ms: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(ms).toLocaleDateString();
}

/** What the Updates tab says for `u` (and whether Check is offered). */
export function updateText(u: UpdateStatus, version: string): { text: string; canCheck: boolean } {
  const when = u.checked ? `, checked ${ago(u.checked)}` : "";
  switch (u.state) {
    case "dev":
      return { text: "Dev builds don't update.", canCheck: false };
    case "checking":
      return { text: "Checking…", canCheck: false };
    case "downloading":
      return { text: `Downloading ${u.version ?? "an update"}…`, canCheck: false };
    case "ready":
      return { text: `${u.version} is ready. The shells keep running through the restart.`, canCheck: false };
    case "latest":
      return { text: `${version} is the newest version${when}.`, canCheck: true };
    case "error":
      return { text: `Couldn't check: ${u.error ?? "unknown error"}${when}.`, canCheck: true };
    default:
      return { text: "Checks every hour.", canCheck: true };
  }
}

export class SettingsModal {
  readonly el: HTMLDivElement;
  visible = false;
  tab: SettingsTab = "general";
  private readonly panel: HTMLDivElement;
  private readonly nav: HTMLDivElement;
  private readonly title: HTMLDivElement;
  private readonly body: HTMLDivElement;
  private settings: Settings | null = null;
  private info: ConfigInfo | null = null;
  private update: UpdateStatus | null = null;
  private fonts: string[] | null | undefined;
  private changelog: [string, string] | null = null;
  /** A redraw waiting for an input to lose focus. */
  private stale = false;

  constructor(
    private readonly api: SettingsApi,
    private readonly hooks: SettingsHooks,
  ) {
    try {
      const t = localStorage.getItem(STORE) as SettingsTab | null;
      if (t && TABS.some((g) => g.tabs.some((x) => x.id === t))) this.tab = t;
    } catch {
      // Storage unavailable: start on General.
    }
    this.el = div("settings-modal");
    css(this.el, {
      position: "absolute",
      inset: "0",
      zIndex: "30",
      display: "none",
      alignItems: "center",
      justifyContent: "center",
      background: "var(--k-scrim)",
      font: "12px -apple-system, system-ui, sans-serif",
      color: "var(--k-strong)",
    });
    this.el.addEventListener("mousedown", (e) => {
      if (e.target === this.el) {
        e.preventDefault();
        this.hide();
      }
    });
    this.panel = div("settings-panel");
    this.panel.tabIndex = -1;
    css(this.panel, {
      display: "flex",
      width: "min(760px, calc(100% - 32px))",
      height: "min(540px, calc(100% - 32px))",
      background: "var(--k-panel)",
      border: "1px solid var(--k-divider)",
      borderRadius: "12px",
      boxShadow: "0 12px 40px var(--k-shadow)",
      overflow: "hidden",
      outline: "none",
    });
    this.nav = div("settings-nav");
    css(this.nav, {
      flex: "0 0 168px",
      background: "var(--k-sidebar)",
      borderRight: "1px solid var(--k-divider)",
      padding: "14px 8px",
      boxSizing: "border-box",
      overflowY: "auto",
    });
    const main = div();
    css(main, { flex: "1 1 auto", minWidth: "0", display: "flex", flexDirection: "column" });
    const head = div();
    css(head, {
      display: "flex",
      alignItems: "center",
      padding: "12px 16px",
      borderBottom: "1px solid var(--k-divider)",
    });
    this.title = div("settings-title");
    css(this.title, { flex: "1", fontSize: "14px", fontWeight: "600", color: "var(--k-bright)" });
    const close = div("settings-close");
    close.innerHTML = svg(`<path d="M4 4l8 8M12 4l-8 8"/>`);
    close.setAttribute("aria-label", "Close");
    css(close, { display: "flex", padding: "3px", borderRadius: "5px", color: "var(--k-muted)" });
    close.addEventListener("click", () => this.hide());
    head.append(this.title, close);
    this.body = div("settings-body");
    css(this.body, { flex: "1 1 auto", overflowY: "auto", padding: "14px 18px 18px", lineHeight: "1.45" });
    // A redraw put off while typing happens when the field is left.
    this.body.addEventListener("focusout", () => {
      queueMicrotask(() => {
        if (this.stale && !this.typing()) this.render();
      });
    });
    main.append(head, this.body);
    this.panel.append(this.nav, main);
    this.el.append(this.panel);
  }

  // ---- open, close ----

  show(tab?: SettingsTab): void {
    if (tab) this.select(tab, false);
    this.visible = true;
    this.el.style.display = "flex";
    this.render();
    this.panel.focus();
    void this.load();
  }

  hide(): void {
    if (!this.visible) return;
    this.visible = false;
    this.el.style.display = "none";
    this.hooks.onClose();
  }

  toggle(): void {
    if (this.visible) this.hide();
    else this.show();
  }

  select(tab: SettingsTab, draw = true): void {
    this.tab = tab;
    try {
      localStorage.setItem(STORE, tab);
    } catch {
      // Storage unavailable: it starts on General next time.
    }
    if (draw) {
      this.render();
      this.body.scrollTop = 0;
    }
  }

  /** Fetch what the tabs show, then draw them again. */
  private async load(): Promise<void> {
    const [info, update, changelog] = await Promise.all([
      this.api.info(),
      this.api.updateStatus(),
      this.changelog ? Promise.resolve(this.changelog) : this.api.changelog(),
    ]);
    this.info = info;
    this.update = update;
    this.changelog = changelog;
    if (this.fonts === undefined) this.fonts = await this.api.fonts();
    this.redraw();
  }

  // ---- what changed outside ----

  setSettings(s: Settings): void {
    this.settings = s;
    this.redraw();
  }

  setInfo(info: ConfigInfo): void {
    this.info = info;
    this.redraw();
  }

  setUpdate(u: UpdateStatus): void {
    this.update = u;
    this.renderNav();
    if (this.tab === "updates") this.redraw();
  }

  private typing(): boolean {
    const a = document.activeElement;
    return !!a && this.body.contains(a) && (a.tagName === "INPUT" || a.tagName === "SELECT");
  }

  /** Draw again, unless that would take the field being typed in away. */
  private redraw(): void {
    if (!this.visible) return;
    if (this.typing()) this.stale = true;
    else this.render();
  }

  // ---- drawing ----

  private render(): void {
    this.stale = false;
    this.renderNav();
    const all = TABS.flatMap((g) => g.tabs);
    this.title.textContent = all.find((t) => t.id === this.tab)!.label;
    const parts: Record<SettingsTab, () => HTMLElement[]> = {
      general: () => this.general(),
      appearance: () => this.appearance(),
      shortcuts: () => this.shortcuts(),
      about: () => this.about(),
      updates: () => this.updates(),
    };
    this.body.dataset.tab = this.tab;
    this.body.replaceChildren(...parts[this.tab]());
  }

  private renderNav(): void {
    const title = div(undefined, "Settings");
    css(title, { fontSize: "14px", fontWeight: "600", color: "var(--k-bright)", padding: "0 8px 10px" });
    const items: HTMLElement[] = [title];
    for (const g of TABS) {
      const h = div(undefined, g.group);
      css(h, {
        fontSize: "10px",
        fontWeight: "600",
        textTransform: "uppercase",
        letterSpacing: "0.6px",
        color: "var(--k-muted)",
        padding: "10px 8px 4px",
      });
      items.push(h);
      for (const t of g.tabs) {
        const on = t.id === this.tab;
        const b = div("settings-tab");
        b.dataset.tab = t.id;
        b.setAttribute("role", "tab");
        b.setAttribute("aria-selected", String(on));
        css(b, {
          display: "flex",
          alignItems: "center",
          gap: "8px",
          padding: "5px 8px",
          marginBottom: "1px",
          borderRadius: "6px",
          background: on ? "var(--k-current)" : "",
          color: on ? "var(--k-current-text)" : "var(--k-text)",
          cursor: "default",
        });
        const icon = document.createElement("span");
        icon.innerHTML = t.icon;
        css(icon, { display: "flex", opacity: on ? "1" : "0.8" });
        const label = document.createElement("span");
        label.textContent = t.label;
        label.style.flex = "1";
        b.append(icon, label);
        // An update waiting for a restart.
        if (t.id === "updates" && this.update?.state === "ready") {
          const dot = document.createElement("span");
          dot.className = "settings-update-dot";
          css(dot, { width: "7px", height: "7px", borderRadius: "50%", background: "var(--k-accent)" });
          b.append(dot);
        }
        b.addEventListener("mousedown", (e) => {
          e.preventDefault();
          this.select(t.id);
        });
        items.push(b);
      }
    }
    this.nav.replaceChildren(...items);
  }

  /** A titled group of rows. */
  private section(title: string, ...rows: HTMLElement[]): HTMLElement {
    const box = div("settings-section");
    css(box, { marginBottom: "18px" });
    const h = div(undefined, title);
    css(h, { fontSize: "11px", fontWeight: "600", color: "var(--k-muted)", marginBottom: "6px" });
    box.append(h, ...rows);
    return box;
  }

  /** A label (and a line under it) with a control on the right. */
  private row(label: string, help: string, control: HTMLElement): HTMLElement {
    const r = div("settings-row");
    css(r, {
      display: "flex",
      alignItems: "center",
      gap: "16px",
      padding: "8px 0",
      borderBottom: "1px solid var(--k-divider)",
    });
    const text = div();
    text.style.flex = "1";
    const l = div("settings-label", label);
    l.style.color = "var(--k-bright)";
    text.append(l);
    if (help) {
      const h = div(undefined, help);
      css(h, { color: "var(--k-muted)", fontSize: "11px", marginTop: "2px" });
      text.append(h);
    }
    r.append(text, control);
    return r;
  }

  /** An on/off switch for a `true`/`false` config key. */
  private switch(key: string, on: boolean, change: (s: Settings, on: boolean) => void): HTMLElement {
    const b = document.createElement("button");
    b.className = "settings-switch";
    b.dataset.key = key;
    b.setAttribute("role", "switch");
    b.setAttribute("aria-checked", String(on));
    css(b, {
      flex: "0 0 auto",
      position: "relative",
      width: "32px",
      height: "18px",
      borderRadius: "9px",
      border: "none",
      padding: "0",
      background: on ? "var(--k-accent)" : "var(--k-divider)",
      cursor: "default",
    });
    const knob = document.createElement("span");
    css(knob, {
      position: "absolute",
      top: "2px",
      left: on ? "16px" : "2px",
      width: "14px",
      height: "14px",
      borderRadius: "50%",
      background: "#fff",
      boxShadow: "0 1px 2px rgba(0,0,0,0.3)",
    });
    b.append(knob);
    b.addEventListener("click", () => {
      // Shown at once; the file's echo confirms it.
      if (this.settings) change(this.settings, !on);
      this.render();
      void this.save(key, [String(!on)]);
    });
    return b;
  }

  private openConfig(reveal: boolean): void {
    this.api.openConfig(reveal).catch((e) => this.flash(`Couldn't open the config file: ${e}`));
  }

  private async save(key: string, values: string[]): Promise<void> {
    try {
      await this.api.set(key, values);
    } catch (e) {
      this.flash(`Couldn't save ${key}: ${e}`);
    }
  }

  /** A line at the top of the tab, for a save that failed. */
  private flash(text: string): void {
    const n = div("settings-error", text);
    css(n, { color: "var(--k-conflicts)", marginBottom: "10px" });
    this.body.prepend(n);
  }

  private general(): HTMLElement[] {
    const s = this.settings;
    const out: HTMLElement[] = [];
    if (s) {
      const rows = [
        this.row(
          "Notify when an agent needs you",
          "A system notification when an agent in a pane you aren't looking at waits for you or finishes.",
          this.switch("agent-notifications", s.agent_notifications, (x, on) => (x.agent_notifications = on)),
        ),
      ];
      if (this.hooks.platform === "mac") {
        rows.push(
          this.row(
            "Keep running in the menu bar",
            "Closing the window or Cmd+Q hides it, so agents can still notify you. Quit Kibann Terminal… ends everything.",
            this.switch("menu-bar-icon", s.menu_bar_icon, (x, on) => (x.menu_bar_icon = on)),
          ),
        );
      }
      rows.push(
        this.row(
          "Preview panes in the sidebar",
          "Hovering a pane in the sidebar shows it, live, beside the sidebar.",
          this.switch("sidebar-preview", s.sidebar_preview, (x, on) => (x.sidebar_preview = on)),
        ),
        this.row(
          "Reopen tabs and splits",
          "After a restart, the tabs and splits come back with fresh shells in the same directories.",
          this.switch("restore-layout", s.restore_layout, (x, on) => (x.restore_layout = on)),
        ),
      );
      out.push(this.section("Behavior", ...rows));
    }
    const file = div("settings-path", this.info?.path ?? "");
    css(file, { font: "11px ui-monospace, SFMono-Regular, Menlo, monospace", color: "var(--k-text)", userSelect: "text", wordBreak: "break-all" });
    const buttons = div();
    css(buttons, { display: "flex", gap: "8px", marginTop: "8px" });
    buttons.append(
      button("Open in Editor", () => this.openConfig(false)),
      button(this.hooks.platform === "mac" ? "Show in Finder" : "Show in Folder", () => this.openConfig(true)),
    );
    const note = div(undefined, "Every setting lives in this file, in Ghostty's syntax. Edits apply when you save it.");
    css(note, { color: "var(--k-muted)", fontSize: "11px", marginBottom: "6px" });
    const rows: HTMLElement[] = [note, file, buttons];
    const problems = this.info?.problems ?? [];
    if (problems.length) {
      const list = div("settings-problems");
      css(list, { marginTop: "10px", color: "var(--k-conflicts)" });
      const h = div(undefined, problems.length === 1 ? "1 line can't be used:" : `${problems.length} lines can't be used:`);
      list.append(h);
      for (const p of problems) {
        const l = div(undefined, p);
        css(l, { font: "11px ui-monospace, SFMono-Regular, Menlo, monospace", marginTop: "2px" });
        list.append(l);
      }
      rows.push(list);
    }
    out.push(this.section("Config file", ...rows));
    return out;
  }

  private appearance(): HTMLElement[] {
    const s = this.settings;
    const info = this.info;
    const theme = div("settings-theme");
    theme.style.color = "var(--k-text)";
    if (info) theme.textContent = info.light === info.dark ? info.light : `Light: ${info.light} · Dark: ${info.dark}`;
    const themeBox = div();
    css(themeBox, { display: "flex", alignItems: "center", gap: "12px" });
    themeBox.append(
      theme,
      button(
        "Choose…",
        () => {
          this.hide();
          this.hooks.chooseTheme();
        },
        true,
      ),
    );
    const out = [
      this.section(
        "Colors",
        this.row("Theme", "Colors the terminals and the window. Choosing shows each theme live on your panes.", themeBox),
      ),
    ];
    if (!s) return out;
    const families = s.font_family;
    const fallbacks = families.slice(1);
    // The first family is chosen here; fallbacks from the file stay after it.
    const setFamily = (name: string) => void this.save("font-family", name ? [name, ...fallbacks] : []);
    let family: HTMLElement;
    const current = families[0] ?? "";
    if (this.fonts) {
      const sel = document.createElement("select");
      sel.className = "settings-font";
      const opt = (value: string, text: string) => {
        const o = document.createElement("option");
        o.value = value;
        o.textContent = text;
        return o;
      };
      sel.append(opt("", "JetBrains Mono (built in)"));
      if (current && !this.fonts.includes(current)) sel.append(opt(current, `${current} (not installed)`));
      for (const f of this.fonts) sel.append(opt(f, f));
      sel.value = current;
      sel.addEventListener("change", () => setFamily(sel.value));
      family = sel;
    } else {
      const input = document.createElement("input");
      input.className = "settings-font";
      input.placeholder = "JetBrains Mono (built in)";
      input.value = current;
      const commit = () => {
        if (input.value.trim() !== current) setFamily(input.value.trim());
      };
      input.addEventListener("change", commit);
      input.addEventListener("keydown", (e) => e.key === "Enter" && input.blur());
      family = input;
    }
    css(family, {
      width: "200px",
      font: "12px -apple-system, system-ui, sans-serif",
      padding: "3px 6px",
      borderRadius: "6px",
      border: "1px solid var(--k-divider)",
      background: "var(--k-bg)",
      color: "var(--k-bright)",
    });
    const size = s.font_size ?? this.hooks.defaultFontSize;
    const setSize = (n: number | null) =>
      void this.save("font-size", n === null ? [] : [String(Math.round(Math.min(72, Math.max(6, n)) * 2) / 2)]);
    const sizeBox = div("settings-size");
    css(sizeBox, { display: "flex", alignItems: "center", gap: "6px" });
    const num = document.createElement("input");
    num.type = "number";
    num.min = "6";
    num.max = "72";
    num.step = "0.5";
    num.value = String(size);
    css(num, {
      width: "56px",
      font: "12px -apple-system, system-ui, sans-serif",
      padding: "3px 6px",
      borderRadius: "6px",
      border: "1px solid var(--k-divider)",
      background: "var(--k-bg)",
      color: "var(--k-bright)",
    });
    num.addEventListener("change", () => {
      const n = Number(num.value);
      if (Number.isFinite(n) && n > 0) setSize(n);
    });
    num.addEventListener("keydown", (e) => e.key === "Enter" && num.blur());
    sizeBox.append(button("−", () => setSize(size - 1)), num, button("+", () => setSize(size + 1)));
    if (s.font_size !== null) sizeBox.append(link("Default", () => setSize(null)));
    const help = fallbacks.length
      ? `Then ${fallbacks.join(", ")} (from the config file), the built-in font, and the system's.`
      : "Tried first; then the built-in font and the system's, for CJK and emoji.";
    out.push(
      this.section(
        "Font",
        this.row("Family", help, family),
        this.row("Size", "Cmd+= and Cmd+- change it for now; this is where they return.", sizeBox),
      ),
    );
    return out;
  }

  private shortcuts(): HTMLElement[] {
    const note = div();
    css(note, { color: "var(--k-muted)", marginBottom: "12px" });
    note.append(
      "Ghostty's shortcuts. To change one, add a ",
      Object.assign(document.createElement("code"), { textContent: "keybind = …" }),
      " line to the config file (",
      link("open it", () => this.openConfig(false)),
      ").",
    );
    return [note, sheetGrid(this.hooks.platform)];
  }

  private about(): HTMLElement[] {
    const version = this.changelog?.[1] ?? "";
    const top = div("settings-about");
    css(top, { display: "flex", flexDirection: "column", alignItems: "center", gap: "4px", padding: "6px 0 14px" });
    const img = document.createElement("img");
    img.src = iconUrl;
    img.alt = "";
    css(img, { width: "72px", height: "72px" });
    const name = div(undefined, "Kibann Terminal");
    css(name, { fontSize: "16px", fontWeight: "600", color: "var(--k-bright)" });
    const v = div("settings-version", version ? `Version ${version}` : "");
    v.style.color = "var(--k-muted)";
    top.append(img, name, v, link("GitHub", () => this.api.openUrl(REPO)));
    const desc = div(undefined, "A terminal for working alongside coding agents, built on libghostty.");
    css(desc, { background: "var(--k-tab)", borderRadius: "8px", padding: "10px 12px", color: "var(--k-text)", marginBottom: "14px" });
    const features = document.createElement("ul");
    css(features, { margin: "0 0 14px", paddingLeft: "18px", color: "var(--k-text)" });
    for (const f of [
      "Agents open panes beside you, run commands and wait on them",
      "Every agent's state at a glance, with a notification when one needs you",
      "Each pane shows its repo, branch and changes; click for the diff",
      "Shells keep running when the window closes",
    ]) {
      const li = document.createElement("li");
      li.textContent = f;
      features.append(li);
    }
    const credits = div();
    css(credits, { color: "var(--k-muted)", fontSize: "11px", display: "flex", flexDirection: "column", gap: "4px" });
    const by = div();
    const heart = Object.assign(document.createElement("span"), { textContent: "♥" });
    heart.style.color = "#ef4444";
    by.append("Built with ", heart, " by ", link("@ctmakes", () => this.api.openUrl("https://ctmakes.com")));
    const more = div();
    more.append(
      "If you like it, try ",
      link("Note67", () => this.api.openUrl("https://note67.com")),
      ", ",
      link("Leapcount", () => this.api.openUrl("https://leapcount.com")),
      " and ",
      link("ZapYap", () => this.api.openUrl("https://zapyap.com")),
      " too.",
    );
    const oss = div(
      undefined,
      "MIT licensed. Built on libghostty (MIT) and Tauri; themes from iTerm2-Color-Schemes (MIT); JetBrains Mono (OFL).",
    );
    credits.append(by, more, oss);
    return [top, desc, this.section("Features", features), credits];
  }

  private updates(): HTMLElement[] {
    const [text, version] = this.changelog ?? ["", ""];
    const u = this.update ?? { state: "idle", version: null, error: null, checked: null };
    const card = div("settings-update");
    css(card, { display: "flex", alignItems: "center", gap: "12px", background: "var(--k-tab)", borderRadius: "8px", padding: "12px" });
    const left = div();
    left.style.flex = "1";
    const v = div(undefined, version ? `Kibann Terminal ${version}` : "Kibann Terminal");
    css(v, { fontWeight: "600", color: "var(--k-bright)" });
    const said = updateText(u as UpdateStatus, version);
    const state = div("settings-update-state", said.text);
    css(state, { color: u.state === "error" ? "var(--k-conflicts)" : "var(--k-muted)", marginTop: "2px" });
    left.append(v, state);
    card.append(left);
    if (u.state === "ready") {
      card.append(
        button(
          `Restart to Update`,
          () => void this.api.installUpdate().catch((e) => this.flash(`Couldn't update: ${e}`)),
          true,
        ),
      );
    } else if (u.state !== "dev") {
      const b = button(
        "Check for Updates",
        () => void this.api.checkUpdates().catch((e) => this.flash(`Couldn't check: ${e}`)),
        true,
      );
      b.disabled = !said.canCheck;
      if (b.disabled) b.style.opacity = "0.5";
      card.append(b);
    }
    const notes = this.section("What's New", changesView(text, version));
    notes.style.marginTop = "18px";
    return [card, notes];
  }
}
