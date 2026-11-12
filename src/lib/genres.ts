import type { Game } from "../types";
import { tk } from "./i18n";

/** Nomes de gêneros da Steam (pt/en) -> nome padronizado usado no filtro da Biblioteca. */
const MAP: Record<string, string> = {
  "ação": "Ação", "acao": "Ação", action: "Ação",
  aventura: "Aventura", adventure: "Aventura",
  rpg: "RPG", "role-playing": "RPG",
  corrida: "Corrida", racing: "Corrida",
  terror: "Terror", horror: "Terror",
  "estratégia": "Estratégia", estrategia: "Estratégia", strategy: "Estratégia",
  "simulação": "Simulação", simulacao: "Simulação", simulation: "Simulação",
  plataforma: "Plataforma", platformer: "Plataforma",
  esporte: "Esporte", esportes: "Esporte", sports: "Esporte",
  luta: "Luta", fighting: "Luta",
  tiro: "Tiro", shooter: "Tiro",
  puzzle: "Quebra-cabeça", "quebra-cabeça": "Quebra-cabeça",
  indie: "Indie", casual: "Casual",
  "multijogador massivo": "Multijogador", "massively multiplayer": "Multijogador",
};
/** Rótulos da Steam que não são gêneros. */
const IGNORE = new Set([
  "gratuito para jogar", "free to play", "acesso antecipado", "early access", "violento", "violent",
  "gore", "nudez", "nudity", "conteúdo sexual", "sexual content", "utilitários", "utilities",
]);

export const GENRE_ORDER: string[] = [
  tk("Ação"), tk("Aventura"), tk("RPG"), tk("Corrida"), tk("Terror"), tk("Estratégia"), tk("Simulação"), tk("Plataforma"),
  tk("Esporte"), tk("Luta"), tk("Tiro"), tk("Quebra-cabeça"), tk("Indie"), tk("Casual"), tk("Multijogador"),
];

/** Gêneros padronizados e sem repetição. Desconhecidos são mantidos como foram escritos. */
export function canonGenres(list: string[]): string[] {
  const out: string[] = [];
  for (const raw of list) {
    const k = raw.trim().toLowerCase();
    if (!k || IGNORE.has(k)) continue;
    const g = MAP[k] ?? raw.trim();
    if (!out.includes(g)) out.push(g);
  }
  return out;
}

/** Gêneros presentes na biblioteca com a contagem de jogos (ordem padrão, depois alfabética). */
export function availableGenres(games: Pick<Game, "genres">[]): [string, number][] {
  const count = new Map<string, number>();
  for (const g of games) for (const c of canonGenres(g.genres ?? [])) count.set(c, (count.get(c) ?? 0) + 1);
  const rank = (g: string) => { const i = GENRE_ORDER.indexOf(g); return i < 0 ? 999 : i; };
  return [...count.entries()].sort((a, b) => rank(a[0]) - rank(b[0]) || a[0].localeCompare(b[0], "pt"));
}

/** Um jogo aparece quando QUALQUER um dos seus gêneros é o selecionado. */
export const matchesGenre = (g: Pick<Game, "genres">, genre: string) =>
  !genre || canonGenres(g.genres ?? []).includes(genre);
