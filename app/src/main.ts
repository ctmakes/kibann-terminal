import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Layout, LayoutAction, Settings, Theme } from "./proto";
import { TauriTransport, inTauri } from "./transport/tauri";
import type { Acted, LayoutBackend } from "./workspace/backend";
import type { Action } from "./workspace/keymap";
import type { GitMap } from "./workspace/pane";
import type { StatusMap } from "./workspace/status";
import type { ThemeApi, ThemeEntry } from "./workspace/theme";
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
  // ●N opens the diff viewer (`kibann pane diff P --view`, run by this
  // executable) in a split beside the pane; again, it focuses that split.
  const diffs = new Map<number, number>();
  const openDiff = async (pane: Pane) => {
    const open = diffs.get(pane.id);
    if (open !== undefined && ws.panes.has(open)) return ws.show(open);
    const exe = await invoke<string | null>("app_exe");
    if (!exe) return;
    const made = await backend.act({
      t: "split",
      pane: pane.id,
      dir: "row",
      argv: [exe, "pane", "diff", `p${pane.id}`, "--view"],
      focus: true,
    });
    if (made.pane !== null) diffs.set(pane.id, made.pane);
  };
  // Themes: the app keeps the choice (config file, system appearance) and
  // colors the terminals through the daemon; the window follows here.
  const now = await invoke<{ name: string; theme: Theme }>("theme_current");
  let themeName = now.name;
  const themes: ThemeApi = {
    list: () => invoke<ThemeEntry[]>("theme_list"),
    preview: (name) => invoke("theme_preview", { name }),
    choose: (name) => invoke("theme_choose", { name }),
    current: () => themeName,
  };
  const ws = new Workspace(el, {
    backend,
    themes,
    firstRunHint: true,
    transport: (pane) => new TauriTransport(pane),
    onTitle: (title) => void win.setTitle(title),
    onEmpty: () => void invoke("app_quit"),
    onContextMenu: (pane) => void showContextMenu(ws, pane),
    // The Dock icon shows how many agents wait for the user (macOS).
    onBlocked: (count) => void invoke("set_badge", { count }),
    onDiff: (pane) => void openDiff(pane).catch((e) => console.warn("kibann: diff:", e)),
    onNotify: ({ pane, title, body }) => void invoke("notify", { pane, title, body }),
    windowFocused: () => document.hasFocus(),
    onKeys: (keys) => void invoke("menu_keys", { keys }),
    view: { onOpenLink: (target) => void invoke("open_link", { target }) },
  });

  ws.setTheme(now.theme);
  // Fonts and notifications, from the same config file; edits apply live.
  ws.setSettings(await invoke<Settings>("settings_current"));
  await listen<Settings>("settings", (e) => ws.setSettings(e.payload));
  await listen<{ name: string; theme: Theme }>("theme", (e) => {
    themeName = e.payload.name;
    ws.setTheme(e.payload.theme);
  });
  await listen<string>("notice", (e) => ws.notice(e.payload));
  // A new version is downloaded: offer to restart into it (shells keep
  // running in the daemon).
  await listen<{ version: string }>("update-ready", (e) =>
    ws.notice(`Kibann Terminal ${e.payload.version} is ready.`, 0, {
      label: "Restart to update",
      run: () => void invoke("update_install").catch((err) => ws.notice(`Couldn't update: ${err}`)),
    }),
  );
  // A notification was clicked: show the pane it's about.
  await listen<number>("focus-pane", (e) => ws.show(e.payload));

  // Menu bar items (macOS) carry the action as JSON in their id.
  await listen<string>("menu-action", (e) => ws.runFromMenu(JSON.parse(e.payload) as Action));

  // The daemon owns the layout and sends it now and whenever it changes (an
  // agent split a pane, a shell exited), all as "layout" events, in order.
  // Panes still running from before (the app was closed, or the page
  // reloaded) come back as they were.
  await listen<Layout>("layout", (e) => ws.apply(e.payload));
  // Agent status comes the same way, right after the first layout.
  await listen<StatusMap>("agent-status", (e) => ws.setStatus(e.payload));
  await listen<GitMap>("pane-git", (e) => ws.setGit(e.payload));
  const layout = await invoke<Layout>("layout_watch");

  // Nothing open (the sessions ended: a restart, or Quit and End
  // Sessions): the saved tabs and splits come back, with fresh shells.
  const settings = await invoke<Settings>("settings_current");
  const restored =
    layout.tabs.length === 0 && settings.restore_layout && !(await invoke<string | null>("dev_layout"))
      ? await invoke<number>("layout_restore").catch((e) => {
          console.warn("kibann: restore:", e);
          return 0;
        })
      : 0;
  if (restored > 0) ws.notice(`Reopened ${restored === 1 ? "1 pane" : `${restored} panes`} where you left off.`, 6000);
  if (layout.tabs.length === 0 && restored === 0) {
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
