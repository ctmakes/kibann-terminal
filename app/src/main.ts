import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TauriTransport, inTauri, liveSessions } from "./transport/tauri";
import type { Transport } from "./transport/types";
import type { Action } from "./workspace/keymap";
import { Workspace, type Pane } from "./workspace/workspace";

const el = document.getElementById("term")!;

/** Native right-click menu. Copy and Paste go through the webview, so they
 * arrive as DOM copy and paste events and take the same path as Cmd+C/V. */
async function showContextMenu(ws: Workspace, pane: Pane): Promise<void> {
  const run = (action: Action) => () => ws.run(action, pane);
  const items = [];
  if (pane.view?.selectionText) items.push(await PredefinedMenuItem.new({ item: "Copy" }));
  items.push(
    await PredefinedMenuItem.new({ item: "Paste" }),
    await PredefinedMenuItem.new({ item: "Separator" }),
    await MenuItem.new({ text: "Split Right", action: run({ kind: "split", dir: "row" }) }),
    await MenuItem.new({ text: "Split Down", action: run({ kind: "split", dir: "column" }) }),
    await PredefinedMenuItem.new({ item: "Separator" }),
    await MenuItem.new({ text: "Close", action: run({ kind: "close" }) }),
  );
  const menu = await Menu.new({ items });
  await menu.popup();
}

async function start(): Promise<void> {
  const win = getCurrentWindow();
  const cwdFrom = (near: Transport | null) =>
    near instanceof TauriTransport && near.id !== null ? near.id : undefined;
  const ws = new Workspace(el, {
    transport: (near) => new TauriTransport({ cwdFrom: cwdFrom(near) }),
    onTitle: (title) => void win.setTitle(title),
    onEmpty: () => void invoke("app_quit"),
    onContextMenu: (pane) => void showContextMenu(ws, pane),
  });

  // Menu bar items (macOS) carry the action as JSON in their id.
  await listen<string>("menu-action", (e) => ws.runFromMenu(JSON.parse(e.payload) as Action));

  // After a page reload the shells are still running: one tab each.
  const live = await liveSessions();
  if (live.length === 0) ws.newTab(new TauriTransport());
  for (const id of live) ws.newTab(new TauriTransport({ id }));

  const steps: Record<string, Action> = {
    right: { kind: "split", dir: "row" },
    down: { kind: "split", dir: "column" },
    tab: { kind: "newTab" },
  };
  if (live.length === 0) {
    for (const s of ((await invoke<string | null>("dev_layout")) ?? "").split(",")) {
      if (steps[s.trim()]) ws.run(steps[s.trim()]);
    }
  }
}

if (inTauri()) {
  await start();
} else {
  el.innerHTML =
    '<p class="notice">No backend here: this page runs inside the Tauri app. ' +
    'Open the <a href="/src/dev/harness.html">renderer harness</a> or the ' +
    '<a href="/src/dev/workspace.html">workspace harness</a> to try it in a browser.</p>';
}
