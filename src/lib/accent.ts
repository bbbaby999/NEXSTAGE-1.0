import { useEffect, useState } from "react";

/**
 * Cor de destaque de uma imagem (capa do jogo), para tingir detalhes da Home. Pega a média dos
 * pixels mais vivos (saturação × brilho) e normaliza para uma cor legível sobre fundo escuro
 * (saturação 45–85%, luminosidade 64%). Devolve "r,g,b" ou `null` (imagem ausente/sem cor/bloqueada).
 */
export function pickAccent(px: Uint8ClampedArray): string | null {
  const scored: { s: number; r: number; g: number; b: number }[] = [];
  for (let i = 0; i < px.length; i += 4) {
    const r = px[i], g = px[i + 1], b = px[i + 2];
    const max = Math.max(r, g, b), min = Math.min(r, g, b);
    if (max < 40) continue; // quase preto não define cor
    const sat = max === 0 ? 0 : (max - min) / max;
    scored.push({ s: sat * (max / 255), r, g, b });
  }
  if (!scored.length) return null;
  scored.sort((a, b) => b.s - a.s);
  const top = scored.slice(0, Math.max(8, Math.floor(scored.length / 4)));
  if (top[0].s < 0.12) return null; // imagem praticamente sem cor: usa o tema
  const avg = top.reduce((a, p) => [a[0] + p.r, a[1] + p.g, a[2] + p.b], [0, 0, 0]).map((v) => v / top.length);
  const [h, s] = rgbToHsl(avg[0], avg[1], avg[2]);
  const [r, g, b] = hslToRgb(h, Math.min(0.85, Math.max(0.45, s)), 0.64);
  return `${r},${g},${b}`;
}

export function useAccent(src: string | null | undefined): string | null {
  const [rgb, setRgb] = useState<string | null>(null);
  useEffect(() => {
    if (!src) { setRgb(null); return; }
    let dead = false;
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      try {
        const c = document.createElement("canvas");
        c.width = c.height = 32;
        const x = c.getContext("2d", { willReadFrequently: true })!;
        x.drawImage(img, 0, 0, 32, 32);
        const v = pickAccent(x.getImageData(0, 0, 32, 32).data);
        if (!dead) setRgb(v);
      } catch { if (!dead) setRgb(null); } // imagem de outro site sem permissão: mantém o tema
    };
    img.onerror = () => { if (!dead) setRgb(null); };
    img.src = src;
    return () => { dead = true; };
  }, [src]);
  return rgb;
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h / 6, s, l];
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const f = (n: number) => {
    const k = (n + h * 12) % 12, a = s * Math.min(l, 1 - l);
    return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))));
  };
  return [f(0), f(8), f(4)];
}
