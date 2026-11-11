import { expect, test, type Page } from "@playwright/test";
import { metrics, openFixture, type HarnessMetrics } from "./harness";

const WHITE = 0xffffff;

function line(page: Page, x: number, y: number, len: number, vertical: boolean): Promise<number[]> {
  return page.evaluate(([x, y, len, v]) => (window as any).__harness.line(x, y, len, v), [x, y, len, vertical] as const);
}

/** Pixel rows (or columns) inside a cell that are fully white at the cell's middle. */
async function strokeOffsets(page: Page, m: HarnessMetrics, col: number, row: number, vertical: boolean): Promise<number[]> {
  const x0 = col * m.cellW;
  const y0 = row * m.cellH;
  const px = vertical
    ? await line(page, x0, y0 + Math.floor(m.cellH / 2), m.cellW, false)
    : await line(page, x0 + Math.floor(m.cellW / 2), y0, m.cellH, true);
  return px.flatMap((v, i) => (v === WHITE ? [i] : []));
}

test.describe("box drawing joins", () => {
  let m: HarnessMetrics;

  test.beforeEach(async ({ page }) => {
    await openFixture(page, "boxjoin");
    m = await metrics(page);
  });

  for (const [name, row, strokes] of [["light", 0, 1], ["heavy", 2, 1], ["double", 4, 2]] as const) {
    test(`horizontal ${name} line is continuous across every cell`, async ({ page }) => {
      const ys = await strokeOffsets(page, m, 0, row, false);
      expect(ys.length).toBe(strokes === 2 ? 2 * m.lineW : name === "heavy" ? 2 * m.lineW : m.lineW);
      for (const y of ys) {
        const px = await line(page, 0, row * m.cellH + y, m.cols * m.cellW, false);
        expect(px.every((v) => v === WHITE), `row offset ${y}`).toBe(true);
      }
    });
  }

  for (const [name, col] of [["light", 0], ["heavy", 2], ["double", 4]] as const) {
    test(`vertical ${name} line is continuous across every row`, async ({ page }) => {
      const xs = await strokeOffsets(page, m, col, 8, true);
      expect(xs.length).toBeGreaterThan(0);
      for (const x of xs) {
        const px = await line(page, col * m.cellW + x, 8 * m.cellH, (m.rows - 8) * m.cellH, true);
        expect(px.every((v) => v === WHITE), `col offset ${x}`).toBe(true);
      }
    });
  }

  test("full blocks tile with no seams", async ({ page }) => {
    for (let dy = 0; dy < m.cellH; dy++) {
      const px = await line(page, 0, 6 * m.cellH + dy, m.cols * m.cellW, false);
      expect(px.every((v) => v === WHITE), `row ${dy}`).toBe(true);
    }
    for (let dx = 0; dx < m.cellW; dx++) {
      const px = await line(page, 6 * m.cellW + dx, 8 * m.cellH, (m.rows - 8) * m.cellH, true);
      expect(px.every((v) => v === WHITE), `col ${dx}`).toBe(true);
    }
  });

  for (const [name, firstCol] of [["┼", 8], ["╬", 20]] as const) {
    test(`${name} cells have matching edges with their neighbors`, async ({ page }) => {
      const row = 9;
      for (let c = firstCol; c < firstCol + 9; c++) {
        const right = await line(page, (c + 1) * m.cellW - 1, row * m.cellH, m.cellH, true);
        const left = await line(page, (c + 1) * m.cellW, row * m.cellH, m.cellH, true);
        expect(left, `between columns ${c} and ${c + 1}`).toEqual(right);
        expect(left.some((v) => v === WHITE)).toBe(true);
      }
      const bottom = await line(page, firstCol * m.cellW, (row + 1) * m.cellH - 1, m.cellW, false);
      const top = await line(page, firstCol * m.cellW, (row + 1) * m.cellH, m.cellW, false);
      expect(top).toEqual(bottom);
    });
  }
});
