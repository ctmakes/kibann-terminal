// Parts: web pages a screen puts in its blocks. Each runs in a frame
// sandboxed away from the app (no same origin, so no Tauri, no storage, no
// reaching into this page) and talks only through postMessage with
// parts/kibann.js. Everything it asks for is checked here against what the
// user allowed it (its part.json's permissions); running programs is
// checked again by the app's backend.

import type { PartNow, PartState, PartTheme, Permissions } from "../proto";
import { PART_API } from "../proto/generated/constants";
import type { BlockSlot } from "./engine";

export type { PartState, PartTheme };

export interface ExecResult {
  code: number | null;
  stdout: string;
  stderr: string;
}

/** What the workspace does for parts. */
export interface PartContext {
  state(): PartState;
  theme(): PartTheme;
  /** Do `action` for the part `part` (already checked against what it may do). */
  run(part: string, action: PartAction, args: Record<string, unknown>): Promise<unknown>;
  exec(part: string, program: string, args: string[], cwd: string | null, timeout: number | null): Promise<ExecResult>;
  /** The pane `width` CSS px wide; with `height`, that tall, cropped
   * around the cursor (where the action is). */
  thumbnail(pane: number, width: number, height?: number): Promise<ImageBitmap | null>;
  /** A key with Cmd or Ctrl pressed in a part: a shortcut, maybe. Ending
   * a pane (Cmd+W) needs `granted.type`, as `pane.close` does. */
  key(
    k: { key: string; code: string; altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean },
    granted: Permissions,
  ): void;
  /** A part let go of the keyboard (a click that wasn't in a text field). */
  release(): void;
  error(part: string, message: string): void;
}

/** What `kibann.run` can do (parts/kibann.d.ts: KibannAction, checked
 * against this by parts-api.check.ts). */
export const PART_ACTIONS = [
  "pane.focus",
  "tab.activate",
  "tab.new",
  "pane.split",
  "pane.close",
  "pane.zoom",
  "pane.type",
  "workspace.open",
  "diff",
  "screen.switch",
  "node.show",
  "node.hide",
  "node.toggle",
  "notify",
  "settings",
  "themes",
  "shortcuts",
  "workspaces",
  "screens",
] as const;
export type PartAction = (typeof PART_ACTIONS)[number];

const isPartAction = (a: string): a is PartAction => (PART_ACTIONS as readonly string[]).includes(a);

/** Actions that need the `type` permission: they run commands or end them.
 * The rest only show things or start fresh shells. */
const TYPING: ReadonlySet<PartAction> = new Set(["pane.type", "pane.close", "workspace.open"]);
/** The most a part keeps (store), as JSON. */
const STORE_MAX = 1 << 20;

interface Frame {
  name: string;
  part: PartNow;
  props: unknown;
  slot: BlockSlot;
  iframe: HTMLIFrameElement;
  ready: boolean;
  /** The state last sent, as JSON, so an unchanged one isn't sent again. */
  last: string;
}

export class PartHost {
  private readonly frames: Frame[] = [];
  private queued = false;
  private readonly onMessage = (e: MessageEvent) => this.message(e);

  constructor(private readonly ctx: PartContext) {
    window.addEventListener("message", this.onMessage);
  }

  /** Put the part `name` in `slot`. */
  mount(name: string, part: PartNow, props: unknown, slot: BlockSlot): void {
    const iframe = document.createElement("iframe");
    iframe.className = "screen-frame";
    iframe.dataset.part = name;
    // Scripts only: no same origin (so no storage, cookies or reaching
    // into the app), no popups, forms, top navigation or downloads.
    iframe.setAttribute("sandbox", "allow-scripts");
    iframe.setAttribute("referrerpolicy", "no-referrer");
    iframe.setAttribute("allow", "");
    iframe.title = name;
    iframe.src = part.url;
    slot.el.append(iframe);
    this.frames.push({ name, part, props, slot, iframe, ready: false, last: "" });
  }

  /** The window changed: send the new state (once per frame at most). */
  changed(): void {
    if (this.queued || this.frames.length === 0) return;
    this.queued = true;
    requestAnimationFrame(() => {
      this.queued = false;
      this.pushState();
    });
  }

  /** The colors or fonts changed. */
  themeChanged(): void {
    const theme = this.ctx.theme();
    for (const f of this.frames) if (f.ready) this.post(f, { t: "theme", theme });
  }

  /** A block was shown or hidden. */
  shown(slot: BlockSlot): void {
    for (const f of this.frames) if (f.slot === slot && f.ready) this.post(f, { t: "show", shown: slot.shown });
  }

  dispose(): void {
    window.removeEventListener("message", this.onMessage);
    for (const f of this.frames) f.iframe.remove();
    this.frames.length = 0;
  }

  private pushState(): void {
    const state = this.ctx.state();
    const json = JSON.stringify(state);
    for (const f of this.frames) {
      if (!f.ready || f.last === json) continue;
      f.last = json;
      this.post(f, { t: "state", state });
    }
  }

  private post(f: Frame, msg: Record<string, unknown>, transfer: Transferable[] = []): void {
    // "*": the frame's origin is opaque (sandboxed), so it can't be named.
    // The frame is ours; what's sent is meant for it.
    f.iframe.contentWindow?.postMessage({ kibann: 1, ...msg }, "*", transfer);
  }

  private message(e: MessageEvent): void {
    const f = this.frames.find((x) => x.iframe.contentWindow === e.source);
    if (!f) return;
    const m = e.data as { kibann?: number; t?: string; [k: string]: unknown };
    if (!m || m.kibann !== 1) return;
    switch (m.t) {
      case "hello": {
        f.ready = true;
        const state = this.ctx.state();
        f.last = JSON.stringify(state);
        this.post(f, {
          t: "init",
          api: PART_API,
          name: f.name,
          props: f.props ?? null,
          granted: f.part.granted,
          focusable: f.part.manifest.focusable,
          theme: this.ctx.theme(),
          state,
        });
        if (!f.slot.shown) this.post(f, { t: "show", shown: false });
        return;
      }
      case "call":
        void this.call(f, Number(m.id), String(m.method), (m.args ?? {}) as Record<string, unknown>);
        return;
      case "key":
        // Only a key the user pressed in the part: one it posts while it
        // doesn't have the keyboard isn't theirs.
        if (document.activeElement !== f.iframe) return;
        this.ctx.key({
          key: String(m.key),
          code: String(m.code),
          altKey: !!m.alt,
          ctrlKey: !!m.ctrl,
          metaKey: !!m.meta,
          shiftKey: !!m.shift,
        }, f.part.granted);
        return;
      case "release":
        this.ctx.release();
        return;
      case "error":
        this.ctx.error(f.name, String(m.message).slice(0, 300));
        return;
    }
  }

  private async call(f: Frame, id: number, method: string, args: Record<string, unknown>): Promise<void> {
    const granted: Permissions = f.part.granted;
    try {
      let value: unknown = null;
      const transfer: Transferable[] = [];
      switch (method) {
        case "run": {
          const action = String(args.action);
          if (!isPartAction(action)) throw new Error(`no action ${action}`);
          if (TYPING.has(action) && !granted.type) throw new Error(`${action} needs the "type" permission`);
          value = await this.ctx.run(f.name, action, (args.args ?? {}) as Record<string, unknown>);
          break;
        }
        case "exec": {
          const program = String(args.program);
          if (!granted.exec.includes(program)) {
            throw new Error(`${program} isn't in the part's "exec" permission`);
          }
          const list = Array.isArray(args.args) ? args.args.map(String) : [];
          const cwd = typeof args.cwd === "string" ? args.cwd : null;
          const timeout = typeof args.timeout === "number" ? args.timeout : null;
          value = await this.ctx.exec(f.name, program, list, cwd, timeout);
          break;
        }
        case "thumbnail": {
          if (!granted.read) throw new Error(`thumbnails need the "read" permission`);
          const bmp = await this.ctx.thumbnail(Number(args.pane), Number(args.width) || 320, Number(args.height) || undefined);
          if (bmp) transfer.push(bmp);
          value = bmp;
          break;
        }
        case "store.get":
          value = this.store(f.name)[String(args.key)] ?? null;
          break;
        case "store.set": {
          const all = this.store(f.name);
          all[String(args.key)] = args.value;
          const json = JSON.stringify(all);
          if (json.length > STORE_MAX) throw new Error("the part's store is full (1 MB)");
          localStorage.setItem(`kibann.part.${f.name}`, json);
          break;
        }
        default:
          throw new Error(`no such call: ${method}`);
      }
      this.post(f, { t: "reply", id, ok: true, value }, transfer);
    } catch (err) {
      this.post(f, { t: "reply", id, ok: false, error: err instanceof Error ? err.message : String(err) });
    }
  }

  private store(name: string): Record<string, unknown> {
    try {
      const v = JSON.parse(localStorage.getItem(`kibann.part.${name}`) ?? "{}");
      return v && typeof v === "object" && !Array.isArray(v) ? v : {};
    } catch {
      return {};
    }
  }
}
