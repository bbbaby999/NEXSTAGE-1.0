import type { Game } from "../types";

/** Quantidade fixa de jogos exibidos na Home. */
export const RECENT_LIMIT = 8;

const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/**
 * Jogos realmente INICIADOS (last_played_at é gravado pelo backend no instante do lançamento;
 * abrir a página de detalhes não altera), do mais recente ao mais antigo, no máximo `limit`.
 * Jogos nunca iniciados não entram. Empate de horário: ordem alfabética (estável).
 */
export function recentGames(games: Game[], limit = RECENT_LIMIT): Game[] {
  return games
    .filter((g) => !!g.lastPlayedAt)
    .sort((a, b) => cmp(b.lastPlayedAt!, a.lastPlayedAt!) || a.title.localeCompare(b.title, "pt"))
    .slice(0, limit);
}
