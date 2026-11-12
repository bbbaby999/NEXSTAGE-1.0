/** Conversões de cor usadas pelo tema personalizado (sem dependências). */

export type RGB = [number, number, number];
export type HSV = [number, number, number]; // h 0–360, s 0–1, v 0–1

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** "#a1b2c3" / "a1b2c3" / "#abc" → [r,g,b]; `null` se inválido. */
export function hexToRgb(hex: string): RGB | null {
  let h = hex.trim().replace(/^#/, "");
  if (/^[0-9a-f]{3}$/i.test(h)) h = h.split("").map((c) => c + c).join("");
  if (!/^[0-9a-f]{6}$/i.test(h)) return null;
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}
export const rgbToHex = ([r, g, b]: RGB) =>
  "#" + [r, g, b].map((v) => clamp(Math.round(v), 0, 255).toString(16).padStart(2, "0")).join("");

export function rgbToHsv([r, g, b]: RGB): HSV {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  let h = 0;
  if (d) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return [h, max ? d / max : 0, max];
}
export function hsvToRgb([h, s, v]: HSV): RGB {
  const c = v * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = v - c;
  const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return [(r + m) * 255, (g + m) * 255, (b + m) * 255];
}

/** Luminância relativa (WCAG). */
export function luminance([r, g, b]: RGB) {
  const f = (c: number) => { c /= 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}
export const contrast = (a: RGB, b: RGB) => {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (l1 + 0.05) / (l2 + 0.05);
};

/** Cor do texto sobre a cor principal (botões): branco ou quase-preto, o de maior contraste. */
export function textOn(hex: string): string {
  const c = hexToRgb(hex) ?? [0, 0, 0];
  return contrast(c, [255, 255, 255]) >= contrast(c, [3, 16, 28]) ? "#ffffff" : "#03101c";
}

/**
 * Destaque automático: mesma matiz da cor principal, mais clara e viva, para o anel de foco, brilhos e
 * realces aparecerem sobre o fundo escuro mesmo quando a principal é muito escura (ex.: #610000 → vermelho vivo).
 */
export function autoAccent(hex: string): string {
  const [h, s, v] = rgbToHsv(hexToRgb(hex) ?? [47, 107, 255]);
  if (s < 0.08) return rgbToHex(hsvToRgb([h, s, Math.max(v, 0.9)])); // cinzas: um cinza claro
  return rgbToHex(hsvToRgb([h, clamp(s * 0.85, 0.55, 0.9), Math.max(v, 0.88)]));
}

/** Mistura `hex` com branco (`t` = fração de branco), para rótulos legíveis. */
export function tint(hex: string, t: number): string {
  const [r, g, b] = hexToRgb(hex) ?? [255, 255, 255];
  return rgbToHex([r + (255 - r) * t, g + (255 - g) * t, b + (255 - b) * t]);
}
