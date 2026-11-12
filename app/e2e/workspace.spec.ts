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

    // The menu item firing too, before or after the key, zooms only once.
    const menu = () => page.evaluate(() => (window as any).__ws.ws.runFromMenu({ kind: "zoom" }));
    await menu();
    await press(page, "Meta+Shift+Enter");
    expect((await state(page)).zoomed).toBe(2);
    await press(page, "Meta+Shift+Enter");
    await menu();
    await settle(page);
    expect((await state(page)).zoomed).toBeNull();
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

test.describe("links", () => {
  /** CSS position of the middle of cell (`x`, `y`) in the first pane. */
  const cell = (page: Page, x: number, y: number) =>
    page.evaluate(
      ([x, y]) => {
        const v = (window as any).__ws.ws.panes.get(1).view;
        const r = v.gridEl.getBoundingClientRect();
        return { x: r.x + ((x + 0.5) * v.m.cellW) / v.m.dpr, y: r.y + ((y + 0.5) * v.m.cellH) / v.m.dpr };
      },
      [x, y],
    );

  const underlines = (page: Page) => page.locator(".term-link-underline");
  const preview = (page: Page) => page.locator(".term-link-preview");

  async function hoverAt(page: Page, x: number, y: number): Promise<void> {
    const p = await cell(page, x, y);
    await page.mouse.move(p.x, p.y);
    await settle(page);
  }

  test("Cmd shows the link under the pointer; releasing it hides the link", async ({ page }) => {
    await open(page, { links: "1" });
    await hoverAt(page, 10, 2);
    await expect(underlines(page)).toHaveCount(0);

    await page.keyboard.down("Meta");
    await hoverAt(page, 11, 2);
    await expect(underlines(page)).toHaveCount(2);
    await expect(preview(page)).toHaveText("https://example.com/very/long/path");
    expect(await page.locator(".term-overlay").evaluate((e) => getComputedStyle(e).cursor)).toBe("pointer");

    // Off the link, then back on: the second row of the link counts too.
    await hoverAt(page, 11, 3);
    await expect(underlines(page)).toHaveCount(0);
    await hoverAt(page, 2, 3);
    await expect(underlines(page)).toHaveCount(2);

    // Releasing Cmd with the pointer still.
    await page.keyboard.up("Meta");
    await settle(page);
    await expect(underlines(page)).toHaveCount(0);
    await expect(preview(page)).toBeHidden();
    expect(await page.locator(".term-overlay").evaluate((e) => getComputedStyle(e).cursor)).toBe("text");

    // Pressing Cmd with the pointer still.
    await page.keyboard.down("Meta");
    await settle(page);
    await expect(underlines(page)).toHaveCount(2);
    await page.keyboard.up("Meta");
  });

  test("Cmd+click opens the link and doesn't reach the terminal", async ({ page }) => {
    await open(page, { links: "1" });
    const presses = () =>
      page.evaluate(() => (window as any).__ws.inputs.filter(({ e }: any) => e.kind === "mouse" && e.action !== "motion").length);

    // A plain click is a click.
    const p = await cell(page, 11, 2);
    await page.mouse.click(p.x, p.y);
    await settle(page);
    expect(await page.evaluate(() => (window as any).__ws.opened)).toEqual([]);
    expect(await presses()).toBe(2);

    await page.keyboard.down("Meta");
    await hoverAt(page, 12, 2);
    await page.mouse.down();
    await page.mouse.up();
    await page.keyboard.up("Meta");
    await settle(page);
    expect(await page.evaluate(() => (window as any).__ws.opened)).toEqual(["https://example.com/very/long/path"]);
    expect(await presses()).toBe(2);
  });

  test("Ctrl on Linux and Windows", async ({ page }) => {
    await open(page, { links: "1", platform: "other" });
    await page.keyboard.down("Control");
    await hoverAt(page, 11, 2);
    await expect(underlines(page)).toHaveCount(2);
    await page.mouse.down();
    await page.mouse.up();
    await page.keyboard.up("Control");
    expect(await page.evaluate(() => (window as any).__ws.opened)).toEqual(["https://example.com/very/long/path"]);
  });

  test("a hovered wrapped link looks right", async ({ page }) => {
    await open(page, { links: "1", w: "400", h: "160" });
    await page.keyboard.down("Meta");
    await hoverAt(page, 11, 2);
    await expect(underlines(page)).toHaveCount(2);
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-link-hover.png");
    await page.keyboard.up("Meta");
  });
});

test.describe("agents", () => {
  test("a pane an agent opens appears without taking focus", async ({ page }) => {
    await open(page);
    await page.keyboard.type("a");
    // As `kibann pane split --direction down` from pane 1 would.
    const made = await page.evaluate(() =>
      (window as any).__ws.agent({ t: "split", pane: 1, dir: "column", focus: false }),
    );
    expect(made.pane).toBe(2);
    await settle(page);
    let s = await state(page);
    expect(s.panes.map((p) => p.id)).toEqual([1, 2]);
    expect(s.panes[1].y).toBeGreaterThan(s.panes[0].y);
    expect(s.focused).toBe(1);
    await page.keyboard.type("b");
    expect(await typed(page)).toEqual({ 1: "ab" });

    // A new tab in the background leaves the tab shown.
    await page.evaluate(() => (window as any).__ws.agent({ t: "new_tab", near: 2, focus: false }));
    await settle(page);
    s = await state(page);
    expect(s.tabs).toBe(2);
    expect(s.active).toBe(0);

    // Asked to, it focuses: the tab switches and typing goes there.
    await page.evaluate(() => (window as any).__ws.agent({ t: "focus", pane: 3 }));
    await settle(page);
    s = await state(page);
    expect(s.active).toBe(1);
    expect(s.focused).toBe(3);
    await page.keyboard.type("c");
    expect(await typed(page)).toEqual({ 1: "ab", 3: "c" });

    // An agent closing a pane gives its space back.
    await page.evaluate(() => (window as any).__ws.agent({ t: "close", pane: 3 }));
    await settle(page);
    s = await state(page);
    expect(s.tabs).toBe(1);
    expect(s.focused).toBe(1);
  });
});

test.describe("restore", () => {
  test("a saved layout comes back the same", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d");
    await press(page, "Meta+Shift+d"); // 1 | (2 over 3)
    await press(page, "Meta+Control+ArrowUp"); // pane 3 grows
    await press(page, "Meta+t");
    await press(page, "Meta+d");
    await press(page, "Meta+Shift+Enter"); // tab 2 zoomed on its second pane
    await press(page, "Meta+1");
    await press(page, "Meta+Alt+ArrowLeft"); // focus pane 1

    const before = await state(page);
    const saved = await page.evaluate(() => (window as any).__ws.snapshot());
    expect(saved.tabs).toHaveLength(2);
    expect(saved.active).toBe(saved.tabs[0].id);
    expect(saved.tabs[1].zoomed).toBe(saved.tabs[1].focused);

    await open(page, { saved: JSON.stringify(saved) });
    const after = await state(page);
    const shape = (s: State) => ({
      tabs: s.tabs,
      active: s.active,
      focusedAt: s.panes.findIndex((p) => p.id === s.focused),
      rects: s.panes.map(({ x, y, w, h }) => ({ x, y, w, h })),
    });
    expect(shape(after)).toEqual(shape(before));
    await press(page, "Meta+2");
    const tab2 = await state(page);
    expect(tab2.zoomed).not.toBeNull();
    expect(tab2.panes).toHaveLength(1);
  });
});

test.describe("agent status", () => {
  type Dots = { panes: Record<number, string | null>; tabs: (string | null)[] };
  const dots = (page: Page): Promise<Dots> => page.evaluate(() => (window as any).__ws.dots());
  async function agent(page: Page, pane: number, s: string | null): Promise<void> {
    await page.evaluate(([p, st]) => (window as any).__ws.setAgent(p, st), [pane, s] as const);
    await settle(page);
  }
  const badge = (page: Page): Promise<number> => page.evaluate(() => (window as any).__ws.badge);

  test("dots on panes, the tab roll-up, done clearing on focus, the badge", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d"); // pane 2 beside 1, focused
    await agent(page, 1, "working");
    let d = await dots(page);
    expect(d.panes).toEqual({ 1: "working", 2: null });
    expect(await page.locator(".tab-bar").isVisible()).toBe(false); // one tab: no bar

    // Pane 1 finishes while pane 2 has focus: done, until focused.
    await agent(page, 1, "idle");
    expect((await dots(page)).panes[1]).toBe("done");
    await press(page, "Meta+[");
    expect((await state(page)).focused).toBe(1);
    expect((await dots(page)).panes[1]).toBe(null);

    // Finishing while seen isn't done.
    await agent(page, 1, "working");
    await agent(page, 1, "idle");
    expect((await dots(page)).panes[1]).toBe(null);

    // Tabs roll up their panes: blocked beats done beats working.
    await press(page, "Meta+t"); // tab 2 with pane 3, shown
    await agent(page, 1, "working");
    await agent(page, 2, "working");
    d = await dots(page);
    expect(d.tabs).toEqual(["working", null]);
    await agent(page, 2, "idle"); // unseen: done
    expect((await dots(page)).tabs[0]).toBe("done");
    await agent(page, 1, "blocked");
    expect((await dots(page)).tabs[0]).toBe("blocked");
    expect(await badge(page)).toBe(1);
    await agent(page, 3, "blocked");
    expect(await badge(page)).toBe(2);
    expect((await dots(page)).tabs).toEqual(["blocked", "blocked"]);
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-agent-dots.png");

    await agent(page, 1, "working");
    await agent(page, 3, null);
    expect(await badge(page)).toBe(0);
    d = await dots(page);
    expect(d.tabs).toEqual(["done", null]);

    // Going back to the tab shows pane 1 (focused there); pane 2 stays
    // done until it is focused.
    await press(page, "Meta+1");
    expect((await dots(page)).panes[2]).toBe("done");
    await press(page, "Meta+]");
    expect((await state(page)).focused).toBe(2);
    d = await dots(page);
    expect(d.panes).toEqual({ 1: "working", 2: null, 3: null });
    expect(d.tabs[0]).toBe("working");
  });

  test("a pane's dot looks right", async ({ page }) => {
    await open(page, { w: "400", h: "200" });
    await agent(page, 1, "blocked");
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-pane-dot.png");
  });
});
