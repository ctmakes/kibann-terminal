// Types for `window.kibann`, which every part's page has (kibann.js is
// loaded before the page's own scripts). Print with `kibann screen api`.

type AgentState = "idle" | "working" | "blocked" | "done";

interface KibannPane {
  id: number;
  /** Its tab's id. */
  tab: number;
  /** The program's title, else the program's name. */
  title: string;
  /** Focused in its tab. */
  focused: boolean;
  /** On screen now (its tab is shown, and it isn't hidden by a zoom). */
  shown: boolean;
  /** Its directory, `~` for home; "" until known. */
  cwd: string;
  /** The repository's name, when the directory is in one. */
  repo: string | null;
  branch: string | null;
  /** Files changed, staged or not (the ●N in Kibann). */
  changed: number;
  ahead: number;
  behind: number;
  /** The coding agent in it (`claude`, `codex`), if any. */
  agent: string | null;
  /** That agent's state; null without one. `blocked`: waiting for you. */
  state: AgentState | null;
  /** When `state` last changed (ms since 1970), as the window saw it. */
  since: number | null;
  /** The terminal's colors, `#rrggbb`. */
  bg: string;
  fg: string;
}

interface KibannTab {
  id: number;
  /** Its name from a workspace, else the focused pane's title. */
  title: string;
  /** Shown now. */
  active: boolean;
  /** Its panes' ids, in order. */
  panes: number[];
  focused: number;
  /** The most urgent agent state in it: blocked, done, working, idle. */
  state: AgentState | null;
}

interface KibannState {
  tabs: KibannTab[];
  /** Every pane, in tab order. */
  panes: KibannPane[];
  /** The focused pane of the shown tab. */
  focused: number | null;
  /** The screen's name. */
  screen: string;
  /** Panes whose agent waits for you. */
  blocked: number;
  /** Whether the window has the user's attention. */
  windowFocused: boolean;
}

interface KibannTheme {
  /** The window's colors by name (also set as CSS variables `--k-NAME`):
   * bg, fg, sidebar, bar, header, panel, tab, tabActive, divider, muted,
   * text, label, strong, title, bright, current, working, blocked, done,
   * changed, conflicts... */
  colors: Record<string, string>;
  /** Kibann's UI font (`--k-font`) and the terminals' (`--k-mono`). */
  font: string;
  mono: string;
  dark: boolean;
}

type KibannAction =
  /** Show a pane: its tab, focused. */
  | ["pane.focus", { pane: number }]
  | ["tab.activate", { tab: number }]
  /** A new tab beside `near` (default: the focused pane), in its directory. */
  | ["tab.new", { near?: number }]
  | ["pane.split", { pane?: number; direction: "right" | "down" }]
  /** End the pane's program and close it. Needs `type`. */
  | ["pane.close", { pane: number }]
  /** Fill the tab with the pane, or back. */
  | ["pane.zoom", { pane?: number }]
  /** Type `text` into the pane (and Enter, with `enter`). Needs `type`. */
  | ["pane.type", { pane: number; text: string; enter?: boolean }]
  /** Open a workspace's tabs (which runs its commands). Needs `type`. */
  | ["workspace.open", { name: string }]
  /** The changes in a pane's checkout, in the diff viewer. */
  | ["diff", { pane: number }]
  | ["screen.switch", { name: string }]
  /** Show, hide or flip a node or floating block by id. Showing a stack's
   * child shows it instead of its siblings. */
  | ["node.show", { id: string }]
  | ["node.hide", { id: string }]
  | ["node.toggle", { id: string }]
  | ["notify", { text: string }]
  /** Kibann's own windows. */
  | ["settings", {}]
  | ["themes", {}]
  | ["shortcuts", {}]
  | ["workspaces", {}]
  | ["screens", {}];

interface Kibann {
  /** Resolves once the first state arrived. */
  readonly ready: Promise<void>;
  /** This block's `props` from screen.json (null without). */
  readonly props: any;
  readonly state: KibannState | null;
  readonly name: string;
  /** What the part may do: its permissions, once the user allowed them. */
  readonly granted: { read: boolean; type: boolean; exec: string[]; net: string[] };

  /** Now (when there's one) and on every change. Returns a stop function. */
  on(type: "state", fn: (s: KibannState) => void): () => void;
  on(type: "props", fn: (p: any) => void): () => void;
  on(type: "theme", fn: (t: KibannTheme) => void): () => void;
  /** The block was shown (true) or hidden (false): pause work while hidden. */
  on(type: "show", fn: (shown: boolean) => void): () => void;

  run<A extends KibannAction>(action: A[0], args?: A[1]): Promise<unknown>;

  /** Run a program listed in the part's `exec` permission, without a
   * shell, with the user's PATH. Up to 30 s (or `timeout` ms) and 4 MB of
   * output. */
  exec(
    program: string,
    args?: string[],
    opts?: { cwd?: string; timeout?: number },
  ): Promise<{ code: number | null; stdout: string; stderr: string }>;

  /** What a pane shows, `width` CSS px wide. With `height`, that tall,
   * showing the rows around the cursor. Needs `read`. */
  thumbnail(pane: number, width?: number, height?: number): Promise<ImageBitmap | null>;
  /** Draw a pane into a canvas at its CSS size: the full width, and the
   * rows around the cursor. Needs `read`. */
  drawPane(canvas: HTMLCanvasElement, pane: number): Promise<boolean>;

  /** Kept per part, between runs (JSON values). */
  store: {
    get(key: string): Promise<any>;
    set(key: string, value: any): Promise<void>;
  };

  notify(text: string): Promise<unknown>;
}

declare const kibann: Kibann;
