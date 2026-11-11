import { expect, test } from "@playwright/test";
import { openFixture } from "./harness";

const SCREENSHOTS: Array<[string, Record<string, string>?]> = [
  ["palette"],
  ["styles"],
  ["boxdraw", { w: "1100", h: "520" }],
  ["unicode"],
  ["tui"],
  ["partial"],
  ["cursor", { style: "block" }],
  ["cursor", { style: "bar" }],
  ["cursor", { style: "underline" }],
  ["cursor", { style: "block_hollow" }],
  ["cursor", { style: "block", wide: "1" }],
];

for (const [name, params] of SCREENSHOTS) {
  const id = [name, ...Object.entries(params ?? {}).filter(([k]) => k !== "w" && k !== "h").map(([, v]) => v)].join("-");
  test(`fixture ${id}`, async ({ page }) => {
    await openFixture(page, name, params);
    await expect(page.locator("#term")).toHaveScreenshot(`${id}.png`);
  });
}

test("unfocused cursor is hollow", async ({ page }) => {
  await openFixture(page, "cursor", { style: "block" });
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(r)));
  await expect(page.locator("#term")).toHaveScreenshot("cursor-unfocused.png");
});
