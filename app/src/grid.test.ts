import { describe, expect, it } from "vitest";
import { Grid } from "./grid";
import { buildFrame, Screen, style } from "./dev/screen";

function screen(cols = 10, rows = 4): Screen {
  const s = new Screen(cols, rows);
  s.write(0, 0, "hello", style(0xff0000));
  s.write(0, 1, "日本x");
  return s;
}

describe("Grid.apply", () => {
  it("applies a full frame and exposes cells", () => {
    const g = new Grid();
    const r = g.apply(buildFrame(1, screen(), null));
    expect(r).toMatchObject({ applied: true, full: true, resized: true });
    expect(g.cols).toBe(10);
    expect(g.rows).toBe(4);
    expect(g.cellAt(0, 0)).toMatchObject({ text: "h", width: 1, style: { fg: 0xff0000 } });
    expect(g.cellAt(0, 1)).toMatchObject({ text: "日", width: 2 });
    expect(g.cellAt(1, 1)).toMatchObject({ text: "", width: 0 });
    expect(g.cellAt(4, 1)).toMatchObject({ text: "x", width: 1 });
    expect(g.cellAt(9, 3)).toMatchObject({ text: "", width: 1 });
  });

  it("applies in-order partial frames and reports only changed rows", () => {
    const g = new Grid();
    const a = screen();
    g.apply(buildFrame(1, a, null));
    const b = a.clone();
    b.write(0, 2, "new");
    const r = g.apply(buildFrame(2, b, a));
    expect(r).toMatchObject({ applied: true, full: false });
    if (r.applied) expect([...r.dirty]).toEqual([2]);
    expect(g.cellAt(1, 2).text).toBe("e");
  });

  it("asks for a full frame after a seq gap and drops partials until it arrives", () => {
    const g = new Grid();
    const a = screen();
    g.apply(buildFrame(1, a, null));
    const b = a.clone();
    b.write(0, 3, "gap");
    expect(g.apply(buildFrame(3, b, a))).toEqual({ applied: false, requestFull: true });
    expect(g.apply(buildFrame(4, b, a))).toEqual({ applied: false, requestFull: false });
    expect(g.apply(buildFrame(5, b, null))).toMatchObject({ applied: true, full: true });
    expect(g.cellAt(0, 3).text).toBe("g");
  });

  it("asks for a full frame when the first frame is partial", () => {
    const g = new Grid();
    const a = screen();
    expect(g.apply(buildFrame(7, a, a))).toEqual({ applied: false, requestFull: true });
  });

  it("asks for a full frame when a partial frame's size differs", () => {
    const g = new Grid();
    g.apply(buildFrame(1, screen(10, 4), null));
    const f = buildFrame(2, screen(10, 4), screen(10, 4));
    f.cols = 12;
    expect(g.apply(f)).toEqual({ applied: false, requestFull: true });
  });

  it("marks every row dirty when the default background changes", () => {
    const g = new Grid();
    const a = screen();
    g.apply(buildFrame(1, a, null));
    const f = buildFrame(2, a, a);
    f.default_bg = 0x000000;
    const r = g.apply(f);
    if (!r.applied) throw new Error("not applied");
    expect(r.dirty.size).toBe(4);
  });

  it("pads short rows with default cells", () => {
    const g = new Grid();
    const f = buildFrame(1, screen(), null);
    f.rows[0].runs = [{ x: 0, len: 2, s: 0, text: ["a", "b"] }];
    g.apply(f);
    const covered = g.lines[0].reduce((n, r) => n + r.len, 0);
    expect(covered).toBe(10);
  });
});

describe("Grid.resizeLocal", () => {
  it("crops and pads, and never leaves a wide head without its tail", () => {
    const g = new Grid();
    g.apply(buildFrame(1, screen(), null));
    g.resizeLocal(1, 6);
    expect(g.cols).toBe(1);
    expect(g.rows).toBe(6);
    expect(g.cellAt(0, 1)).toMatchObject({ text: "", width: 1 });
    expect(g.lines[5].reduce((n, r) => n + r.len, 0)).toBe(1);
  });
});
