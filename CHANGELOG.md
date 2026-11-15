# Changelog

Each version's section becomes its GitHub release notes and what the
in-app update notice links to. Newest first. A version that's never
released on its own can be marked `## [x.y.z] - Included in <version>`;
its notes then go out with that version.

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
