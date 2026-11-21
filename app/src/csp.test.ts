import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/** The app's source files, without tests and the browser harnesses. */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return f === "dev" ? [] : sources(p);
    return /\.ts$/.test(f) && !/\.test\.ts$/.test(f) ? [p] : [];
  });
}

// Release builds serve the page with a CSP that Tauri adds nonces to, and a
// nonce makes WebKit ignore 'unsafe-inline': a <style> element made at run
// time is dropped there (while dev builds, with no CSP, show it fine). 0.7.0
// shipped with the screen's layout rules in one, and the window lost its
// terminals. Rules go in a .css file the module imports.
describe("the release CSP", () => {
  it("isn't broken by a <style> element made at run time", () => {
    const root = fileURLToPath(new URL(".", import.meta.url));
    const found = sources(root).filter((f) => /createElement\(\s*["'`]style["'`]\s*\)/.test(readFileSync(f, "utf8")));
    expect(found).toEqual([]);
  });
});
