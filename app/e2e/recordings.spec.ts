// Frames from the real emulator (crates/term/recordings, replayed by vt-replay)
// drawn by the renderer: the backend's frame builder and the painter together.

import { expect, test, type Page } from "@playwright/test";

async function openRecording(page: Page, name: string, params: Record<string, string>): Promise<void> {
  const q = new URLSearchParams({ replay: name, bare: "1", ...params });
  await page.goto(`/src/dev/harness.html?${q}`);
  await page.waitForFunction(() => (window as any).__harness?.ready === true);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

/** The grid as text, one string per row, without trailing blank rows. */
function gridText(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const g = (window as any).__harness.view.grid;
    const out: string[] = [];
    for (let y = 0; y < g.rows; y++) {
      let s = "";
      for (let x = 0; x < g.cols; x++) {
        const c = g.cellAt(x, y);
        if (c.width === 0) continue;
        s += c.text || " ";
      }
      out.push(s.trimEnd());
    }
    // A local resize can pad the fixed-size recording with blank rows.
    while (out.length && !out[out.length - 1]) out.pop();
    return out;
  });
}

// Sizes hold the recorded grid at 14px: about 8.4 × 17 CSS px per cell.
const SHOTS: Array<[string, string, Record<string, string>]> = [
  ["vim", "end", { w: "860", h: "560" }],
  ["vim", "1000", { w: "860", h: "560" }],
  ["man", "end", { w: "780", h: "520" }],
  ["sync", "end", { w: "350", h: "150" }],
];

for (const [name, at, size] of SHOTS) {
  test(`recording ${name} at ${at}`, async ({ page }) => {
    await openRecording(page, name, { at, ...size });
    const h = await page.evaluate(() => {
      const t = (window as any).__harness.transport;
      return { fullRequests: t.fullRequests, sent: t.next };
    });
    expect(h.fullRequests).toBe(0);
    expect(h.sent).toBeGreaterThan(1);
    await expect(page.locator("#term")).toHaveScreenshot(`recording-${name}-${at}.png`);
  });
}

test("real-time playback ends on the same screen as jumping to the end", async ({ page }) => {
  await openRecording(page, "sync", { at: "end", w: "350", h: "150" });
  const expected = await gridText(page);
  expect(expected[1]).toContain("frame 6 of 6");

  await openRecording(page, "sync", { speed: "4", w: "350", h: "150" });
  // While playing, a held frame is never shown half drawn.
  await page.waitForFunction(() => {
    const h = (window as any).__harness;
    const g = h.view.grid;
    const row = (y: number) => {
      let s = "";
      for (let x = 0; x < g.cols; x++) s += g.cellAt(x, y).text || " ";
      return s;
    };
    const top = row(0).startsWith("┌");
    const bottom = row(3).startsWith("└");
    if (top !== bottom) (window as any).__tornFrame = true;
    return h.transport.done && h.paints > 0 && row(1).includes("frame 6");
  }, undefined, { polling: "raf", timeout: 10_000 });
  expect(await page.evaluate(() => (window as any).__tornFrame ?? false)).toBe(false);
  expect(await gridText(page)).toEqual(expected);
});
