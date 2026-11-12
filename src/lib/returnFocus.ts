/**
 * Lembra qual jogo estava aberto na página de detalhes quando ela foi fechada, para que a
 * Biblioteca devolva o foco a esse jogo ao voltar (em vez de ir para o primeiro da lista).
 * Só vale se a Biblioteca abrir logo em seguida (voltar direto), não numa visita posterior.
 */
let last: { id: number; at: number } | null = null;

export function rememberClosedGame(id: number) { last = { id, at: Date.now() }; }

export function takeClosedGame(): number | null {
  const v = last && Date.now() - last.at < 1500 ? last.id : null;
  last = null;
  return v;
}
