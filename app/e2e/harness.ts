import type { Page } from "@playwright/test";

export interface HarnessMetrics {
  dpr: number;
  cellW: number;
  cellH: number;
  baseline: number;
  lineW: number;
  pad: number;
  cols: number;
  rows: number;
}

/** Open the harness on a fixture and wait for its first paint. */
export async function openFixture(page: Page, fixture: string, extra: Record<string, string | number> = {}): Promise<void> {
  const q = new URLSearchParams({ fixture, bare: "1", w: "800", h: "480", still: "1", ...Object.fromEntries(Object.entries(extra).map(([k, v]) => [k, String(v)])) });
  await page.goto(`/src/dev/harness.html?${q}`);
  await page.waitForFunction(() => (window as any).__harness?.ready === true);
  // Let a pending rAF paint settle.
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

export function metrics(page: Page): Promise<HarnessMetrics> {
  return page.evaluate(() => (window as any).__harness.metrics());
}
