import { canonGenres } from "./genres.ts";

export interface StatGame {
  platform: string; genres: string[]; playtimeSeconds: number; lastPlayedAt: string | null; completed: boolean;
}

/** Estatísticas reais do perfil a partir da biblioteca (sem estimativas). "Em andamento" = já iniciado e não concluído. */
export function computeStats(games: StatGame[]) {
  const platforms = new Map<string, number>();
  const genres = new Map<string, number>();
  let seconds = 0;
  for (const g of games) {
    seconds += g.playtimeSeconds;
    if (g.playtimeSeconds > 0) {
      platforms.set(g.platform, (platforms.get(g.platform) ?? 0) + g.playtimeSeconds);
      for (const c of canonGenres(g.genres ?? [])) genres.set(c, (genres.get(c) ?? 0) + g.playtimeSeconds);
    }
  }
  const top = (m: Map<string, number>) =>
    [...m.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0], "pt"))[0]?.[0] ?? null;
  return {
    total: games.length,
    completed: games.filter((g) => g.completed).length,
    inProgress: games.filter((g) => !g.completed && (g.playtimeSeconds > 0 || !!g.lastPlayedAt)).length,
    seconds,
    topPlatform: top(platforms),
    topGenre: top(genres),
  };
}

export type SectionId = "stats" | "favorites" | "activity";
export type Layout = "cinematic" | "compact" | "stats";
export interface PageCfg { layout: Layout; order: SectionId[]; hidden: SectionId[]; accent: string | null }
export const SECTIONS: SectionId[] = ["stats", "favorites", "activity"];
export const DEFAULT_CFG: PageCfg = { layout: "cinematic", order: [...SECTIONS], hidden: [], accent: null };

/** Lê a configuração salva com tolerância: valores inválidos voltam ao padrão, seções ausentes são acrescentadas. */
export function normalizeCfg(raw: string | null | undefined): PageCfg {
  let o: Partial<PageCfg> = {};
  try { o = raw ? JSON.parse(raw) : {}; } catch { o = {}; }
  const order = (Array.isArray(o.order) ? o.order : []).filter((s, i, a): s is SectionId => SECTIONS.includes(s as SectionId) && a.indexOf(s) === i);
  for (const s of SECTIONS) if (!order.includes(s)) order.push(s);
  return {
    layout: o.layout === "compact" || o.layout === "stats" || o.layout === "cinematic" ? o.layout : "cinematic",
    order,
    hidden: (Array.isArray(o.hidden) ? o.hidden : []).filter((s): s is SectionId => SECTIONS.includes(s as SectionId)),
    accent: typeof o.accent === "string" && /^#[0-9a-fA-F]{6}$/.test(o.accent) ? o.accent : null,
  };
}

/** Favoritos escolhidos (3 a 5): mantém a ordem salva e descarta jogos que não existem mais na biblioteca. */
export function normalizeShowcase(raw: string | null | undefined, existing: number[]): number[] {
  let a: unknown = [];
  try { a = raw ? JSON.parse(raw) : []; } catch { a = []; }
  const ids = (Array.isArray(a) ? a : []).filter((n): n is number => typeof n === "number" && existing.includes(n));
  return ids.filter((n, i) => ids.indexOf(n) === i).slice(0, 5);
}
export const showcaseValid = (n: number) => n === 0 || (n >= 3 && n <= 5);
