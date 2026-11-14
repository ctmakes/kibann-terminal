# Kibann Terminal

A terminal for working alongside coding agents, built on libghostty-vt (Ghostty's terminal core) in a Tauri app.

- **Agents work where you can see them.** Claude Code (or any script) running in a pane can open splits and tabs beside you, run commands in them, read their output and wait on them. You can click into any of those panes and take over.
- **See every agent at a glance.** A dot on each pane shows whether its agent is working, waiting for you or finished. Tabs, the sidebar and the Dock show it too, and you get a notification when an agent you aren't watching needs you.
- **Every pane knows where it is.** Each pane shows its repo, worktree, branch, uncommitted files and ahead/behind. Click the count to see the diff.
- **Shells outlive the window.** A background process keeps your shells running when the app closes. Tabs and splits come back after a restart.
- **A real terminal.** Ghostty's emulation, about 900 themes, your own fonts, and Ghostty's shortcuts and config syntax.

Kibann is at its first release (0.1.0). See [CHANGELOG.md](CHANGELOG.md) for what each version brings.

## Install

Download the `.dmg` from the [latest release](https://github.com/ctmakes/kibann-terminal/releases/latest), open it, and drag **Kibann Terminal** to Applications. Builds are for Macs with Apple silicon. They are signed and notarized, and they update themselves.

Linux works when built from source (see [Development](#development)). Windows doesn't yet: sessions there run inside the app and end with it.

## Using it

### Tabs and splits

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
| Settings | Cmd+, | Ctrl+, |
| Bigger / smaller text, default size | Cmd+= / Cmd+- / Cmd+0 | Ctrl+= / Ctrl+- / Ctrl+0 |
| Find, next / previous match | Cmd+F, Cmd+G / Cmd+Shift+G | Ctrl+Shift+F, F3 / Shift+F3 |

- Cmd+/ (or Help → Keyboard Shortcuts) shows every shortcut in the app, and any key closes it. You can change any shortcut in the config file (see [Keyboard shortcuts](#keyboard-shortcuts)).
- Ctrl+Tab and Ctrl+Shift+Tab also switch tabs. On macOS, the actions are also in the menu bar.
- You can drag the dividers. The right-click menu has Split Right, Split Down and Close.
- With more than one tab, each tab shows its number, the one Cmd+1–8 goes to; the sidebar shows it on each pane's row.
- New panes and tabs start in the focused pane's directory. A pane closes when its shell exits.

### Shells outlive the app

The shells run in a background process, the daemon, which also owns the tabs and splits. That changes what closing and quitting do:

- **Closing the window or quitting (Cmd+Q)** leaves the shells running. Open the app again and they're back, and each pane shows what its shell printed in the meantime.
- **Closing a pane** ends its shell.
- **Quit and End Sessions** in the app menu ends every shell and the daemon. The daemon also exits on its own 10 s after the last shell and window are gone.
- **After a reboot or log out**, the app opens with the same tabs and splits, at the same sizes and with the same pane focused. Each pane is a fresh shell in the directory it was in, but scrollback and running programs don't come back. Closing the last pane on purpose starts fresh next time instead.

The layout is kept in `~/Library/Application Support/kibann/layout.json` (`$XDG_STATE_HOME/kibann/` on Linux) and written a few seconds after each change. `restore-layout = false` in the config turns this off.

**The menu-bar icon (macOS).**
- **Closing the window, or its last pane,** only hides the window. The app keeps running, so agents can still notify you, and the icon shows a dot while one waits.
- **The icon's menu** shows the version. From it you can bring the window back (the Dock icon does that too), open a tab, go to a waiting agent, install a downloaded update, read What's New, or open Settings.
- **Quit Kibann Terminal** in that menu asks first, then ends the shells, the daemon and the app.

`menu-bar-icon = false` in the config removes the icon. Closing the window then quits the app, as does closing the last pane on other platforms.

Each shell gets `TERM_PROGRAM=kibann`. Variables describing the terminal the app was started from (`TERM_PROGRAM`, `TMUX`, `GHOSTTY_*` and the like) are not passed on.

### Agents drive panes

Every shell in Kibann can run `kibann`, a command line for opening panes and tabs, typing into them, reading them and waiting on them. An agent running in a pane uses it to work beside you. Its panes don't take your focus unless asked to, and they persist like your own.

```sh
p=$(kibann pane split --direction right | jq -r .pane.id)   # {"pane":{"id":"p4","tab":"t1"}}
kibann pane run "$p" "npm test" --wait                       # {"exit_code":0}
kibann pane read "$p" --source recent --lines 40
kibann pane wait "$p" --regex '^ready' --timeout 30000
kibann pane list        # also: tab new, pane send / keys / focus / close / current
kibann skill install    # the guide for Claude Code, to ~/.claude/skills/kibann
```

Run `kibann skill install` once, and Claude Code learns to use it.

- **Environment:** each shell gets `KIBANN_PANE` (its pane, like `p3`), `KIBANN_SOCKET` and `KIBANN_BIN`, and `kibann` is on its `PATH`.
- **Output:** results are JSON. Errors are JSON on stderr with exit code 1, and a bad command line exits 2.
- **Help:** `kibann help` lists every command, and [`skills/kibann/SKILL.md`](skills/kibann/SKILL.md) is the guide agents read.
- **Security:** anything that can open the socket can type into every pane. That's why the socket is private to your user (`$TMPDIR/kibann/daemon.sock` on macOS, `$XDG_RUNTIME_DIR/kibann/` on Linux, with the daemon's log beside it).

### Agent status at a glance

Panes running Claude Code show what it is doing with a dot in their top-right corner:

- **Green, pulsing:** it's working.
- **Amber:** it's waiting for you, at a permission prompt or a question.
- **Blue:** it finished while you were looking elsewhere. The dot clears when you focus the pane.

An idle agent and a plain shell show nothing. Each tab shows its most urgent dot (amber, then blue, then green), and the Dock icon counts the agents waiting for you.

Kibann reads this from Claude Code's title and the bottom of its screen rather than from hooks, which miss an Esc mid-turn or a cancelled prompt (an approach learned from [herdr](https://github.com/herdrdev/herdr)). Agents and scripts can use it too:
- `kibann pane list` shows each pane's `agent` and `state`.
- `kibann pane wait P --until done,blocked` waits for one of those states.
- A script can report its own state with `kibann pane report --agent NAME --state working|idle|blocked`, and stop with `pane release`.

### Notifications

When an agent in a pane you aren't looking at starts waiting for you or finishes, a system notification says so, for example "Claude Code is waiting for you", with the repo, branch and pane title. A pane counts as looked at when it's the focused pane and the window is in front. With the window behind, every pane counts as not looked at. Clicking a notification brings the window forward and shows that pane. Turn them off with `agent-notifications = false`.

### Where each pane is

Every pane has a slim header showing where it is:
- **Outside a repository:** the directory (`~/Downloads`).
- **Inside one:** the repository, the linked worktree if any, and the branch, then how many files differ from HEAD (`●5`, with staged, unstaged and untracked files counted once) and the commits ahead of and behind the upstream (`↑2 ↓1`).
- **A merge or rebase stopped midway** shows as `MERGING` or `REBASING`, with conflicted files as `!2`.

Tabs show the focused pane's branch after their title.

The header updates from filesystem events rather than polling, so an idle workspace costs nothing:

- **One `git status` per checkout**, shared by every pane in it.
- **Ignored files don't count**, so a build writing to `target/` starts no run.
- **Bursts of changes** cost one run.
- **Git runs with `--no-optional-locks`**, so it never takes the index lock an agent's `git commit` needs.

Ahead/behind are as fresh as your last fetch; Kibann never fetches. `kibann pane list` includes the same information as `git`.

**Click `●5`** to see the changes. A viewer opens in a split beside the pane, with the changed files on top (untracked ones too) and the selected file's diff below.
- Up/down or j/k pick a file, space and b page, and q closes it.
- It follows changes while open, and clicking `●5` again focuses it.
- Very large diffs are cut short, with a note.
- From a shell, `kibann pane diff P` prints the same patch, and `--stat` lists the files as JSON.

### The sidebar

The sidebar on the left lists every pane of every tab, one compact row each (agent dot, title, branch, `●5`), grouped by repository (panes outside one are grouped by directory).
- **Order:** groups with an agent waiting for you come first, then finished, then working.
- **Hover** a row to see a live preview of the pane beside the sidebar.
- **Click** a row to go to its pane, or its `●5` for the diff.

The sidebar never hides completely.
- **Cmd+Shift+B** (Ctrl+Shift+B elsewhere), View → Toggle Sidebar, or the button at its bottom folds it to a narrow rail with one badge per pane, and opens it again. Badges still click and preview.
- **A badge** is the pane's agent dot and a letter: the program's (`V` for vim), or, for a shell, the directory's (`K` for `~/src/kibann`). When two neighbours would look alike, they get two letters.
- **Drag its edge** to resize it. Dragging it nearly shut folds it, and its state and width are remembered.

The bottom row also has buttons for the theme picker, the shortcut sheet and Settings; hover one for its name and key. The `?` at the end of the tab bar also opens the shortcut sheet.

### Find

**Cmd+F** (Ctrl+Shift+F elsewhere, or Edit → Find…) opens a find bar on the focused pane.
- **Searching:** it searches the screen and the whole scrollback as you type, with letters matching either case, and keeps searching as new output arrives.
- **Matches:** every match in view is highlighted, and the bar counts them ("3 of 17").
- **Moving:** Enter or Cmd+G goes to the next match up the scrollback, and Shift+Enter or Cmd+Shift+G goes back down, scrolling to it.
- **Closing:** Esc closes the bar and returns to the bottom.

Each pane has its own bar and remembers its text.

### Selection, scrollback and links

Selection and scrollback work like Ghostty's:

- **Selecting text:** drag to select. Double-click selects a word, triple-click a line, and Alt-drag selects a rectangle.
- **Autoscroll:** dragging past the top or bottom edge scrolls the view.
- **Programs that use the mouse:** when a program tracks the mouse (vim, htop), hold Shift to select instead.
- **Copy and paste:** Cmd+C / Cmd+V on macOS, Ctrl+Shift+C / Ctrl+Shift+V elsewhere. The right-click menu offers Copy when something is selected, and always offers Paste.
- **Scrollback:**
  - It holds 10,000 lines.
  - Move through it with the wheel or Shift+PageUp / Shift+PageDown.
  - Jump to the top or bottom with Cmd+Home / Cmd+End on macOS, or Shift+Home / Shift+End elsewhere.
  - Typing returns the view to the bottom and clears the selection.

Links (Cmd on macOS, Ctrl elsewhere):

- **Hover with Cmd held** to underline the link under the pointer and see where it goes in the bottom-left corner. **Cmd+click** opens it.
- **What counts as a link:**
  - Hidden links that programs print (OSC 8, as from `ls --hyperlink`, `gcc` or `delta`).
  - URLs (`http`, `https`, `file`, `mailto`, `ftp`, `ssh`, `git`).
  - File paths that exist, relative to the pane's directory. A `:line:col` suffix is allowed (`src/main.ts:12:3`), but the file opens at the top.

  Links that wrap onto the next row, or start in scrollback, work as one link.
- **Opening:** URLs go to the default browser or mail app. Files open in their default app and directories in Finder. Apps, scripts and other files that could run something when opened are shown in Finder instead.
- **Try it:** `echo https://github.com`, `printf '\e]8;;https://example.com\e\\click me\e]8;;\e\\\n'`, or `ls`.

### Updates

The app checks for a new version a few seconds after it starts and then every hour. You can also check now with Kibann Terminal → Check for Updates…, or in Settings → Updates. It downloads the update in the background and says when it's ready, with **Restart to update**; the shells keep running through the restart. Settings → Updates shows the release notes (Help → What's New and the menu-bar icon's menu open it there), and the first start of a new version offers them.

## Settings

**Cmd+,** (Ctrl+, elsewhere), Kibann Terminal → Settings…, or the gear at the bottom of the sidebar opens Settings:

- **General:** notifications, the menu-bar icon (macOS) and restoring tabs and splits. Also the config file's location, with buttons to open it or show it in Finder, and any lines in it that can't be used.
- **Appearance:** the theme (Choose… opens the live picker), the font (a list of your installed monospaced fonts on macOS) and its size.
- **Shortcuts:** every shortcut. They're changed in the config file, below.
- **About** and **Updates:** the version, Check for Updates, and the release notes.

Settings writes the config file, keeping your comments and the order of your lines, so the file and Settings never disagree.

## Configuration

Settings live in one file, `~/.config/kibann/config` (or `$XDG_CONFIG_HOME/kibann/config`). It uses Ghostty's syntax, and changes apply when you save it. A line that can't be used shows a notice naming it, and is skipped.

```
theme = light:Kibann Light,dark:Kibann Dark
font-family = Berkeley Mono
font-size = 13
keybind = super+shift+d=new_split:down

agent-notifications = true   # notify when an agent waits or finishes
restore-layout = true        # bring tabs and splits back after a reboot
menu-bar-icon = true         # macOS: keep running in the menu bar when the window closes
```

### Themes

View → Theme…, or the half-circle button at the bottom of the sidebar, opens a searchable list of about 900 themes. They're the [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) collection, with the same names as Ghostty's.
- **Picking:** moving through the list shows each theme live on every pane. Enter keeps it, and Esc goes back.
- **What it colors:** the terminals (programs asking for the background color get the theme's) and the window around them. A light theme makes the tab bar, sidebar, headers and title bar light too, even on a dark system. Colors a program sets itself still win.

The picker saves the choice to the config, for the system appearance in use:

```
theme = Dracula
# or one for each system appearance:
theme = light:Kibann Light,dark:Kibann Dark
```

- **Your own themes** go in `~/.config/kibann/themes/`, one file per theme in Ghostty's format, named as the theme. They win over a bundled theme of the same name.
- **A bad theme name** shows a notice, and the default is used instead.
- **The defaults** are Kibann Dark and Kibann Light.

### Fonts

```
font-family = Berkeley Mono
font-family = Symbols Nerd Font   # repeat for fallbacks, in order
font-size = 13
```

Fonts named here are tried first, then the bundled JetBrains Mono, then the system's fallbacks for CJK and emoji. Cmd+= and Cmd+- (Ctrl elsewhere, or View → Bigger / Smaller) change the size in every pane, and Cmd+0 goes back to the config's size.

### Keyboard shortcuts

`keybind` lines change the shortcuts. They use Ghostty's syntax and action names, so most of a Ghostty config's keybinds work as they are. The shortcut sheet and the menu bar follow the changes.

```
keybind = super+shift+d=new_split:down
keybind = super+k=unbind
keybind = clear                    # drop every default first
```

**Triggers** are modifiers (`super`/`cmd`, `ctrl`, `alt`/`opt`, `shift`) plus a key (`a`, `1`, `equal`, `bracket_left`, `arrow_up`, `enter`, `f3`, …).

**Actions:**
- **Tabs:** `new_tab`, `previous_tab`, `next_tab`, `goto_tab:N`, `last_tab`.
- **Panes and splits:** `close_surface`, `new_split:right|down`, `goto_split:previous|next|left|right|up|down`, `resize_split:left|right|up|down`, `equalize_splits`, `toggle_split_zoom`.
- **Font size:** `increase_font_size`, `decrease_font_size`, `reset_font_size`.
- **Find:** `start_search`, `navigate_search:next|previous`.
- **Kibann's own:** `toggle_sidebar`, `show_shortcuts`, `open_theme_picker`, `open_settings`.
- **Removing a binding:** `unbind`.

Key sequences (`a>b`) and `global:` bindings aren't supported.

## Development

```
crates/proto/   message types shared by the backend and the web renderer (source of truth)
crates/term/    terminal session: PTY, libghostty-vt emulator, frame builder, input encoders
crates/daemon/  the daemon that owns the sessions and the layout (kibannd), its client, and the kibann CLI
skills/kibann/  the guide that teaches agents the CLI
app/            web frontend: canvas renderer, input handling, browser harness, tests
app/src-tauri/  Tauri shell: one window; turns commands into daemon requests
```

### The app

You need Rust, Node and Zig 0.17 on `PATH` (libghostty-vt builds from source).

```sh
cd app
npm install
npm run tauri dev
npm run tauri build -- --bundles app              # macOS .app in target/release/bundle
KIBANN_COMMAND="htop" npm run tauri dev           # run a program instead of the login shell
KIBANN_LAYOUT="right,down,tab" npm run tauri dev  # start with splits and tabs
KIBANN_RECORD=/tmp/s.cast npm run tauri dev       # record sessions (asciicast) for debugging
```

The daemon is the app's own executable, run with `--daemon`. Dev builds use `daemon-dev.sock` and replace a daemon left by an earlier build. As a result, a Rust rebuild ends your dev sessions, but sessions in the installed app are unaffected.

### The renderer

```sh
cd app
npm run dev            # then open http://127.0.0.1:1420/src/dev/harness.html
npm test               # unit tests (Vitest)
npx playwright install chromium webkit
npm run test:e2e       # screenshots, pixel joins, input, frames in Chromium + WebKit at 1x/2x
npm run bench          # paint-time budget (opt-in)
npm run gen:proto      # regenerate app/src/proto/generated from crates/proto
npm run gen:recordings # replay crates/term/recordings/*.cast into app/src/dev/recordings
```

The harness renders fixtures (`palette`, `styles`, `boxdraw`, `unicode`, `cursor`, `tui`, `partial`, `stress`, …) through the real renderer, with a replay transport standing in for the backend, and logs every input event the page would send. With `?replay=vim` (or `man`, `sync`), it instead plays frames the real emulator built from recorded program output; see [`crates/term/recordings/README.md`](crates/term/recordings/README.md).

The screenshot baselines in `app/e2e/__screenshots__` were made on macOS. On other platforms, regenerate them with `npm run test:e2e:update`.

### The backend

```sh
cargo test             # needs Zig 0.17 on PATH to build libghostty-vt
cargo run -p term --bin vt-record -- -o out.cast -- vim    # record a program (asciicast v2)
cargo run -p term --bin vt-replay -- out.cast -o out.jsonl # frames a session would send
```

### Releasing

`scripts/bump-version.sh x.y.z` releases from `main`. [RELEASE.md](RELEASE.md) has the whole process: the secrets, the by-hand checks and publishing the draft.

## License

MIT; see [LICENSE](LICENSE). The bundled themes come from [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) (MIT).
