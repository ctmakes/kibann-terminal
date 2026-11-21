import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, normalize, resolve } from "node:path";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

const REPO = resolve(import.meta.dirname, "..");
/** Where the dev page finds parts: Kibann's, then the e2e tests' own. */
const PART_DIRS = [join(REPO, "parts"), join(import.meta.dirname, "e2e/fixtures/parts")];
const SCREEN_DIRS = [join(REPO, "screens"), join(import.meta.dirname, "e2e/fixtures/screens")];

const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
};

/**
 * The screens and parts for the workspace harness (src/dev/workspace.ts),
 * served as the app's `kpart://` scheme serves them (daemon::screens):
 * `/kpart/NAME/...` is a part's file, with kibann.js put first in its
 * page; `/kscreens/` lists the screens and `/kscreens/NAME` is one.
 */
function screens(): Plugin {
  return {
    name: "kibann-screens",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const url = new URL(req.url ?? "/", "http://x");
        const send = (body: string | Buffer, type: string) => {
          res.setHeader("Content-Type", type);
          res.setHeader("Cache-Control", "no-store");
          res.setHeader("Access-Control-Allow-Origin", "*");
          res.end(body);
        };
        if (url.pathname.startsWith("/kpart/")) {
          const rel = normalize(decodeURIComponent(url.pathname.slice("/kpart/".length)));
          if (rel.startsWith("..")) return next();
          if (rel === "kibann.js") return send(readFileSync(join(REPO, "parts/kibann.js")), TYPES[".js"]);
          const file = PART_DIRS.map((d) => join(d, rel)).find((f) => existsSync(f) && statSync(f).isFile());
          if (!file) return next();
          let body: string | Buffer = readFileSync(file);
          if (rel.endsWith("/index.html")) {
            const text = body.toString("utf8");
            const at = text.toLowerCase().indexOf("<head>");
            const tag = `<script src="../kibann.js"></script>`;
            body = at >= 0 ? text.slice(0, at + 6) + tag + text.slice(at + 6) : tag + text;
          }
          return send(body, TYPES[extname(file)] ?? "application/octet-stream");
        }
        if (url.pathname === "/kscreens/") {
          const list = SCREEN_DIRS.flatMap((d) =>
            existsSync(d)
              ? readdirSync(d)
                  .filter((n) => existsSync(join(d, n, "screen.json")))
                  .map((name) => ({
                    name,
                    about: JSON.parse(readFileSync(join(d, name, "screen.json"), "utf8")).about ?? null,
                    builtin: d === SCREEN_DIRS[0],
                    path: null,
                  }))
              : [],
          );
          return send(JSON.stringify(list), TYPES[".json"]);
        }
        if (url.pathname.startsWith("/kscreens/")) {
          const name = url.pathname.slice("/kscreens/".length);
          const file = SCREEN_DIRS.map((d) => join(d, name, "screen.json")).find((f) => /^[a-z0-9-]+$/.test(name) && existsSync(f));
          if (!file) return next();
          return send(readFileSync(file), TYPES[".json"]);
        }
        next();
      });
    },
  };
}

export default defineConfig({
  plugins: [screens()],
  // Tauri expects a fixed port and no clearing of its log output.
  server: { port: 1420, strictPort: true, host: "127.0.0.1" },
  clearScreen: false,
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "index.html"),
        harness: resolve(import.meta.dirname, "src/dev/harness.html"),
        workspace: resolve(import.meta.dirname, "src/dev/workspace.html"),
      },
    },
  },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
