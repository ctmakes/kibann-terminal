import { describe, expect, it } from "vitest";
import { boxOps, isBoxChar, lineArms, type Op } from "./boxdraw";

/** Rasterize rect ops into a w×h bitmap: 1 fg, 0 bg. Rect-only (no paths). */
function raster(ops: Op[], w: number, h: number): Uint8Array {
  const px = new Uint8Array(w * h);
  for (const op of ops) {
    if (op.k !== "rect") throw new Error("path op in a line glyph");
    for (let y = op.y; y < op.y + op.h; y++) {
      for (let x = op.x; x < op.x + op.w; x++) {
        if (x >= 0 && x < w && y >= 0 && y < h) px[y * w + x] = op.bg ? 0 : 1;
      }
    }
  }
  return px;
}

/** Lengths of fg runs along an edge. */
function edgeRuns(px: Uint8Array, w: number, h: number, edge: "top" | "right" | "bottom" | "left"): number[] {
  const vals: number[] = [];
  if (edge === "top" || edge === "bottom") {
    const y = edge === "top" ? 0 : h - 1;
    for (let x = 0; x < w; x++) vals.push(px[y * w + x]);
  } else {
    const x = edge === "left" ? 0 : w - 1;
    for (let y = 0; y < h; y++) vals.push(px[y * w + x]);
  }
  const runs: number[] = [];
  let n = 0;
  for (const v of [...vals, 0]) {
    if (v) n++;
    else if (n) {
      runs.push(n);
      n = 0;
    }
  }
  return runs;
}

function components(px: Uint8Array, w: number, h: number): number {
  const seen = new Uint8Array(w * h);
  let count = 0;
  for (let i = 0; i < px.length; i++) {
    if (!px[i] || seen[i]) continue;
    count++;
    const stack = [i];
    seen[i] = 1;
    while (stack.length) {
      const j = stack.pop()!;
      const x = j % w;
      const y = (j - x) / w;
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const nx = x + dx;
        const ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue;
        const k = ny * w + nx;
        if (px[k] && !seen[k]) {
          seen[k] = 1;
          stack.push(k);
        }
      }
    }
  }
  return count;
}

const SIZES: Array<[number, number, number]> = [
  [8, 18, 1],
  [9, 19, 1],
  [17, 37, 2],
  [20, 40, 3],
];

describe("line glyphs", () => {
  const cps = Array.from({ length: 0x80 }, (_, i) => 0x2500 + i).filter((cp) => lineArms(cp));

  it("covers every plain join in U+2500–257F", () => {
    expect(cps.length).toBe(0x80 - 8 - 4 - 4 - 3); // minus dashes, double dashes, arcs, diagonals
  });

  for (const [w, h, t] of SIZES) {
    it(`meets each cell edge with the right strokes at ${w}x${h}, line ${t}`, () => {
      for (const cp of cps) {
        const arms = lineArms(cp)!;
        const px = raster(boxOps(cp, w, h, t)!, w, h);
        const edges = ["top", "right", "bottom", "left"] as const;
        edges.forEach((edge, i) => {
          const k = arms[i];
          const expected = k === 0 ? [] : k === 1 ? [t] : k === 2 ? [2 * t] : [t, t];
          expect(edgeRuns(px, w, h, edge), `U+${cp.toString(16)} ${edge}`).toEqual(expected);
        });
        // Light and heavy glyphs are a single connected shape.
        if (!arms.includes(3)) expect(components(px, w, h), `U+${cp.toString(16)}`).toBe(1);
      }
    });

    it(`draws double lines with the right inner structure at ${w}x${h}, line ${t}`, () => {
      const expected: Record<string, number> = {
        "═": 2, "║": 2, "╔": 2, "╗": 2, "╚": 2, "╝": 2, "╬": 4, "╠": 3, "╣": 3, "╦": 3, "╩": 3,
        "╪": 1, "╫": 1, "╒": 1, "╓": 1, "╤": 2, "╥": 1, "╨": 1, "╧": 2, "╢": 2, "╟": 2, "╞": 1, "╡": 1,
      };
      for (const [ch, n] of Object.entries(expected)) {
        const cp = ch.codePointAt(0)!;
        expect(components(raster(boxOps(cp, w, h, t)!, w, h), w, h), ch).toBe(n);
      }
    });
  }

  it("joins horizontally: a light line's stroke is at the same rows in every glyph", () => {
    const [w, h, t] = [9, 19, 1];
    const rowsOf = (cp: number, edge: "left" | "right") => {
      const px = raster(boxOps(cp, w, h, t)!, w, h);
      const x = edge === "left" ? 0 : w - 1;
      return Array.from({ length: h }, (_, y) => px[y * w + x]).join("");
    };
    const ref = rowsOf(0x2500, "right");
    for (const cp of [0x250c, 0x251c, 0x252c, 0x253c, 0x2514, 0x2576]) expect(rowsOf(cp, "right")).toBe(ref);
    for (const cp of [0x2510, 0x2524, 0x252c, 0x253c, 0x2518, 0x2574]) expect(rowsOf(cp, "left")).toBe(ref);
  });
});

describe("blocks, braille, powerline", () => {
  it("fills the full block completely", () => {
    const px = raster(boxOps(0x2588, 9, 19, 1)!, 9, 19);
    expect(px.every((v) => v === 1)).toBe(true);
  });

  it("splits halves without overlap or gaps", () => {
    const top = raster(boxOps(0x2580, 9, 19, 1)!, 9, 19);
    const bottom = raster(boxOps(0x2584, 9, 19, 1)!, 9, 19);
    for (let i = 0; i < top.length; i++) expect(top[i] + bottom[i]).toBe(1);
    const left = raster(boxOps(0x258c, 9, 19, 1)!, 9, 19);
    const right = raster(boxOps(0x2590, 9, 19, 1)!, 9, 19);
    for (let i = 0; i < left.length; i++) expect(left[i] + right[i]).toBe(1);
  });

  it("draws one dot per braille bit", () => {
    expect(boxOps(0x28ff, 9, 19, 1)!.length).toBe(8);
    expect(boxOps(0x2801, 9, 19, 1)!.length).toBe(1);
    expect(boxOps(0x2800, 9, 19, 1)!.length).toBe(0);
  });

  it("handles every codepoint it claims", () => {
    for (let cp = 0; cp < 0x10000; cp++) {
      if (isBoxChar(cp)) expect(boxOps(cp, 9, 19, 1), cp.toString(16)).not.toBeNull();
    }
    expect(isBoxChar(0xe0b4)).toBe(false);
  });
});
