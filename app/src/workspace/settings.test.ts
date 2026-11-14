import { describe, expect, it } from "vitest";
import { ago, updateText, type UpdateStatus } from "./settings";

const at = (state: UpdateStatus["state"], more: Partial<UpdateStatus> = {}): UpdateStatus => ({
  state,
  version: null,
  error: null,
  checked: null,
  ...more,
});

describe("Settings → Updates", () => {
  it("says how long ago", () => {
    const now = 1_000_000_000_000;
    expect(ago(now - 5_000, now)).toBe("just now");
    expect(ago(now - 5 * 60_000, now)).toBe("5 min ago");
    expect(ago(now - 3 * 3_600_000, now)).toBe("3 h ago");
  });

  it("says where updating stands, and when Check makes sense", () => {
    expect(updateText(at("idle"), "0.1.0")).toEqual({ text: "Checks every hour.", canCheck: true });
    expect(updateText(at("dev"), "0.1.0").canCheck).toBe(false);
    expect(updateText(at("checking"), "0.1.0")).toEqual({ text: "Checking…", canCheck: false });
    expect(updateText(at("downloading", { version: "0.1.1" }), "0.1.0").text).toBe("Downloading 0.1.1…");
    expect(updateText(at("ready", { version: "0.1.1" }), "0.1.0").text).toMatch(/^0\.1\.1 is ready\./);
    expect(updateText(at("latest", { checked: Date.now() }), "0.1.0").text).toBe("0.1.0 is the newest version, checked just now.");
    const err = updateText(at("error", { error: "offline" }), "0.1.0");
    expect(err).toEqual({ text: "Couldn't check: offline.", canCheck: true });
  });
});
