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
  const root = fileURLToPath(new URL(".", import.meta.url));
  const offending = (re: RegExp) => sources(root).filter((f) => re.test(readFileSync(f, "utf8")));

  it("isn't broken by a <style> element made at run time", () => {
    expect(offending(/createElement\(\s*["'`]style["'`]\s*\)/)).toEqual([]);
  });

  // The nonce drops inline style attributes too: in markup set as HTML, and
  // set with setAttribute. `el.style.x = …` (CSSOM) is allowed.
  it("isn't broken by style attributes or <style> in markup", () => {
    expect(offending(/setAttribute\(\s*["'`]style["'`]/)).toEqual([]);
    expect(offending(/["'`][^"'`\n]*(<style\b|\sstyle=)/)).toEqual([]);
  });
});
