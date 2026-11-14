import { describe, expect, it } from "vitest";
import { Flag, Underline, withUnderline } from "../proto";
import { Grid } from "../grid";
import { computeMetrics } from "../metrics";
import { buildFrame, Screen, style } from "../dev/screen";
import { planCursor, planRow, type PaintOp } from "./plan";

// JetBrains Mono at 14px, 1x: an 8×18 cell with 1px lines.
const m = computeMetrics({ advance: 8.4, ascent: 14.28, descent: 4.2 }, 14, 1);

function grid(s: Screen): Grid {
  const g = new Grid();
  g.apply(buildFrame(1, s, null));
  return g;
}

const kinds = (ops: PaintOp[]) => ops.map((o) => o.k);

describe("planRow", () => {
  it("draws backgrounds, then glyphs, then decorations", () => {
    const s = new Screen(6, 1);
    s.write(0, 0, "ab", style(0xff0000, 0x0000ff, withUnderline(0, Underline.SINGLE)));
    const ops = planRow(grid(s), 0, 6, m);
    expect(kinds(ops)).toEqual(["fill", "fill", "glyph", "glyph", "fill"]);
    // The whole row in the default background first, then the run's.
    expect(ops[0]).toMatchObject({ x: 0, y: 0, w: 6 * m.cellW, h: m.cellH, color: s.defaultBg });
    expect(ops[1]).toMatchObject({ x: 0, w: 2 * m.cellW, color: 0x0000ff });
    expect(ops[3]).toMatchObject({ k: "glyph", text: "b", x: m.cellW, y: 0, cells: 1, fg: 0xff0000, font: 0, alpha: 1 });
    expect(ops[4]).toMatchObject({ x: 0, y: m.underlineY, w: 2 * m.cellW, h: m.lineW, color: 0xff0000 });
  });

  it("puts each row at its own top and skips blank cells", () => {
    const s = new Screen(4, 3);
    s.write(1, 2, "x y");
    const glyphs = planRow(grid(s), 2, 4, m).filter((o) => o.k === "glyph");
    expect(glyphs).toMatchObject([
      { text: "x", x: m.cellW, y: 2 * m.cellH },
      { text: "y", x: 3 * m.cellW, y: 2 * m.cellH },
    ]);
  });

  it("gives wide characters two cells", () => {
    const s = new Screen(4, 1);
    s.write(0, 0, "日x");
    const glyphs = planRow(grid(s), 0, 4, m).filter((o) => o.k === "glyph");
    expect(glyphs).toMatchObject([
      { text: "日", x: 0, cells: 2 },
      { text: "x", x: 2 * m.cellW, cells: 1 },
    ]);
  });

  it("picks the font from bold and italic, and halves faint text", () => {
    const s = new Screen(4, 1);
    s.write(0, 0, "a", style(0xffffff, null, Flag.BOLD));
    s.write(1, 0, "b", style(0xffffff, null, Flag.ITALIC));
    s.write(2, 0, "c", style(0xffffff, null, Flag.BOLD | Flag.ITALIC | Flag.FAINT));
    const glyphs = planRow(grid(s), 0, 4, m).filter((o) => o.k === "glyph");
    expect(glyphs.map((g) => g.k === "glyph" && [g.font, g.alpha])).toEqual([
      [1, 1],
      [2, 1],
      [3, 0.5],
    ]);
  });

  it("draws nothing for invisible text, not even its decorations", () => {
    const s = new Screen(3, 1);
    s.write(0, 0, "hid", style(0xffffff, 0x333333, Flag.INVISIBLE | Flag.STRIKE));
    expect(kinds(planRow(grid(s), 0, 3, m))).toEqual(["fill", "fill"]);
  });

  it("draws box-drawing characters as geometry with the cell's background", () => {
    const s = new Screen(2, 1);
    s.write(0, 0, "═", style(0x00ff00, 0x101010));
    const box = planRow(grid(s), 0, 2, m).find((o) => o.k === "box");
    expect(box).toMatchObject({ x: 0, y: 0, w: m.cellW, h: m.cellH, fg: 0x00ff00, bg: 0x101010 });
    expect(box?.k === "box" && box.ops.length).toBeGreaterThan(0);
  });

  it("uses the underline color when set, and the text color for strike and overline", () => {
    const s = new Screen(2, 1);
    const flags = withUnderline(Flag.STRIKE | Flag.OVERLINE, Underline.DOUBLE);
    s.write(0, 0, "ab", style(0xaaaaaa, null, flags, 0xff00ff));
    const deco = planRow(grid(s), 0, 2, m).slice(3);
    expect(deco.map((o) => o.k === "fill" && [o.y, o.color])).toEqual([
      [m.underlineY, 0xff00ff],
      [Math.min(m.underlineY + 2 * m.lineW, m.cellH - m.lineW), 0xff00ff],
      [m.strikeY, 0xaaaaaa],
      [0, 0xaaaaaa],
    ]);
  });

  it("cuts dotted and dashed underlines at the run's end", () => {
    for (const ul of [Underline.DOTTED, Underline.DASHED]) {
      const s = new Screen(3, 1);
      s.write(0, 0, "abc", style(0xffffff, null, withUnderline(0, ul)));
      const segs = planRow(grid(s), 0, 3, m).filter((o) => o.k === "fill" && o.y === m.underlineY);
      const last = segs[segs.length - 1];
      expect(segs.length).toBeGreaterThan(1);
      expect(last.k === "fill" && last.x + last.w).toBeLessThanOrEqual(3 * m.cellW);
    }
  });

  it("makes one wave for a curly underline", () => {
    const s = new Screen(3, 1);
    s.write(0, 0, "abc", style(0xffffff, null, withUnderline(0, Underline.CURLY)));
    const wave = planRow(grid(s), 0, 3, m).filter((o) => o.k === "wave");
    expect(wave).toMatchObject([{ x: 0, w: 3 * m.cellW, cellW: m.cellW, color: 0xffffff }]);
  });
});

describe("planCursor", () => {
  it("draws the cell's character over a block in the cell's background", () => {
    const s = new Screen(4, 1);
    s.write(0, 0, "a", style(0xffffff, 0x222222));
    s.cursor = { ...s.cursor, color: 0x00ff00 };
    const ops = planCursor(grid(s), 0, 0, 4, "block", m);
    expect(ops).toMatchObject([
      { k: "fill", x: 0, w: m.cellW, color: 0x00ff00 },
      { k: "glyph", text: "a", fg: 0x222222 },
    ]);
  });

  it("covers a wide character, but not past the last column", () => {
    const s = new Screen(3, 1);
    s.write(0, 0, "日");
    s.write(2, 0, "日");
    const g = grid(s);
    expect(planCursor(g, 0, 0, 3, "underline", m)[0]).toMatchObject({ w: 2 * m.cellW });
    expect(planCursor(g, 2, 0, 3, "underline", m)[0]).toMatchObject({ w: m.cellW });
  });

  it("has one shape per style", () => {
    const g = grid(new Screen(2, 1));
    expect(planCursor(g, 0, 0, 2, "block_hollow", m)).toMatchObject([{ k: "frame", lw: m.lineW }]);
    expect(planCursor(g, 0, 0, 2, "bar", m)).toMatchObject([{ k: "fill", h: m.cellH }]);
    const [u] = planCursor(g, 0, 0, 2, "underline", m);
    expect(u.k === "fill" && u.y + u.h).toBe(m.cellH);
    // A block on an empty cell is just the block.
    expect(kinds(planCursor(g, 0, 0, 2, "block", m))).toEqual(["fill"]);
  });
});
