import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Layout, LayoutAction } from "./proto";
import { TauriTransport, inTauri } from "./transport/tauri";
import type { Acted, LayoutBackend } from "./workspace/backend";
import type { Action } from "./workspace/keymap";
import type { StatusMap } from "./workspace/status";
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
  const backend: LayoutBackend = {
    act: (action: LayoutAction) => invoke<Acted>("layout_act", { action }),
  };
  const ws = new Workspace(el, {
    backend,
    transport: (pane) => new TauriTransport(pane),
    onTitle: (title) => void win.setTitle(title),
    onEmpty: () => void invoke("app_quit"),
    onContextMenu: (pane) => void showContextMenu(ws, pane),
    // The Dock icon shows how many agents wait for the user (macOS).
    onBlocked: (count) => void invoke("set_badge", { count }),
    view: { onOpenLink: (target) => void invoke("open_link", { target }) },
  });

  // Menu bar items (macOS) carry the action as JSON in their id.
  await listen<string>("menu-action", (e) => ws.runFromMenu(JSON.parse(e.payload) as Action));

  // The daemon owns the layout and sends it now and whenever it changes (an
  // agent split a pane, a shell exited), all as "layout" events, in order.
  // Panes still running from before (the app was closed, or the page
  // reloaded) come back as they were.
  await listen<Layout>("layout", (e) => ws.apply(e.payload));
  // Agent status comes the same way, right after the first layout.
  await listen<StatusMap>("agent-status", (e) => ws.setStatus(e.payload));
  const layout = await invoke<Layout>("layout_watch");

  if (layout.tabs.length === 0) {
    const first = await backend.act({ t: "new_tab", focus: true });
    // KIBANN_LAYOUT: splits and tabs to start with.
    let last = first.pane;
    for (const s of ((await invoke<string | null>("dev_layout")) ?? "").split(",").map((s) => s.trim())) {
      if (last === null) break;
      const a: LayoutAction | null =
        s === "right" || s === "down"
          ? { t: "split", pane: last, dir: s === "right" ? "row" : "column", focus: true }
          : s === "tab"
            ? { t: "new_tab", near: last, focus: true }
            : null;
      if (a) last = (await backend.act(a)).pane;
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
