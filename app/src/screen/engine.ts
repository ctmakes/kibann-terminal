// A screen (screens/NAME/screen.json) made into elements: rows and columns
// are flex boxes, stacks show one child, blocks get their content from the
// workspace (`fill`): the terminals, Kibann's sidebar or tab bar, or a
// part's frame. Floating blocks sit over the window at an edge.
//
// Sizes a user drags are kept per screen and node id (localStorage).

import type { Screen, ScreenContent, ScreenFloating, ScreenNode } from "../proto";
import "./screen.css";

/** Where a block's content goes, and whether it can be seen. */
export interface BlockSlot {
  el: HTMLDivElement;
  content: ScreenContent;
  /** On screen: not hidden, and every stack above shows it. */
  shown: boolean;
}

export interface ScreenViewOptions {
  /** Put `slot.content` into `slot.el`. */
  fill(slot: BlockSlot): void;
  /** A block was shown or hidden. */
  onShown?(slot: BlockSlot): void;
  /** Sizes changed (a drag, a node shown or hidden). */
  onResize?(): void;
}

interface Placed {
  node: ScreenNode | null;
  floating: ScreenFloating | null;
  el: HTMLElement;
  /** Its content's box: the node itself, or its inner box when it has a
   * drag handle. */
  box: HTMLElement;
  parent: Placed | null;
  children: Placed[];
  hidden: boolean;
  /** A stack's child shown. */
  active: number;
  slot: BlockSlot | null;
  /** A floating block's: pinned open. */
  open: boolean;
  /** A floating block's: open while the pointer is over it. */
  hover: boolean;
  /** A floating block's strip that opens it on hover. */
  strip?: HTMLDivElement;
}

const STORE = "kibann.screen";

export class ScreenView {
  readonly el: HTMLDivElement;
  private readonly byId = new Map<string, Placed>();
  private readonly blocks: Placed[] = [];
  private root: Placed;

  constructor(
    readonly name: string,
    readonly screen: Screen,
    private readonly opts: ScreenViewOptions,
  ) {
    this.el = document.createElement("div");
    this.el.className = "screen";
    this.el.dataset.screen = name;
    if (screen.chrome.background) this.el.style.background = screen.chrome.background;
    this.root = this.build(screen.root, null, null);
    this.root.el.style.flex = "1 1 auto";
    this.el.append(this.root.el);
    for (const f of screen.floating) this.el.append(this.floating(f).el);
    this.refreshShown(false);
    for (const b of this.blocks) if (b.slot) this.opts.fill(b.slot);
  }

  /** Every block, in the order of the file (floating ones last). */
  get slots(): BlockSlot[] {
    return this.blocks.flatMap((b) => (b.slot ? [b.slot] : []));
  }

  /** Whether a node or floating block has the id `id`. */
  has(id: string): boolean {
    return this.byId.has(id);
  }

  /**
   * Show, hide or flip the node or floating block `id`. Showing a stack's
   * child shows it instead of its siblings; a floating block opens and
   * closes. False when there's no such id.
   */
  set(id: string, how: "show" | "hide" | "toggle"): boolean {
    const p = this.byId.get(id);
    if (!p) return false;
    if (p.floating) {
      p.open = how === "toggle" ? !p.open : how === "show";
      this.placeFloating(p);
    } else if (p.parent?.node?.type === "stack") {
      // In a stack, showing a child shows it instead of its siblings.
      const stack = p.parent;
      const i = stack.children.indexOf(p);
      const showing = stack.active === i && !p.hidden;
      if (how === "hide" || (how === "toggle" && showing)) p.hidden = true;
      else {
        p.hidden = false;
        stack.active = i;
      }
      this.showStack(stack);
    } else {
      p.hidden = how === "toggle" ? !p.hidden : how === "hide";
      p.el.style.display = p.hidden ? "none" : "";
    }
    this.refreshShown(true);
    this.opts.onResize?.();
    return true;
  }

  dispose(): void {
    this.el.remove();
  }

  // ---- building ----

  private build(node: ScreenNode, parent: Placed | null, parentKind: string | null): Placed {
    const resizable = node.resizable && (parentKind === "row" || parentKind === "column");
    const el = document.createElement("div");
    el.className = `screen-node screen-${node.type}`;
    if (node.id) el.dataset.node = node.id;
    // A node with a drag handle gets an inner box for its look (and
    // clipping), so the handle can sit across its edge.
    let box: HTMLElement = el;
    if (resizable) {
      box = document.createElement("div");
      box.className = "screen-inner";
      el.append(box);
      if (node.type === "block") {
        box.classList.add("screen-block");
        el.classList.remove("screen-block");
      }
    }
    for (const [k, v] of Object.entries(node.style)) box.style.setProperty(k, v);
    if (node.type === "row" || node.type === "column") box.style.flexDirection = node.type;
    if (node.type === "stack") box.style.flexDirection = "column";
    const p: Placed = {
      node,
      floating: null,
      el,
      box,
      parent,
      children: [],
      hidden: node.hidden,
      active: 0,
      slot: null,
      open: false,
      hover: false,
    };
    if (node.id) this.byId.set(node.id, p);
    if (p.hidden) el.style.display = "none";
    this.size(p, parentKind);
    if (node.type === "block" && node.content) {
      p.slot = { el: box as HTMLDivElement, content: node.content, shown: false };
      this.blocks.push(p);
    }
    for (const child of node.children) {
      const c = this.build(child, p, node.type);
      p.children.push(c);
      box.append(c.el);
    }
    if (node.type === "stack") {
      const i = node.active ? node.children.findIndex((c) => c.id === node.active) : 0;
      p.active = Math.max(0, i);
      for (const c of p.children) c.el.style.flex = "1 1 0px";
      this.showStack(p);
    }
    if (resizable && node.id) this.handle(p, parentKind as "row" | "column");
    return p;
  }

  private size(p: Placed, parentKind: string | null): void {
    const n = p.node!;
    const s = p.el.style;
    const row = parentKind === "row";
    const saved = n.id && n.resizable ? this.saved(n.id) : null;
    const size = n.size;
    if (saved !== null) s.flex = `0 0 ${this.clamp(n, saved)}px`;
    else if (size === undefined) s.flex = "1 1 0px";
    else if (size === "auto") s.flex = "0 0 auto";
    else if ("px" in size) s.flex = `0 0 ${size.px}px`;
    else s.flex = `${size.weight} 1 0px`;
    if (n.min !== undefined) s.setProperty(row ? "min-width" : "min-height", `${n.min}px`);
    if (n.max !== undefined) s.setProperty(row ? "max-width" : "max-height", `${n.max}px`);
  }

  private clamp(n: ScreenNode, px: number): number {
    return Math.round(Math.min(n.max ?? 10000, Math.max(n.min ?? 0, px)));
  }

  private key(id: string): string {
    return `${STORE}.${this.name}.${id}`;
  }

  private saved(id: string): number | null {
    try {
      const v = Number(localStorage.getItem(this.key(id)));
      return Number.isFinite(v) && v > 0 ? v : null;
    } catch {
      return null;
    }
  }

  /** A grab area across `p`'s trailing edge that sets its size. */
  private handle(p: Placed, dir: "row" | "column"): void {
    const h = document.createElement("div");
    h.className = `screen-handle ${dir}`;
    p.el.append(h);
    h.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      h.setPointerCapture(e.pointerId);
      const start = dir === "row" ? e.clientX : e.clientY;
      const rect = p.el.getBoundingClientRect();
      const from = dir === "row" ? rect.width : rect.height;
      let px = from;
      this.el.classList.add("screen-dragging");
      const move = (ev: PointerEvent) => {
        px = this.clamp(p.node!, from + (dir === "row" ? ev.clientX : ev.clientY) - start);
        p.el.style.flex = `0 0 ${px}px`;
        this.opts.onResize?.();
      };
      const up = () => {
        h.removeEventListener("pointermove", move);
        h.removeEventListener("pointerup", up);
        h.removeEventListener("pointercancel", up);
        this.el.classList.remove("screen-dragging");
        try {
          localStorage.setItem(this.key(p.node!.id!), String(px));
        } catch {
          // Not kept; the size still applies until the screen changes.
        }
      };
      h.addEventListener("pointermove", move);
      h.addEventListener("pointerup", up);
      h.addEventListener("pointercancel", up);
    });
  }

  private showStack(stack: Placed): void {
    stack.children.forEach((c, i) => {
      c.el.style.display = i === stack.active && !c.hidden ? "" : "none";
    });
  }

  private floating(f: ScreenFloating): Placed {
    const el = document.createElement("div");
    el.className = "screen-floating";
    el.dataset.node = f.id;
    for (const [k, v] of Object.entries(f.style)) el.style.setProperty(k, v);
    const p: Placed = {
      node: null,
      floating: f,
      el,
      box: el,
      parent: null,
      children: [],
      hidden: false,
      active: 0,
      slot: { el, content: f.content, shown: false },
      open: f.open,
      hover: false,
    };
    this.byId.set(f.id, p);
    this.blocks.push(p);
    const across = f.edge === "left" || f.edge === "right";
    const s = el.style;
    s.setProperty(across ? "width" : "height", `${f.depth}px`);
    s.setProperty(f.edge, "0");
    const along = across ? ["top", "bottom", "height"] : ["left", "right", "width"];
    const len = f.length ?? null;
    if (f.align === "stretch" || len === null) {
      s.setProperty(along[0], "0");
      s.setProperty(along[1], "0");
    } else {
      s.setProperty(along[2], `${len}px`);
      if (f.align === "start") s.setProperty(along[0], "0");
      else if (f.align === "end") s.setProperty(along[1], "0");
      else s.setProperty(along[0], `calc(50% - ${len / 2}px)`);
    }
    if (f.peek > 0) {
      // A strip over the part showing while closed: the pointer over a
      // frame sends this page nothing, so the strip notices it instead.
      const strip = document.createElement("div");
      strip.className = "screen-peek";
      const edgeSide = { left: "right", right: "left", top: "bottom", bottom: "top" }[f.edge];
      strip.style.setProperty(edgeSide, "0");
      strip.style.setProperty(across ? "width" : "height", `${Math.max(f.peek, 6)}px`);
      strip.style.setProperty(across ? "top" : "left", "0");
      strip.style.setProperty(across ? "bottom" : "right", "0");
      el.append(strip);
      p.strip = strip;
      strip.addEventListener("pointerenter", () => {
        p.hover = true;
        this.placeFloating(p);
        // Closes once the pointer is back in this page away from it.
        const away = (e: PointerEvent) => {
          const r = el.getBoundingClientRect();
          if (e.clientX >= r.left && e.clientX < r.right && e.clientY >= r.top && e.clientY < r.bottom) return;
          document.removeEventListener("pointermove", away, true);
          p.hover = false;
          this.placeFloating(p);
        };
        document.addEventListener("pointermove", away, true);
      });
    }
    this.placeFloating(p);
    return p;
  }

  private placeFloating(p: Placed): void {
    const f = p.floating!;
    const open = p.open || p.hover;
    const away = open ? 0 : f.depth - f.peek;
    const sign = f.edge === "left" || f.edge === "top" ? -1 : 1;
    const axis = f.edge === "left" || f.edge === "right" ? "X" : "Y";
    p.el.style.transform = away ? `translate${axis}(${sign * away}px)` : "";
    p.el.style.visibility = !open && f.peek === 0 ? "hidden" : "";
    if (p.strip) p.strip.style.display = open ? "none" : "";
    if (p.slot) {
      const shown = open || f.peek > 0;
      if (shown !== p.slot.shown) {
        p.slot.shown = shown;
        this.opts.onShown?.(p.slot);
      }
    }
  }

  /** Work out which blocks can be seen; tell about the ones that changed. */
  private refreshShown(tell: boolean): void {
    const walk = (p: Placed, visible: boolean) => {
      const here = visible && !p.hidden;
      if (p.slot && !p.floating && p.slot.shown !== here) {
        p.slot.shown = here;
        if (tell) this.opts.onShown?.(p.slot);
      }
      p.children.forEach((c, i) => walk(c, here && (p.node?.type !== "stack" || i === p.active)));
    };
    walk(this.root, true);
  }
}
