// The sidebar: every pane in every tab as one compact row (agent dot, title,
// branch, ●N), grouped by repository, the groups needing attention first.
// Hovering a row shows a live preview of the pane beside the sidebar.
// Clicking a row shows its pane; clicking ●N opens its diff.

import type { AgentState, PaneDir } from "../proto";
import type { PaneId } from "./layout";
import { headerParts, type GitMap } from "./pane";
import { makeDot, showDot, type StatusMap } from "./status";

export const SIDEBAR = { min: 140, max: 480, default: 220 };
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

interface Row {
  el: HTMLDivElement;
  title: HTMLSpanElement;
  dot: HTMLSpanElement;
  line: HTMLSpanElement;
}

export interface SidebarOptions {
  /** Show this pane (its tab, focused). */
  onPick(pane: PaneId): void;
  /** ●N was clicked on this pane's row. */
  onDiff(pane: PaneId): void;
  /** The sidebar opened, closed or changed width. */
  onResize(): void;
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
      font: "12px -apple-system, system-ui, sans-serif",
      overflow: "hidden",
    });
    this.list = document.createElement("div");
    this.list.className = "sidebar-list";
    Object.assign(this.list.style, {
      position: "absolute",
      inset: "0",
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
      borderRight: "1px solid #2a2a2a",
      boxSizing: "border-box",
    });
    grip.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      this.hidePreview();
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
      border: "1px solid #3a3a3a",
      boxShadow: "0 6px 24px rgba(0,0,0,0.5)",
    });
    this.el.append(this.list, grip, this.preview);
    this.load();
    this.apply();
  }

  // ---- open, close, width ----

  toggle(): void {
    this.visible = !this.visible;
    this.hidePreview();
    this.apply();
    this.save();
  }

  setWidth(w: number): void {
    this.width = Math.round(Math.min(SIDEBAR.max, Math.max(SIDEBAR.min, w)));
    this.apply();
  }

  private apply(): void {
    this.el.style.display = this.visible ? "block" : "none";
    this.el.style.width = `${this.width}px`;
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
      Object.assign(h.style, {
        margin: nodes.length ? "8px 6px 2px" : "0 6px 2px",
        color: "#7d7d7d",
        fontSize: "11px",
        fontWeight: "600",
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap",
      });
      nodes.push(h);
      for (const p of g.panes) {
        const r = this.rows.get(p.id) ?? this.makeRow(p.id);
        this.fillRow(r, p, git[String(p.id)], status);
        nodes.push(r.el);
      }
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
    el.append(slot, title, line);
    el.addEventListener("mousedown", (e) => {
      e.preventDefault(); // keep focus in the terminal
      const t = e.target as HTMLElement;
      if (t.classList.contains("pane-changed")) this.opts.onDiff(id);
      else this.opts.onPick(id);
    });
    el.addEventListener("mouseenter", () => this.showPreview(id));
    const r: Row = { el, title, dot, line };
    this.rows.set(id, r);
    return r;
  }

  private fillRow(r: Row, p: CardPane, place: PaneDir | undefined, status: StatusMap): void {
    r.title.textContent = p.title || "shell";
    r.el.title = place?.dir ?? "";
    const current = p.focused && p.shown;
    r.el.style.background = current ? "#24324f" : "";
    r.title.style.color = current ? "#ffffff" : p.shown ? "#d8d8d8" : "#a8a8a8";
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
          color: x.kind === "changed" ? "#d29922" : x.kind === "conflicts" ? "#f85149" : "#7d7d7d",
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
    if (!this.visible || !p || !row || !src || src.width === 0 || src.height === 0) {
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
