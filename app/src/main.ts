import { Menu, PredefinedMenuItem } from "@tauri-apps/api/menu";
import { TerminalView } from "./terminal";
import { TauriTransport, inTauri } from "./transport/tauri";

/** Native right-click menu. Copy and Paste go through the webview, so they
 * arrive as DOM copy and paste events and take the same path as Cmd+C/V. */
async function showContextMenu(view: TerminalView | null): Promise<void> {
  const items = [];
  if (view?.selectionText) items.push(await PredefinedMenuItem.new({ item: "Copy" }));
  items.push(await PredefinedMenuItem.new({ item: "Paste" }));
  const menu = await Menu.new({ items });
  await menu.popup();
}

const el = document.getElementById("term")!;

if (inTauri()) {
  let view: TerminalView | null = null;
  view = await TerminalView.create(el, new TauriTransport(), { onContextMenu: () => void showContextMenu(view) });
} else {
  el.innerHTML =
    '<p class="notice">No backend here: this page runs inside the Tauri app. ' +
    'Open the <a href="/src/dev/harness.html">renderer harness</a> to see fixtures in a browser.</p>';
}
