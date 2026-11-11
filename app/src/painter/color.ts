const cssCache = new Map<number, string>();

export function css(rgb: number): string {
  let s = cssCache.get(rgb);
  if (s === undefined) {
    s = "#" + (rgb & 0xffffff).toString(16).padStart(6, "0");
    cssCache.set(rgb, s);
  }
  return s;
}
