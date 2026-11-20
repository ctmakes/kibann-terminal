---
name: kibann-layouts
description: Build, save and open Kibann terminal layout files (tabs and splits with a directory and a command per pane). Use when $KIBANN_PANE is set and the user asks to set up a workspace or layout ("make a layout for this repo", "set up Claude on the left and the dev server on the right", "save this setup"), or to change or open one.
---

# Kibann layout files

A layout file describes tabs and splits, with a directory and a command
for each pane. `kibann layout open` opens it next to the user's tabs.
You're in Kibann when `$KIBANN_PANE` is set.

## Rules

- **Find the real commands.** Read the repo for them (`package.json`
  scripts, `Makefile`, `justfile`, `Cargo.toml`, `Procfile`,
  `docker-compose.yml`, the README). Don't guess `npm run dev`.
- **Where to write it:**
  - `.kibann/layout.toml` in the repo when it's for the project. Others
    may get it, so keep it free of anything personal.
  - `~/.config/kibann/layouts/NAME.toml` when it's the user's own.
  - Ask when it's unclear. Never overwrite an existing layout without
    asking.
- **Keep it small.** Two to four panes a tab, and few tabs. Ask before
  going past six panes.
- **Check after every edit:** `kibann layout check FILE`. Fix what it
  reports; each problem names its line and column.
- **Open only when the user asks.** Opening runs the commands. Before
  `--yes`, show the user the commands `layout check` lists under `runs`
  and get their go-ahead. A file in a repo isn't trusted until they agree.
- **No secrets or tokens** in commands, since a project layout gets
  committed. In a shared layout, use commands that work in any shell.
- **"Save this setup"** means `kibann layout save NAME`, not writing the
  file by hand. Tidy what it writes afterwards if asked (titles,
  relative paths).

## The format (TOML)

```toml
name = "myapp"              # optional; defaults to the file's name

[[tab]]
title = "dev"               # optional; else the focused pane's title
split = "right"             # "right" (side by side) or "down" (stacked)
panes = [
  { command = "claude", size = 60, focus = true },
  { split = "down", panes = [
      { command = "npm run dev" },
      { cwd = "docs" },     # a plain shell in ./docs
  ] },
]

[[tab]]
title = "git"
command = ["lazygit", "tig", "git log --oneline --graph"]
```

- **A tab** is a pane, plus `title`, and `focus = true` to show it after
  opening (default: the first tab).
- **A pane** is a split (`split` and two or more `panes`) or a leaf:
  - `cwd`: relative to the parent's directory. `~` is home.
  - `command`: typed into the login shell. When it ends, the pane is
    left at a prompt.
  - `exec = ["prog", "arg"]`: runs instead of the shell; the pane closes
    when it exits.
  - `command = ["btop", "htop", "top"]`: the first one installed runs.
    `missing = "brew install btop"` is shown when none is.
  - `command = { macos = "macmon", linux = "nvtop", default = "top" }`: a
    command per system. Each entry can be a list too.
  - `size`: percent of the split, 5 to 95. Panes without one share the
    rest equally.
  - `focus = true`: the pane focused in its tab.
- **Paths:** in `.kibann/layout.toml`, relative paths start at the
  project's root, wherever it's cloned. In other files they start where
  the layout is opened. `root = "path"` at the top sets the start
  explicitly, relative to the file.
- **Limits:** 16 tabs and 32 panes. Unknown keys are errors.

A project layout for a web app with an API:

```toml
[[tab]]
title = "app"
split = "right"
panes = [
  { command = "claude", size = 55, focus = true },
  { split = "down", panes = [
      { cwd = "web", command = "npm run dev" },
      { cwd = "api", command = "cargo watch -x run" },
      { command = "kibann watch 5 -- git status --short" },
  ] },
]
```

`kibann watch SECS -- CMD...` reruns a command every SECS seconds, like
`watch`, on every platform.

## Commands

```sh
kibann layout list                 # {"layouts":[{"name","origin","path","about"}]}
                                   # origin: project, user, builtin
kibann layout check [NAME|FILE]    # {"name","panes","runs":[{"cwd","run"}],"asks",...}
                                   # or error "invalid" with every problem
kibann layout open [NAME|FILE] [--yes]
                                   # no NAME: the project's .kibann/layout.toml
kibann layout save NAME [--tab t2] [--force]
                                   # the window (or one tab) to the user's layouts
kibann layout save --to .kibann/layout.toml [--force]
                                   # the project's; directories relative to it
kibann layout save NAME --from system
                                   # copy a built-in layout to edit
```

`open` fails with code `untrusted` when the file runs commands the user
hasn't agreed to; show them the `runs` list and pass `--yes` once they
say so. Built-in layouts: `system` (CPU, GPU, disks), `ports` (what's
listening), `agents`, `agents-4` and `review`.
