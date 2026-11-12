import { expect, test, type Page } from "@playwright/test";
import { metrics, openFixture } from "./harness";

const inputs = (page: Page) => page.evaluate(() => (window as any).__harness.inputs as any[]);
const nextFrame = (page: Page) => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

test.describe("keyboard", () => {
  test("sends press, text and release for typed keys", async ({ page }) => {
    await openFixture(page, "cursor");
    await page.keyboard.type("hi");
    await page.keyboard.press("Enter");
    const keys = (await inputs(page)).filter((e) => e.kind === "key");
    expect(keys.map((e) => [e.action, e.code, e.text])).toEqual([
      ["press", "KeyH", "h"],
      ["release", "KeyH", null],
      ["press", "KeyI", "i"],
      ["release", "KeyI", null],
      ["press", "Enter", null],
      ["release", "Enter", null],
    ]);
  });

  test("sends modifiers and no text for control combinations", async ({ page }) => {
    await openFixture(page, "cursor");
    await page.keyboard.press("Control+c");
    await page.keyboard.press("Shift+ArrowUp");
    const presses = (await inputs(page)).filter((e) => e.kind === "key" && e.action === "press");
    expect(presses).toContainEqual(expect.objectContaining({ code: "KeyC", text: null, mods: 2 }));
    expect(presses).toContainEqual(expect.objectContaining({ code: "ArrowUp", text: null, mods: 1 }));
  });

  test("reports focus changes", async ({ page }) => {
    await openFixture(page, "cursor");
    await page.evaluate(() => (document.activeElement as HTMLElement).blur());
    await page.locator("#term").click();
    const focus = (await inputs(page)).filter((e) => e.kind === "focus").map((e) => e.focused);
    expect(focus).toEqual([true, false, true]);
  });
});

test.describe("text input", () => {
  test("composition shows a preedit and sends the committed text once", async ({ page }) => {
    await openFixture(page, "cursor");
    const overlayHasInk = () =>
      page.evaluate(() => {
        const c = (window as any).__harness.view.overlay as HTMLCanvasElement;
        const d = c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data;
        let n = 0;
        for (let i = 3; i < d.length; i += 4) if (d[i]) n++;
        return n;
      });
    const before = await overlayHasInk();
    await page.evaluate(() => {
      const ta = document.querySelector("textarea")!;
      ta.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
      ta.dispatchEvent(new CompositionEvent("compositionupdate", { data: "にほん" }));
    });
    expect(await overlayHasInk()).toBeGreaterThan(before);
    await page.evaluate(() => {
      const ta = document.querySelector("textarea")!;
      ta.dispatchEvent(new CompositionEvent("compositionend", { data: "日本" }));
      ta.dispatchEvent(new InputEvent("input", { data: "日本", inputType: "insertText" }));
    });
    const texts = (await inputs(page)).filter((e) => e.kind === "text");
    expect(texts).toEqual([{ kind: "text", data: "日本" }]);
  });

  test("text inserted without a keydown is sent (emoji picker)", async ({ page }) => {
    await openFixture(page, "cursor");
    await page.evaluate(() => {
      const ta = document.querySelector("textarea")!;
      ta.dispatchEvent(new InputEvent("input", { data: "🚀", inputType: "insertText" }));
    });
    expect((await inputs(page)).filter((e) => e.kind === "text")).toEqual([{ kind: "text", data: "🚀" }]);
  });

  test("paste is sent as a paste event", async ({ page }) => {
    await openFixture(page, "cursor");
    await page.evaluate(() => {
      const dt = new DataTransfer();
      dt.setData("text/plain", "echo pasted\n");
      document.querySelector("textarea")!.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    });
    expect((await inputs(page)).filter((e) => e.kind === "paste")).toEqual([{ kind: "paste", data: "echo pasted\n" }]);
  });
});

test.describe("mouse", () => {
  test("clicks send press and release in device pixels from the grid origin", async ({ page }) => {
    await openFixture(page, "cursor", { mouse: "normal" });
    const m = await metrics(page);
    const box = (await page.locator(".term-overlay").boundingBox())!;
    const cx = 5.5 * (m.cellW / m.dpr);
    const cy = 3.5 * (m.cellH / m.dpr);
    await page.mouse.click(box.x + cx, box.y + cy);
    const mouse = (await inputs(page)).filter((e) => e.kind === "mouse");
    expect(mouse.map((e) => [e.action, e.button])).toEqual([["press", 1], ["release", 1]]);
    expect(Math.floor(mouse[0].x / m.cellW)).toBe(5);
    expect(Math.floor(mouse[0].y / m.cellH)).toBe(3);
  });

  test("motion is only sent while tracking asks for it", async ({ page }) => {
    await openFixture(page, "cursor", { mouse: "normal" });
    const box = (await page.locator(".term-overlay").boundingBox())!;
    await page.mouse.move(box.x + 20, box.y + 20);
    await page.mouse.move(box.x + 120, box.y + 60);
    expect((await inputs(page)).filter((e) => e.action === "motion")).toEqual([]);

    await openFixture(page, "cursor", { mouse: "any" });
    await page.mouse.move(box.x + 20, box.y + 20);
    await page.mouse.move(box.x + 120, box.y + 60, { steps: 4 });
    expect((await inputs(page)).filter((e) => e.action === "motion").length).toBeGreaterThan(1);
  });

  test("a drag without tracking sends every move, past the edges too", async ({ page }) => {
    await openFixture(page, "cursor");
    const m = await metrics(page);
    const box = (await page.locator(".term-overlay").boundingBox())!;
    const cw = m.cellW / m.dpr;
    await page.mouse.move(box.x + 2 * cw + 1, box.y + 10);
    await page.mouse.down();
    // Two moves inside one cell: both count, the selection's end depends on them.
    await page.mouse.move(box.x + 4 * cw + 1, box.y + 10);
    await page.mouse.move(box.x + 4 * cw + cw - 1, box.y + 10);
    await page.mouse.move(box.x + 4 * cw, box.y - 30);
    await page.mouse.up();
    const motion = (await inputs(page)).filter((e) => e.action === "motion");
    expect(motion.length).toBe(3);
    expect(motion.every((e) => e.button === 1)).toBe(true);
    expect(Math.floor(motion[0].x / m.cellW)).toBe(Math.floor(motion[1].x / m.cellW));
    expect(motion[2].y).toBeLessThan(0);
  });

  test("wheel scrolls in whole rows", async ({ page }) => {
    await openFixture(page, "cursor", { mouse: "normal" });
    const m = await metrics(page);
    // Compare against the delta the engine actually delivered: emulated
    // device scale factors change it (Chromium at 2x halves it).
    await page.evaluate(() => {
      (window as any).__dy = 0;
      document.addEventListener("wheel", (e) => ((window as any).__dy += e.deltaY), { capture: true });
    });
    const box = (await page.locator(".term-overlay").boundingBox())!;
    await page.mouse.move(box.x + 50, box.y + 50);
    await page.mouse.wheel(0, (m.cellH / m.dpr) * 3);
    await nextFrame(page);
    const delivered: number = await page.evaluate(() => (window as any).__dy);
    const rows = (await inputs(page)).filter((e) => e.kind === "wheel").reduce((n, e) => n + e.dy_rows, 0);
    expect(rows).toBe(Math.trunc(delivered / (m.cellH / m.dpr)));
    expect(rows).toBeGreaterThan(0);
  });
});

test.describe("frames", () => {
  test("resizing the container resizes the grid and gets a full frame", async ({ page }) => {
    await openFixture(page, "tui");
    const before = await metrics(page);
    await page.evaluate(() => {
      const el = document.getElementById("term")!;
      el.style.width = "600px";
      el.style.height = "300px";
    });
    await expect.poll(async () => (await metrics(page)).cols).toBeLessThan(before.cols);
    const after = await metrics(page);
    expect(after.rows).toBeLessThan(before.rows);
    const resizes = await page.evaluate(() => (window as any).__harness.transport.resizes);
    expect(resizes.at(-1)).toMatchObject({ cols: after.cols, rows: after.rows, cell_w: after.cellW, cell_h: after.cellH });
    const log = await page.evaluate(() => (window as any).__harness.paintLog);
    expect(log.at(-1).full).toBe(true);
  });

  test("partial frames repaint only the rows that changed", async ({ page }) => {
    await openFixture(page, "partial", { still: "0" });
    await expect.poll(() => page.evaluate(() => (window as any).__harness.paints)).toBeGreaterThan(4);
    const m = await metrics(page);
    const log: Array<{ full: boolean; dirty: number[] | null }> = await page.evaluate(() => (window as any).__harness.paintLog);
    const partials = log.filter((p) => !p.full);
    expect(partials.length).toBeGreaterThan(2);
    for (const p of partials) {
      // Title (0), blank rows (1, 4) and the footer never change.
      for (const y of [0, 1, 4, m.rows - 1]) expect(p.dirty).not.toContain(y);
      expect(p.dirty).toContain(2);
    }
  });

  test("a lost frame triggers a full frame and the screen recovers", async ({ page }) => {
    await openFixture(page, "partial", { still: "1" });
    const result = await page.evaluate(async () => {
      const h = (window as any).__harness;
      const raf = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      h.transport.dropNext = true;
      h.transport.step(); // lost
      await raf();
      h.transport.step(); // arrives out of order
      await raf();
      await raf();
      return { fullRequests: h.transport.fullRequests, tick: h.view.grid.cellAt(5, 3).text };
    });
    expect(result.fullRequests).toBe(1);
    // "tick 2": the mirror caught up to the latest screen.
    expect(result.tick).toBe("2");
  });

  test("the backend is not sent more frames while two are unacknowledged", async ({ page }) => {
    await openFixture(page, "partial", { still: "1" });
    const sent = await page.evaluate(() => {
      const t = (window as any).__harness.transport;
      return [t.step(), t.step(), t.step()];
    });
    expect(sent).toEqual([true, true, false]);
  });
});

test.describe("clicks and the context menu", () => {
  const menus = (page: Page) => page.evaluate(() => (window as any).__harness.contextMenus as number);

  test("clicks keep focus on the terminal and never select the page", async ({ page }) => {
    await openFixture(page, "cursor");
    const box = (await page.locator("#term").boundingBox())!;
    const state = () =>
      page.evaluate(() => {
        const sel = window.getSelection();
        return { active: document.activeElement?.tagName, selected: !!sel && sel.rangeCount > 0 && !sel.isCollapsed };
      });
    // In the padding outside the grid, then on the grid.
    for (const [x, y] of [[box.x + 1, box.y + 1], [box.x + 100, box.y + 100]]) {
      for (const clickCount of [1, 2, 3]) {
        await page.mouse.click(x, y, { clickCount });
        expect(await state()).toEqual({ active: "TEXTAREA", selected: false });
      }
    }
    // Select All used to select the whole page (and tint the canvas blue) once
    // a click in the padding had moved focus to the body.
    await page.keyboard.press("ControlOrMeta+a");
    expect((await state()).selected).toBe(false);
    await page.keyboard.type("ok");
    const keys = (await inputs(page)).filter((e) => e.kind === "key" && e.action === "press" && e.text).map((e) => e.text);
    expect(keys).toEqual(["ok"].flatMap((s) => [...s]));
  });

  test("right-click opens the menu when no program tracks the mouse", async ({ page }) => {
    await openFixture(page, "cursor");
    await page.mouse.click(120, 120, { button: "right" });
    expect(await menus(page)).toBe(1);
  });

  test("right-click goes to a program that tracks the mouse; Shift opens the menu instead", async ({ page }) => {
    await openFixture(page, "cursor", { mouse: "normal" });
    await page.mouse.click(120, 120, { button: "right" });
    expect(await menus(page)).toBe(0);
    const right = () => inputs(page).then((es) => es.filter((e) => e.kind === "mouse" && e.button === 2));
    expect((await right()).map((e) => e.action)).toEqual(["press", "release"]);

    await page.keyboard.down("Shift");
    await page.mouse.click(120, 120, { button: "right" });
    await page.keyboard.up("Shift");
    expect(await menus(page)).toBe(1);
    expect((await right()).length).toBe(2);
  });
});

test.describe("selection and scrollback", () => {
  /** Apply a full frame of blank rows carrying extra fields. */
  const frame = (page: Page, extra: Record<string, unknown>) =>
    page.evaluate(async (extra) => {
      const v = (window as any).__harness.view;
      const g = v.grid;
      v.enqueue({
        seq: g.lastSeq + 1, full: true, cols: g.cols, rows_total: g.rows,
        default_fg: g.defaultFg, default_bg: g.defaultBg, styles: [], rows: [],
        cursor: g.cursor, mouse_tracking: "none", focus_reporting: false,
        scrollbar: { total: g.rows, offset: 0, len: g.rows }, ...extra,
      });
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    }, extra);

  test("copy puts the selected text on the clipboard", async ({ page }) => {
    await openFixture(page, "cursor");
    await frame(page, { selection_text: "line 1\nline 2" });
    // A frame without the field keeps the text.
    await frame(page, {});
    const copy = () =>
      page.evaluate(() => {
        const ta = document.querySelector(".term-input") as HTMLTextAreaElement;
        ta.focus();
        const before = new Event("beforecopy", { cancelable: true });
        ta.dispatchEvent(before);
        // Read what the terminal's handler put on the event, after it ran.
        let copied: string | null = null;
        const read = (e: ClipboardEvent) => (copied = e.defaultPrevented ? e.clipboardData!.getData("text/plain") : null);
        document.addEventListener("copy", read);
        document.execCommand("copy");
        document.removeEventListener("copy", read);
        return { enabled: before.defaultPrevented, copied };
      });
    expect(await copy()).toEqual({ enabled: true, copied: "line 1\nline 2" });

    await frame(page, { selection_text: "" });
    expect(await copy()).toEqual({ enabled: false, copied: null });
  });

  test("scroll shortcuts are sent as scroll events, not keys", async ({ page }) => {
    await openFixture(page, "cursor", { platform: "other" });
    await page.keyboard.press("Shift+PageUp");
    await page.keyboard.press("Shift+End");
    const es = (await inputs(page)).filter((e) => e.kind === "scroll" || (e.kind === "key" && /Page|End/.test(e.code)));
    expect(es).toEqual([
      { kind: "scroll", by: "pages", n: -1 },
      { kind: "scroll", by: "bottom", n: 0 },
    ]);
  });

  test("the scrollbar shows only while scrolled back", async ({ page }) => {
    await openFixture(page, "cursor");
    const bar = page.locator(".term-scrollbar");
    await expect(bar).toBeHidden();
    const rows = await page.evaluate(() => (window as any).__harness.view.grid.rows);
    await frame(page, { scrollbar: { total: rows * 4, offset: rows * 3, len: rows } });
    await expect(bar).toBeHidden();
    await frame(page, { scrollbar: { total: rows * 4, offset: 0, len: rows } });
    await expect(bar).toBeVisible();
    const b = (await bar.boundingBox())!;
    const t = (await page.locator(".term").boundingBox())!;
    expect(b.y - t.y).toBeLessThan(20); // at the top of the history
    expect(b.height).toBeLessThan(t.height / 3);
  });
});
