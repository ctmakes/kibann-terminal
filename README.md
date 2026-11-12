# kibann-terminal

An agent-first terminal (Tauri 2 + libghostty-vt). Work in progress.

## Layout

```
crates/proto/   message types shared by the backend and the web renderer (source of truth)
crates/term/    terminal session: PTY, libghostty-vt emulator, frame builder, input encoders
app/            web frontend: canvas renderer, input handling, browser harness, tests
app/src-tauri/  Tauri shell: one window, one session; turns commands into session calls
```

## App

```sh
cd app
npm install
npm run tauri dev      # needs Zig 0.17 on PATH (libghostty-vt builds from source)
npm run tauri build -- --bundles app   # macOS .app in target/release/bundle
KIBANN_COMMAND="htop" npm run tauri dev   # run a program instead of the login shell
KIBANN_LAYOUT="right,down,tab" npm run tauri dev   # start with splits and tabs
KIBANN_RECORD=/tmp/s.cast npm run tauri dev   # record sessions (asciicast) for debugging
```

Tabs and splits use Ghostty's default shortcuts:

| Action | macOS | Linux / Windows |
|---|---|---|
| New tab | Cmd+T | Ctrl+Shift+T |
| Close pane | Cmd+W | Ctrl+Shift+W |
| Split right / down | Cmd+D / Cmd+Shift+D | Ctrl+Shift+O / Ctrl+Shift+E |
| Previous / next pane | Cmd+[ / Cmd+] | Ctrl+Super+[ / ] |
| Pane in a direction | Cmd+Opt+Arrows | Ctrl+Alt+Arrows |
| Resize pane | Cmd+Ctrl+Arrows | Ctrl+Super+Shift+Arrows |
| Equalize panes | Cmd+Ctrl+= | Ctrl+Super+Shift+= |
| Zoom pane | Cmd+Shift+Enter | Ctrl+Shift+Enter |
| Previous / next tab | Cmd+Shift+[ / ] | Ctrl+PageUp / PageDown |
| Tab 1–8, last tab | Cmd+1–8, Cmd+9 | Alt+1–8, Alt+9 |

Ctrl+Tab and Ctrl+Shift+Tab also switch tabs. On macOS the actions are in the menu bar too. Dividers can be dragged, and the right-click menu has Split Right, Split Down and Close.

New panes and tabs start in the focused pane's directory. A pane closes when its shell exits, and the app quits when the last pane closes. The shell doesn't inherit variables describing the terminal the app was started from (`TERM_PROGRAM`, `TMUX`, `GHOSTTY_*` and the like); it gets `TERM_PROGRAM=kibann`.

Selection and scrollback work like Ghostty's:

- **Selecting text:** drag to select. Double-click selects a word, triple-click a line, and Alt-drag selects a rectangle.
- **Autoscroll:** dragging past the top or bottom edge scrolls the view.
- **Programs that use the mouse:** when a program tracks the mouse (vim, htop), hold Shift to select instead.
- **Copy and Paste:**
  - Cmd+C / Cmd+V on macOS; Ctrl+Shift+C / Ctrl+Shift+V elsewhere.
  - The right-click menu offers Copy when something is selected, and always Paste.
- **Scrollback:**
  - Holds 10,000 lines.
  - Move through it with the wheel or Shift+PageUp / Shift+PageDown.
  - Jump to the top or bottom with Cmd+Home / Cmd+End on macOS, or Shift+Home / Shift+End elsewhere.
  - Typing returns the view to the bottom and clears the selection.

## Renderer

```sh
cd app
npm install
npm run dev            # then open http://127.0.0.1:1420/src/dev/harness.html
npm test               # unit tests (Vitest)
npx playwright install chromium webkit
npm run test:e2e       # screenshots, pixel joins, input, frames in Chromium + WebKit at 1x/2x
npm run bench          # paint-time budget (opt-in)
npm run gen:proto      # regenerate app/src/proto/generated from crates/proto
npm run gen:recordings # replay crates/term/recordings/*.cast into app/src/dev/recordings
```

The harness renders fixtures (`palette`, `styles`, `boxdraw`, `unicode`, `cursor`, `tui`,
`partial`, `stress`, …) through the real renderer with a replay transport standing in for the
backend, and logs every input event the page would send. With `?replay=vim` (or `man`, `sync`)
it plays frames the real emulator built from recorded program output instead; see
`crates/term/recordings/README.md`.

## Backend

```sh
cargo test -p term     # needs Zig 0.17 on PATH to build libghostty-vt
cargo run -p term --bin vt-record -- -o out.cast -- vim    # record a program (asciicast v2)
cargo run -p term --bin vt-replay -- out.cast -o out.jsonl # frames a session would send
```

Screenshot baselines in `app/e2e/__screenshots__` were made on macOS; regenerate them with
`npm run test:e2e:update` on other platforms.
