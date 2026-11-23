import { appendFileSync, existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join, normalize, resolve } from "node:path";
import type { Connect, Plugin } from "vite";
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
 * served as the app's `kpart://` scheme serves them (files::screens):
 * `/kpart/NAME/...` is a part's file, with kibann.js put first in its
 * page; `/kscreens/` lists the screens and `/kscreens/NAME` is one.
 */
function screens(): Plugin {
  const serve: Connect.NextHandleFunction = (req, res, next) => {
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
  };
  return {
    name: "kibann-screens",
    configureServer: (server) => void server.middlewares.use(serve),
    configurePreviewServer: (server) => void server.middlewares.use(serve),
  };
}

/** Where the release-CSP preview writes the violations browsers report. */
export const CSP_REPORTS = join(tmpdir(), "kibann-csp-violations.jsonl");
const NONCE = "kibann-csp-check";

/**
 * The release build's CSP (tauri.conf.json) as Tauri serves it: Tauri adds
 * a nonce to `style-src` and `script-src` (and to the page's own inline
 * tags), and with a nonce WebKit and Chromium ignore 'unsafe-inline', so a
 * `<style>` made at run time or a `style=` attribute is dropped. The
 * harness fetches screens and frames parts from its own origin where the
 * app uses `kpart://`, so `connect-src` and `frame-src` allow 'self' too.
 * Violations are reported to `/csp-report`.
 */
function releaseCspHeader(): string {
  const conf = JSON.parse(readFileSync(join(import.meta.dirname, "src-tauri/tauri.conf.json"), "utf8"));
  const directives = (conf.app.security.csp as string).split(";").map((d) => d.trim().split(/\s+/));
  for (const d of directives) {
    if (d[0] === "style-src" || d[0] === "script-src") d.push(`'nonce-${NONCE}'`);
    if (d[0] === "connect-src" || d[0] === "frame-src") d.push("'self'");
  }
  if (!directives.some((d) => d[0] === "script-src")) directives.push(["script-src", "'self'", `'nonce-${NONCE}'`]);
  directives.push(["report-uri", "/csp-report"]);
  return directives.map((d) => d.join(" ")).join("; ");
}

/** `vite build --mode csp` and `vite preview --mode csp`: the built pages
 * under the release CSP, for the e2e suite's `webkit-csp` project. */
function releaseCsp(): Plugin {
  const header = releaseCspHeader();
  return {
    name: "kibann-release-csp",
    transformIndexHtml: (html) => html.replace(/<(style|script)\b/g, `<$1 nonce="${NONCE}"`),
    configurePreviewServer(server) {
      writeFileSync(CSP_REPORTS, "");
      server.middlewares.use((req, res, next) => {
        if (req.method === "POST" && req.url === "/csp-report") {
          let body = "";
          req.on("data", (c) => (body += c));
          req.on("end", () => {
            appendFileSync(CSP_REPORTS, body.replace(/\n/g, " ") + "\n");
            res.statusCode = 204;
            res.end();
          });
          return;
        }
        res.setHeader("Content-Security-Policy", header);
        next();
      });
    },
  };
}

export default defineConfig(({ mode }) => ({
  plugins: [screens(), ...(mode === "csp" ? [releaseCsp()] : [])],
  // Tauri expects a fixed port and no clearing of its log output.
  server: { port: 1420, strictPort: true, host: "127.0.0.1" },
  preview: { port: 1421, strictPort: true, host: "127.0.0.1" },
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
}));
