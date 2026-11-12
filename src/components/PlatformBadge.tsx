import { ReactNode, RefObject, useLayoutEffect, useRef, useState } from "react";
import {
  siPlaystation, siPlaystation2, siPlaystation3, siPlaystation4, siPlaystation5, siPlaystationportable, siPlaystationvita,
} from "simple-icons";

/**
 * Selo discreto da plataforma no canto da capa (Biblioteca).
 * PlayStation (1–5, PSP, Vita): formas oficiais do pacote simple-icons (CC0).
 * PC, Xbox, Switch, Super Nintendo e Nintendinho: a Microsoft e a Nintendo não permitem distribuir os
 * logotipos em pacotes de ícones, então são símbolos simplificados reconhecíveis (bandeira de 4
 * janelas, "X" na esfera, Joy-Con, os 4 botões do SNES e o controle do NES).
 */
const si = (d: string) => <path d={d} fill="currentColor" />;
const XBOX = (
  <>
    <circle cx="12" cy="12" r="10" fill="none" stroke="currentColor" strokeWidth="2" />
    <path d="M6.6 6.4c2.9 1.7 5.4 4.2 5.4 5.6 0-1.4 2.5-3.9 5.4-5.6M6.6 17.6c2.9-1.7 5.4-4.2 5.4-5.6 0 1.4 2.5 3.9 5.4 5.6"
      fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" />
  </>
);

const SWITCH = (
  <>
    <path d="M10 2.5H7.2A4.7 4.7 0 0 0 2.5 7.2v9.6a4.7 4.7 0 0 0 4.7 4.7H10z" fill="none" stroke="currentColor" strokeWidth="1.8" />
    <circle cx="6.6" cy="8" r="1.9" fill="currentColor" />
    <path d="M13.2 2.5h3.6a4.7 4.7 0 0 1 4.7 4.7v9.6a4.7 4.7 0 0 1-4.7 4.7h-3.6zM17.4 13.8a2 2 0 1 0 0 4 2 2 0 0 0 0-4z" fill="currentColor" fillRule="evenodd" />
  </>
);

interface Mark { glyph: ReactNode; label: string; suffix?: string }
const MARKS: Record<string, Mark> = {
  "PC": { label: "PC (Windows)", glyph: <path fill="currentColor" d="M2.5 4.6 10.6 3.4v8.1H2.5zM11.8 3.2 21.5 1.8v9.7h-9.7zM2.5 12.7h8.1v8.1l-8.1-1.2zM11.8 12.7h9.7v9.5l-9.7-1.4z" /> },
  "XBOX": { label: "Xbox", glyph: XBOX },
  "Xbox 360": { label: "Xbox 360", glyph: XBOX, suffix: "360" },
  "Xbox Series": { label: "Xbox Series X|S", glyph: XBOX, suffix: "X|S" },
  "Playstation 1": { label: "PlayStation", glyph: si(siPlaystation.path) },
  "Playstation 2": { label: "PlayStation 2", glyph: si(siPlaystation2.path) },
  "Playstation 3": { label: "PlayStation 3", glyph: si(siPlaystation3.path) },
  "Playstation 4": { label: "PlayStation 4", glyph: si(siPlaystation4.path) },
  "Playstation 5": { label: "PlayStation 5", glyph: si(siPlaystation5.path) },
  "PSP": { label: "PlayStation Portable", glyph: si(siPlaystationportable.path) },
  "Ps Vita": { label: "PlayStation Vita", glyph: si(siPlaystationvita.path) },
  "Nintendo Switch": { label: "Nintendo Switch", glyph: SWITCH },
  "Nintendo Switch 2": { label: "Nintendo Switch 2", glyph: SWITCH, suffix: "2" },
  "Super Nintendo": {
    label: "Super Nintendo",
    glyph: (
      <>
        <circle cx="12" cy="5" r="3.3" fill="currentColor" />
        <circle cx="19" cy="12" r="3.3" fill="currentColor" />
        <circle cx="12" cy="19" r="3.3" fill="currentColor" />
        <circle cx="5" cy="12" r="3.3" fill="none" stroke="currentColor" strokeWidth="1.8" />
      </>
    ),
  },
  "Nintendinho": {
    label: "Nintendinho (NES)",
    glyph: (
      <>
        <rect x="1.5" y="6.5" width="21" height="11" rx="1.6" fill="none" stroke="currentColor" strokeWidth="1.6" />
        <path d="M5.6 9.6h1.6v1.6h1.6v1.6H7.2v1.6H5.6v-1.6H4v-1.6h1.6z" fill="currentColor" />
        <circle cx="16.4" cy="12.8" r="1.4" fill="currentColor" />
        <circle cx="19.6" cy="12.8" r="1.4" fill="currentColor" />
      </>
    ),
  },
};

// viewBox justo de cada símbolo (medido uma vez): logotipos largos, como "PS5", ocupam a largura certa
const boxes = new Map<string, string>();

/** Nome completo da plataforma (ex.: "PlayStation 2"), ou o próprio valor se não houver símbolo. */
export const platformLabel = (platform: string) => MARKS[platform]?.label ?? platform;
export const hasPlatformMark = (platform: string) => !!MARKS[platform];

/** Símbolo da plataforma em tamanho livre (ex.: capas padrão dos emuladores). Mesmos desenhos do selo. */
export function PlatformGlyph({ platform, height, maxWidth = height * 3.2 }: { platform: string; height: number; maxWidth?: number }) {
  const m = MARKS[platform];
  const g = useRef<SVGGElement>(null);
  const vb = useTightBox(platform, !!m, g);
  if (!m) return null;
  const [, , w, h] = vb.split(" ").map(Number);
  return (
    <span className="plat-glyph" role="img" aria-label={m.label}>
      <svg viewBox={vb} width={Math.min(maxWidth, (w / h) * height)} height={height} aria-hidden="true"><g ref={g}>{m.glyph}</g></svg>
      {m.suffix && <b>{m.suffix}</b>}
    </span>
  );
}

function useTightBox(platform: string, known: boolean, g: RefObject<SVGGElement | null>) {
  const [vb, setVb] = useState(() => boxes.get(platform) ?? "0 0 24 24");
  useLayoutEffect(() => {
    const cached = boxes.get(platform);
    if (cached) { if (cached !== vb) setVb(cached); return; }
    if (!known || !g.current) return;
    try {
      const b = g.current.getBBox();
      if (b.width > 0 && b.height > 0) {
        const pad = 1.2;
        const v = `${(b.x - pad).toFixed(2)} ${(b.y - pad).toFixed(2)} ${(b.width + pad * 2).toFixed(2)} ${(b.height + pad * 2).toFixed(2)}`;
        boxes.set(platform, v);
        setVb(v);
      }
    } catch { /* fora da árvore renderizada: mantém 24×24 */ }
  }, [platform, known]); // eslint-disable-line react-hooks/exhaustive-deps
  return vb;
}

export default function PlatformBadge({ platform }: { platform: string }) {
  const m = MARKS[platform];
  const g = useRef<SVGGElement>(null);
  const vb = useTightBox(platform, !!m, g); // outro selo da mesma plataforma já mediu → usa a medida
  if (!m) return null;
  const [, , w, h] = vb.split(" ").map(Number);
  const width = Math.min(46, Math.max(14, (w / h) * 14));
  return (
    <span className="plat-badge" title={m.label} aria-label={m.label} role="img">
      <svg viewBox={vb} width={width} height={14} aria-hidden="true"><g ref={g}>{m.glyph}</g></svg>
      {m.suffix && <b>{m.suffix}</b>}
    </span>
  );
}
