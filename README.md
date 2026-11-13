# kibann-terminal

An agent-first terminal (Tauri 2 + libghostty-vt). Work in progress.

## Layout

```
crates/proto/   message types shared by the backend and the web renderer (source of truth)
crates/term/    terminal session: PTY, libghostty-vt emulator, frame builder, input encoders
crates/daemon/  the daemon that owns the sessions and the layout (kibannd), its client, and the kibann CLI
skills/kibann/  the guide that teaches agents the CLI
app/            web frontend: canvas renderer, input handling, browser harness, tests
app/src-tauri/  Tauri shell: one window; turns commands into daemon requests
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
| Sidebar | Cmd+Shift+B | Ctrl+Shift+B |
| All shortcuts | Cmd+/ | Ctrl+Shift+/ |
| Bigger / smaller text, default size | Cmd+= / Cmd+- / Cmd+0 | Ctrl+= / Ctrl+- / Ctrl+0 |
| Find, next / previous match | Cmd+F, Cmd+G / Cmd+Shift+G | Ctrl+Shift+F, F3 / Shift+F3 |

Cmd+/ (or Help → Keyboard Shortcuts) shows every shortcut in the app; any key closes it. Every shortcut can be changed in the config file (see [Keyboard shortcuts](#keyboard-shortcuts)). Ctrl+Tab and Ctrl+Shift+Tab also switch tabs. On macOS the actions are in the menu bar too. Dividers can be dragged, and the right-click menu has Split Right, Split Down and Close.

Shells outlive the app. Closing the window or quitting (Cmd+Q) leaves them running in a background process, the daemon, which also owns the tabs and splits; opening the app again brings them back, with each pane showing what its shell printed meanwhile. Closing a pane still ends its shell, and the daemon exits 10 s after the last shell and window are gone. **Quit and End Sessions** in the app menu ends every shell and the daemon. A reboot or log out ends the shells. After that, the app opens with the same tabs and splits, at the same sizes and with the same pane focused, each a fresh shell in the directory it was in (scrollback and running programs don't come back). The daemon keeps the layout in `~/Library/Application Support/kibann/layout.json` (`$XDG_STATE_HOME/kibann/` on Linux), written a few seconds after each change; `restore-layout = false` in the config turns it off. Closing the last pane on purpose starts fresh next time.

The daemon is the app's own executable run with `--daemon`, listening on a socket only your user can open (`$TMPDIR/kibann/daemon.sock` on macOS, `$XDG_RUNTIME_DIR/kibann/` on Linux; its log is beside it). Dev builds use `daemon-dev.sock` and replace a daemon from an earlier build, which ends dev sessions after a Rust rebuild. On Windows (not yet supported by the daemon) sessions run inside the app and end with it.

New panes and tabs start in the focused pane's directory. A pane closes when its shell exits, and the app quits when the last pane closes. The shell doesn't inherit variables describing the terminal the app was started from (`TERM_PROGRAM`, `TMUX`, `GHOSTTY_*` and the like); it gets `TERM_PROGRAM=kibann`.

### Agents drive panes

Every shell in Kibann can run `kibann`, a command line for opening panes and tabs, typing into them, reading them and waiting on them. An agent (Claude Code, or a script) running in a pane uses it to work beside you where you can watch, and you can click into any pane it opens and take over. Its panes don't take your focus unless asked to, and they persist like your own.

```sh
p=$(kibann pane split --direction right | jq -r .pane.id)   # {"pane":{"id":"p4","tab":"t1"}}
kibann pane run "$p" "npm test" --wait                       # {"exit_code":0}
kibann pane read "$p" --source recent --lines 40
kibann pane wait "$p" --regex '^ready' --timeout 30000
kibann pane list        # also: tab new, pane send / keys / focus / close / current
kibann skill install    # the guide for Claude Code, to ~/.claude/skills/kibann
```

Each shell gets `KIBANN_PANE` (its pane, like `p3`), `KIBANN_SOCKET` and `KIBANN_BIN`, and `kibann` is on its `PATH` (a link to the app beside the socket). Output is JSON; errors are JSON on stderr with exit code 1, and a bad command line exits 2. `kibann help` lists everything; `skills/kibann/SKILL.md` is the guide agents read. Anything that can open the socket can type into every pane, so only your own user can.

### Where each pane is

Every pane has a slim header showing where it is: outside a repository, the directory (`~/Downloads`); inside one, the repository, the linked worktree if any, and the branch, then how many files differ from HEAD (`●5`, staged, unstaged and untracked counted once) and commits ahead of and behind the upstream (`↑2 ↓1`). A merge or rebase stopped in the middle shows as `MERGING` or `REBASING`, with conflicted files as `!2`. Tabs show the focused pane's branch after their title.

It updates from filesystem events, not polling, so an idle workspace costs nothing:

- **One `git status` per checkout**, shared by every pane in it.
- **Ignored files don't count**, so a build writing to `target/` starts no run.
- **Bursts of changes** cost one run.
- **Git is run with `--no-optional-locks`**, so it never takes the index lock an agent's `git commit` needs.

Ahead/behind are as fresh as your last fetch; Kibann never fetches. `kibann pane list` includes the same information as `git`.

**Click `●5`** to see the changes: a viewer opens in a split beside the pane, with the changed files on top (untracked ones too) and the selected file's diff below. Up/down or j/k pick a file, space and b page, q closes it. It follows changes while open; clicking `●5` again focuses it. Very large diffs are cut, with a note. From a shell, `kibann pane diff P` prints the same patch (`--stat` lists the files as JSON).

### The sidebar

The sidebar lists every pane of every tab on the left, one compact row each (agent dot, title, branch, `●5`), grouped by repository (panes outside one by directory). Hover a row to see a live preview of the pane beside the sidebar. Groups with an agent waiting for you come first, then finished, then working. Click a row to go to its pane; click its `●5` for the diff.

It's never hidden. **Cmd+Shift+B** (Ctrl+Shift+B elsewhere, View → Toggle Sidebar, or the sidebar button at its bottom) folds it to a narrow rail, one badge per pane, which still click and preview, and opens it again. A badge is the pane's agent dot and a letter: the program's (`V` for vim), or for a shell the directory's (`K` for `~/src/kibann`); two neighbours that would look alike get two letters. Drag its edge to resize it; dragging it nearly shut folds it. Its state and width are remembered. Its bottom row also has buttons for the theme picker and the shortcut sheet (hover one for its name and key), and the `?` at the end of the tab bar opens the shortcut sheet too.

### Themes

View → Theme… (or the half-circle button at the bottom of the sidebar) opens a searchable list of about 900 themes, the [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) collection with the same names as Ghostty's. Moving through it shows each theme live on every pane; Enter keeps it, Esc goes back. A theme colors the terminals (programs asking for the background color get the theme's) and the window around them: a light theme makes the tab bar, sidebar, headers and title bar light too, even on a dark system. Colors a program sets itself still win.

The choice is saved in `~/.config/kibann/config` (or `$XDG_CONFIG_HOME/kibann/config`), Ghostty syntax, which can be edited by hand and applies when saved:

```
theme = Dracula
# or one for each system appearance:
theme = light:Kibann Light,dark:Kibann Dark
```

The picker sets the theme for the appearance in use. Your own themes go in `~/.config/kibann/themes/`, one file per theme in Ghostty's format, named as the theme; they win over a bundled one of the same name. A bad name or line shows a notice and the default is used. The defaults are Kibann Dark and Kibann Light.

### Fonts

The same config file sets the font, and applies when saved:

```
font-family = Berkeley Mono
font-family = Symbols Nerd Font   # repeat for fallbacks, in order
font-size = 13
```

Fonts named here are tried first, then the bundled JetBrains Mono, then the system's fallbacks for CJK and emoji. Cmd+= and Cmd+- (Ctrl elsewhere, or View → Bigger / Smaller) change the size in every pane, and Cmd+0 goes back to the config's.

### Notifications

When an agent in a pane you aren't looking at starts waiting for you or finishes, a system notification says so ("Claude Code is waiting for you", with the repo, branch and pane title). A pane counts as looked at when it's the focused pane and the window is in front; with the window behind, every pane counts. Clicking one brings the window forward and shows that pane. Turn them off with `agent-notifications = false`.

### Find

**Cmd+F** (Ctrl+Shift+F elsewhere, Edit → Find…) opens a find bar on the focused pane. It searches the screen and the whole scrollback as you type (letters match either case) and keeps searching as output arrives; every match in view is highlighted and the bar counts them ("3 of 17"). Enter or Cmd+G goes to the next match up the scrollback, Shift+Enter or Cmd+Shift+G back down, scrolling to it. Esc closes the bar and returns to the bottom. Each pane has its own bar and remembers its text.

### Keyboard shortcuts

`keybind` lines in the config change the shortcuts, in Ghostty's syntax and with Ghostty's action names, so most of a Ghostty config's keybinds work as they are. They apply when the file is saved; the shortcut sheet and the menu bar follow.

```
keybind = super+shift+d=new_split:down
keybind = super+k=unbind
keybind = clear                    # drop every default first
```

Triggers are modifiers (`super`/`cmd`, `ctrl`, `alt`/`opt`, `shift`) and a key (`a`, `1`, `equal`, `bracket_left`, `arrow_up`, `enter`, `f3`, …). Actions: `new_tab`, `close_surface`, `new_split:right|down`, `goto_split:previous|next|left|right|up|down`, `resize_split:left|right|up|down`, `equalize_splits`, `toggle_split_zoom`, `previous_tab`, `next_tab`, `goto_tab:N`, `last_tab`, `increase_font_size`, `decrease_font_size`, `reset_font_size`, `start_search`, `navigate_search:next|previous`, `unbind`, and Kibann's own `toggle_sidebar`, `show_shortcuts`, `open_theme_picker`. Key sequences (`a>b`) and `global:` bindings aren't supported; a line that can't be used is named in a notice and skipped.

### Agent status at a glance

Panes running Claude Code show what it is doing with a dot in their top-right corner: **green, pulsing** while it works, **amber** when it waits for you (a permission prompt or question), and **blue** when it finished while you were looking elsewhere (it clears when you focus the pane). An idle agent and a plain shell show nothing. Each tab shows its most urgent dot (amber, then blue, then green), and the Dock icon counts the agents waiting for you.

Kibann reads this from Claude Code's title and the bottom of its screen, not from hooks, which miss an Esc mid-turn or a cancelled prompt (an approach learned from [herdr](https://github.com/herdrdev/herdr)). Agents can use it too: `kibann pane list` shows each pane's `agent` and `state`, `kibann pane wait P --until done,blocked` waits for one, and a script can report its own state with `kibann pane report --agent NAME --state working|idle|blocked` (and `pane release`).

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

Links (Cmd on macOS, Ctrl elsewhere):

- **Hover with Cmd held** to underline the link under the pointer and see where it goes in the bottom-left corner. **Cmd+click** opens it.
- **What counts:** hidden links that programs print (OSC 8, as from `ls --hyperlink`, `gcc` or `delta`), URLs (`http`, `https`, `file`, `mailto`, `ftp`, `ssh`, `git`), and file paths that exist, relative to the pane's directory. A `:line:col` suffix is allowed (`src/main.ts:12:3`) but the file opens at the top. Links that wrap onto the next row, or start in scrollback, work as one link.
- **Opening:** URLs go to the default browser or mail app; files open in their default app and directories in Finder. Apps, scripts and other files that could run something when opened are shown in Finder instead.
- Try it: `echo https://github.com`, `printf '\e]8;;https://example.com\e\\click me\e]8;;\e\\\n'`, or `ls`.

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

## License

MIT; see [LICENSE](LICENSE). The bundled themes come from [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) (MIT).
