// Tabs and splits, driven through the workspace harness (fake transports).

import { expect, test, type Page } from "@playwright/test";

interface PaneBox {
  id: number;
  x: number;
  y: number;
  w: number;
  h: number;
}
interface State {
  tabs: number;
  active: number;
  focused: number;
  zoomed: number | null;
  panes: PaneBox[];
}

async function open(page: Page, extra: Record<string, string> = {}): Promise<void> {
  const q = new URLSearchParams({ w: "800", h: "500", platform: "mac", ...extra });
  await page.goto(`/src/dev/workspace.html?${q}`);
  await settle(page);
}

/** Wait until every pane's view exists and two frames have painted. */
async function settle(page: Page): Promise<void> {
  await page.waitForFunction(() => (window as any).__ws?.ready() === true);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

const state = (page: Page): Promise<State> => page.evaluate(() => (window as any).__ws.state());

/** Text typed into each pane, by transport number. */
const typed = (page: Page): Promise<Record<number, string>> =>
  page.evaluate(() => {
    const out: Record<number, string> = {};
    for (const { n, e } of (window as any).__ws.inputs) {
      if (e.kind === "key" && e.action === "press" && e.text) out[n] = (out[n] ?? "") + e.text;
    }
    return out;
  });

async function press(page: Page, keys: string): Promise<void> {
  await page.keyboard.press(keys);
  await settle(page);
}

test.describe("splits", () => {
  test("split right and down, typing reaches only the focused pane", async ({ page }) => {
    await open(page);
    await page.keyboard.type("a");
    await press(page, "Meta+d");
    let s = await state(page);
    expect(s.panes.map((p) => p.id)).toEqual([1, 2]);
    expect(s.focused).toBe(2);
    expect(s.panes[0].w).toBeCloseTo(400, -1);
    expect(s.panes[1].x).toBeGreaterThan(s.panes[0].x + s.panes[0].w - 1);
    await page.keyboard.type("b");

    await press(page, "Meta+Shift+d");
    s = await state(page);
    expect(s.panes.map((p) => p.id)).toEqual([1, 2, 3]);
    const [, two, three] = s.panes;
    expect(three.x).toBe(two.x);
    expect(three.y).toBeGreaterThan(two.y + two.h - 1);
    await page.keyboard.type("c");

    expect(await typed(page)).toEqual({ 1: "a", 2: "b", 3: "c" });
  });

  test("each pane is sized on its own", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d");
    const sizes = await page.evaluate(() =>
      (window as any).__ws.transports.map((t: any) => t.resizes[t.resizes.length - 1]?.cols),
    );
    // Half the width each, so well under the single pane's columns.
    expect(sizes[0]).toBeGreaterThan(30);
    expect(sizes[0]).toBeLessThan(55);
    expect(Math.abs(sizes[1] - sizes[0])).toBeLessThanOrEqual(1);
  });

  test("focus moves by direction, by cycling and by clicking", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d");
    await press(page, "Meta+Shift+d"); // 1 | (2 over 3), 3 focused
    await press(page, "Meta+Alt+ArrowLeft");
    expect((await state(page)).focused).toBe(1);
    await press(page, "Meta+Alt+ArrowRight");
    expect((await state(page)).focused).toBe(2);
    await press(page, "Meta+Alt+ArrowDown");
    expect((await state(page)).focused).toBe(3);
    await press(page, "Meta+BracketRight");
    expect((await state(page)).focused).toBe(1);
    await press(page, "Meta+BracketLeft");
    expect((await state(page)).focused).toBe(3);

    const s = await state(page);
    const two = s.panes.find((p) => p.id === 2)!;
    await page.mouse.click(two.x + 40, two.y + 40);
    await settle(page);
    expect((await state(page)).focused).toBe(2);
    await page.keyboard.type("x");
    expect(await typed(page)).toEqual({ 2: "x" });
  });

  test("closing gives the space back; the last pane quits", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d");
    await press(page, "Meta+w");
    let s = await state(page);
    expect(s.panes.map((p) => p.id)).toEqual([1]);
    expect(s.panes[0].w).toBeCloseTo(800, -1);
    expect(s.focused).toBe(1);
    expect(await page.evaluate(() => (window as any).__ws.transports[1].closed)).toBe(true);

    // A program exiting closes its pane too.
    await press(page, "Meta+Shift+d");
    await page.evaluate(() => (window as any).__ws.transports[2].exit(0));
    await settle(page);
    s = await state(page);
    expect(s.panes.map((p) => p.id)).toEqual([1]);

    await press(page, "Meta+w");
    expect(await page.evaluate(() => (window as any).__ws.quit)).toBe(true);
  });

  test("dragging a divider resizes; shortcuts resize and equalize", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d");
    const div = (await page.locator(".tab-stage:not([style*='none']) .divider").boundingBox())!;
    await page.mouse.move(div.x + div.width / 2, 250);
    await page.mouse.down();
    await page.mouse.move(200, 250, { steps: 4 });
    await page.mouse.up();
    await settle(page);
    let s = await state(page);
    expect(s.panes[0].w).toBeCloseTo(200, -1);

    await press(page, "Meta+Alt+ArrowLeft"); // focus pane 1
    await press(page, "Meta+Control+ArrowRight");
    s = await state(page);
    expect(s.panes[0].w).toBeCloseTo(210, -1);

    await press(page, "Meta+Control+Equal");
    s = await state(page);
    expect(s.panes[0].w).toBeCloseTo(400, -1);
  });

  test("zoom fills the tab and comes back", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d");
    await press(page, "Meta+Shift+Enter");
    let s = await state(page);
    expect(s.zoomed).toBe(2);
    expect(s.panes.map((p) => p.id)).toEqual([2]);
    expect(s.panes[0].w).toBeCloseTo(800, -1);
    await press(page, "Meta+Shift+Enter");
    s = await state(page);
    expect(s.zoomed).toBeNull();
    expect(s.panes).toHaveLength(2);
  });

  test("Linux and Windows shortcuts", async ({ page }) => {
    await open(page, { platform: "other" });
    await press(page, "Control+Shift+o");
    await press(page, "Control+Shift+e");
    expect((await state(page)).panes).toHaveLength(3);
    await press(page, "Control+Alt+ArrowLeft");
    expect((await state(page)).focused).toBe(1);
    await press(page, "Control+Shift+t");
    expect((await state(page)).tabs).toBe(2);
  });

  test("three panes look right", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d");
    await press(page, "Meta+Shift+d");
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-three-panes.png");
  });
});

test.describe("tabs", () => {
  test("the tab bar appears with a second tab and switches tabs", async ({ page }) => {
    await open(page);
    await expect(page.locator(".tab-bar")).toBeHidden();
    await press(page, "Meta+t");
    await expect(page.locator(".tab-bar")).toBeVisible();
    let s = await state(page);
    expect(s.tabs).toBe(2);
    expect(s.active).toBe(1);
    // New tabs start next to the pane that was focused (its directory).
    expect(await page.evaluate(() => (window as any).__ws.transports.length)).toBe(2);

    await page.keyboard.type("t");
    await press(page, "Meta+1");
    expect((await state(page)).active).toBe(0);
    await page.keyboard.type("o");
    expect(await typed(page)).toEqual({ 1: "o", 2: "t" });

    await press(page, "Meta+Shift+BracketRight");
    expect((await state(page)).active).toBe(1);
    await press(page, "Control+Tab");
    expect((await state(page)).active).toBe(0);

    await page.locator(".tab").nth(1).click();
    await settle(page);
    expect((await state(page)).active).toBe(1);

    await press(page, "Meta+w");
    s = await state(page);
    expect(s.tabs).toBe(1);
    await expect(page.locator(".tab-bar")).toBeHidden();
  });

  test("tabs and the window show the focused pane's title", async ({ page }) => {
    await open(page);
    await press(page, "Meta+t");
    await page.evaluate(() => {
      const t = (window as any).__ws.transports;
      t[0].title = "vim";
      t[0].requestFull();
      t[1].title = "zsh";
      t[1].requestFull();
    });
    await settle(page);
    await expect(page.locator(".tab-title")).toHaveText(["vim", "zsh"]);
    const titles = await page.evaluate(() => (window as any).__ws.titles);
    expect(titles[titles.length - 1]).toBe("zsh");
    await press(page, "Meta+1");
    const after = await page.evaluate(() => (window as any).__ws.titles);
    expect(after[after.length - 1]).toBe("vim");
  });
});
