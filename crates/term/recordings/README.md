# Recordings

Real program output in [asciicast v2](https://docs.asciinema.org/manual/asciicast/v2/), used to test the frame builder and the renderer together.

| Recording | Program | Covers |
|---|---|---|
| `vim.cast` | vim 9.1, `habamax`, truecolor | syntax colors, split windows, visual selection, search highlight, CJK and emoji |
| `man.cast` | `man ls` in less | bold and underline from a pager, standout search highlight |
| `sync.cast` | `assets/sync.sh` | synchronized output (mode 2026): half-drawn screens must never be shown |

`npm run gen:recordings` in `app/` records nothing. It replays the `.cast` files into `app/src/dev/recordings/*.jsonl`, and `cargo test -p term` fails if those are stale. Open them in the harness with `?replay=vim` (add `at=end` or `at=MS` to jump, `speed=N` to play faster).

To re-record, run `./record.sh` here (needs vim); output varies slightly between runs, so update the screenshot baselines afterwards. When a script ends, the recorder stops the program without recording its exit. In script mode an emulator answers the program's terminal queries, as a real terminal would.

To record something new by hand:

```sh
cargo run -p term --bin vt-record -- -o claude.cast -- claude    # interactive, Unix only
cargo run -p term --bin vt-replay -- claude.cast -o ../../app/src/dev/recordings/claude.jsonl
```

Check a recording for personal details (paths, user names, account info) before committing it.
