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

/** The collapsed sidebar's width (it's never hidden). */
const RAIL = 44;

/** Open the harness; `w` is the width beside the sidebar's rail, so pane
 * sizes read the same as without it. */
async function open(page: Page, extra: Record<string, string> = {}): Promise<void> {
  const o = { w: "800", h: "500", platform: "mac", ...extra };
  const q = new URLSearchParams({ ...o, w: String(Number(o.w) + RAIL) });
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
    await page.mouse.move(RAIL + 200, 250, { steps: 4 });
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

  test("double-clicking a pane's header zooms it, and again comes back", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d");
    // The left pane's header: it zooms that one, not the focused one.
    await page.locator('.pane[data-pane="1"] .pane-header').dblclick();
    await settle(page);
    let s = await state(page);
    expect(s.zoomed).toBe(1);
    expect(s.focused).toBe(1);
    await page.locator('.pane[data-pane="1"] .pane-header').dblclick();
    await settle(page);
    s = await state(page);
    expect(s.zoomed).toBeNull();
    expect(s.panes).toHaveLength(2);
    // One pane: nothing to zoom.
    await press(page, "Meta+w");
    await page.locator(".pane-header").first().dblclick();
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
    // Each tab shows its number, the one Cmd+N goes to.
    await expect(page.locator(".tab-number")).toHaveText(["1", "2"]);
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

  test("the menu bar's waiting item shows the waiting agent's pane", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d"); // panes 1 and 2
    await press(page, "Meta+t"); // tab 2: pane 3, shown
    expect(await page.evaluate(() => (window as any).__ws.showWaiting())).toBe(false);
    await agent(page, 2, "blocked");
    expect(await page.evaluate(() => (window as any).__ws.showWaiting())).toBe(true);
    await settle(page);
    const s = await state(page);
    expect(s.panes.map((p: { id: number }) => p.id)).toEqual([1, 2]);
    expect(s.focused).toBe(2);
  });

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

test.describe("pane header", () => {
  const git = (g: Record<string, unknown>) => ({
    root: "/r/kibann-terminal",
    repo: "kibann-terminal",
    worktree: null,
    branch: "feat/term",
    detached: null,
    upstream: "origin/feat/term",
    ahead: 0,
    behind: 0,
    changed: 0,
    conflicts: 0,
    operation: null,
    ...g,
  });
  async function place(page: Page, pane: number, p: unknown): Promise<void> {
    await page.evaluate(([id, pl]) => (window as any).__ws.setPlace(id, pl), [pane, p] as const);
    await settle(page);
  }
  const headers = (page: Page) => page.evaluate(() => (window as any).__ws.headers());

  test("repo, worktree, branch, counts and plain directories", async ({ page }) => {
    await open(page);
    await press(page, "Meta+d");
    await press(page, "Meta+Shift+d");
    await place(page, 1, { dir: "/r/kibann-terminal", shown: "~/kibann-terminal", git: git({ changed: 5, ahead: 2, behind: 1 }) });
    await place(page, 2, {
      dir: "/w/fix-links",
      shown: "/w/fix-links",
      git: git({ worktree: "fix-links", branch: "fix/links", changed: 1, operation: "rebase", conflicts: 1 }),
    });
    await place(page, 3, { dir: "/Users/x/Downloads", shown: "~/Downloads", git: null });
    const h = await headers(page);
    expect(h.panes).toEqual({
      1: "kibann-terminal ⎇ feat/term ●5 ↑2 ↓1",
      2: "kibann-terminal › fix-links ⎇ fix/links REBASING !1 ●1",
      3: "~/Downloads",
    });
    // The terminal sits below the header: every pane keeps its size when
    // its place changes.
    const before = await state(page);
    await place(page, 3, { dir: "/r/kibann-terminal", shown: "~", git: git({}) });
    expect((await state(page)).panes).toEqual(before.panes);
    await agentDot(page, 3);
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-pane-headers.png");
  });

  test("tabs show the focused pane's branch", async ({ page }) => {
    await open(page);
    await press(page, "Meta+t");
    await place(page, 1, { dir: "/r", shown: "/r", git: git({ branch: null, detached: "9f12549" }) });
    await place(page, 2, { dir: "/r", shown: "/r", git: git({}) });
    const tabs = (await headers(page)).tabs as string[];
    expect(tabs[0]).toMatch(/ · 9f12549$/);
    expect(tabs[1]).toMatch(/ · feat\/term$/);
  });

  test("a narrow pane gives way at the branch first", async ({ page }) => {
    await open(page, { w: "260", h: "120" });
    await place(page, 1, {
      dir: "/r",
      shown: "/r",
      git: git({ branch: "feature/a-very-long-branch-name-indeed", changed: 12, ahead: 3 }),
    });
    const w = await page.evaluate(() => {
      const r = (s: string) =>
        document.querySelector(`.pane-header ${s}`)!.getBoundingClientRect().width;
      return { changed: r(".pane-changed"), sync: r(".pane-sync"), branch: r(".pane-branch") };
    });
    expect(w.changed).toBeGreaterThan(10);
    expect(w.sync).toBeGreaterThan(10);
    expect(w.branch).toBeLessThan(150);
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-pane-header-narrow.png");
  });

  async function agentDot(page: Page, pane: number): Promise<void> {
    await page.evaluate((p) => (window as any).__ws.setAgent(p, "working"), pane);
    await settle(page);
  }
});

test.describe("sidebar", () => {
  const repo = (name: string, g: Record<string, unknown> = {}) => ({
    dir: `/r/${name}`,
    shown: `~/${name}`,
    git: {
      root: `/r/${name}`,
      repo: name,
      worktree: null,
      branch: "main",
      detached: null,
      upstream: null,
      ahead: 0,
      behind: 0,
      changed: 0,
      conflicts: 0,
      operation: null,
      ...g,
    },
  });
  async function place(page: Page, pane: number, p: unknown): Promise<void> {
    await page.evaluate(([id, pl]) => (window as any).__ws.setPlace(id, pl), [pane, p] as const);
    await settle(page);
  }
  const sidebar = (page: Page) => page.evaluate(() => (window as any).__ws.sidebar());

  /** Three tabs: 1|2, 3, 4. */
  async function three(page: Page): Promise<void> {
    await open(page, { w: "900", h: "560" });
    await press(page, "Meta+d");
    await press(page, "Meta+t");
    await press(page, "Meta+t");
    await place(page, 1, repo("alpha", { changed: 3 }));
    await place(page, 2, { dir: "/Users/x", shown: "~", git: null });
    await place(page, 3, repo("beta", { branch: "feature/x", ahead: 1 }));
    await place(page, 4, repo("alpha", { worktree: "alpha-wt", branch: "fix" }));
  }

  test("Cmd+Shift+B and its button expand and collapse it; it's never gone", async ({ page }) => {
    await three(page);
    const side = page.locator(".sidebar");
    // Collapsed: a rail, one badge per pane of the tab's ones too.
    expect((await sidebar(page)).expanded).toBe(false);
    expect((await side.boundingBox())!.width).toBeLessThan(50);
    expect(await page.locator(".sidebar-badge:visible").allTextContents()).toHaveLength(4);
    const before = (await state(page)).panes[0].w;
    await press(page, "Meta+Shift+b");
    expect((await sidebar(page)).expanded).toBe(true);
    const after = (await state(page)).panes[0].w;
    expect(before - after).toBeGreaterThan(150);
    await expect(page.locator(".sidebar-badge:visible")).toHaveCount(0);
    await press(page, "Meta+Shift+b");
    expect((await sidebar(page)).expanded).toBe(false);
    await expect(side).toBeVisible();

    // The button at its bottom does the same, and says so with its key.
    const fold = page.locator(".sidebar-fold");
    expect((await fold.boundingBox())!.y).toBeGreaterThan(500);
    await fold.hover();
    await expect(page.locator(".tooltip")).toHaveText("Expand sidebar⇧⌘B");
    await fold.click();
    expect((await sidebar(page)).expanded).toBe(true);
    await page.mouse.move(400, 300);
    await fold.hover();
    await expect(page.locator(".tooltip")).toHaveText("Collapse sidebar⇧⌘B");
    // Above the button, since the bar is at the bottom.
    expect((await page.locator(".tooltip").boundingBox())!.y).toBeLessThan((await fold.boundingBox())!.y);
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-sidebar-button.png");
    // Focus stays in the pane.
    await page.locator(".sidebar-fold").click();
    expect((await sidebar(page)).expanded).toBe(false);
    await page.keyboard.type("x");
    expect((await typed(page))[4]).toBe("x");
  });

  test("each row shows its tab's number, with more than one tab", async ({ page }) => {
    await three(page);
    await press(page, "Meta+Shift+b");
    const nums = () =>
      page.locator(".sidebar-card").evaluateAll((els) =>
        els.map((e) => {
          const n = e.querySelector<HTMLElement>(".sidebar-tab")!;
          return `${(e as HTMLElement).dataset.pane}:${n.style.display === "none" ? "-" : n.textContent}`;
        }),
      );
    // Grouped by repo (alpha: 1 and 4, then ~, then beta), each with its tab.
    expect(await nums()).toEqual(["1:1", "4:3", "2:1", "3:2"]);
    // On the rail, the badge's tooltip names the tab.
    await press(page, "Meta+Shift+b");
    expect(await page.locator('.sidebar-card[data-pane="3"]').getAttribute("title")).toContain("Tab 2");
    // Down to one tab: no numbers.
    await press(page, "Meta+Shift+b");
    await press(page, "Meta+w");
    await press(page, "Meta+w");
    await settle(page);
    expect((await nums()).every((n) => n.endsWith(":-"))).toBe(true);
    await expect(page.locator(".tab-bar")).toBeHidden();
  });

  test("its gear opens Settings, and says its key", async ({ page }) => {
    await three(page);
    await press(page, "Meta+Shift+b");
    const gear = page.locator(".sidebar-settings");
    await gear.hover();
    await expect(page.locator(".tooltip")).toHaveText("Settings⌘,");
    await gear.click();
    await expect(page.locator(".settings-modal")).toBeVisible();
    // Like the other extras, it isn't on the rail.
    await page.keyboard.press("Escape");
    await press(page, "Meta+Shift+b");
    await expect(gear).toBeHidden();
  });

  test("sidebar-preview = false: hovering shows no preview, and Settings turns it back on", async ({ page }) => {
    await three(page);
    await page.evaluate(() => (window as any).__ws.setSettings({ sidebar_preview: false }));
    await page.locator('.sidebar-card[data-pane="3"]').hover();
    await page.waitForTimeout(100);
    await expect(page.locator(".sidebar-preview")).toBeHidden();
    await press(page, "Meta+Comma");
    await page.locator('.settings-tab[data-tab="general"]').click();
    const sw = page.locator('.settings-switch[data-key="sidebar-preview"]');
    await expect(sw).toHaveAttribute("aria-checked", "false");
    await sw.click();
    expect(await page.evaluate(() => (window as any).__ws.saved)).toEqual([["sidebar-preview", ["true"]]]);
    await page.keyboard.press("Escape");
    await page.mouse.move(600, 300);
    await page.locator('.sidebar-card[data-pane="2"]').hover();
    await expect(page.locator(".sidebar-preview")).toBeVisible();
  });

  test("the rail: a badge shows its pane and previews it", async ({ page }) => {
    await three(page);
    await page.locator('.sidebar-card[data-pane="2"]').click();
    await settle(page);
    expect((await state(page)).focused).toBe(2);
    await page.locator('.sidebar-card[data-pane="3"]').hover();
    await expect(page.locator(".sidebar-preview")).toBeVisible();
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-sidebar-rail.png");
  });

  test("dragging the edge in folds it to the rail, and out opens it", async ({ page }) => {
    await three(page);
    await press(page, "Meta+Shift+b");
    const drag = async (to: number) => {
      const box = (await page.locator(".sidebar").boundingBox())!;
      await page.mouse.move(box.x + box.width - 1, box.y + 100);
      await page.mouse.down();
      await page.mouse.move(box.x + to, box.y + 100, { steps: 4 });
      await page.mouse.up();
      await settle(page);
    };
    await drag(40);
    expect((await sidebar(page)).expanded).toBe(false);
    await drag(250);
    const s = await sidebar(page);
    expect(s.expanded).toBe(true);
    expect(s.width).toBeGreaterThan(200);
  });

  test("every pane in every tab, grouped by repository", async ({ page }) => {
    await three(page);
    await press(page, "Meta+Shift+b");
    expect((await sidebar(page)).items).toEqual(["# alpha", 1, 4, "# ~", 2, "# beta", 3]);
    // An agent waiting for you brings its group to the top.
    await page.evaluate(() => (window as any).__ws.setAgent(3, "blocked"));
    await settle(page);
    expect((await sidebar(page)).items).toEqual(["# beta", 3, "# alpha", 1, 4, "# ~", 2]);
  });

  test("clicking a card shows its pane; ●N asks for the diff", async ({ page }) => {
    await three(page);
    await press(page, "Meta+Shift+b");
    await page.locator('.sidebar-card[data-pane="2"] .sidebar-title').click();
    await settle(page);
    let s = await state(page);
    expect(s.active).toBe(0);
    expect(s.focused).toBe(2);
    await page.keyboard.type("x");
    expect((await typed(page))[2]).toBe("x");

    await page.locator('.sidebar-card[data-pane="1"] .pane-changed').click();
    // And from the pane's own header.
    await page.locator('.pane[data-pane="2"]').waitFor();
    await page.locator('.pane[data-pane="1"] .pane-header .pane-changed').click();
    expect(await page.evaluate(() => (window as any).__ws.diffs)).toEqual([1, 1]);
    s = await state(page);
    expect(s.focused).toBe(2); // ●N doesn't move focus
  });

  test("dragging the edge resizes it, and it's remembered", async ({ page }) => {
    await three(page);
    await press(page, "Meta+Shift+b");
    const box = (await page.locator(".sidebar").boundingBox())!;
    await page.mouse.move(box.x + box.width - 1, box.y + 100);
    await page.mouse.down();
    await page.mouse.move(box.x + 320, box.y + 100, { steps: 4 });
    await page.mouse.up();
    await settle(page);
    expect((await sidebar(page)).width).toBeGreaterThan(300);
    await page.reload();
    await settle(page);
    const s = await sidebar(page);
    expect(s.expanded).toBe(true);
    expect(s.width).toBeGreaterThan(300);
  });

  test("rows are compact; hovering one previews its pane", async ({ page }) => {
    await three(page);
    await press(page, "Meta+Shift+b");
    const rows = await page.locator(".sidebar-card").evaluateAll((els) =>
      els.map((e) => e.getBoundingClientRect().height),
    );
    expect(rows.length).toBe(4);
    for (const h of rows) expect(h).toBeLessThanOrEqual(26);

    const preview = page.locator(".sidebar-preview");
    await expect(preview).toBeHidden();
    await page.locator('.sidebar-card[data-pane="3"]').hover();
    await expect(preview).toBeVisible();
    // Beside the sidebar, with the pane's text drawn in it.
    const side = (await page.locator(".sidebar").boundingBox())!;
    const box = (await preview.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(side.x + side.width);
    const lit = await preview.evaluate((c: HTMLCanvasElement) => {
      const d = c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data;
      let n = 0;
      for (let i = 0; i < d.length; i += 4) if (d[i + 3] > 0 && d[i] + d[i + 1] + d[i + 2] > 300) n++;
      return n;
    });
    expect(lit).toBeGreaterThan(50);
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-sidebar.png");

    // It follows the pointer, and goes away when the pointer leaves.
    await page.locator('.sidebar-card[data-pane="1"]').hover();
    const moved = (await preview.boundingBox())!;
    expect(moved.y).not.toBe(box.y);
    await page.mouse.move(side.x + side.width + 300, side.y + 300);
    await expect(preview).toBeHidden();
  });
});

test.describe("shortcut sheet", () => {
  const sheet = (page: Page) => page.evaluate(() => (window as any).__ws.shortcuts());

  test("Cmd+/ shows every shortcut; a key closes it and goes nowhere", async ({ page }) => {
    await open(page, { w: "900", h: "560" });
    expect((await sheet(page)).visible).toBe(false);
    await press(page, "Meta+/");
    const s = await sheet(page);
    expect(s.visible).toBe(true);
    expect(s.rows).toContain("Sidebar: ⇧⌘B");
    expect(s.rows).toContain("Pane in a direction: ⌥⌘ Arrows");
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-shortcuts.png");

    // Esc closes it without reaching the terminal.
    await press(page, "Escape");
    expect((await sheet(page)).visible).toBe(false);
    expect((await typed(page))[1] ?? "").toBe("");

    // Cmd+/ again toggles; a click anywhere closes.
    await press(page, "Meta+/");
    await press(page, "Meta+/");
    expect((await sheet(page)).visible).toBe(false);
    await press(page, "Meta+/");
    await page.mouse.click(450, 280);
    expect((await sheet(page)).visible).toBe(false);

    // A shortcut pressed while it's open closes it and runs.
    await press(page, "Meta+/");
    await press(page, "Meta+Shift+b");
    expect((await sheet(page)).visible).toBe(false);
    expect((await page.evaluate(() => (window as any).__ws.sidebar())).expanded).toBe(true);
  });
});

test.describe("themes", () => {
  const theme = (page: Page) => page.evaluate(() => (window as any).__ws.theme());
  const bg = (page: Page, sel: string) =>
    page.evaluate((s) => getComputedStyle(document.querySelector(s)!).backgroundColor, sel);

  test("the window follows the theme", async ({ page }) => {
    await open(page, { w: "900", h: "560" });
    await press(page, "Meta+t");
    await press(page, "Meta+Shift+b");
    expect(await bg(page, ".sidebar")).toBe("rgb(16, 16, 16)");
    await page.evaluate(() => (window as any).__ws.setPlace(2, {
      dir: "/r/app", shown: "~/app",
      git: { root: "/r/app", repo: "app", worktree: null, branch: "main", detached: null, upstream: null,
             ahead: 0, behind: 0, changed: 4, conflicts: 0, operation: null },
    }));
    await page.evaluate(() => (window as any).__ws.setAgent(2, "blocked"));
    await page.evaluate(() => (window as any).__ws.setTheme("Kibann Light"));
    await settle(page);
    // Light surfaces, dark text, and the status colors' light set.
    const [r] = (await bg(page, ".sidebar")).match(/\d+/g)!.map(Number);
    expect(r).toBeGreaterThan(230);
    expect(await bg(page, ".tab-bar")).not.toBe("rgb(17, 17, 17)");
    const changed = await page.evaluate(
      () => getComputedStyle(document.querySelector(".pane-header .pane-changed")!).color,
    );
    expect(changed).toBe("rgb(154, 103, 0)");
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-theme-light.png");
  });

  test("the picker previews as you move, Enter keeps, Esc goes back", async ({ page }) => {
    await open(page, { w: "900", h: "560" });
    await press(page, "Meta+Shift+b");
    await page.locator(".sidebar-themes").click();
    const input = page.locator(".theme-picker-input");
    await expect(input).toBeFocused();
    const rows = () => page.locator(".theme-picker-row").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.name));
    expect((await rows())[0]).toBe("Kibann Dark");

    // Searching, then moving: each row is shown on the real window.
    await page.keyboard.type("light");
    expect(await rows()).toEqual(["Kibann Light", "Solarized Light"]);
    await page.keyboard.press("ArrowDown");
    await expect.poll(async () => (await theme(page)).showing).toBe("Solarized Light");
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-theme-picker.png");
    // Esc goes back to the theme from before; nothing was kept.
    await page.keyboard.press("Escape");
    await expect.poll(() => theme(page)).toEqual({ kept: "Kibann Dark", showing: "Kibann Dark" });
    expect((await page.evaluate(() => (window as any).__ws.previews)).at(-1)).toBe(null);
    await expect(page.locator(".theme-picker")).toBeHidden();

    // Enter keeps it; the keys typed in the picker never reached a pane.
    await page.locator(".sidebar-themes").click();
    await page.keyboard.type("drac");
    await page.keyboard.press("Enter");
    await expect.poll(() => theme(page)).toEqual({ kept: "Dracula", showing: "Dracula" });
    expect((await typed(page))[1] ?? "").toBe("");
    // And focus is back in the terminal.
    await page.keyboard.type("x");
    expect((await typed(page))[1]).toBe("x");
  });

  test("the shortcut sheet is a click away; the first launch says so", async ({ page }) => {
    await open(page, { w: "900", h: "560", hint: "1" });
    await expect(page.locator(".notice")).toContainText("⌘/");
    await page.reload();
    await settle(page);
    await expect(page.locator(".notice")).toHaveCount(0);

    await press(page, "Meta+t");
    await page.locator(".tab-help").click();
    expect((await page.evaluate(() => (window as any).__ws.shortcuts())).visible).toBe(true);
    await press(page, "Escape");
    await press(page, "Meta+Shift+b");
    await page.locator(".sidebar-shortcuts").click();
    expect((await page.evaluate(() => (window as any).__ws.shortcuts())).visible).toBe(true);
  });
});

test.describe("settings", () => {
  const font = (page: Page) => page.evaluate(() => (window as any).__ws.font());

  test("Cmd+= and Cmd+- zoom every pane; Cmd+0 goes back to the config's size", async ({ page }) => {
    await open(page, { w: "800", h: "500" });
    await press(page, "Meta+d");
    const before = await font(page);
    expect(before.size).toBe(14);
    const cols = (await state(page)).panes[0].w;
    await press(page, "Meta+Equal");
    await press(page, "Meta+Equal");
    await expect.poll(async () => (await font(page)).cells[0]![0]).toBeGreaterThan(before.cells[0]![0]);
    const big = await font(page);
    expect(big.size).toBe(16);
    expect(big.cells[0]).toEqual(big.cells[1]); // every pane
    expect((await state(page)).panes[0].w).toBe(cols); // the layout stays
    // A new pane starts at the zoomed size.
    await press(page, "Meta+t");
    await expect.poll(async () => (await font(page)).cells[2]?.[0]).toBe(big.cells[0]![0]);
    await press(page, "Meta+Minus");
    expect((await font(page)).size).toBe(15);
    await press(page, "Meta+0");
    expect((await font(page)).size).toBe(14);
    // Typing never reached a terminal.
    expect(await typed(page)).toEqual({});
  });

  test("the config's font size and family apply live", async ({ page }) => {
    await open(page);
    const before = await font(page);
    await page.evaluate(() => (window as any).__ws.setSettings({ font_size: 18 }));
    await expect.poll(async () => (await font(page)).cells[0]![1]).toBeGreaterThan(before.cells[0]![1]);
    // Zoom is from the config's size; Cmd+0 comes back to it.
    await press(page, "Meta+Equal");
    await press(page, "Meta+0");
    expect((await font(page)).size).toBe(18);
    // A family the system has (a wide one, so the cells change): Verdana on
    // macOS, one of the others on Linux (CI).
    const mono = (await font(page)).cells[0]![0];
    await page.evaluate(() =>
      (window as any).__ws.setSettings({
        font_size: 18,
        font_family: ["Verdana", "DejaVu Sans", "Liberation Sans", "FreeSans"],
      }),
    );
    await expect.poll(async () => (await font(page)).cells[0]![0]).not.toBe(mono);
  });
});

test.describe("settings modal", () => {
  const modal = (page: Page) => page.locator(".settings-modal");
  const tab = (page: Page, t: string) => page.locator(`.settings-tab[data-tab="${t}"]`).click();
  const saved = (page: Page) => page.evaluate(() => (window as any).__ws.saved);
  const calls = (page: Page) => page.evaluate(() => (window as any).__ws.configCalls);

  test("Cmd+, opens it; Esc, Cmd+, ×, and a click outside close it; keys stay out of the panes", async ({ page }) => {
    await open(page);
    await press(page, "Meta+Comma");
    await expect(modal(page)).toBeVisible();
    await page.keyboard.type("ab");
    expect(await typed(page)).toEqual({});
    await page.keyboard.press("Escape");
    await expect(modal(page)).toBeHidden();
    // Focus is back in the terminal.
    await page.keyboard.type("x");
    expect(await typed(page)).toEqual({ 1: "x" });
    await press(page, "Meta+Comma");
    await press(page, "Meta+Comma");
    await expect(modal(page)).toBeHidden();
    await press(page, "Meta+Comma");
    await page.locator(".settings-close").click();
    await expect(modal(page)).toBeHidden();
    await press(page, "Meta+Comma");
    await page.mouse.click(10, 10);
    await expect(modal(page)).toBeHidden();
  });

  test("a switch writes its setting, and follows the file", async ({ page }) => {
    await open(page);
    await press(page, "Meta+Comma");
    await tab(page, "general");
    const sw = page.locator('.settings-switch[data-key="menu-bar-icon"]');
    await expect(sw).toHaveAttribute("aria-checked", "true");
    await sw.click();
    await expect(sw).toHaveAttribute("aria-checked", "false");
    expect(await saved(page)).toEqual([["menu-bar-icon", ["false"]]]);
    // An edit by hand shows while it's open.
    await page.evaluate(() => (window as any).__ws.setSettings({ agent_notifications: false, menu_bar_icon: false }));
    await expect(page.locator('.settings-switch[data-key="agent-notifications"]')).toHaveAttribute("aria-checked", "false");
    await expect(modal(page)).toHaveScreenshot("settings-general.png");
  });

  test("the font: a family from the list keeps the file's fallbacks; the size steps and resets", async ({ page }) => {
    await open(page);
    await page.evaluate(() => (window as any).__ws.setSettings({ font_family: ["Ghost Font", "Symbols Nerd Font"], font_size: 13 }));
    await press(page, "Meta+Comma");
    await tab(page, "appearance");
    const family = page.locator("select.settings-font");
    await expect(family).toHaveValue("Ghost Font");
    await expect(family.locator("option").nth(1)).toHaveText("Ghost Font (not installed)");
    await expect(page.locator(".settings-body")).toContainText("Then Symbols Nerd Font");
    await family.selectOption("Menlo");
    await page.locator(".settings-size button", { hasText: "+" }).click();
    await page.locator(".settings-size a", { hasText: "Default" }).click();
    expect(await saved(page)).toEqual([
      ["font-family", ["Menlo", "Symbols Nerd Font"]],
      ["font-size", ["14"]],
      ["font-size", []],
    ]);
    // The built-in font: the family lines go.
    await family.selectOption("");
    expect((await saved(page)).at(-1)).toEqual(["font-family", []]);
  });

  test("off macOS the font is a typed name", async ({ page }) => {
    await open(page, { fonts: "none", platform: "other" });
    await press(page, "Control+Comma");
    await tab(page, "appearance");
    const input = page.locator("input.settings-font");
    await input.fill("Fira Code");
    await input.press("Enter");
    expect(await saved(page)).toEqual([["font-family", ["Fira Code"]]]);
    // No menu bar off macOS.
    await tab(page, "general");
    await expect(page.locator('.settings-switch[data-key="menu-bar-icon"]')).toHaveCount(0);
    await expect(page.locator(".settings-body")).toContainText("Show in Folder");
  });

  test("Choose… closes it and opens the theme picker", async ({ page }) => {
    await open(page);
    await press(page, "Meta+Comma");
    await tab(page, "appearance");
    await expect(page.locator(".settings-theme")).toHaveText("Light: Kibann Light · Dark: Kibann Dark");
    await page.locator(".settings-button", { hasText: "Choose…" }).click();
    await expect(modal(page)).toBeHidden();
    await expect(page.locator(".theme-picker")).toBeVisible();
  });

  test("the config file: open, show, and the lines it can't use", async ({ page }) => {
    await open(page);
    await press(page, "Meta+Comma");
    await tab(page, "general");
    await expect(page.locator(".settings-path")).toHaveText("/Users/x/.config/kibann/config");
    await page.locator(".settings-button", { hasText: "Open in Editor" }).click();
    await page.locator(".settings-button", { hasText: "Show in Finder" }).click();
    expect(await calls(page)).toEqual(["edit", "reveal"]);
    await expect(page.locator(".settings-problems")).toHaveCount(0);
    await page.evaluate(() => (window as any).__ws.setProblems(["line 4: bad font-size: huge"]));
    await expect(page.locator(".settings-problems")).toContainText("1 line can't be used:line 4: bad font-size: huge");
  });

  test("updates: the release notes, Check, and a ready update", async ({ page }) => {
    await open(page);
    // Help → What's New opens it here.
    await page.evaluate(() => (window as any).__ws.ws.openSettings("updates"));
    const body = page.locator(".settings-body");
    await expect(page.locator(".settings-title")).toHaveText("Updates");
    await expect(body.locator(".changes-release")).toHaveCount(2);
    await expect(body.locator(".changes-release").nth(1)).toContainText("0.1.02026-11-14 · this version");
    await expect(body.locator(".changes-release strong")).toHaveText("tabs");
    await expect(body.locator(".changes-release code")).toHaveText("splits");
    await expect(body).not.toContainText("Intro, not shown.");
    await expect(page.locator(".settings-update-state")).toHaveText("Checks every hour.");
    await page.locator(".settings-button", { hasText: "Check for Updates" }).click();
    await expect(page.locator(".settings-update-state")).toHaveText("0.1.0 is the newest version, checked just now.");
    await page.evaluate(() => (window as any).__ws.setUpdate({ state: "ready", version: "0.1.1", error: null, checked: Date.now() }));
    await expect(page.locator(".settings-update-dot")).toBeVisible();
    await page.locator(".settings-button", { hasText: "Restart to Update" }).click();
    expect(await calls(page)).toEqual(["check", "install"]);
  });

  test("About: the version and its links; the tab is remembered", async ({ page }) => {
    await open(page);
    await press(page, "Meta+Comma");
    await tab(page, "about");
    await expect(page.locator(".settings-version")).toHaveText("Version 0.1.0");
    await page.locator(".settings-about a", { hasText: "GitHub" }).click();
    expect(await page.evaluate(() => (window as any).__ws.opened)).toEqual(["https://github.com/ctmakes/kibann-terminal"]);
    await page.keyboard.press("Escape");
    await page.reload();
    await settle(page);
    await press(page, "Meta+Comma");
    await expect(page.locator(".settings-title")).toHaveText("About");
  });
});


test.describe("notifications", () => {
  const notes = (page: Page) => page.evaluate(() => (window as any).__ws.notes);
  const agent = (page: Page, pane: number, s: string) =>
    page.evaluate(([p, st]) => (window as any).__ws.setAgent(p, st), [pane, s] as const);

  test("other agents are named in their notifications", async ({ page }) => {
    await open(page);
    await press(page, "Meta+t"); // pane 2, focused; pane 1 is in the hidden tab
    const set = (s: string) =>
      page.evaluate((st) => (window as any).__ws.setAgent(1, st, "opencode"), s);
    await set("working");
    await set("blocked");
    await settle(page);
    expect((await notes(page)).at(-1)).toMatchObject({ pane: 1, title: "OpenCode is waiting for you" });
    await set("working");
    await set("idle");
    await settle(page);
    expect((await notes(page)).at(-1)).toMatchObject({ pane: 1, title: "OpenCode finished" });
  });

  test("an agent elsewhere that waits or finishes says so; the pane in view doesn't", async ({ page }) => {
    await open(page);
    await press(page, "Meta+t"); // pane 2, focused; pane 1 is in the hidden tab
    await page.evaluate(() => (window as any).__ws.setPlace(1, {
      dir: "/r/app", shown: "~/app",
      git: { root: "/r/app", repo: "app", worktree: null, branch: "main", detached: null, upstream: null,
             ahead: 0, behind: 0, changed: 0, conflicts: 0, operation: null },
    }));
    await agent(page, 1, "working");
    await agent(page, 2, "working");
    await settle(page);
    expect(await notes(page)).toEqual([]);

    await agent(page, 1, "blocked");
    await agent(page, 2, "blocked"); // the pane being looked at
    await settle(page);
    expect(await notes(page)).toEqual([{ pane: 1, title: "Claude Code is waiting for you", body: "app · main" }]);

    // With the window in the background, every pane counts.
    await page.evaluate(() => (window as any).__ws.setWindowFocused(false));
    await agent(page, 2, "working");
    await agent(page, 2, "idle"); // finished, unseen: done
    await settle(page);
    expect((await notes(page)).at(-1)).toMatchObject({ pane: 2, title: "Claude Code finished" });

    // Turned off in the config.
    const n = (await notes(page)).length;
    await page.evaluate(() => (window as any).__ws.setSettings({ agent_notifications: false }));
    await agent(page, 1, "working");
    await agent(page, 1, "blocked");
    await settle(page);
    expect(await notes(page)).toHaveLength(n);
  });
});

test.describe("find", () => {
  const count = (page: Page) => page.locator(".find-count").textContent();

  test("Cmd+F finds in the focused pane; Enter steps; Esc closes and types again", async ({ page }) => {
    await open(page, { links: "1" });
    await press(page, "Meta+f");
    const bar = page.locator(".find-bar");
    await expect(bar).toBeVisible();
    await expect(page.locator(".find-input")).toBeFocused();

    await page.keyboard.type("EXAMPLE");
    await settle(page);
    expect(await count(page)).toBe("1 found");
    await expect(page.locator(".term-find-match")).toHaveCount(1);

    await press(page, "Enter");
    expect(await count(page)).toBe("1 of 1");
    await expect(page.locator(".term-find-selected")).toHaveCount(1);
    await expect(page.locator("#ws")).toHaveScreenshot("workspace-find.png");

    await page.keyboard.type("zzz");
    await settle(page);
    expect(await count(page)).toBe("No matches");
    await expect(page.locator(".term-find-match")).toHaveCount(0);

    // Cmd+G with the field empty again steps too.
    for (let i = 0; i < 3; i++) await page.keyboard.press("Backspace");
    await press(page, "Meta+g");
    expect(await count(page)).toBe("1 of 1");

    await press(page, "Escape");
    await expect(bar).toBeHidden();
    await expect(page.locator(".term-find-match")).toHaveCount(0);
    await page.keyboard.type("a");
    expect(await typed(page)).toEqual({ 1: "a" });
    const kinds = await page.evaluate(() =>
      (window as any).__ws.inputs.map((x: any) => x.e.kind).filter((k: string) => k.startsWith("find")),
    );
    expect(kinds.at(-1)).toBe("find_end");
  });

  test("each pane has its own bar; reopening keeps the text", async ({ page }) => {
    await open(page);
    await press(page, "Meta+f");
    await page.keyboard.type("pane");
    await settle(page);
    expect(await count(page)).toBe("1 found");
    await press(page, "Escape");
    await press(page, "Meta+d");
    await press(page, "Meta+f");
    await expect(page.locator(".find-bar:visible")).toHaveCount(1);
    await expect(page.locator(".find-bar:visible .find-input")).toHaveValue("");
    await press(page, "Escape");
    await press(page, "Meta+BracketLeft");
    await press(page, "Meta+f");
    await expect(page.locator(".find-bar:visible .find-input")).toHaveValue("pane");
    expect(await page.locator(".find-bar:visible .find-count").textContent()).toBe("1 found");
  });
});

test.describe("keybinds", () => {
  test("the config's keybind lines change keys and the sheet, live", async ({ page }) => {
    await open(page);
    await page.evaluate(() =>
      (window as any).__ws.setSettings({ keybind: ["super+shift+d=unbind", "super+e=new_split:down", "super+k=bogus"] }),
    );
    await expect(page.locator(".notice", { hasText: 'unknown action "bogus"' })).toHaveCount(1);
    await press(page, "Meta+Shift+d");
    expect((await state(page)).panes).toHaveLength(1);
    await press(page, "Meta+e");
    expect((await state(page)).panes).toHaveLength(2);
    await press(page, "Meta+Slash");
    const row = page.locator(".shortcut-row", { hasText: "Split right / down" });
    await expect(row.locator("kbd")).toHaveText(["⌘D / ⌘E"]);
    await press(page, "Escape");
    // Back to the defaults when the lines go.
    await page.evaluate(() => (window as any).__ws.setSettings({ keybind: [] }));
    await press(page, "Meta+Shift+d");
    expect((await state(page)).panes).toHaveLength(3);
  });
});

test.describe("notices", () => {
  test("a notice with a button stays until used or dismissed", async ({ page }) => {
    await open(page);
    await page.evaluate(() => (window as any).__ws.noticeWithButton("Kibann Terminal 0.2.0 is ready.", "Restart to update"));
    const n = page.locator(".notice", { hasText: "0.2.0 is ready" });
    await expect(n).toBeVisible();
    await page.waitForTimeout(300);
    await n.locator(".notice-action").click();
    await expect(n).toHaveCount(0);
    expect(await page.evaluate(() => (window as any).__ws.clicked)).toBe(1);
    await page.evaluate(() => (window as any).__ws.noticeWithButton("again", "Go"));
    await page.locator(".notice", { hasText: "again" }).locator(".notice-close").click();
    await expect(page.locator(".notice", { hasText: "again" })).toHaveCount(0);
    expect(await page.evaluate(() => (window as any).__ws.clicked)).toBe(1);
  });
});
