export const FONT_FAMILY = '"JetBrains Mono"';
/** Per-glyph fallbacks for CJK and emoji, then whatever the system has. */
export const FONT_STACK = `${FONT_FAMILY}, "SF Mono", Menlo, Consolas, "DejaVu Sans Mono", "Hiragino Sans", "PingFang SC", "Microsoft YaHei", "Noto Sans CJK JP", "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", monospace`;
export const DEFAULT_FONT_SIZE = 14;
/** The zoom limits (CSS px), as the config file's (`proto::config`). */
export const FONT_SIZE_MIN = 6;
export const FONT_SIZE_MAX = 72;

/** The user's fonts (config `font-family`), tried before the bundled one. */
let families: string[] = [];

/** Set the user's fonts; returns whether they changed. Views remeasure. */
export function setFontFamilies(f: string[]): boolean {
  const next = f.map((x) => x.replace(/["\\;{}]/g, "").trim()).filter(Boolean);
  if (next.join("\u0000") === families.join("\u0000")) return false;
  families = next;
  return true;
}

/** The CSS font stack: the user's fonts, then the bundled one and fallbacks. */
export function fontStack(): string {
  return families.length ? `${families.map((f) => `"${f}"`).join(", ")}, ${FONT_STACK}` : FONT_STACK;
}
/** Multiplier on the font's own ascent + descent. JetBrains Mono's are already ~1.32em. */
export const LINE_HEIGHT = 1.0;
export const PADDING_CSS = 4;

export interface RawFontMetrics {
  /** CSS px at the given font size. */
  advance: number;
  ascent: number;
  descent: number;
}

export interface Metrics {
  dpr: number;
  fontSize: number;
  /** All below are device pixels, integers. */
  cellW: number;
  cellH: number;
  baseline: number;
  lineW: number;
  underlineY: number;
  strikeY: number;
  pad: number;
}

export function computeMetrics(raw: RawFontMetrics, fontSize: number, dpr: number): Metrics {
  const cellW = Math.max(1, Math.round(raw.advance * dpr));
  const textH = (raw.ascent + raw.descent) * dpr;
  const cellH = Math.max(1, Math.round(textH * LINE_HEIGHT));
  const baseline = Math.round(raw.ascent * dpr + (cellH - textH) / 2);
  const lineW = Math.max(1, Math.round(cellH / 18));
  const descentDev = raw.descent * dpr;
  // Leave room below for a double underline (two strokes one line apart).
  const underlineY = Math.min(
    cellH - 3 * lineW,
    baseline + Math.max(lineW, Math.round(descentDev * 0.35)),
  );
  const strikeY = Math.round(baseline - raw.ascent * dpr * 0.3);
  return {
    dpr,
    fontSize,
    cellW,
    cellH,
    baseline,
    lineW,
    underlineY: Math.max(baseline, underlineY),
    strikeY,
    pad: Math.round(PADDING_CSS * dpr),
  };
}

export function fontString(m: Pick<Metrics, "dpr" | "fontSize">, bold: boolean, italic: boolean): string {
  // Canvas draws in device pixels, so the font size is scaled by dpr.
  const px = m.fontSize * m.dpr;
  return `${italic ? "italic " : ""}${bold ? "700 " : "400 "}${px}px ${fontStack()}`;
}

/** Wait for all four faces so the first measurement is the real font. */
export async function loadFonts(fontSize = DEFAULT_FONT_SIZE): Promise<void> {
  if (typeof document === "undefined" || !document.fonts) return;
  const specs = ["400", "700", "italic 400", "italic 700"].flatMap((s) => [
    `${s} ${fontSize}px ${FONT_FAMILY}`,
    // The user's fonts are system fonts: loading them is quick, and makes
    // sure the first measurement uses them.
    ...families.map((f) => `${s} ${fontSize}px "${f}"`),
  ]);
  await Promise.all(specs.map((s) => document.fonts.load(s, "MgÅ█")));
}

export function measureFont(fontSize: number): RawFontMetrics {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d")!;
  ctx.font = `400 ${fontSize}px ${fontStack()}`;
  const m = ctx.measureText("M");
  // fontBoundingBox* is the font's own ascent/descent; fall back to em ratios.
  const ascent = m.fontBoundingBoxAscent || fontSize * 0.8;
  const descent = m.fontBoundingBoxDescent || fontSize * 0.2;
  // Average over many glyphs to get the true advance without rounding error.
  const advance = ctx.measureText("M".repeat(100)).width / 100;
  return { advance, ascent, descent };
}

export function deviceRatio(override?: number): number {
  if (override && override > 0) return override;
  return typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
}

/** Calls `cb` when the device pixel ratio changes, e.g. moving between displays. */
export function watchDpr(cb: () => void): () => void {
  let mql: MediaQueryList | null = null;
  const listen = () => {
    mql = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    mql.addEventListener("change", onChange, { once: true });
  };
  const onChange = () => {
    cb();
    listen();
  };
  listen();
  return () => mql?.removeEventListener("change", onChange);
}
