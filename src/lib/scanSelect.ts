import type { ScanItem } from "../types";
import { tk } from "./i18n";

/** Só jogos novos ou executáveis secundários podem ser marcados; duplicados, inválidos e não identificados não. */
export const selectable = (i: ScanItem) => i.status === "new" || i.status === "secondary";

/** "Compatível": novo e (executável do Windows ou já com emulador compatível cadastrado). */
export const isCompatible = (i: ScanItem) => i.status === "new" && (i.kind === "windows" || i.compatible.length > 0);

export const compatiblePaths = (items: ScanItem[]) => items.filter(isCompatible).map((i) => i.path);

export function counts(items: ScanItem[]) {
  const c = { total: items.length, new: 0, secondary: 0, duplicate: 0, invalid: 0, unrecognized: 0, compatible: 0 };
  for (const i of items) { c[i.status]++; if (isCompatible(i)) c.compatible++; }
  return c;
}

/** Resumo para a confirmação: quantos por plataforma e quantos ficarão sem emulador associado. */
export function summarize(items: ScanItem[], chosen: Set<string>, emu: Map<string, number | null>) {
  const byPlatform: Record<string, number> = {};
  let withoutEmulator = 0, count = 0;
  for (const i of items) {
    if (!chosen.has(i.path) || !selectable(i)) continue;
    count++;
    const p = i.platform ?? "Sem plataforma";
    byPlatform[p] = (byPlatform[p] ?? 0) + 1;
    if (i.kind !== "windows" && !emu.get(i.path)) withoutEmulator++;
  }
  return { count, byPlatform, withoutEmulator };
}

export const STATUS_LABEL: Record<ScanItem["status"], string> = {
  new: tk("Novo"), secondary: tk("Secundário"), duplicate: tk("Já na biblioteca"), invalid: tk("Inválido"), unrecognized: tk("Não identificado"),
};
