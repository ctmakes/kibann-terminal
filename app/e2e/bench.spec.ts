import { expect, test } from "@playwright/test";
import { openFixture } from "./harness";

// Paint-time budget from plans/done/renderer.md: full 240×70 repaint < 6 ms and a
// 3-row partial < 1 ms on an M-series Mac. Only runs with BENCH=1, since
// timings on shared CI machines are noise.
test.skip(!process.env.BENCH, "set BENCH=1 to run");

test("paint budget at 240x70", async ({ page }, info) => {
  // At 14px JetBrains Mono a cell is ~8.4×18.5 CSS px.
  await page.setViewportSize({ width: 2100, height: 1350 });
  // 16 colors: a realistic palette. The default 256 is a worst case.
  await openFixture(page, "stress", { w: 2060, h: 1310, colors: "16" });
  const r = await page.evaluate(() => (window as any).__harness.bench(60));
  console.log(`[${info.project.name}] ${r.cols}x${r.rows}: full ${r.fullMs.toFixed(2)} ms, 3 rows ${r.rowsMs.toFixed(2)} ms`);
  expect(r.cols).toBeGreaterThanOrEqual(240);
  expect(r.rows).toBeGreaterThanOrEqual(70);
  expect.soft(r.fullMs).toBeLessThan(6);
  expect.soft(r.rowsMs).toBeLessThan(1);
});
