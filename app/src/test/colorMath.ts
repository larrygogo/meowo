// 颜色守护测试共用的算法：WCAG 2.x 对比度、Machado 2009 色觉模拟、CIEDE2000。
// 放在一处是因为品牌色、状态色、语法色的几条守护测试都要用同一把尺子，各写一份迟早算出不同的数。
export type Rgba = [number, number, number, number];

export function parseColor(s: string): Rgba {
  const t = s.trim();
  if (t.startsWith("#")) {
    const h = t.length === 4 ? [...t.slice(1)].map((c) => c + c).join("") : t.slice(1, 7);
    return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)).concat(1) as Rgba;
  }
  const m = /rgba?\(([^)]+)\)/.exec(t);
  if (!m) throw new Error(`认不出的颜色：${s}`);
  const p = m[1].split(",").map((x) => parseFloat(x));
  return [p[0], p[1], p[2], p[3] ?? 1];
}

/** 半透明前景叠到不透明底上的合成色。 */
export function over(fg: Rgba, bg: Rgba): Rgba {
  const a = fg[3];
  return [0, 1, 2].map((i) => fg[i] * a + bg[i] * (1 - a)).concat(1) as Rgba;
}

const lin = (c: number) => {
  const v = c / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
};
const unlin = (v: number) => {
  const c = Math.min(1, Math.max(0, v));
  return 255 * (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);
};
const lum = (c: Rgba) => 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);

export function contrast(a: Rgba, b: Rgba): number {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

export type Vision = "normal" | "deuteranopia" | "protanopia";
const MACHADO: Record<Exclude<Vision, "normal">, number[][]> = {
  deuteranopia: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
  protanopia: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
};

export function simulate(c: Rgba, v: Vision): Rgba {
  if (v === "normal") return c;
  const l = [lin(c[0]), lin(c[1]), lin(c[2])];
  return MACHADO[v].map((row) => unlin(row[0] * l[0] + row[1] * l[1] + row[2] * l[2])).concat(1) as Rgba;
}

function lab(c: Rgba): [number, number, number] {
  const [r, g, b] = [lin(c[0]), lin(c[1]), lin(c[2])];
  const X = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
  const Y = 0.2126729 * r + 0.7151522 * g + 0.072175 * b;
  const Z = 0.0193339 * r + 0.119192 * g + 0.9503041 * b;
  const f = (t: number) => (t > 216 / 24389 ? Math.cbrt(t) : ((24389 / 27) * t + 16) / 116);
  const [fx, fy, fz] = [f(X / 0.95047), f(Y), f(Z / 1.08883)];
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
}

const rad = (d: number) => (d * Math.PI) / 180;
const deg = (r: number) => (r * 180) / Math.PI;

export function deltaELab(p: [number, number, number], q: [number, number, number]): number {
  const [L1, a1, b1] = p;
  const [L2, a2, b2] = q;
  const Cb = (Math.hypot(a1, b1) + Math.hypot(a2, b2)) / 2;
  const G = 0.5 * (1 - Math.sqrt(Cb ** 7 / (Cb ** 7 + 25 ** 7)));
  const [a1p, a2p] = [(1 + G) * a1, (1 + G) * a2];
  const [C1p, C2p] = [Math.hypot(a1p, b1), Math.hypot(a2p, b2)];
  const hue = (b: number, a: number, C: number) => (C ? (deg(Math.atan2(b, a)) + 360) % 360 : 0);
  const [h1, h2] = [hue(b1, a1p, C1p), hue(b2, a2p, C2p)];
  const dL = L2 - L1;
  const dC = C2p - C1p;
  let dh = C1p * C2p === 0 ? 0 : h2 - h1;
  if (dh > 180) dh -= 360;
  else if (dh < -180) dh += 360;
  const dH = 2 * Math.sqrt(C1p * C2p) * Math.sin(rad(dh / 2));
  const Lb = (L1 + L2) / 2;
  const Cbp = (C1p + C2p) / 2;
  let hb: number;
  if (C1p * C2p === 0) hb = h1 + h2;
  else if (Math.abs(h1 - h2) <= 180) hb = (h1 + h2) / 2;
  else hb = h1 + h2 < 360 ? (h1 + h2 + 360) / 2 : (h1 + h2 - 360) / 2;
  const T = 1 - 0.17 * Math.cos(rad(hb - 30)) + 0.24 * Math.cos(rad(2 * hb)) + 0.32 * Math.cos(rad(3 * hb + 6)) - 0.2 * Math.cos(rad(4 * hb - 63));
  const dth = 30 * Math.exp(-(((hb - 275) / 25) ** 2));
  const Rc = 2 * Math.sqrt(Cbp ** 7 / (Cbp ** 7 + 25 ** 7));
  const Sl = 1 + (0.015 * (Lb - 50) ** 2) / Math.sqrt(20 + (Lb - 50) ** 2);
  const Sc = 1 + 0.045 * Cbp;
  const Sh = 1 + 0.015 * Cbp * T;
  const Rt = -Math.sin(rad(2 * dth)) * Rc;
  return Math.sqrt((dL / Sl) ** 2 + (dC / Sc) ** 2 + (dH / Sh) ** 2 + Rt * (dC / Sc) * (dH / Sh));
}

/** 两个颜色在某种色觉下的 CIEDE2000 色差。 */
export function deltaE(a: Rgba, b: Rgba, v: Vision = "normal"): number {
  return deltaELab(lab(simulate(a, v)), lab(simulate(b, v)));
}
