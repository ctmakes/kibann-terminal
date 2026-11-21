---
name: kibann-screens
description: Design and build Kibann terminal screens (what the whole window shows and where) and parts (web pages in a screen's blocks, fed live with panes, agents and git). Use when the user asks to change how the Kibann window looks or is laid out ("make me a dashboard of my agents", "put the tabs at the bottom", "a retro screen", "show my PRs beside the terminals"), or to add, change or share a screen or part.
---

# Kibann screens and parts

A **screen** is the whole window: a tree of rows, columns and stacks whose
leaves are blocks. A block shows the terminals, Kibann's sidebar or tab
bar, or a **part**: a web page (HTML, CSS, JS) that gets the window's state
live and can act on it. The user switches screens with View → Screen… or
Cmd+Shift+L. You're in Kibann when `$KIBANN_PANE` is set.

```
~/.config/kibann/screens/NAME/screen.json    the screen
~/.config/kibann/screens/NAME/preview.png    optional, for sharing
~/.config/kibann/parts/NAME/part.json        a part: what it is and may do
~/.config/kibann/parts/NAME/index.html       its page (and any files beside it)
```

## Rules

- **Start from a copy.** `kibann screen new NAME --from default` (or
  `board`, `focus`, `departures`) and `kibann screen part NAME` (a page to
  start from) or `--from board`. Never edit Kibann's built-in ones; a
  screen or part of yours with the same name replaces it.
- **Check after every edit:** `kibann screen check NAME`. Fix what it
  reports; a mistake in the JSON names its line and column.
- **Show it only when the user asks:** `kibann screen use NAME`. The
  window follows edits to the files within a second, so once it's shown
  the user sees each change. A screen with a mistake isn't shown; the last
  good one stays, with a notice saying what's wrong.
- **Ask for as little as you can.** A part that only shows state needs no
  permissions. Each one (below) is asked of the user before it works.
- **Use the theme's colors** (`var(--k-bg)` and the rest), not fixed ones,
  unless the user wants a look of its own. The window can be light or dark.
- **Keep the terminals.** Almost every screen should have exactly one block
  with `{"tab": "active"}` (the shown tab's splits) or `{"pane":
  "focused"}` (only the focused pane).
- **Stay in your lane.** A part's page is sandboxed: no `localStorage`
  (use `kibann.store`), no `fetch` unless its `net` permission names the
  host, no reaching the app. Don't try to get around it.

## screen.json

```json
{
  "about": "Agents as cards on the left, the terminals on the right",
  "chrome": { "background": "var(--k-sidebar)", "colors": { "blocked": "#ff9f1c" } },
  "root": {
    "type": "row",
    "style": { "gap": "6px", "padding": "6px" },
    "children": [
      { "type": "block", "id": "board", "size": { "px": 420 }, "min": 240, "max": 900,
        "resizable": true, "content": { "part": "board" } },
      { "type": "column", "children": [
          { "type": "block", "size": "auto", "content": { "part": "tabs" } },
          { "type": "block", "content": { "tab": "active" } }
      ] }
    ]
  },
  "floating": [
    { "id": "agents", "edge": "left", "align": "center", "depth": 300, "length": 480,
      "peek": 6, "content": { "part": "agents" } }
  ]
}
```

- **`type`:** `row` (side by side), `column` (top to bottom), `stack` (one
  child at a time: `active` names it, `node.show` switches), `block`.
- **`size`** along the parent's direction: `{"px": N}`, `{"weight": N}` (a
  share of what's left; the default is 1), or `"auto"` (its content's
  size: the sidebar and tab bar size themselves).
- **`min`, `max`:** pixels. **`resizable`:** its edge toward the next
  sibling can be dragged; needs a `px` size and an `id` (the size is kept).
- **`hidden`:** starts hidden; `node.toggle` shows it.
- **`style`:** `background`, `border` (and `border-top`…), `border-radius`,
  `box-shadow`, `gap`, `margin`, `opacity`, `outline`, `padding`. No
  `url(...)`.
- **A block's `content`:** `{"tab": "active"}`, `{"pane": "focused"}`,
  `{"part": "sidebar"}` (Kibann's sidebar), `{"part": "tabs"}` (Kibann's
  tab bar, hidden with one tab), or `{"part": NAME, "props": {...}}`.
  Each of these can be on a screen once, except web parts.
- **`floating`:** blocks over the window at an `edge`, `align`ed `start`,
  `center`, `end` or `stretch`, `depth` px out from it and `length` px
  along it. `peek` px show while closed and the rest slides in on hover;
  `"peek": 0` hides it until `node.toggle` opens it.
- **`chrome`:** `background` (behind the blocks), `colors` (the window's
  colors by name, over the theme's; parts get them too), `font` (parts'
  `--k-font`).

Built-in parts: `board` (agent cards with live pictures), `agents` (a
compact list), `status` (a status line), `departures` (a split-flap
board). `kibann screen parts` lists them all.

## A part

`part.json`:

```json
{
  "about": "Open pull requests for the focused pane's repository",
  "permissions": { "exec": ["gh"] },
  "focusable": false
}
```

- **`permissions`**, each asked of the user, and nothing else is allowed:
  - `"read": true`: see what the terminals show (`kibann.thumbnail`).
  - `"type": true`: type into terminals, open workspaces.
  - `"exec": ["git", "gh"]`: run these programs (by name, without a shell).
  - `"net": ["api.github.com"]`: connect to these hosts (https and wss).
- **`focusable`:** keeps the keyboard after a click (a part with text
  fields). Otherwise a click hands the keys back to the terminal (clicks
  in inputs still type there).

`index.html` gets `kibann` before its own scripts (`kibann screen api`
prints the types):

```js
kibann.on("state", (s) => render(s));   // now and on every change
// s.panes: [{id, tab, title, focused, shown, cwd, repo, branch, changed,
//            ahead, behind, agent, state, since, bg, fg}]
// s.tabs:  [{id, title, active, panes, focused, state}]
// s.focused, s.screen, s.blocked, s.windowFocused
kibann.props;                            // the block's "props"
kibann.on("show", (shown) => {});        // hidden blocks should pause work
kibann.run("pane.focus", { pane: 3 });   // also tab.activate, tab.new,
                                         // pane.split {direction: right|down},
                                         // pane.close, pane.zoom, diff,
                                         // screen.switch, node.show / hide /
                                         // toggle {id}, notify {text},
                                         // settings, themes, screens
kibann.run("pane.type", { pane: 3, text: "npm test", enter: true }); // "type"
await kibann.exec("git", ["status", "--porcelain"], { cwd: pane.cwd }); // "exec"
await kibann.drawPane(canvas, 3); // around the cursor      // "read"
await kibann.store.set("collapsed", true); await kibann.store.get("collapsed");
```

- **State:** `state` is `idle`, `working`, `blocked` (waiting for the user:
  show these first) or `done` (finished while nobody looked); null without
  an agent. `since` is when it last changed (ms).
- **Colors:** `--k-bg`, `--k-fg`, `--k-sidebar`, `--k-bar`, `--k-panel`,
  `--k-tab`, `--k-tabActive`, `--k-edge`, `--k-divider`, `--k-muted`,
  `--k-text`, `--k-label`, `--k-strong`, `--k-title`, `--k-bright`,
  `--k-current`, `--k-accent`, and the status colors `--k-working`,
  `--k-blocked`, `--k-done`, `--k-changed`, `--k-conflicts`. Fonts:
  `--k-font`, `--k-mono` (the terminals').
- **Titles and paths come from programs:** put them in the page with
  `textContent`, never `innerHTML`.
- **Timers:** `setInterval` is fine; pause when hidden. Draw thumbnails at
  most once or twice a second.

## Sharing

A screen and its parts are folders. To share them, put them in a git repo
(`screens/NAME/`, `parts/NAME/`, a `preview.png` and a README). To add
someone's: read every file first, tell the user what each part's
`permissions` ask for, then copy the folders into `~/.config/kibann/`
under names that aren't taken, and run `kibann screen check`.
