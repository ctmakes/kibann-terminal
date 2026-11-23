// Between the window and its parts (screen/parts.ts): the window as parts
// see it, the theme they get, and what their actions do. Parts' API is in
// parts/kibann.d.ts.

import { fontStack } from "../metrics";
import type { Chrome, LayoutAction, Theme } from "../proto";
import type { PartAction, PartState, PartTheme } from "../screen/parts";
import type { ScreenApi } from "../screen/picker";
import type { Action } from "./keymap";
import { panes, type PaneId } from "./layout";
import { paneName, type GitMap } from "./pane";
import { agentName, blockedCount, rollUp, type StatusMap } from "./status";
import { chromeFor, isDark } from "./theme";
import type { Pane, Tab } from "./workspace";
import type { WorkspaceApi } from "./workspaces";

/** What a tab is called: the name it was given, or its focused pane's. */
export function tabName(t: Tab, all: ReadonlyMap<PaneId, Pane>): string {
  return t.title || paneName(all.get(t.focused)?.title);
}

/** The window's state, as far as parts are told. */
export interface WindowView {
  readonly tabs: readonly Tab[];
  readonly active: Tab | null;
  readonly panes: ReadonlyMap<PaneId, Pane>;
  readonly status: StatusMap;
  readonly git: GitMap;
  /** The terminals block shows only the focused pane. */
  readonly solo: boolean;
  readonly screenName: string;
  windowFocused(): boolean;
}

const hex = (c: number | undefined, or: string) => (c === undefined ? or : `#${c.toString(16).padStart(6, "0")}`);

/** The window as parts see it. */
export function partState(w: WindowView): PartState {
  const tabs = w.tabs.map((t) => ({
    id: t.id,
    title: tabName(t, w.panes),
    active: t === w.active,
    panes: panes(t.tree),
    focused: t.focused,
    state: rollUp(w.status, panes(t.tree)),
  }));
  const all = w.tabs.flatMap((t) =>
    panes(t.tree).flatMap((id) => {
      const p = w.panes.get(id);
      if (!p) return [];
      const place = w.git[String(id)];
      const g = place?.git ?? null;
      const st = w.status[String(id)];
      const onScreen = t === w.active && (t.zoomed === null ? !w.solo || t.focused === id : t.zoomed === id);
      return [
        {
          id,
          tab: t.id,
          title: paneName(p.title),
          focused: t.focused === id,
          shown: onScreen,
          cwd: place?.shown ?? "",
          repo: g?.repo ?? null,
          branch: g?.branch ?? g?.detached ?? null,
          changed: g?.changed ?? 0,
          ahead: g?.ahead ?? 0,
          behind: g?.behind ?? 0,
          agent: st ? agentName(st.agent) : null,
          state: st?.state ?? null,
          since: st?.since ?? null,
          bg: hex(p.view?.grid.defaultBg, "#000000"),
          fg: hex(p.view?.grid.defaultFg, "#ffffff"),
        },
      ];
    }),
  );
  return {
    tabs,
    panes: all,
    focused: w.active?.focused ?? null,
    screen: w.screenName,
    blocked: blockedCount(w.status),
    windowFocused: w.windowFocused(),
  };
}

/** The window's colors and fonts for parts: the theme's chrome, with the
 * screen's own colors over it. */
export function partTheme(theme: Theme, chrome: Chrome | undefined): PartTheme {
  const colors: Record<string, string> = {};
  for (const [k, v] of Object.entries(chromeFor(theme))) colors[k.replace(/^--k-/, "")] = v;
  for (const [k, v] of Object.entries(chrome?.colors ?? {})) colors[k] = v;
  return {
    colors,
    font: chrome?.font ?? "-apple-system, system-ui, sans-serif",
    mono: fontStack(),
    dark: isDark(theme),
  };
}

/** What parts' actions may do in the window. */
export interface PartActions {
  pane(id: PaneId): Pane | undefined;
  tab(id: number): Tab | undefined;
  focused(): Pane | null;
  show(pane: PaneId): void;
  activate(tab: Tab): void;
  send(a: LayoutAction): void;
  run(action: Action, on: Pane | null): void;
  diff(pane: Pane): void;
  workspaces?: WorkspaceApi;
  screens?: ScreenApi;
  /** Show, hide or toggle a node of the screen; false when there's none. */
  setNode(id: string, how: "show" | "hide" | "toggle"): boolean;
  notice(text: string): void;
}

/** Do what part `part` asked: `action` with arguments `a` (checked here:
 * they come from a part). */
export async function runPartAction(
  w: PartActions,
  part: string,
  action: PartAction,
  a: Record<string, unknown>,
): Promise<unknown> {
  const num = (k: string): number => {
    const v = a[k];
    if (typeof v !== "number" || !Number.isInteger(v)) throw new Error(`${action}: ${k} must be a pane or tab id`);
    return v;
  };
  const pane = (k = "pane"): Pane => {
    const p = w.pane(num(k));
    if (!p) throw new Error(`${action}: no pane ${String(a[k])}`);
    return p;
  };
  const str = (k: string): string => {
    if (typeof a[k] !== "string") throw new Error(`${action}: ${k} must be text`);
    return a[k] as string;
  };
  switch (action) {
    case "pane.focus":
      w.show(pane().id);
      return null;
    case "tab.activate": {
      const t = w.tab(num("tab"));
      if (!t) throw new Error(`tab.activate: no tab ${String(a.tab)}`);
      w.activate(t);
      return null;
    }
    case "tab.new":
      w.send({ t: "new_tab", near: a.near === undefined ? w.focused()?.id : pane("near").id, focus: true });
      return null;
    case "pane.split": {
      const p = a.pane === undefined ? w.focused() : pane();
      if (!p) throw new Error("pane.split: no pane");
      const dir = a.direction === "down" ? "column" : a.direction === "right" ? "row" : null;
      if (!dir) throw new Error(`pane.split: direction is "right" or "down"`);
      w.send({ t: "split", pane: p.id, dir, focus: true });
      return null;
    }
    case "pane.close":
      w.send({ t: "close", pane: pane().id });
      return null;
    case "pane.zoom": {
      const p = a.pane === undefined ? w.focused() : pane();
      if (p) w.run({ kind: "zoom" }, p);
      return null;
    }
    case "pane.type": {
      const p = pane();
      const text = str("text") + (a.enter ? "\r" : "");
      p.transport.input({ kind: "text", data: text });
      return null;
    }
    case "workspace.open": {
      const name = str("name");
      const api = w.workspaces;
      if (!api) throw new Error("workspaces can't be opened here");
      const near = w.focused()?.id ?? null;
      const found = (await api.list(near)).find((x) => x.name === name);
      if (!found) throw new Error(`no workspace named ${name}`);
      return api.open(found, near);
    }
    case "diff":
      w.diff(pane());
      return null;
    case "screen.switch": {
      const name = str("name");
      if (!w.screens) throw new Error("screens can't be switched here");
      await w.screens.choose(name);
      return null;
    }
    case "node.show":
    case "node.hide":
    case "node.toggle": {
      const id = str("id");
      const how = action.slice(5) as "show" | "hide" | "toggle";
      if (!w.setNode(id, how)) throw new Error(`${action}: no node ${id} on this screen`);
      return null;
    }
    case "notify":
      w.notice(`${part}: ${str("text").slice(0, 200)}`);
      return null;
    case "settings":
    case "themes":
    case "shortcuts":
    case "workspaces":
    case "screens":
      w.run({ kind: action }, w.focused());
      return null;
    default: {
      const never: never = action;
      throw new Error(`no action ${String(never)}`);
    }
  }
}
