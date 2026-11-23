// Screens (screens/NAME/screen.json) and parts in the workspace harness:
// the built-in screens, switching, floating blocks, resizing, and what a
// part may and may not do. The default screen is covered by
// workspace.spec.ts, which runs on it unchanged.

import { expect, test, type Page } from "@playwright/test";

async function open(page: Page, extra: Record<string, string> = {}): Promise<void> {
  const q = new URLSearchParams({ w: "1100", h: "640", platform: "mac", ...extra });
  await page.goto(`/src/dev/workspace.html?${q}`);
  await page.waitForFunction(() => (window as any).__ws?.ready() === true);
  if (extra.screen) await page.waitForFunction((n) => document.querySelector(".workspace")?.getAttribute("data-screen") === n, extra.screen);
}

const screen = (page: Page) => page.evaluate(() => (document.querySelector(".workspace") as HTMLElement).dataset.screen);

/** Three panes over two tabs, with agents in each state. */
async function scene(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const w = (window as any).__ws;
    const tick = () => new Promise((r) => setTimeout(r, 50));
    w.ws.run({ kind: "split", dir: "row" });
    await tick();
    w.ws.run({ kind: "newTab" });
    await tick();
    const place = (repo: string, branch: string, changed: number) => ({
      dir: `/src/${repo}`,
      shown: `~/src/${repo}`,
      git: { root: `/src/${repo}`, repo, worktree: null, branch, detached: null, upstream: null, ahead: 0, behind: 0, changed, conflicts: 0, operation: null },
    });
    w.setPlace(1, place("leapcount", "main", 3));
    w.setPlace(2, place("leapcount", "billing", 0));
    w.setPlace(3, place("api", "fix", 1));
    w.setAgent(1, "working", "claude");
    w.setAgent(2, "blocked", "claude");
    w.setAgent(3, "done", "codex");
    await tick();
  });
}

const focused = (page: Page) => page.evaluate(() => (window as any).__ws.ws.active.focused as number);

test.describe("screens", () => {
  test("the board shows every pane as a card, waiting agents first; a click shows the pane", async ({ page }) => {
    await open(page, { screen: "board" });
    await scene(page);
    const board = page.frameLocator('iframe[data-part="board"]');
    await expect(board.locator(".card")).toHaveCount(3);
    await expect(board.locator(".card").first()).toHaveClass(/blocked/);
    await expect(board.locator(".card").first().locator(".where")).toHaveText("leapcount · billing");
    await expect(board.locator("#n-blocked b")).toHaveText("1");
    await expect(board.locator("#n-working b")).toHaveText("1");
    // Pane 3 (tab 2) is shown; clicking the waiting agent's card shows pane 2.
    expect(await focused(page)).toBe(3);
    await board.locator(".card").first().click();
    await expect.poll(() => focused(page)).toBe(2);
    expect(await page.evaluate(() => (window as any).__ws.ws.tabs.indexOf((window as any).__ws.ws.active))).toBe(0);
    // The click handed the keyboard back to the terminal.
    await page.keyboard.type("z");
    await expect
      .poll(() => page.evaluate(() => (window as any).__ws.inputs.filter((i: any) => i.n === 2 && i.e.text === "z").length))
      .toBe(1);
  });

  test("the picker tries each screen as you move; Enter keeps it, Esc goes back", async ({ page }) => {
    await open(page);
    expect(await screen(page)).toBe("default");
    await page.evaluate(() => (window as any).__ws.ws.run({ kind: "screens" }));
    await expect(page.locator(".screen-picker-input")).toBeFocused();
    const rows = page.locator(".screen-picker-row");
    await expect(rows).toHaveCount(5); // four built in, the tests' probe
    await page.keyboard.press("ArrowUp"); // from default to the one before it
    await expect.poll(() => screen(page)).toBe("board");
    await page.keyboard.press("Escape");
    await expect.poll(() => screen(page)).toBe("default");
    await expect(page.locator(".screen-picker")).toBeHidden();

    await page.evaluate(() => (window as any).__ws.ws.run({ kind: "screens" }));
    await page.keyboard.type("split-flap");
    await expect(rows).toHaveCount(1);
    await expect.poll(() => screen(page)).toBe("departures");
    await page.keyboard.press("Enter");
    await expect(page.locator(".screen-picker")).toBeHidden();
    expect(await screen(page)).toBe("departures");
    // Kept: another screen tried and abandoned comes back to it.
    await page.evaluate(() => (window as any).__ws.ws.run({ kind: "screens" }));
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Escape");
    await expect.poll(() => screen(page)).toBe("departures");
  });

  test("Cmd+Shift+L and Cmd+Shift+K step through the screens; Ctrl+Alt+Shift+Esc goes back to the default", async ({ page }) => {
    await open(page);
    await page.keyboard.press("Meta+Shift+KeyL");
    await expect.poll(() => screen(page)).toBe("departures");
    await page.keyboard.press("Meta+Shift+KeyK");
    await expect.poll(() => screen(page)).toBe("default");
    await page.keyboard.press("Meta+Shift+KeyK");
    await expect.poll(() => screen(page)).toBe("board");
    await page.keyboard.press("Control+Alt+Shift+Escape");
    await expect.poll(() => screen(page)).toBe("default");
  });

  test("the departures board lists every pane with its remark", async ({ page }) => {
    await open(page, { screen: "departures" });
    await scene(page);
    const board = page.frameLocator('iframe[data-part="departures"]');
    await expect(board.locator(".row")).toHaveCount(3);
    await expect(board.locator(".row").first()).toHaveClass(/r-blocked/);
    // The flaps settle on their letters.
    await expect
      .poll(() => board.locator(".row").first().locator(".remark .f").evaluateAll((fs) => fs.map((f) => f.textContent).join("").trim()))
      .toBe("SEE AGENT");
    // Waiting, done, working: the last row is pane 1's (pane 3 is shown).
    expect(await focused(page)).toBe(3);
    await board.locator(".row").nth(2).click();
    await expect.poll(() => focused(page)).toBe(1);
  });

  test("focus shows only the focused pane; the agents slide in from the edge", async ({ page }) => {
    await open(page, { screen: "focus" });
    await scene(page);
    await page.evaluate(() => (window as any).__ws.ws.show(1));
    await expect
      .poll(() => page.evaluate(() => (window as any).__ws.state().panes.map((p: any) => p.id)))
      .toEqual([1]);
    // Cmd+] moves to the other pane of the tab, which then fills the block.
    await page.keyboard.press("Meta+BracketRight");
    await expect
      .poll(() => page.evaluate(() => (window as any).__ws.state().panes.map((p: any) => p.id)))
      .toEqual([2]);
    const status = page.frameLocator('iframe[data-part="status"]');
    await expect(status.locator("#wait")).toContainText("1 waiting");

    const float = page.locator('.screen-floating[data-node="agents"]');
    const box = async () => (await float.boundingBox())!;
    expect((await box()).x).toBeLessThan(-200);
    await page.mouse.move(400, 300);
    await page.mouse.move(2, 320, { steps: 3 });
    await expect.poll(async () => (await box()).x).toBeGreaterThan(-1);
    await page.frameLocator('iframe[data-part="agents"]').locator("li").filter({ hasText: "codex" }).click();
    await expect.poll(() => focused(page)).toBe(3);
    // Away from it, it slides back.
    await page.mouse.move(700, 300, { steps: 3 });
    await expect.poll(async () => (await box()).x).toBeLessThan(-200);
  });

  test("a resizable block's edge drags, and the size is kept", async ({ page }) => {
    await open(page, { screen: "board" });
    const block = page.locator('[data-node="board"]');
    const before = (await block.boundingBox())!;
    const handle = block.locator(".screen-handle");
    const h = (await handle.boundingBox())!;
    await page.mouse.move(h.x + h.width / 2, h.y + 200);
    await page.mouse.down();
    await page.mouse.move(h.x + h.width / 2 + 120, h.y + 200, { steps: 4 });
    await page.mouse.up();
    const after = (await block.boundingBox())!;
    expect(Math.round(after.width - before.width)).toBe(120);
    await page.reload();
    await page.waitForFunction(() => document.querySelector(".workspace")?.getAttribute("data-screen") === "board");
    expect(Math.round((await page.locator('[data-node="board"]').boundingBox())!.width)).toBe(Math.round(after.width));
  });

  test("a part gets the window's state, theme and props, and can do only what it was allowed", async ({ page }) => {
    await open(page, { screen: "probe" });
    const probe = page.frameLocator('iframe[data-part="probe"]');
    await expect(probe.locator("#result")).not.toHaveText("waiting");
    const r = JSON.parse((await probe.locator("#result").textContent())!);
    expect(r.props).toEqual({ hello: "world" });
    expect(r.panes).toBe(1);
    expect(r.bg).toBe("#000000");
    // Sandboxed: no storage, no reaching the window around it.
    expect(r.storage).toBe("blocked");
    expect(r.parent).toBe("blocked");
    expect(r.git).toBe("ok");
    expect(r.ls).toMatch(/denied: ls isn't in the part's "exec" permission/);
    expect(r.thumb).toBe("ok");
    expect(r.nope).toBe("denied: no action no.such");
    expect(r.store).toBe(41);
    expect(await page.evaluate(() => (window as any).__ws.execs)).toEqual(["probe: git status"]);
    // Typing into a pane.
    await probe.locator("#type").click();
    await expect(probe.locator("#type")).toHaveText("ok");
    // Showing and hiding blocks by id.
    const extra = page.locator('[data-node="extra"]');
    await expect(extra).toBeVisible();
    await probe.locator("#toggle").click();
    await expect(extra).toBeHidden();
    await probe.locator("#toggle").click();
    await expect(extra).toBeVisible();
  });

  test("without the user's say-so a part's permissions do nothing", async ({ page }) => {
    await open(page, { screen: "probe", deny: "1" });
    const probe = page.frameLocator('iframe[data-part="probe"]');
    await expect(probe.locator("#result")).not.toHaveText("waiting");
    const r = JSON.parse((await probe.locator("#result").textContent())!);
    expect(r.git).toMatch(/denied/);
    expect(r.thumb).toMatch(/denied: thumbnails need the "read" permission/);
    expect(await page.evaluate(() => (window as any).__ws.execs)).toEqual([]);
    await probe.locator("#type").click();
    await expect(probe.locator("#type")).toHaveText(/denied: pane.type needs the "type" permission/);
    await probe.locator("#close").click();
    await expect(probe.locator("#close")).toHaveText(/denied: pane.close needs the "type" permission/);
    expect(await page.evaluate(() => (window as any).__ws.ws.panes.size)).toBe(1);
  });

  test("a part can't press shortcuts for the user", async ({ page }) => {
    await open(page, { screen: "probe", deny: "1" });
    const probe = page.frameLocator('iframe[data-part="probe"]');
    await expect(probe.locator("#result")).not.toHaveText("waiting");
    const frame = (await page.locator('iframe[data-part="probe"]').elementHandle())!.contentFrame();
    // Posted while the terminal has the keyboard: not the user's key.
    await (await frame)!.evaluate(() =>
      parent.postMessage({ kibann: 1, t: "key", key: "t", code: "KeyT", alt: false, ctrl: false, meta: true, shift: false }, "*"),
    );
    await page.waitForTimeout(200);
    expect(await page.evaluate(() => (window as any).__ws.ws.tabs.length)).toBe(1);
    // Pressed in the part, but ending a pane needs "type".
    await probe.locator("#field").click();
    await expect(probe.locator("#field")).toBeFocused();
    await page.keyboard.press("Meta+w");
    await page.waitForTimeout(200);
    expect(await page.evaluate(() => (window as any).__ws.ws.panes.size)).toBe(1);
  });

  test("a shortcut pressed in a part still works", async ({ page }) => {
    await open(page, { screen: "probe" });
    const probe = page.frameLocator('iframe[data-part="probe"]');
    await probe.locator("#field").click();
    await expect(probe.locator("#field")).toBeFocused();
    await page.keyboard.press("Meta+t");
    await expect.poll(() => page.evaluate(() => (window as any).__ws.ws.tabs.length)).toBe(2);
  });
});
