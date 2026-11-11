/** xterm's 256-color palette, as the backend would resolve it. */
export const BASE16 = [
  0x1d1f21, 0xcc6666, 0xb5bd68, 0xf0c674, 0x81a2be, 0xb294bb, 0x8abeb7, 0xc5c8c6,
  0x666666, 0xd54e53, 0xb9ca4a, 0xe7c547, 0x7aa6da, 0xc397d8, 0x70c0b1, 0xeaeaea,
];

export function xterm256(i: number): number {
  if (i < 16) return BASE16[i];
  if (i < 232) {
    const n = i - 16;
    const lv = [0, 95, 135, 175, 215, 255];
    return (lv[Math.floor(n / 36)] << 16) | (lv[Math.floor(n / 6) % 6] << 8) | lv[n % 6];
  }
  const g = 8 + (i - 232) * 10;
  return (g << 16) | (g << 8) | g;
}

export function hsl(h: number, s: number, l: number): number {
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))));
  };
  return (f(0) << 16) | (f(8) << 8) | f(4);
}

export function luminance(rgb: number): number {
  return (0.299 * ((rgb >> 16) & 255) + 0.587 * ((rgb >> 8) & 255) + 0.114 * (rgb & 255)) / 255;
}

export function contrast(rgb: number): number {
  return luminance(rgb) > 0.55 ? 0x000000 : 0xffffff;
}

/** Deterministic PRNG so fixtures render identically every run. */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
