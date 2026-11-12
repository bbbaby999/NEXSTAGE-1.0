/**
 * Gameboot: transição curta (≈3,5 s, com áudio original) exibida SOMENTE ao iniciar um jogo pelo
 * botão Jogar. Fluxo: valida o jogo → NEXSTAGE em primeiro plano e em tela cheia → animação completa
 * com áudio → tela escura → SÓ ENTÃO o jogo é iniciado (o backend minimiza o NEXSTAGE ao lançar;
 * por isso o lançamento não pode acontecer durante a animação).
 * A animação fica em <GameBootHost /> (montado no App); aqui só existe o pedido.
 */
import { reducedMotion } from "./motion";

export interface GamebootGame { title: string; cover: string | null; platform: string }
export interface GamebootRequest {
  game: GamebootGame;
  launch: () => Promise<void>;
  resolve: () => void;
  reject: (e: unknown) => void;
}

let enabled = true;
export const setGamebootEnabled = (on: boolean) => { enabled = on; };

/**
 * Inicia o jogo com a animação (quando ligada); resolve/rejeita conforme o lançamento real.
 * `check` valida antes da animação: um erro (executável ausente, sem emulador...) aparece na hora.
 */
export async function launchWithGameboot(game: GamebootGame, launch: () => Promise<void>, check?: () => Promise<void>): Promise<void> {
  if (!enabled || reducedMotion()) return launch();
  if (check) await check();
  return new Promise<void>((resolve, reject) => {
    const ev = new CustomEvent<GamebootRequest>("nexstage:gameboot", { detail: { game, launch, resolve, reject }, cancelable: true });
    window.dispatchEvent(ev);
    if (!ev.defaultPrevented) launch().then(resolve, reject); // host ausente ou ocupado: lança direto
  });
}
