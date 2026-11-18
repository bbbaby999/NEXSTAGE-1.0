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

/**
 * Mesmo esquema para a página de um jogo do Game Pass aberta pela Home: ao voltar, o carrossel devolve o
 * foco ao cartão que abriu a página (e na mesma aba).
 */
let lastGamePass: { key: string; at: number } | null = null;

export function rememberGamePass(key: string) { lastGamePass = { key, at: Date.now() }; }

export function takeGamePass(): string | null {
  const v = lastGamePass && Date.now() - lastGamePass.at < 60_000 ? lastGamePass.key : null;
  lastGamePass = null;
  return v;
}
