import { ReactNode, RefObject, useLayoutEffect, useRef, useState } from "react";
import {
  siPlaystation, siPlaystation2, siPlaystation3, siPlaystation4, siPlaystation5, siPlaystationportable, siPlaystationvita,
} from "simple-icons";

/**
 * Selo discreto da plataforma no canto da capa (Biblioteca).
 * PlayStation (1–5, PSP, Vita): formas oficiais do pacote simple-icons (CC0).
 * PC, Xbox, Switch, Super Nintendo e Nintendinho: a Microsoft e a Nintendo não permitem distribuir os
 * logotipos em pacotes de ícones, então são símbolos simplificados reconhecíveis (bandeira de 4
 * janelas, "X" na esfera, Joy-Con, os 4 botões do SNES e o controle do NES). Wii, Wii U, GameCube, DS, 3DS,
 * GBA e Dreamcast seguem a mesma ideia (controle, cubo, telas, espiral), sem os logotipos das marcas.
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
  "Wii U": {
    label: "Wii U",
    glyph: (
      <>
        <rect x="1.5" y="5" width="21" height="14" rx="4" fill="none" stroke="currentColor" strokeWidth="1.7" />
        <rect x="7" y="8" width="10" height="8" rx="1" fill="currentColor" />
        <circle cx="4.4" cy="10" r="1.1" fill="currentColor" />
        <circle cx="19.6" cy="14" r="1.1" fill="currentColor" />
      </>
    ),
  },
  "Wii": {
    label: "Wii",
    glyph: (
      <>
        <rect x="8.5" y="1.5" width="7" height="21" rx="3.2" fill="none" stroke="currentColor" strokeWidth="1.7" />
        <circle cx="12" cy="6" r="1.5" fill="currentColor" />
        <rect x="11.2" y="10" width="1.6" height="5" rx=".8" fill="currentColor" />
      </>
    ),
  },
  "GameCube": {
    label: "Nintendo GameCube",
    glyph: (
      <>
        <path d="M12 1.8 21 7v10l-9 5.2L3 17V7z" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round" />
        <path d="M12 7.2 16.5 9.8v5.2L12 17.6 7.5 15V9.8z" fill="currentColor" />
      </>
    ),
  },
  "Nintendo 3DS": {
    label: "Nintendo 3DS",
    glyph: (
      <>
        <rect x="4.5" y="1.5" width="15" height="9.5" rx="1.6" fill="none" stroke="currentColor" strokeWidth="1.6" />
        <rect x="4.5" y="13" width="15" height="9.5" rx="1.6" fill="none" stroke="currentColor" strokeWidth="1.6" />
        <rect x="9" y="15.2" width="6" height="5" rx=".6" fill="currentColor" />
      </>
    ),
    suffix: "3D",
  },
  "Nintendo DS": {
    label: "Nintendo DS",
    glyph: (
      <>
        <rect x="4.5" y="1.5" width="15" height="9.5" rx="1.6" fill="none" stroke="currentColor" strokeWidth="1.6" />
        <rect x="4.5" y="13" width="15" height="9.5" rx="1.6" fill="none" stroke="currentColor" strokeWidth="1.6" />
        <rect x="8" y="4" width="8" height="4.5" rx=".6" fill="currentColor" />
        <rect x="8" y="15.5" width="8" height="4.5" rx=".6" fill="currentColor" />
      </>
    ),
  },
  "Game Boy Advance": {
    label: "Game Boy Advance",
    glyph: (
      <>
        <rect x="1.5" y="5.5" width="21" height="13" rx="5" fill="none" stroke="currentColor" strokeWidth="1.6" />
        <rect x="8" y="8" width="8" height="8" rx="1" fill="currentColor" />
        <path d="M3.6 11.2h1.2V10h1.2v1.2h1.2v1.2H6v1.2H4.8v-1.2H3.6z" fill="currentColor" />
        <circle cx="19.2" cy="11" r="1" fill="currentColor" />
        <circle cx="18" cy="13.2" r="1" fill="currentColor" />
      </>
    ),
  },
  "Nintendo 64": {
    label: "Nintendo 64",
    glyph: (
      <>
        <path d="M3 9.5c0-2 1.5-3.5 3.5-3.5h11c2 0 3.5 1.5 3.5 3.5V12l-2.5 6.5H16L13.5 13h-3L8 18.5H5.5L3 12z" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round" />
        <circle cx="12" cy="10" r="1.6" fill="currentColor" />
        <path d="M6 9.4h1.2V8.2h1.2v1.2h1.2v1.2H8.4v1.2H7.2v-1.2H6z" fill="currentColor" />
      </>
    ),
  },
  "Game Boy": {
    label: "Game Boy",
    glyph: (
      <>
        <rect x="5" y="1.5" width="14" height="21" rx="2.2" fill="none" stroke="currentColor" strokeWidth="1.6" />
        <rect x="7.5" y="4" width="9" height="7.5" rx=".8" fill="currentColor" />
        <path d="M7.6 15.8h1.2v-1.2H10v1.2h1.2V17H10v1.2H8.8V17H7.6z" fill="currentColor" />
        <circle cx="15.8" cy="15.6" r="1.1" fill="currentColor" />
      </>
    ),
  },
  "Game Boy Color": {
    label: "Game Boy Color",
    glyph: (
      <>
        <rect x="5" y="1.5" width="14" height="21" rx="2.2" fill="none" stroke="currentColor" strokeWidth="1.6" />
        <rect x="7.5" y="4" width="9" height="7.5" rx=".8" fill="none" stroke="currentColor" strokeWidth="1.5" />
        <circle cx="10.2" cy="7.8" r="1.2" fill="currentColor" />
        <circle cx="13.8" cy="7.8" r="1.2" fill="currentColor" />
        <path d="M7.6 15.8h1.2v-1.2H10v1.2h1.2V17H10v1.2H8.8V17H7.6z" fill="currentColor" />
        <circle cx="15.8" cy="15.6" r="1.1" fill="currentColor" />
      </>
    ),
    suffix: "C",
  },
  "Mega Drive": {
    label: "Sega Mega Drive",
    glyph: (
      <>
        <path d="M2 8.5h20v7H2z" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round" />
        <circle cx="7" cy="12" r="2" fill="none" stroke="currentColor" strokeWidth="1.5" />
        <path d="M11 11h9M11 13h9" stroke="currentColor" strokeWidth="1.3" />
      </>
    ),
  },
  "Dreamcast": {
    label: "Sega Dreamcast",
    glyph: (
      <path d="M12 3.2a8.8 8.8 0 1 0 8.8 8.8h-2.4A6.4 6.4 0 1 1 12 5.6a4.4 4.4 0 1 1-4.4 4.4h2.2A2.2 2.2 0 1 0 12 7.8"
        fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
    ),
  },
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
