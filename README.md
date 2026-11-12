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
```

The window closes when its shell exits.

## Renderer

```sh
cd app
npm install
npm run dev            # then open http://localhost:5173/src/dev/harness.html
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
