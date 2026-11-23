# Changelog

Each version's section becomes its GitHub release notes and what the
in-app update notice links to. Newest first. A version that's never
released on its own can be marked `## [x.y.z] - Included in <version>`;
its notes then go out with that version.

## [0.8.1] - Unreleased

- A pane split from the CLI while the window is covered or closed gets
  its new size at once. The pane it split from used to stay at its old
  width until you showed the window.
- A `kibann` command started just as the terminal service starts no longer
  waits several seconds and starts a second one that gives up.
- This update restarts the terminal service once (it asks first when
  something is running in a pane).

## [0.8.0] - 2026-11-23

- If the terminal service stops (it crashed, or something ended it), the
  window starts a new one and reopens your saved tabs and splits, instead
  of quitting.
- The window no longer stops responding after many notifications pile up
  in Notification Center. A newer notification about a pane replaces the
  one before it.
- Keys reach a pane at once even while it prints a lot, and `kibann pane
  list` no longer waits on busy panes.
- Config lines all read the same way: a value in quotes, and a `# comment`
  after it, now work for `theme` too (`theme = "Nord" # cold`).
- `kibann pane run --wait` works in fish and PowerShell as well as in
  POSIX shells.
- A pane opened for a command that ends at once closes with it, instead
  of sometimes staying open and empty.
- An agent that finishes in a pane you aren't looking at is marked done
  even while `kibann pane diff --view` is open.
- Panels in a screen need the "type" permission to close a pane, and can't
  press keys for you.
- Panes in tabs you aren't looking at no longer draw, and a pane that stops
  getting updates asks for them again on its own.
- A pane no longer stays in a fallback font when the terminal font loads
  late.
- `TERM_PROGRAM_VERSION` in your shells is Kibann's version.
- This update restarts the terminal service once (it asks first when
  something is running in a pane).

## [0.7.1] - 2026-11-21

- The window shows your terminals again. In 0.7.0 it showed only the tab
  bar, with no sidebar and no panes.
- A working agent's dot pulses, as it was always meant to.

## [0.7.0] - 2026-11-21

- Screens: change what the whole window shows and where. Pick one with
  View → Screen… (each shows as you move through the list; Enter keeps
  it), or step through them with Cmd+Shift+L and Cmd+Shift+K.
  Ctrl+Alt+Shift+Esc goes back to Kibann's own window, the `default`
  screen. Built in:
  - `board`: every pane as a card with a live picture, its agent and how
    long it's been at it. Waiting agents come first; click a card to
    show its pane.
  - `focus`: one terminal filling the window and a thin status line;
    your agents slide in from the left edge.
  - `departures`: your agents on a split-flap departures board.
- Make your own by asking your agent ("put my agents as cards beside the
  terminals"). Run `kibann skill install` again to give it the new
  `kibann-screens` skill. A screen is a folder of JSON; its panels are
  web pages that see your tabs, agents and git live. They're sandboxed:
  seeing or typing into your terminals, running programs or using the
  network each needs your OK first. `kibann screen` lists, checks and
  copies them. The README has the details.
- Layout files are now **workspace files**: View → Open Workspace…,
  `kibann workspace open`, `.kibann/workspace.toml`, and
  `~/.config/kibann/workspaces/`. Rename a `.kibann/layout.toml` or a
  file in `~/.config/kibann/layouts/` to match.
- The window no longer goes empty when the terminal service is from an
  older version and you open a workspace; Kibann offers to restart the
  service instead.
- This update restarts the terminal service once (it asks first when
  something is running in a pane).

## [0.6.0] - 2026-11-21

- Layout files: set up tabs and splits in one go, with a folder and a
  command for each pane. Open one with View → Open Layout… or
  `kibann layout open NAME`; save the window as one with View → Save
  Layout… or `kibann layout save NAME`. A repo can carry its own in
  `.kibann/layout.toml`. Built in: `system`, `ports`, `agents`,
  `agents-4` and `review`. A layout that isn't yours shows the commands
  it would run and asks first. The README has the format.
- Agents can build layouts for you ("make a layout for this repo"). Run
  `kibann skill install` again to give them the new `kibann-layouts`
  skill.
- `kibann watch SECS -- CMD` reruns a command every few seconds, like
  `watch`.
- Updates restart the terminal service only when they change how the
  terminal behaves. Until now every update did, giving you fresh shells or
  asking first when something was running in them. The first update to
  this version still restarts it once.

## [0.5.2] - 2026-11-21

- A pane no longer freezes after the Mac sleeps. A busy pane, such as
  one running Claude Code, could stop updating for good: it showed the
  same screen and seemed to ignore typing, even Ctrl+C, though the
  program kept running. A pane now notices when its screen has stopped
  arriving, when you come back to the window or type into it, and picks
  up again within a couple of seconds.

## [0.5.1] - 2026-11-16

- A folder you turn into a git repository (`git init`, or a clone into
  it) now shows its branch and uncommitted changes as soon as you're back
  at the prompt. Until now they appeared only after you left the folder
  and came back.

## [0.5.0] - 2026-11-15

- Updates now take full effect. The terminal service that keeps your shells
  running survived updates, so fixes to how the terminal behaves (such as
  0.4.0's Shift-click) only arrived once it happened to restart. After an
  update, Kibann now restarts the service on its own when every shell is at
  its prompt, and your tabs and splits come back in the same folders.
  When something is running in a pane, it asks first and names what would
  end.
- The first time, the service from the earlier version hadn't saved your
  layout; Kibann now saves it before restarting, so your tabs come back.

## [0.4.0] - 2026-11-15

- Shift-click extends the selection to where you click, as in Ghostty (by
  words or lines after a double or triple click). It used to clear the
  selection instead.

## [0.3.0] - 2026-11-15

- Drag the scrollbar to scroll through a pane's history.
- Your tabs and splits are now saved, so they come back (with fresh
  shells) after the terminal service ends, for example after "Quit and End
  Sessions". Until now the layout was never written to disk.

## [0.2.2] - 2026-11-15

- A pane no longer goes blank after the app sits unattended for a while
  (until you scrolled it): when the system drops a pane's pixels, it's
  painted again from what it shows.

## [0.2.1] - 2026-11-15

- A downloaded update is hard to miss: the menu-bar icon (macOS) and the
  tray icon (Linux) show a "!", and the sidebar's bottom row keeps an
  Update button (a red dot when the sidebar is collapsed) after its notice
  is dismissed.

## [0.2.0] - 2026-11-14

The first public build, for Macs with Apple silicon and for Linux.

- Linux: an AppImage and a `.deb`, for x86_64 and arm64, that update
  themselves. A tray icon keeps it running when the window closes (where
  the desktop shows tray icons), Settings lists the installed monospaced
  fonts, and opening it again shows the running window.

## [0.1.0] - Included in 0.2.0

- A terminal built on libghostty: true color, wide characters and emoji,
  box drawing drawn as lines, scrollback, links you can Cmd+click, and
  find in scrollback (Cmd+F).
- Selection like Ghostty's (double-click a word, triple-click a line,
  Alt-drag a rectangle), and Copy and Paste in the right-click menu.
  Cmd+Backspace, Cmd+Left/Right and Option+Left/Right edit the command
  line as in other Mac apps.
- Tabs and splits with Ghostty's shortcuts, all listed with Cmd+/ and all
  changeable in the config file (`keybind = ...`, Ghostty's syntax). Tabs
  show their number (Cmd+1–8), in the tab bar and the sidebar. Drag the
  dividers, or zoom a pane to fill its tab (Cmd+Shift+Enter, or
  double-click its header).
- Shells outlive the window: a background process keeps them running, and
  the app picks them up again when it opens. After a restart the tabs and
  splits come back with fresh shells in the same directories.
- Every pane shows where it is: the repo, branch, worktree, uncommitted
  files and ahead/behind, kept current as files change. Click the count to
  see the diff.
- Agent status at a glance: a dot on each pane running Claude Code, Codex
  or OpenCode (working, waiting for you, finished), rolled up on tabs and
  the Dock icon, with a notification when an agent you aren't looking at
  waits or finishes. Clicking it shows the pane.
- A sidebar of every pane grouped by repository, with live previews; it
  folds to a narrow rail.
- About 900 themes with a live picker; the whole window follows the theme.
  Font family and size in the config, and Cmd+= / Cmd+- to zoom.
- A `kibann` command in every pane, so agents can open panes, run
  commands, read output and wait on each other; `kibann skill install`
  teaches it to Claude Code, Codex and OpenCode.
- An icon in the menu bar: closing the window (or Cmd+Q) keeps the app
  running there, so agents can still notify you, with a dot when one
  waits. Its menu shows the version and brings the window back; its Quit
  ends everything.
- Updates itself: it checks hourly, downloads in the background and offers
  to restart (the shells keep running). Settings → Updates (or Help →
  What's New) shows these notes.
- Settings (Cmd+,): the switches, theme, font and size, every shortcut,
  updates and About in one place. It writes the config file, so editing by
  hand still works.
