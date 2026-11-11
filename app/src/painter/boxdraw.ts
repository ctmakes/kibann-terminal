// Box drawing, block elements, braille and Powerline glyphs drawn as geometry
// so that neighboring cells join without gaps. Everything is computed in
// integer device pixels relative to the cell's top-left corner.
//
// `boxOps` is pure and cached; `drawBoxOps` paints the ops into a cell.

export type Op =
  | { k: "rect"; x: number; y: number; w: number; h: number; bg?: boolean; a?: number }
  | { k: "fill"; pts: Array<[number, number]> }
  | { k: "stroke"; lw: number; path: PathCmd[] };

export type PathCmd =
  | ["M", number, number]
  | ["L", number, number]
  | ["A", number, number, number, number, number, boolean];

export function isBoxChar(cp: number): boolean {
  return (
    (cp >= 0x2500 && cp <= 0x259f) ||
    (cp >= 0x2800 && cp <= 0x28ff) ||
    (cp >= 0xe0b0 && cp <= 0xe0b3)
  );
}

// Arms of U+2500–257F as "up right down left": 0 none, 1 light, 2 heavy, 3 double.
// Dashes, arcs and diagonals are "x" and handled separately.
const LINES =
  "0101 0202 1010 2020 x x x x x x x x " + // 2500–250B
  "0110 0210 0120 0220 0011 0012 0021 0022 1100 1200 2100 2200 1001 1002 2001 2002 " + // 250C–251B
  "1110 1210 2110 1120 2120 2210 1220 2220 1011 1012 2011 1021 2021 2012 1022 2022 " + // 251C–252B
  "0111 0112 0211 0212 0121 0122 0221 0222 1101 1102 1201 1202 2101 2102 2201 2202 " + // 252C–253B
  "1111 1112 1211 1212 2111 1121 2121 2112 2211 1122 1221 2212 1222 2122 2221 2222 " + // 253C–254B
  "x x x x 0303 3030 0310 0130 0330 0013 0031 0033 1300 3100 3300 1003 3001 3003 " + // 254C–255D
  "1310 3130 3330 1013 3031 3033 0313 0131 0333 1303 3101 3303 1313 3131 3333 " + // 255E–256C
  "x x x x x x x " + // 256D–2573
  "0001 1000 0100 0010 0002 2000 0200 0020 0201 1020 0102 2010"; // 2574–257F

const LINE_TABLE: Array<[number, number, number, number] | null> = LINES.trim()
  .split(/\s+/)
  .map((s) => (s === "x" ? null : ([...s].map(Number) as [number, number, number, number])));

if (LINE_TABLE.length !== 0x80) {
  throw new Error(`box drawing table has ${LINE_TABLE.length} entries, expected 128`);
}

/** Arms for a codepoint in U+2500–257F, or null when it isn't a plain line join. */
export function lineArms(cp: number): [number, number, number, number] | null {
  if (cp < 0x2500 || cp > 0x257f) return null;
  return LINE_TABLE[cp - 0x2500];
}

type Span = [number, number];

const cache = new Map<string, Op[] | null>();

export function boxOps(cp: number, w: number, h: number, t: number): Op[] | null {
  const key = `${cp}:${w}:${h}:${t}`;
  let ops = cache.get(key);
  if (ops === undefined) {
    ops = build(cp, w, h, t);
    if (cache.size > 4096) cache.clear();
    cache.set(key, ops);
  }
  return ops;
}

function build(cp: number, w: number, h: number, t: number): Op[] | null {
  const arms = lineArms(cp);
  if (arms) return lines(arms, w, h, t);
  if (cp >= 0x2504 && cp <= 0x250b) return dashes(cp, w, h, t);
  if (cp >= 0x254c && cp <= 0x254f) return dashes(cp, w, h, t);
  if (cp >= 0x256d && cp <= 0x2570) return arc(cp, w, h, t);
  if (cp >= 0x2571 && cp <= 0x2573) return diagonal(cp, w, h, t);
  if (cp >= 0x2580 && cp <= 0x259f) return block(cp, w, h);
  if (cp >= 0x2800 && cp <= 0x28ff) return braille(cp, w, h);
  if (cp >= 0xe0b0 && cp <= 0xe0b3) return powerline(cp, w, h, t);
  return null;
}

/** Light stroke start along an axis of length `size`. */
function lightStart(size: number, t: number): number {
  return Math.floor(size / 2) - Math.floor(t / 2);
}

/** Extent of a stroke of the given weight across its axis. */
function span(k: number, m: number, t: number): Span {
  if (k === 2) return [m - Math.floor(t / 2), m - Math.floor(t / 2) + 2 * t];
  if (k === 3) return [m - t, m + 2 * t];
  return [m, m + t];
}

function union(a: Span | null, b: Span | null, fallback: Span): Span {
  if (!a && !b) return fallback;
  if (!a) return b!;
  if (!b) return a;
  return [Math.min(a[0], b[0]), Math.max(a[1], b[1])];
}

export function lines(arms: [number, number, number, number], w: number, h: number, t: number): Op[] {
  const [u, r, d, l] = arms;
  const mx = lightStart(w, t);
  const my = lightStart(h, t);
  const ops: Op[] = [];

  const vSpan = union(u ? span(u, mx, t) : null, d ? span(d, mx, t) : null, [mx, mx + t]);
  const hSpan = union(l ? span(l, my, t) : null, r ? span(r, my, t) : null, [my, my + t]);
  const vDouble = u === 3 || d === 3;
  const hDouble = l === 3 || r === 3;
  const vSingleThrough = !!u && !!d && u !== 3 && d !== 3;
  const hSingleThrough = !!l && !!r && l !== 3 && r !== 3;
  const vNonDouble = (!!u || !!d) && u !== 3 && d !== 3;
  const hNonDouble = (!!l || !!r) && l !== 3 && r !== 3;

  const rect = (x0: number, y0: number, x1: number, y1: number, bg = false) => {
    if (x1 > x0 && y1 > y0) ops.push({ k: "rect", x: x0, y: y0, w: x1 - x0, h: y1 - y0, bg: bg || undefined });
  };

  // Bands: each arm is a solid band from the cell edge to (and over) the center.
  if (l) {
    const [y0, y1] = span(l, my, t);
    let end = vSpan[1];
    if (vDouble && l !== 3 && !r) end = u === 3 && d === 3 ? mx : vSpan[1];
    rect(0, y0, end, y1);
  }
  if (r) {
    const [y0, y1] = span(r, my, t);
    let start = vSpan[0];
    if (vDouble && r !== 3 && !l) start = u === 3 && d === 3 ? mx + t : vSpan[0];
    rect(start, y0, w, y1);
  }
  if (u) {
    const [x0, x1] = span(u, mx, t);
    let end = hSpan[1];
    if (hDouble && u !== 3 && !d) end = l === 3 && r === 3 ? my : hSpan[1];
    rect(x0, 0, x1, end);
  }
  if (d) {
    const [x0, x1] = span(d, mx, t);
    let start = hSpan[0];
    if (hDouble && d !== 3 && !u) start = l === 3 && r === 3 ? my + t : hSpan[0];
    rect(x0, start, x1, h);
  }

  // Gaps: the channel between the two strokes of a double line, painted in bg.
  // A channel runs through the center unless a single line crosses it, or it
  // turns a corner into a single line.
  if (l === 3) {
    const stop = vNonDouble && (vSingleThrough || !r);
    rect(0, my, stop ? vSpan[0] : mx + t, my + t, true);
  }
  if (r === 3) {
    const stop = vNonDouble && (vSingleThrough || !l);
    rect(stop ? vSpan[1] : mx, my, w, my + t, true);
  }
  if (u === 3) {
    const stop = hNonDouble && (hSingleThrough || !d);
    rect(mx, 0, mx + t, stop ? hSpan[0] : my + t, true);
  }
  if (d === 3) {
    const stop = hNonDouble && (hSingleThrough || !u);
    rect(mx, stop ? hSpan[1] : my, mx + t, h, true);
  }
  return ops;
}

function dashes(cp: number, w: number, h: number, t: number): Op[] {
  // [count, heavy, vertical]
  const spec: Record<number, [number, boolean, boolean]> = {
    0x2504: [3, false, false], 0x2505: [3, true, false],
    0x2506: [3, false, true], 0x2507: [3, true, true],
    0x2508: [4, false, false], 0x2509: [4, true, false],
    0x250a: [4, false, true], 0x250b: [4, true, true],
    0x254c: [2, false, false], 0x254d: [2, true, false],
    0x254e: [2, false, true], 0x254f: [2, true, true],
  };
  const [n, heavy, vertical] = spec[cp];
  const len = vertical ? h : w;
  const across = vertical ? w : h;
  const [s0, s1] = span(heavy ? 2 : 1, lightStart(across, t), t);
  const gap = Math.max(1, Math.round(len / n / 3));
  const ops: Op[] = [];
  for (let i = 0; i < n; i++) {
    // Each dash is centered in its segment so dashes are evenly spaced across cells.
    const a = Math.round((i * len) / n) + Math.floor(gap / 2);
    const b = Math.round(((i + 1) * len) / n) - Math.ceil(gap / 2);
    if (b <= a) continue;
    ops.push(vertical ? { k: "rect", x: s0, y: a, w: s1 - s0, h: b - a } : { k: "rect", x: a, y: s0, w: b - a, h: s1 - s0 });
  }
  return ops;
}

function arc(cp: number, w: number, h: number, t: number): Op[] {
  // Centerline of the light strokes this arc must meet.
  const cx = lightStart(w, t) + t / 2;
  const cy = lightStart(h, t) + t / 2;
  const r = Math.max(1, Math.floor(Math.min(w, h) / 2));
  const path: PathCmd[] = [];
  switch (cp) {
    case 0x256d: // ╭ down and right
      path.push(["M", w, cy], ["L", cx + r, cy], ["A", cx + r, cy + r, r, -Math.PI / 2, Math.PI, true], ["L", cx, h]);
      break;
    case 0x256e: // ╮ down and left
      path.push(["M", 0, cy], ["L", cx - r, cy], ["A", cx - r, cy + r, r, -Math.PI / 2, 0, false], ["L", cx, h]);
      break;
    case 0x256f: // ╯ up and left
      path.push(["M", 0, cy], ["L", cx - r, cy], ["A", cx - r, cy - r, r, Math.PI / 2, 0, true], ["L", cx, 0]);
      break;
    case 0x2570: // ╰ up and right
      path.push(["M", w, cy], ["L", cx + r, cy], ["A", cx + r, cy - r, r, Math.PI / 2, Math.PI, false], ["L", cx, 0]);
      break;
  }
  return [{ k: "stroke", lw: t, path }];
}

function diagonal(cp: number, w: number, h: number, t: number): Op[] {
  const ops: Op[] = [];
  if (cp === 0x2571 || cp === 0x2573) ops.push({ k: "stroke", lw: t, path: [["M", w, 0], ["L", 0, h]] });
  if (cp === 0x2572 || cp === 0x2573) ops.push({ k: "stroke", lw: t, path: [["M", 0, 0], ["L", w, h]] });
  return ops;
}

function block(cp: number, w: number, h: number): Op[] {
  const R = (x0: number, y0: number, x1: number, y1: number, a?: number): Op => ({ k: "rect", x: x0, y: y0, w: x1 - x0, h: y1 - y0, a });
  // Same boundary as the "lower n/8" blocks so ▀ and ▄ tile exactly.
  const hy = h - Math.round(h / 2);
  const hx = Math.round(w / 2);
  const lower = (n: number) => R(0, h - Math.round((h * n) / 8), w, h);
  const left = (n: number) => R(0, 0, Math.round((w * n) / 8), h);
  const UL = R(0, 0, hx, hy), UR = R(hx, 0, w, hy), LL = R(0, hy, hx, h), LR = R(hx, hy, w, h);
  switch (cp) {
    case 0x2580: return [R(0, 0, w, hy)];
    case 0x2588: return [R(0, 0, w, h)];
    case 0x2590: return [R(hx, 0, w, h)];
    case 0x2591: return [R(0, 0, w, h, 0.25)];
    case 0x2592: return [R(0, 0, w, h, 0.5)];
    case 0x2593: return [R(0, 0, w, h, 0.75)];
    case 0x2594: return [R(0, 0, w, Math.round(h / 8))];
    case 0x2595: return [R(w - Math.round(w / 8), 0, w, h)];
    case 0x2596: return [LL];
    case 0x2597: return [LR];
    case 0x2598: return [UL];
    case 0x2599: return [UL, LL, LR];
    case 0x259a: return [UL, LR];
    case 0x259b: return [UL, UR, LL];
    case 0x259c: return [UL, UR, LR];
    case 0x259d: return [UR];
    case 0x259e: return [UR, LL];
    case 0x259f: return [UR, LL, LR];
  }
  if (cp >= 0x2581 && cp <= 0x2587) return [lower(cp - 0x2580)];
  if (cp >= 0x2589 && cp <= 0x258f) return [left(8 - (cp - 0x2588))];
  return [];
}

function braille(cp: number, w: number, h: number): Op[] {
  const bits = cp - 0x2800;
  // Dot numbering: 1 2 3 7 down the left column, 4 5 6 8 down the right.
  const order: Array<[number, number]> = [[0, 0], [0, 1], [0, 2], [1, 0], [1, 1], [1, 2], [0, 3], [1, 3]];
  const dot = Math.max(1, Math.round(Math.min(w / 4, h / 8)));
  const ops: Op[] = [];
  for (let i = 0; i < 8; i++) {
    if (!(bits & (1 << i))) continue;
    const [c, r] = order[i];
    const x = Math.round((w * (2 * c + 1)) / 4 - dot / 2);
    const y = Math.round((h * (2 * r + 1)) / 8 - dot / 2);
    ops.push({ k: "rect", x, y, w: dot, h: dot });
  }
  return ops;
}

function powerline(cp: number, w: number, h: number, t: number): Op[] {
  const mid = h / 2;
  switch (cp) {
    case 0xe0b0: return [{ k: "fill", pts: [[0, 0], [w, mid], [0, h]] }];
    case 0xe0b2: return [{ k: "fill", pts: [[w, 0], [0, mid], [w, h]] }];
    case 0xe0b1: return [{ k: "stroke", lw: t, path: [["M", 0, 0], ["L", w, mid], ["L", 0, h]] }];
    case 0xe0b3: return [{ k: "stroke", lw: t, path: [["M", w, 0], ["L", 0, mid], ["L", w, h]] }];
  }
  return [];
}

/** Paint ops into the cell at (x, y). Returns false if nothing was drawn. */
export function drawBoxOps(
  ctx: CanvasRenderingContext2D,
  ops: Op[],
  x: number,
  y: number,
  w: number,
  h: number,
  fg: string,
  bg: string,
): void {
  const alpha = ctx.globalAlpha;
  let clipped = false;
  for (const op of ops) {
    if (op.k === "rect") {
      ctx.fillStyle = op.bg ? bg : fg;
      if (op.a !== undefined) ctx.globalAlpha = alpha * op.a;
      ctx.fillRect(x + op.x, y + op.y, op.w, op.h);
      if (op.a !== undefined) ctx.globalAlpha = alpha;
      continue;
    }
    if (!clipped) {
      // Paths can antialias past the cell edge; keep them inside.
      ctx.save();
      ctx.beginPath();
      ctx.rect(x, y, w, h);
      ctx.clip();
      clipped = true;
    }
    ctx.beginPath();
    if (op.k === "fill") {
      op.pts.forEach(([px, py], i) => (i ? ctx.lineTo(x + px, y + py) : ctx.moveTo(x + px, y + py)));
      ctx.closePath();
      ctx.fillStyle = fg;
      ctx.fill();
    } else {
      for (const c of op.path) {
        if (c[0] === "M") ctx.moveTo(x + c[1], y + c[2]);
        else if (c[0] === "L") ctx.lineTo(x + c[1], y + c[2]);
        else ctx.arc(x + c[1], y + c[2], c[3], c[4], c[5], c[6]);
      }
      ctx.lineWidth = op.lw;
      ctx.lineCap = "butt";
      ctx.lineJoin = "miter";
      ctx.strokeStyle = fg;
      ctx.stroke();
    }
  }
  if (clipped) ctx.restore();
}
