// Settings (Cmd+,): a modal over the workspace with tabs down the left, as
// in Note67. The config file stays the source of truth: a change here is
// written to it, and comes back through the same path as an edit by hand.

import type { Platform } from "../input/keyboard";
import type { Settings } from "../proto";
import iconUrl from "../../src-tauri/icons/128x128@2x.png";
import { changesView } from "./changes";
import { sheetGrid } from "./shortcuts";
import "../ui.css";
import "./settings.css";

export type SettingsTab = "general" | "appearance" | "shortcuts" | "about" | "updates";

import type { ConfigInfo, UpdateStatus } from "../proto";
export type { ConfigInfo, UpdateStatus };

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
  b.addEventListener("mouseenter", () => {
    if (b.disabled) return;
    if (primary) b.style.filter = "brightness(1.1)";
    else b.style.background = "var(--k-tab)";
  });
  b.addEventListener("mouseleave", () => {
    b.style.filter = "";
    b.style.background = "";
  });
  b.addEventListener("click", run);
  return b;
}

function link(text: string, run: () => void): HTMLAnchorElement {
  const a = document.createElement("a");
  a.textContent = text;
  a.className = "settings-link";
  a.href = "#";
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
    this.el.style.display = "none";
    this.el.addEventListener("mousedown", (e) => {
      if (e.target === this.el) {
        e.preventDefault();
        this.hide();
      }
    });
    this.panel = div("settings-panel");
    this.panel.tabIndex = -1;
    this.nav = div("settings-nav");
    const main = div("settings-main");
    const head = div("settings-head");
    this.title = div("settings-title");
    const close = div("settings-close");
    close.innerHTML = svg(`<path d="M4 4l8 8M12 4l-8 8"/>`);
    close.setAttribute("aria-label", "Close");
    close.addEventListener("click", () => this.hide());
    head.append(this.title, close);
    this.body = div("settings-body");
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
    const items: HTMLElement[] = [div("settings-nav-title", "Settings")];
    for (const g of TABS) {
      items.push(div("settings-group", g.group));
      for (const t of g.tabs) {
        const b = div("settings-tab");
        b.dataset.tab = t.id;
        b.setAttribute("role", "tab");
        b.setAttribute("aria-selected", String(t.id === this.tab));
        const icon = document.createElement("span");
        icon.className = "settings-tab-icon";
        icon.innerHTML = t.icon;
        const label = document.createElement("span");
        label.className = "settings-tab-label";
        label.textContent = t.label;
        b.append(icon, label);
        // An update waiting for a restart.
        if (t.id === "updates" && this.update?.state === "ready") {
          const dot = document.createElement("span");
          dot.className = "settings-update-dot";
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
    box.append(div("settings-section-title", title), ...rows);
    return box;
  }

  /** A label (and a line under it) with a control on the right. */
  private row(label: string, help: string, control: HTMLElement): HTMLElement {
    const r = div("settings-row");
    const text = div("settings-row-text");
    text.append(div("settings-label", label));
    if (help) text.append(div("settings-help", help));
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
    const knob = document.createElement("span");
    knob.className = "settings-knob";
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
    this.body.prepend(div("settings-error", text));
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
      const mac = this.hooks.platform === "mac";
      rows.push(
        this.row(
          mac ? "Keep running in the menu bar" : "Keep running in the tray",
          mac
            ? "Closing the window or Cmd+Q hides it, so agents can still notify you. Quit Kibann Terminal… ends everything."
            : "Closing the window hides it to the tray icon, so agents can still notify you; its Quit ends everything. Needs a desktop with a tray (on GNOME, the AppIndicator extension); without one, closing quits.",
          this.switch("menu-bar-icon", s.menu_bar_icon, (x, on) => (x.menu_bar_icon = on)),
        ),
      );
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
    const buttons = div("settings-buttons");
    buttons.append(
      button("Open in Editor", () => this.openConfig(false)),
      button(this.hooks.platform === "mac" ? "Show in Finder" : "Show in Folder", () => this.openConfig(true)),
    );
    const note = div("settings-note", "Every setting lives in this file, in Ghostty's syntax. Edits apply when you save it.");
    const rows: HTMLElement[] = [note, file, buttons];
    const problems = this.info?.problems ?? [];
    if (problems.length) {
      const list = div("settings-problems");
      const h = div(undefined, problems.length === 1 ? "1 line can't be used:" : `${problems.length} lines can't be used:`);
      list.append(h);
      for (const p of problems) list.append(div("settings-problem", p));
      rows.push(list);
    }
    out.push(this.section("Config file", ...rows));
    return out;
  }

  private appearance(): HTMLElement[] {
    const s = this.settings;
    const info = this.info;
    const theme = div("settings-theme");
    if (info) theme.textContent = info.light === info.dark ? info.light : `Light: ${info.light} · Dark: ${info.dark}`;
    const themeBox = div("settings-theme-box");
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
      sel.className = "settings-field settings-font";
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
      input.className = "settings-field settings-font";
      input.placeholder = "JetBrains Mono (built in)";
      input.value = current;
      const commit = () => {
        if (input.value.trim() !== current) setFamily(input.value.trim());
      };
      input.addEventListener("change", commit);
      input.addEventListener("keydown", (e) => e.key === "Enter" && input.blur());
      family = input;
    }
    const size = s.font_size ?? this.hooks.defaultFontSize;
    const setSize = (n: number | null) =>
      void this.save("font-size", n === null ? [] : [String(Math.round(Math.min(72, Math.max(6, n)) * 2) / 2)]);
    const sizeBox = div("settings-size");
    const num = document.createElement("input");
    num.className = "settings-field settings-size-input";
    num.type = "number";
    num.min = "6";
    num.max = "72";
    num.step = "0.5";
    num.value = String(size);
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
        this.row(
          "Size",
          `${this.hooks.platform === "mac" ? "Cmd+= and Cmd+-" : "Ctrl+= and Ctrl+-"} change it for now; this is where they return.`,
          sizeBox,
        ),
      ),
    );
    return out;
  }

  private shortcuts(): HTMLElement[] {
    const note = div("settings-intro");
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
    const img = document.createElement("img");
    img.className = "settings-icon";
    img.src = iconUrl;
    img.alt = "";
    const name = div("settings-name", "Kibann Terminal");
    const v = div("settings-version", version ? `Version ${version}` : "");
    top.append(img, name, v, link("GitHub", () => this.api.openUrl(REPO)));
    const desc = div("settings-desc", "A terminal for working alongside coding agents, built on libghostty.");
    const features = document.createElement("ul");
    features.className = "settings-features";
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
    const credits = div("settings-credits");
    const by = div();
    const heart = Object.assign(document.createElement("span"), { className: "settings-heart", textContent: "♥" });
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
    const left = div("settings-update-text");
    const v = div("settings-update-name", version ? `Kibann Terminal ${version}` : "Kibann Terminal");
    const said = updateText(u as UpdateStatus, version);
    const state = div(u.state === "error" ? "settings-update-state error" : "settings-update-state", said.text);
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
      card.append(b);
    }
    const notes = this.section("What's New", changesView(text, version));
    notes.classList.add("settings-notes");
    return [card, notes];
  }
}
