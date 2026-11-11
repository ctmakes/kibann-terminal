import { TerminalView } from "./terminal";
import { TauriTransport, inTauri } from "./transport/tauri";

const el = document.getElementById("term")!;

if (inTauri()) {
  await TerminalView.create(el, new TauriTransport());
} else {
  el.innerHTML =
    '<p class="notice">No backend here: this page runs inside the Tauri app. ' +
    'Open the <a href="/src/dev/harness.html">renderer harness</a> to see fixtures in a browser.</p>';
}
