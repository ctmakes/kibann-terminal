// The sidebar: every pane in every tab as a card (a live thumbnail, where it
// is, its agent dot), grouped by repository, the groups needing attention
// first. Clicking a card shows its pane; clicking ●N opens its diff.

import type { AgentState, PaneDir } from "../proto";
import type { PaneId } from "./layout";
import { headerParts, type GitMap } from "./pane";
import { makeDot, showDot, type StatusMap } from "./status";

export const SIDEBAR = { min: 160, max: 480, default: 240 };
/** Thumbnails redraw at most this often per pane. */
const THUMB_MS = 500;
/** Text smaller than this is a grey smear; crop instead of shrinking. */
const MIN_SCALE = 0.4;
const THUMB_MAX_H = 120;
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

interface Card {
  el: HTMLDivElement;
  thumb: HTMLCanvasElement;
  line: HTMLDivElement;
  title: HTMLDivElement;
  dot: HTMLSpanElement;
  timer: number;
  dirty: boolean;
}

export interface SidebarOptions {
  /** Show this pane (its tab, focused). */
  onPick(pane: PaneId): void;
  /** ●N was clicked on this pane's card. */
  onDiff(pane: PaneId): void;
  /** The sidebar opened, closed or changed width. */
  onResize(): void;
}

export class Sidebar {
  readonly el: HTMLDivElement;
  private readonly list: HTMLDivElement;
  private readonly cards = new Map<PaneId, Card>();
  private panes: CardPane[] = [];
  visible = false;
  width = SIDEBAR.default;

  constructor(private readonly opts: SidebarOptions) {
    this.el = document.createElement("div");
    this.el.className = "sidebar";
    Object.assign(this.el.style, {
      position: "relative",
      flex: "0 0 auto",
      display: "none",
      background: "#101010",
      boxSizing: "border-box",
      color: "#9a9a9a",
      font: "11px -apple-system, system-ui, sans-serif",
      overflow: "hidden",
    });
    this.list = document.createElement("div");
    this.list.className = "sidebar-list";
    Object.assign(this.list.style, {
      position: "absolute",
      inset: "0",
      overflowY: "auto",
      padding: "8px",
      boxSizing: "border-box",
    });
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
      borderRight: "1px solid #2a2a2a",
      boxSizing: "border-box",
    });
    grip.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      grip.setPointerCapture(e.pointerId);
      const left = this.el.getBoundingClientRect().left;
      const move = (ev: PointerEvent) => this.setWidth(ev.clientX - left);
      const up = () => {
        grip.removeEventListener("pointermove", move);
        grip.removeEventListener("pointerup", up);
        this.save();
      };
      grip.addEventListener("pointermove", move);
      grip.addEventListener("pointerup", up);
    });
    this.el.append(this.list, grip);
    this.load();
    this.apply();
  }

  // ---- open, close, width ----

  toggle(): void {
    this.visible = !this.visible;
    this.apply();
    this.save();
    if (this.visible) for (const id of this.cards.keys()) this.paintNow(id);
  }

  setWidth(w: number): void {
    this.width = Math.round(Math.min(SIDEBAR.max, Math.max(SIDEBAR.min, w)));
    this.apply();
  }

  private apply(): void {
    this.el.style.display = this.visible ? "block" : "none";
    this.el.style.width = `${this.width}px`;
    for (const id of this.cards.keys()) this.sizeThumb(id);
    this.opts.onResize();
  }

  private load(): void {
    try {
      const s = JSON.parse(localStorage.getItem(STORE) ?? "null");
      if (s && typeof s === "object") {
        this.visible = s.visible === true;
        if (typeof s.width === "number") this.width = Math.min(SIDEBAR.max, Math.max(SIDEBAR.min, s.width));
      }
    } catch {
      // Storage may be unavailable; the defaults stand.
    }
  }

  private save(): void {
    try {
      localStorage.setItem(STORE, JSON.stringify({ visible: this.visible, width: this.width }));
    } catch {
      // Not remembered this time.
    }
  }

  // ---- content ----

  /** Show these panes, grouped, with their places and states. */
  render(panes: CardPane[], git: GitMap, status: StatusMap): void {
    this.panes = panes;
    const groups = groupPanes(panes, git, status);
    const keep = new Set(panes.map((p) => p.id));
    for (const [id, c] of this.cards) {
      if (keep.has(id)) continue;
      clearTimeout(c.timer);
      c.el.remove();
      this.cards.delete(id);
    }
    const nodes: HTMLElement[] = [];
    for (const g of groups) {
      const h = document.createElement("div");
      h.className = "sidebar-group";
      h.textContent = g.label;
      Object.assign(h.style, {
        margin: "6px 2px 4px",
        color: "#d0d0d0",
        fontWeight: "600",
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
      });
      nodes.push(h);
      for (const p of g.panes) {
        const c = this.cards.get(p.id) ?? this.makeCard(p.id);
        this.fillCard(c, p, git[String(p.id)], status);
        nodes.push(c.el);
      }
    }
    this.list.replaceChildren(...nodes);
    for (const p of panes) this.sizeThumb(p.id);
  }

  private makeCard(id: PaneId): Card {
    const el = document.createElement("div");
    el.className = "sidebar-card";
    el.dataset.pane = String(id);
    Object.assign(el.style, {
      marginBottom: "8px",
      padding: "4px",
      borderRadius: "5px",
      border: "1px solid transparent",
      cursor: "default",
    });
    const thumb = document.createElement("canvas");
    thumb.className = "sidebar-thumb";
    Object.assign(thumb.style, { display: "block", width: "100%", borderRadius: "3px" });
    const row = document.createElement("div");
    Object.assign(row.style, { display: "flex", alignItems: "center", gap: "6px", marginTop: "4px" });
    const title = document.createElement("div");
    title.className = "sidebar-title";
    Object.assign(title.style, {
      flex: "1 1 auto",
      minWidth: "0",
      overflow: "hidden",
      textOverflow: "ellipsis",
      whiteSpace: "nowrap",
      color: "#c8c8c8",
    });
    const dot = makeDot();
    row.append(title, dot);
    const line = document.createElement("div");
    line.className = "sidebar-line";
    Object.assign(line.style, { display: "flex", gap: "6px", overflow: "hidden", whiteSpace: "nowrap", marginTop: "2px" });
    el.append(thumb, row, line);
    el.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus in the terminal
      const t = e.target as HTMLElement;
      if (t.classList.contains("pane-changed")) this.opts.onDiff(id);
      else this.opts.onPick(id);
    });
    const c: Card = { el, thumb, line, title, dot, timer: 0, dirty: true };
    this.cards.set(id, c);
    return c;
  }

  private fillCard(c: Card, p: CardPane, place: PaneDir | undefined, status: StatusMap): void {
    c.title.textContent = p.title || "shell";
    c.el.title = place?.dir ?? "";
    c.el.style.borderColor = p.focused && p.shown ? "#4a7dff" : "transparent";
    c.el.style.background = p.shown ? "#1b1b1b" : "transparent";
    const s = status[String(p.id)];
    showDot(c.dot, s && s.state !== "idle" ? s.state : null, s?.agent);
    // The header's parts without the repo or directory (the group says it).
    const parts = headerParts(place).filter((x) => x.kind !== "place");
    c.line.replaceChildren(
      ...parts.map((x) => {
        const span = document.createElement("span");
        span.className = `pane-${x.kind}`;
        span.textContent = x.text;
        Object.assign(span.style, {
          overflow: "hidden",
          textOverflow: "ellipsis",
          flex: x.kind === "branch" ? "0 1 auto" : "0 0 auto",
          color: x.kind === "changed" ? "#d29922" : x.kind === "conflicts" ? "#f85149" : "",
          cursor: x.kind === "changed" ? "pointer" : "",
        });
        return span;
      }),
    );
    c.thumb.style.background = p.bg;
  }

  // ---- thumbnails ----

  private pane(id: PaneId): CardPane | undefined {
    return this.panes.find((p) => p.id === id);
  }

  private sizeThumb(id: PaneId): void {
    const c = this.cards.get(id);
    const src = this.pane(id)?.canvas;
    if (!c) return;
    const w = Math.max(1, this.width - 26);
    const scale = src && src.width > 0 ? Math.max((w * (window.devicePixelRatio || 1)) / src.width, MIN_SCALE) : 1;
    const aspect = src && src.width > 0 ? (src.height * scale) / (w * (window.devicePixelRatio || 1)) : 0.6;
    const h = Math.max(24, Math.min(THUMB_MAX_H, Math.round(w * aspect)));
    const dpr = window.devicePixelRatio || 1;
    if (c.thumb.width !== Math.round(w * dpr) || c.thumb.height !== Math.round(h * dpr)) {
      c.thumb.width = Math.round(w * dpr);
      c.thumb.height = Math.round(h * dpr);
      c.thumb.style.height = `${h}px`;
      c.dirty = true;
      this.paintNow(id);
    }
  }

  /** The pane painted: redraw its thumbnail soon (at most twice a second). */
  painted(id: PaneId): void {
    const c = this.cards.get(id);
    if (!c) return;
    c.dirty = true;
    if (!this.visible || c.timer) return;
    c.timer = window.setTimeout(() => {
      c.timer = 0;
      this.paintNow(id);
    }, THUMB_MS);
  }

  private paintNow(id: PaneId): void {
    const c = this.cards.get(id);
    const src = this.pane(id)?.canvas;
    if (!c || !src || !this.visible || !c.dirty) return;
    c.dirty = false;
    const ctx = c.thumb.getContext("2d");
    if (!ctx || src.width === 0 || src.height === 0) return;
    ctx.clearRect(0, 0, c.thumb.width, c.thumb.height);
    // Shrink to the card's width, but not past MIN_SCALE (then the left
    // part shows). Vertically, show the part around the cursor, where the
    // activity is: a prompt, an agent's input box.
    const scale = Math.max(c.thumb.width / src.width, MIN_SCALE);
    const w = Math.min(src.width, c.thumb.width / scale);
    const h = Math.min(src.height, c.thumb.height / scale);
    const at = this.pane(id)?.cursor() ?? 1;
    const top = Math.max(0, Math.min(src.height - h, at * src.height - h * 0.7));
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(src, 0, top, w, h, 0, 0, w * scale, h * scale);
  }

  dispose(): void {
    for (const c of this.cards.values()) clearTimeout(c.timer);
  }
}
