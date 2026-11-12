import { Menu, PredefinedMenuItem } from "@tauri-apps/api/menu";
import { TerminalView } from "./terminal";
import { TauriTransport, inTauri } from "./transport/tauri";

/** Native right-click menu. Paste goes through the webview, so it arrives as a
 * DOM paste event and takes the same path as Cmd+V. */
async function showContextMenu(): Promise<void> {
  const menu = await Menu.new({ items: [await PredefinedMenuItem.new({ item: "Paste" })] });
  await menu.popup();
}

const el = document.getElementById("term")!;

if (inTauri()) {
  await TerminalView.create(el, new TauriTransport(), { onContextMenu: () => void showContextMenu() });
} else {
  el.innerHTML =
    '<p class="notice">No backend here: this page runs inside the Tauri app. ' +
    'Open the <a href="/src/dev/harness.html">renderer harness</a> to see fixtures in a browser.</p>';
}
