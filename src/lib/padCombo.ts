import { t } from "./i18n";
/** Espelho da regra do backend (input.rs): máscara de botões do controle e validação anti-conflito. */
export const PAD_BUTTONS = [
  "A (✕)", "B (○)", "X (□)", "Y (△)", "LB (L1)", "RB (R1)", "LT (L2)", "RT (R2)",
  "Select (Create)", "Start (Options)", "L3", "R3", "↑", "↓", "←", "→",
] as const;
const MENU_BITS = (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11); // Select, Start, L3, R3

export const popcount = (m: number) => { let n = 0; for (let i = 0; i < 16; i++) if (m & (1 << i)) n++; return n; };
export const comboLabel = (m: number) =>
  m ? PAD_BUTTONS.filter((_, i) => m & (1 << i)).join(" + ") : t("Não definido");

/** null = válida. Precisa de 2 a 3 botões e de ao menos um botão de menu; no app, a combinação é SEGURADA. */
export function validateCombo(m: number): string | null {
  const n = popcount(m);
  if (n < 2 || n > 3) return t("Use de 2 a 3 botões ao mesmo tempo.");
  if (!(m & MENU_BITS)) return t("Inclua Select, Start, L3 ou R3 na combinação: assim ela não atrapalha os comandos do jogo.");
  return null;
}
export const combosConflict = (a: number, b: number) => a !== 0 && b !== 0 && ((a & b) === a || (a & b) === b);
