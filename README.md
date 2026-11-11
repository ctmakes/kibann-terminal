# kibann-terminal

An agent-first terminal (Tauri 2 + libghostty-vt). Work in progress.

## Layout

```
crates/proto/   message types shared by the backend and the web renderer (source of truth)
app/            web frontend: canvas renderer, input handling, browser harness, tests
```

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
```

The harness renders fixtures (`palette`, `styles`, `boxdraw`, `unicode`, `cursor`, `tui`,
`partial`, `stress`, …) through the real renderer with a replay transport standing in for the
backend, and logs every input event the page would send.

Screenshot baselines in `app/e2e/__screenshots__` were made on macOS; regenerate them with
`npm run test:e2e:update` on other platforms.
