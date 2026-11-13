// The sidebar: every pane in every tab as one compact row (agent dot, title,
// branch, ●N), grouped by repository, the groups needing attention first.
// Hovering a row shows a live preview of the pane beside the sidebar.
// Clicking a row shows its pane; clicking ●N opens its diff.
//
// It's never hidden: collapsed, it's a narrow rail with one badge per pane
// (agent dot and first letter), which still click and preview.

import type { AgentState, PaneDir } from "../proto";
import type { PaneId } from "./layout";
import { headerParts, type GitMap } from "./pane";
import { makeDot, showDot, type StatusMap } from "./status";
import { tooltip } from "./tooltip";

export const SIDEBAR = { min: 140, max: 480, default: 220 };
/** The collapsed rail's width. */
export const RAIL = 44;
/** The bottom row's height. */
const BAR_H = 30;
/** The sidebar icon: a window with its left column marked. */
const ICON = `<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3">
<rect x="1.5" y="2.5" width="13" height="11" rx="2.5"/><line x1="5.5" y1="2.5" x2="5.5" y2="13.5"/></svg>`;
/** Theme: a circle, half dark. */
const THEME_ICON = `<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3">
<circle cx="8" cy="8" r="5.75"/><path d="M8 2.25a5.75 5.75 0 0 1 0 11.5z" fill="currentColor" stroke="none"/></svg>`;
/** Shortcuts: a keyboard. */
const KEYS_ICON = `<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3">
<rect x="1.5" y="4" width="13" height="8.5" rx="1.75"/>
<path d="M4 6.75h.01M6.67 6.75h.01M9.33 6.75h.01M12 6.75h.01M5 9.75h6" stroke-linecap="round" stroke-width="1.4"/></svg>`;
/** Dragging the edge narrower than this collapses it. */
const COLLAPSE_AT = 90;
/** The preview redraws at most this often. */
const PREVIEW_MS = 250;
/** The preview's scale: half size is small but still readable. */
const PREVIEW_SCALE = 0.5;
/** The preview's largest size (CSS px); a bigger pane is cropped. */
const PREVIEW_MAX = { w: 440, h: 260 };
const ROW_H = 24;
const STORE = "kibann.sidebar";

/** A pane as the sidebar needs it. */
export interface CardPane {
  id: PaneId;
  title: string;
  /** Index of its tab, for order. */
  tab: number;
  /** In the tab shown. */
  shown: boolean;
  focused: boolean;
  canvas: HTMLCanvasElement | null;
  /** The terminal's background, behind the thumbnail. */
  bg: string;
  /** Where the cursor is, as a fraction of the screen's height. */
  cursor: () => number;
}

export interface Group {
  key: string;
  label: string;
  panes: CardPane[];
}

const URGENCY: Partial<Record<AgentState, number>> = { blocked: 0, done: 1, working: 2 };

/** Panes grouped by repository (or directory), the groups with the most
 * urgent agent first, then in the order their first pane appears. Panes
 * keep their own order within a group. */
export function groupPanes(panes: CardPane[], git: GitMap, status: StatusMap): Group[] {
  const groups = new Map<string, Group>();
  for (const p of panes) {
    const place: PaneDir | undefined = git[String(p.id)];
    const key = place?.git ? `repo:${place.git.repo}` : place ? `dir:${place.shown}` : "unknown";
    const label = place?.git ? place.git.repo : place ? place.shown : "…";
    let g = groups.get(key);
    if (!g) groups.set(key, (g = { key, label, panes: [] }));
    g.panes.push(p);
  }
  const rank = (g: Group) =>
    Math.min(3, ...g.panes.map((p) => URGENCY[status[String(p.id)]?.state ?? "idle"] ?? 3));
  const order = [...groups.values()];
  return order
    .map((g, i) => ({ g, i, r: rank(g) }))
    .sort((a, b) => a.r - b.r || a.i - b.i)
    .map((x) => x.g);
}

/** Titles that only name the shell: a pane showing one is told apart by
 * its directory instead. */
const SHELLS = new Set(["shell", "sh", "bash", "zsh", "fish", "nu", "dash", "ksh", "tcsh", "pwsh", "powershell", "cmd"]);

/** What a pane's rail badge is made from: the program it runs, else its
 * directory's name. */
export function badgeSource(title: string, place: PaneDir | undefined): string {
  const t = title.trim().replace(/^-/, "");
  if (t && !SHELLS.has(t.toLowerCase().replace(/\.exe$/, ""))) return t;
  if (!place) return t || "·";
  const name = place.shown.split(/[\\/]/).filter(Boolean).pop() ?? "";
  return name === "" ? "/" : name;
}

/** The rail badges for panes in order (as their sources): one letter each,
 * two where a pane would read the same as the one before or after it. */
export function badgeTexts(sources: string[]): string[] {
  const chars = sources.map((s) => [...s.replace(/^[^\p{L}\p{N}~/]+/u, "") || s]);
  const one = chars.map((c) => (c[0] ?? "·").toUpperCase());
  return one.map((b, i) => {
    const clash = (j: number) => j >= 0 && j < one.length && one[j] === b && sources[j] !== sources[i];
    if (!clash(i - 1) && !clash(i + 1)) return b;
    const rest = chars[i].slice(1).find((ch) => /[\p{L}\p{N}]/u.test(ch));
    return rest ? b + rest.toLowerCase() : b;
  });
}

interface Row {
  el: HTMLDivElement;
  title: HTMLSpanElement;
  dot: HTMLSpanElement;
  line: HTMLSpanElement;
  /** The first letter, shown on the rail. */
  badge: HTMLSpanElement;
}

export interface SidebarOptions {
  /** Show this pane (its tab, focused). */
  onPick(pane: PaneId): void;
  /** ●N was clicked on this pane's row. */
  onDiff(pane: PaneId): void;
  /** The sidebar expanded, collapsed or changed width. */
  onResize(): void;
  /** The bottom row's buttons. */
  onShortcuts(): void;
  onThemes?(): void;
  /** The shortcut sheet's key, shown in its button's tooltip. */
  shortcutKey: () => string;
  /** The key that collapses and expands it, shown in its button's tooltip. */
  sidebarKey: () => string;
}

export class Sidebar {
  readonly el: HTMLDivElement;
  private readonly list: HTMLDivElement;
  private readonly rows = new Map<PaneId, Row>();
  private panes: CardPane[] = [];
  /** The hover preview: one canvas, for the pane under the pointer. */
  private readonly preview: HTMLCanvasElement;
  private previewing: PaneId | null = null;
  private previewTimer = 0;
  /** Expanded to a list; otherwise the rail. */
  expanded = false;
  width = SIDEBAR.default;
  /** The bottom row's buttons besides collapse, hidden on the rail. */
  private readonly extras: HTMLElement[] = [];
  private readonly bar: HTMLDivElement;
  private last: { git: GitMap; status: StatusMap } | null = null;

  constructor(private readonly opts: SidebarOptions) {
    this.el = document.createElement("div");
    this.el.className = "sidebar";
    Object.assign(this.el.style, {
      position: "relative",
      flex: "0 0 auto",
      display: "block",
      background: "var(--k-sidebar)",
      boxSizing: "border-box",
      color: "var(--k-text)",
      font: "12px -apple-system, system-ui, sans-serif",
      overflow: "hidden",
    });
    this.list = document.createElement("div");
    this.list.className = "sidebar-list";
    Object.assign(this.list.style, {
      position: "absolute",
      inset: `0 0 ${BAR_H}px 0`,
      overflowY: "auto",
      padding: "6px 4px",
      boxSizing: "border-box",
    });
    this.list.addEventListener("mouseleave", () => this.hidePreview());
    const grip = document.createElement("div");
    grip.className = "sidebar-grip";
    Object.assign(grip.style, {
      position: "absolute",
      top: "0",
      right: "0",
      width: "5px",
      height: "100%",
      cursor: "col-resize",
      zIndex: "2",
      // The sidebar's edge line, inside the grab area.
      borderRight: "1px solid var(--k-edge)",
      boxSizing: "border-box",
    });
    grip.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      this.hidePreview();
      grip.setPointerCapture(e.pointerId);
      const left = this.el.getBoundingClientRect().left;
      const move = (ev: PointerEvent) => {
        // Narrow enough, it folds to the rail; wider, it opens again.
        const x = ev.clientX - left;
        if (x < COLLAPSE_AT) this.setExpanded(false);
        else {
          this.width = Math.round(Math.min(SIDEBAR.max, Math.max(SIDEBAR.min, x)));
          if (this.expanded) this.apply();
          else this.setExpanded(true);
        }
      };
      const up = () => {
        grip.removeEventListener("pointermove", move);
        grip.removeEventListener("pointerup", up);
        this.save();
      };
      grip.addEventListener("pointermove", move);
      grip.addEventListener("pointerup", up);
    });
    // Fixed, so the sidebar's overflow doesn't clip it; it floats over the
    // panes beside the sidebar and never takes the pointer.
    this.preview = document.createElement("canvas");
    this.preview.className = "sidebar-preview";
    Object.assign(this.preview.style, {
      position: "fixed",
      display: "none",
      zIndex: "20",
      pointerEvents: "none",
      borderRadius: "6px",
      border: "1px solid var(--k-divider)",
      boxShadow: "0 6px 24px var(--k-shadow)",
    });
    // Bottom: the theme picker and the shortcut sheet on the left, the
    // button that collapses and expands it on the right.
    this.bar = document.createElement("div");
    this.bar.className = "sidebar-bar";
    Object.assign(this.bar.style, {
      position: "absolute",
      bottom: "0",
      left: "0",
      right: "5px",
      height: `${BAR_H}px`,
      borderTop: "1px solid var(--k-edge)",
      display: "flex",
      alignItems: "center",
      gap: "2px",
      padding: "0 6px",
      boxSizing: "border-box",
    });
    const icon = (cls: string, svg: string, run: () => void) => {
      const b = document.createElement("div");
      b.className = cls;
      b.innerHTML = svg;
      Object.assign(b.style, {
        display: "flex",
        flex: "0 0 auto",
        padding: "3px 5px",
        borderRadius: "5px",
        color: "var(--k-muted)",
        cursor: "default",
      });
      b.addEventListener("mouseenter", () => {
        b.style.background = "var(--k-tab)";
        b.style.color = "var(--k-strong)";
      });
      b.addEventListener("mouseleave", () => {
        b.style.background = "";
        b.style.color = "var(--k-muted)";
      });
      b.addEventListener("mousedown", (e) => {
        e.preventDefault(); // keep focus in the terminal
        run();
      });
      return b;
    };
    if (opts.onThemes) {
      const themes = icon("sidebar-themes", THEME_ICON, opts.onThemes);
      tooltip(themes, "Theme…", "", true);
      this.extras.push(themes);
    }
    const keys = icon("sidebar-shortcuts", KEYS_ICON, opts.onShortcuts);
    tooltip(keys, "Keyboard shortcuts", opts.shortcutKey, true);
    const spacer = document.createElement("div");
    spacer.style.flex = "1";
    this.extras.push(keys, spacer);
    const fold = icon("sidebar-fold", ICON, () => this.toggle());
    tooltip(fold, () => (this.expanded ? "Collapse sidebar" : "Expand sidebar"), opts.sidebarKey, true);
    this.bar.append(...this.extras, fold);
    this.el.append(this.bar, this.list, grip, this.preview);
    this.load();
    this.apply();
  }

  // ---- open, close, width ----

  toggle(): void {
    this.setExpanded(!this.expanded);
    this.save();
  }

  private setExpanded(on: boolean): void {
    if (on === this.expanded) return;
    this.expanded = on;
    this.hidePreview();
    this.apply();
    // Rows look different on the rail; draw them again.
    if (this.last) this.render(this.panes, this.last.git, this.last.status);
  }

  setWidth(w: number): void {
    this.width = Math.round(Math.min(SIDEBAR.max, Math.max(SIDEBAR.min, w)));
    this.apply();
  }

  private apply(): void {
    this.el.style.width = `${this.expanded ? this.width : RAIL}px`;
    this.el.dataset.expanded = String(this.expanded);
    // The rail has room for one button.
    for (const e of this.extras) e.style.display = this.expanded ? "flex" : "none";
    this.bar.style.justifyContent = this.expanded ? "flex-end" : "center";
    this.bar.style.padding = this.expanded ? "0 6px" : "0";
    this.bar.style.right = this.expanded ? "5px" : "0";
    this.opts.onResize();
  }

  private load(): void {
    try {
      const s = JSON.parse(localStorage.getItem(STORE) ?? "null");
      if (s && typeof s === "object") {
        // `visible` is the older name, from when it could be hidden.
        this.expanded = (s.expanded ?? s.visible) === true;
        if (typeof s.width === "number") this.width = Math.min(SIDEBAR.max, Math.max(SIDEBAR.min, s.width));
      }
    } catch {
      // Storage may be unavailable; the defaults stand.
    }
  }

  private save(): void {
    try {
      localStorage.setItem(STORE, JSON.stringify({ expanded: this.expanded, width: this.width }));
    } catch {
      // Not remembered this time.
    }
  }

  // ---- content ----

  /** Show these panes, grouped, with their places and states. */
  render(panes: CardPane[], git: GitMap, status: StatusMap): void {
    this.panes = panes;
    this.last = { git, status };
    const groups = groupPanes(panes, git, status);
    const keep = new Set(panes.map((p) => p.id));
    for (const [id, r] of this.rows) {
      if (keep.has(id)) continue;
      r.el.remove();
      this.rows.delete(id);
    }
    if (this.previewing !== null && !keep.has(this.previewing)) this.hidePreview();
    const nodes: HTMLElement[] = [];
    for (const g of groups) {
      const h = document.createElement("div");
      h.className = "sidebar-group";
      h.textContent = g.label;
      h.title = g.label;
      Object.assign(
        h.style,
        this.expanded
          ? {
              margin: nodes.length ? "8px 6px 2px" : "0 6px 2px",
              color: "var(--k-muted)",
              fontSize: "11px",
              fontWeight: "600",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }
          : // On the rail a group is a short line between its badges.
            {
              margin: nodes.length ? "6px 8px" : "0",
              height: nodes.length ? "1px" : "0",
              background: "var(--k-divider)",
              fontSize: "0",
              overflow: "hidden",
            },
      );
      nodes.push(h);
      const badges = badgeTexts(g.panes.map((p) => badgeSource(p.title, git[String(p.id)])));
      g.panes.forEach((p, i) => {
        const r = this.rows.get(p.id) ?? this.makeRow(p.id);
        this.fillRow(r, p, git[String(p.id)], status, badges[i]);
        nodes.push(r.el);
      });
    }
    this.list.replaceChildren(...nodes);
    if (this.previewing !== null) this.paintPreview();
  }

  private makeRow(id: PaneId): Row {
    const el = document.createElement("div");
    el.className = "sidebar-card";
    el.dataset.pane = String(id);
    Object.assign(el.style, {
      display: "flex",
      alignItems: "center",
      gap: "6px",
      height: `${ROW_H}px`,
      padding: "0 6px",
      borderRadius: "4px",
      cursor: "default",
      whiteSpace: "nowrap",
    });
    // The dot's slot keeps titles aligned whether or not there's an agent.
    const slot = document.createElement("span");
    Object.assign(slot.style, { flex: "0 0 8px", display: "flex", justifyContent: "center" });
    const dot = makeDot();
    slot.append(dot);
    const title = document.createElement("span");
    title.className = "sidebar-title";
    Object.assign(title.style, {
      flex: "1 1 auto",
      minWidth: "24px",
      overflow: "hidden",
      textOverflow: "ellipsis",
    });
    const line = document.createElement("span");
    line.className = "sidebar-line";
    Object.assign(line.style, {
      flex: "0 1 auto",
      display: "flex",
      gap: "5px",
      minWidth: "0",
      maxWidth: "60%",
      overflow: "hidden",
      fontSize: "11px",
    });
    const badge = document.createElement("span");
    badge.className = "sidebar-badge";
    Object.assign(badge.style, {
      flex: "0 0 20px",
      letterSpacing: "-0.3px",
      textAlign: "center",
      fontWeight: "600",
      overflow: "hidden",
    });
    el.append(slot, badge, title, line);
    el.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus in the terminal
      const t = e.target as HTMLElement;
      if (t.classList.contains("pane-changed")) this.opts.onDiff(id);
      else this.opts.onPick(id);
    });
    el.addEventListener("mouseenter", () => this.showPreview(id));
    const r: Row = { el, title, dot, line, badge };
    this.rows.set(id, r);
    return r;
  }

  private fillRow(r: Row, p: CardPane, place: PaneDir | undefined, status: StatusMap, badge: string): void {
    const name = p.title || "shell";
    r.title.textContent = name;
    r.badge.textContent = badge;
    const full = this.expanded;
    r.el.title = full ? (place?.dir ?? "") : [name, place?.shown].filter(Boolean).join(" — ");
    r.badge.style.display = full ? "none" : "";
    r.title.style.display = full ? "" : "none";
    r.line.style.display = full ? "flex" : "none";
    r.el.style.padding = full ? "0 6px" : "0 4px";
    r.el.style.gap = full ? "6px" : "3px";
    const current = p.focused && p.shown;
    r.el.style.background = current ? "var(--k-current)" : "";
    const color = current ? "var(--k-current-text)" : p.shown ? "var(--k-title)" : "var(--k-label)";
    r.title.style.color = color;
    r.badge.style.color = color;
    const s = status[String(p.id)];
    showDot(r.dot, s && s.state !== "idle" ? s.state : null, s?.agent);
    // The header's parts without the repo or directory (the group says it).
    const parts = headerParts(place).filter((x) => x.kind !== "place");
    r.line.replaceChildren(
      ...parts.map((x) => {
        const span = document.createElement("span");
        span.className = `pane-${x.kind}`;
        span.textContent = x.text;
        Object.assign(span.style, {
          overflow: "hidden",
          textOverflow: "ellipsis",
          flex: x.kind === "branch" ? "0 1 auto" : "0 0 auto",
          minWidth: x.kind === "branch" ? "0" : "",
          color: x.kind === "changed" ? "var(--k-changed)" : x.kind === "conflicts" ? "var(--k-conflicts)" : "var(--k-muted)",
          cursor: x.kind === "changed" ? "pointer" : "",
        });
        return span;
      }),
    );
  }

  // ---- the hover preview ----

  private pane(id: PaneId): CardPane | undefined {
    return this.panes.find((p) => p.id === id);
  }

  private showPreview(id: PaneId): void {
    this.previewing = id;
    this.paintPreview();
  }

  private hidePreview(): void {
    this.previewing = null;
    clearTimeout(this.previewTimer);
    this.previewTimer = 0;
    this.preview.style.display = "none";
  }

  /** The pane painted: if it's the one previewed, redraw soon. */
  painted(id: PaneId): void {
    if (id !== this.previewing || this.previewTimer) return;
    this.previewTimer = window.setTimeout(() => {
      this.previewTimer = 0;
      this.paintPreview();
    }, PREVIEW_MS);
  }

  /** Draw the previewed pane at half size beside its row; a pane bigger
   * than the preview shows its left part, around the cursor vertically
   * (where the activity is: a prompt, an agent's input box). */
  private paintPreview(): void {
    const id = this.previewing;
    const p = id === null ? undefined : this.pane(id);
    const row = id === null ? undefined : this.rows.get(id);
    const src = p?.canvas;
    if (!p || !row || !src || src.width === 0 || src.height === 0) {
      this.preview.style.display = "none";
      return;
    }
    const dpr = window.devicePixelRatio || 1;
    const w = Math.min(PREVIEW_MAX.w, Math.round((src.width / dpr) * PREVIEW_SCALE));
    const h = Math.min(PREVIEW_MAX.h, Math.round((src.height / dpr) * PREVIEW_SCALE));
    const c = this.preview;
    if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) {
      c.width = Math.round(w * dpr);
      c.height = Math.round(h * dpr);
      c.style.width = `${w}px`;
      c.style.height = `${h}px`;
    }
    const side = this.el.getBoundingClientRect();
    const at = row.el.getBoundingClientRect();
    const top = Math.max(4, Math.min(window.innerHeight - h - 6, at.top + at.height / 2 - h / 2));
    c.style.left = `${side.right + 6}px`;
    c.style.top = `${top}px`;
    c.style.background = p.bg;
    c.style.display = "block";
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, c.width, c.height);
    const sw = Math.min(src.width, c.width / PREVIEW_SCALE);
    const sh = Math.min(src.height, c.height / PREVIEW_SCALE);
    const cursor = p.cursor();
    const sy = Math.max(0, Math.min(src.height - sh, cursor * src.height - sh * 0.7));
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(src, 0, sy, sw, sh, 0, 0, sw * PREVIEW_SCALE, sh * PREVIEW_SCALE);
  }

  dispose(): void {
    this.hidePreview();
  }
}
