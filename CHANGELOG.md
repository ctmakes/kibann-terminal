# Changelog

Each version's section becomes its GitHub release notes and what the
in-app update notice links to. Newest first. A version that's never
released on its own can be marked `## [x.y.z] - Included in <version>`;
its notes then go out with that version.

## [0.1.0] - Unreleased

The first public build, for Macs with Apple silicon.

- A terminal built on libghostty: true color, wide characters and emoji,
  box drawing drawn as lines, selection, scrollback, links you can
  Cmd+click, and find in scrollback (Cmd+F).
- Tabs and splits with Ghostty's shortcuts, all listed with Cmd+/ and all
  changeable in the config file (`keybind = ...`, Ghostty's syntax). Tabs
  show their number (Cmd+1–8), in the tab bar and the sidebar.
- Shells outlive the window: a background process keeps them running, and
  the app picks them up again when it opens. After a restart the tabs and
  splits come back with fresh shells in the same directories.
- Every pane shows where it is: the repo, branch, worktree, uncommitted
  files and ahead/behind, kept current as files change. Click the count to
  see the diff.
- Agent status at a glance: a dot on each pane running Claude Code
  (working, waiting for you, finished), rolled up on tabs and the Dock
  icon, with a notification when an agent you aren't looking at waits or
  finishes. Clicking it shows the pane.
- A sidebar of every pane grouped by repository, with live previews; it
  folds to a narrow rail.
- About 900 themes with a live picker; the whole window follows the theme.
  Font family and size in the config, and Cmd+= / Cmd+- to zoom.
- A `kibann` command in every pane, so agents can open panes, run
  commands, read output and wait on each other.
- An icon in the menu bar: closing the window keeps the app running there,
  so agents can still notify you, with a dot when one waits. Its menu
  shows the version and brings the window back; its Quit ends everything.
- Updates itself: it checks hourly, downloads in the background and offers
  to restart (the shells keep running). Settings → Updates (or Help →
  What's New) shows these notes.
- Settings (Cmd+,): the switches, theme, font and size, every shortcut,
  updates and About in one place. It writes the config file, so editing by
  hand still works.
