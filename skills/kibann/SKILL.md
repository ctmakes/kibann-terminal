---
name: kibann
description: Open panes and tabs in the Kibann terminal, run commands or other agents in them, read their output and wait for them, while the user watches. Use when $KIBANN_PANE is set and the user asks for work to run in parallel or visibly (in a split, another pane or tab), or to check on another pane.
---

# Driving Kibann panes

You are running in a pane of Kibann Terminal when `$KIBANN_PANE` is set
(for example `p3`). The `kibann` command (also at `$KIBANN_BIN`) opens panes
and tabs next to yours, types into them, reads them and waits on them. The
user sees every pane live and can click into any of them.

Only use it when the user asked for parallel or visible work, or to look at
another pane. For ordinary commands, keep using your own shell tool.

## Rules

- **Work beside yourself.** `kibann pane split` with no pane opens next to
  your own pane, in your directory. Use `--direction right` when your pane is
  wide and `--direction down` when it is tall or already narrow. Use
  `kibann tab new` only for long-running or unrelated work.
- **Don't take the user's focus.** New panes don't take focus unless you
  pass `--focus`; don't, unless the user asked to see something.
- **Read ids from the output.** Every command prints JSON; take the new
  pane's id from `.pane.id`. Don't guess ids.
- **Close only what you opened**, and only when it's no longer useful to
  the user (they may want to read it). Never close the user's panes or your
  own.
- **Never answer another agent's prompt** (a permission question, a menu)
  in a pane you didn't start. Tell the user it is waiting for them.
- **Never end the daemon or quit the app.**

## Commands

```sh
kibann pane split [--direction right|down] [--cwd DIR] [--focus]
                                  # {"pane":{"id":"p4","tab":"t1"}}
kibann tab new [--cwd DIR] [--focus]
kibann pane run P "npm test" --wait --timeout 600000
                                  # waits, then {"exit_code":0}
kibann pane run P "npm run dev"   # types the command and Enter; returns at once
kibann pane send P "text"         # types without Enter
kibann pane keys P ctrl+c         # also enter, esc, tab, up, down, y, ...
kibann pane read P --lines 40     # the screen as text (--source recent: scrollback too)
kibann pane wait P --match "ready on" --timeout 30000   # text already there counts
kibann pane wait P --regex "error|done"
kibann pane wait P --exit         # the pane's shell exits
kibann pane wait P --until done,blocked --timeout 900000
                                  # P's agent finished or waits for the user
kibann pane list                  # every pane: id, tab, title, cwd, focused,
                                  # agent ("claude" or null) and state
kibann pane current               # your own pane
kibann pane focus P
kibann pane close P
```

Errors are JSON on stderr with exit code 1 (`timeout`, `not_found`,
`not_running`, ...); a bad command line exits 2.

## Patterns

Run tests beside you and report:

```sh
p=$(kibann pane split --direction right | jq -r .pane.id)
kibann pane run "$p" "npm test" --wait --timeout 600000
kibann pane read "$p" --source recent --lines 60
```

Start a server and wait until it is up:

```sh
p=$(kibann pane split --direction down | jq -r .pane.id)
kibann pane run "$p" "npm run dev"
kibann pane wait "$p" --match "Local:" --timeout 60000
```

Run another agent where the user can watch it:

```sh
p=$(kibann pane split --direction right | jq -r .pane.id)
kibann pane run "$p" "claude -p 'review the diff in this repo' > review.md" --wait --timeout 900000
```

Start another Claude Code where the user can watch it, and wait for it:

```sh
p=$(kibann pane split --direction right | jq -r .pane.id)
kibann pane run "$p" "claude 'fix the failing test in src/parse.rs'"
kibann pane wait "$p" --until working --timeout 60000
kibann pane wait "$p" --until done,idle,blocked --timeout 1800000
kibann pane read "$p" --lines 60
```

Agent states: `working`, `blocked` (waiting for the user to answer a
prompt), `idle`, and `done` (finished while the user wasn't looking at the
pane; it turns `idle` once they focus it). Kibann reads them from Claude
Code's screen and title. Wait for `working` first: right after `run`, the
agent hasn't started yet and the pane has no agent. If `blocked` comes back,
read the pane and tell the user what it asks; don't answer it.

A script or other agent can say what it is doing itself, which beats what
the screen shows until it releases it or exits to the shell:

```sh
kibann pane report --agent mybot --state working --seq 1
kibann pane report --agent mybot --state idle --seq 2
kibann pane release
```

`wait --match` and `--regex` look at every line on the screen and in the
scrollback, including the command line you typed: `run P "make; echo
built"` then `wait P --match built` matches at once. Wait for something
only the output has (`--regex '^built$'`), or use `run --wait`.

`run --wait` appends `; printf ...` to the command to learn its exit code,
so give it one command line for a POSIX shell (zsh, bash); a trailing `&`
or comment breaks it. Interactive programs that never return need `run`
without `--wait`, then `wait --match` or `read`.
