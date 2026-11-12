import { Device, useGlyphState } from "../lib/inputDevice";
import { t, tk } from "../lib/i18n";

export type GlyphKey = "confirm" | "back" | "x" | "l1" | "r1" | "l2" | "r2" | "menu" | "dpad";
export const GLYPHS: Record<Device, Record<GlyphKey, string>> = {
  xbox: { confirm: "A", back: "B", x: "X", l1: "LB", r1: "RB", l2: "LT", r2: "RT", menu: "☰", dpad: "✥" },
  playstation: { confirm: "✕", back: "○", x: "□", l1: "L1", r1: "R1", l2: "L2", r2: "R2", menu: "Options", dpad: "✥" },
  // Nintendo: o botão de baixo (B) confirma e o da direita (A) volta, como na posição física padrão;
  // o da esquerda (Y) ocupa o lugar do X/□.
  nintendo: { confirm: "B", back: "A", x: "Y", l1: "L", r1: "R", l2: "ZL", r2: "ZR", menu: "+", dpad: "✥" },
  generic: { confirm: "A", back: "B", x: "X", l1: "LB", r1: "RB", l2: "LT", r2: "RT", menu: "Start", dpad: "✥" },
  keyboard: { confirm: "Enter", back: "Esc", x: "—", l1: "Q", r1: "E", l2: "—", r2: "—", menu: tk("Menu"), dpad: tk("Setas") },
};

/** Símbolos do PlayStation desenhados em vetor: mesmo tamanho e centro em qualquer fonte. */
export const PS_SHAPES: Partial<Record<GlyphKey, JSX.Element>> = {
  x: <rect x="3" y="3" width="10" height="10" rx="1" />,
  confirm: <path d="M3.5 3.5l9 9M12.5 3.5l-9 9" />,
  back: <circle cx="8" cy="8" r="5.2" />,
};

/** Botão físico → ícone padrão daquele botão (os sem ícone próprio usam EXTRA_LABEL). */
const BTN_GLYPH: Record<string, GlyphKey | undefined> = { A: "confirm", B: "back", X: "x", LB: "l1", RB: "r1", LT: "l2", RT: "r2", START: "menu" };
const EXTRA_LABEL: Record<Exclude<Device, "keyboard">, Record<string, string>> = {
  xbox: { Y: "Y", SELECT: "View", L3: "LS", R3: "RS" },
  playstation: { Y: "△", SELECT: "Create", L3: "L3", R3: "R3" },
  nintendo: { Y: "X", SELECT: "−", L3: "LS", R3: "RS" },
  generic: { Y: "Y", SELECT: "Select", L3: "L3", R3: "R3" },
};

/** Ícone de comando que acompanha o último dispositivo usado. */
export default function Glyph({ k: key }: { k: GlyphKey }) {
  const { device: d, map } = useGlyphState();
  // Mapeamento individual do controle em uso (Central de Controles): o ícone mostra o botão físico que
  // faz a ação (ex.: padrão japonês → "confirmar" aparece no botão da direita).
  const btn = map?.[key];
  const k: GlyphKey = btn ? (BTN_GLYPH[btn] ?? key) : key;
  if (btn && !BTN_GLYPH[btn] && d !== "keyboard") return <i className={`glyph ${d} g-${key}`}>{EXTRA_LABEL[d][btn] ?? btn}</i>;
  const shape = d === "playstation" ? PS_SHAPES[k] : undefined;
  if (shape) return <i className={`glyph ${d} g-${k} vec`} aria-label={GLYPHS[d][k]}><svg viewBox="0 0 16 16" aria-hidden="true">{shape}</svg></i>;
  return <i className={`glyph ${d} g-${k}`}>{d === "keyboard" ? t(GLYPHS[d][k]) : GLYPHS[d][k]}</i>;
}
