/**
 * "Mais opções" de um jogo em qualquer tela: botão direito do mouse sobre uma capa (Home, Biblioteca,
 * Zerei!, perfil...), o botão "Mais opções" da página do jogo e o botão Menu/Options do controle.
 *
 * Mesmo menu da página do jogo (Zerado, Capturas, Coleções, Abrir pasta, Editar jogo, Remover), com as
 * janelas que ele abre (Coleções, Editar jogo, confirmação de remoção). O host fica montado no App e
 * atende um pedido por vez; depois de mudar algo, avisa as telas com o evento `library-changed`
 * (o mesmo que o backend usa), e cada lista recarrega.
 */
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { emit } from "@tauri-apps/api/event";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import { CaptureMenu, ICON, type MenuAction } from "./CaptureUi";
import Modal from "./Modal";
import Button from "./Button";
import GameForm from "./GameForm";
import CollectionPicker from "./CollectionPicker";
import { t } from "../lib/i18n";
import type { Game } from "../types";

interface Req { id: number; gameId: number; x?: number; y?: number; from?: DOMRect | null }
let seq = 0;
let current: Req | null = null;
const subs = new Set<() => void>();
const notify = () => subs.forEach((f) => f());
const useReq = () => useSyncExternalStore((cb) => { subs.add(cb); return () => { subs.delete(cb); }; }, () => current);

/** Abre o "Mais opções" do jogo. `x/y`: posição do clique (botão direito); `from`: de onde o menu sai na animação. */
export function openGameMenu(gameId: number, at?: { x?: number; y?: number; from?: DOMRect | null }) {
  current = { id: ++seq, gameId, ...at };
  notify();
}
const closeReq = () => { current = null; notify(); };

/** Capa de jogo em foco: o botão Options abre o menu dela (GameMenuHost), não o da tela. */
export const cardFocused = () => !!(document.activeElement as HTMLElement | null)?.closest?.("[data-game-id]");

/** Algo aberto por cima (janela, lista, menu): o menu não abre. */
export const layerOpen = () => !!document.querySelector(".overlay, .ctx-root, .pick-root, .kb-root, .fp-modal, .sm-root:not(.leaving)");

/**
 * Botão direito sobre um cartão de jogo (qualquer elemento com `data-game-id`): abre o "Mais opções"
 * dele. Instalado uma vez pelo host; o menu padrão do navegador é bloqueado em main.tsx.
 */
function useCardContextMenu() {
  useEffect(() => {
    const onCtx = (e: MouseEvent) => {
      const el = (e.target as HTMLElement | null)?.closest?.<HTMLElement>("[data-game-id]");
      const id = Number(el?.dataset.gameId);
      if (!el || !id || layerOpen()) return;
      e.preventDefault();
      const cover = el.querySelector<HTMLElement>(".cover, .replay-cover, .steam-cover") ?? el;
      openGameMenu(id, { x: e.clientX, y: e.clientY, from: cover.getBoundingClientRect() });
    };
    // Botão Menu/Options do controle (ou tecla de menu) com uma capa em foco: o menu desse jogo
    const onPad = () => {
      const el = (document.activeElement as HTMLElement | null)?.closest?.<HTMLElement>("[data-game-id]");
      const id = Number(el?.dataset.gameId);
      if (!el || !id || layerOpen()) return;
      const cover = el.querySelector<HTMLElement>(".cover, .replay-cover, .steam-cover") ?? el;
      openGameMenu(id, { from: cover.getBoundingClientRect() });
    };
    window.addEventListener("contextmenu", onCtx);
    window.addEventListener("nexstage:context", onPad);
    return () => { window.removeEventListener("contextmenu", onCtx); window.removeEventListener("nexstage:context", onPad); };
  }, []);
}

export default function GameMenuHost() {
  const { profile } = useSession();
  useCardContextMenu();
  const req = useReq();
  if (!profile || !req) return null;
  return <GameMenu key={req.id} req={req} profileId={profile.id} />;
}

function GameMenu({ req, profileId }: { req: Req; profileId: number }) {
  const { toast, setOpenGameId, setView, setGalleryIntent } = useSession();
  const [game, setGame] = useState<Game | null>(null);
  const [caps, setCaps] = useState(0);
  const [stage, setStage] = useState<"menu" | "collections" | "edit" | "remove">("menu");
  const stageRef = useRef(stage);
  stageRef.current = stage;

  useEffect(() => {
    let live = true;
    api.listLibrary(profileId)
      .then((l) => { if (!live) return; const g = l.find((x) => x.id === req.gameId); if (g) setGame(g); else closeReq(); })
      .catch(() => closeReq());
    api.listGallery(profileId).then((l) => { if (live) setCaps(l.filter((i) => i.gameId === req.gameId).length); }).catch(() => {});
    return () => { live = false; };
  }, [profileId, req.gameId]);

  if (!game) return null;
  const changed = () => { emit("library-changed").catch(() => {}); };
  const run = async (fn: () => Promise<unknown>, ok?: string) => {
    try { await fn(); if (ok) toast(ok); changed(); } catch (e) { toast(errMsg(e), "err"); }
  };

  const actions: MenuAction[] = [
    // zerado = registro "zerado" no Zerei! ligado a este jogo; editar ou desfazer é lá
    game.completed
      ? { label: t("✓ Zerado"), icon: ICON.select, run: () => { closeReq(); setView("zerei"); } }
      : { label: t("Marcar como zerado"), icon: ICON.select,
          run: () => { closeReq(); run(() => api.zereiMarkGame(profileId, game.id), t("Marcado como zerado: o jogo foi incluído no Zerei!.")); } },
    { label: t("Capturas de tela{v0}", { v0: caps ? ` (${caps})` : "" }), icon: ICON.open,
      run: () => { closeReq(); setGalleryIntent({ gameId: game.id }); setView("gallery"); } },
    { label: t("Coleções"), icon: ICON.folder, run: () => setStage("collections") },
    { label: t("Abrir pasta de arquivos"), icon: ICON.folder,
      run: () => { closeReq(); api.revealGameFolder(profileId, game.id).catch((e) => toast(errMsg(e), "err")); } },
    { label: t("Editar jogo"), icon: ICON.edit, run: () => setStage("edit") },
    { label: t("Remover"), icon: ICON.trash, danger: true, run: () => setStage("remove") },
  ];

  if (stage === "collections") return <CollectionPicker profileId={profileId} gameId={game.id} onClose={() => { changed(); closeReq(); }} />;
  if (stage === "edit") return <GameForm profileId={profileId} game={game} onClose={closeReq} onSaved={() => { changed(); closeReq(); }} />;
  if (stage === "remove") {
    return (
      <Modal title={t("Remover {title} da biblioteca?", { title: game.title })} onClose={closeReq}>
        <p className="muted">{t("O jogo e o histórico de sessões dele saem da biblioteca deste perfil. Os arquivos do jogo no computador não são apagados.")}</p>
        <div className="row end">
          <Button variant="ghost" onClick={closeReq}>{t("Cancelar")}</Button>
          <Button variant="danger" onClick={() => { closeReq(); run(async () => { await api.removeGame(profileId, game.id); setOpenGameId(null); }, t("Jogo removido da biblioteca.")); }}>{t("Confirmar remoção")}</Button>
        </div>
      </Modal>
    );
  }
  // CaptureMenu fecha antes de executar a ação: só encerra o pedido se a ação não abriu uma janela
  return <CaptureMenu actions={actions} from={req.from} hero={{ cover: game.cover, title: game.title, sub: game.platform }}
    onClose={() => setTimeout(() => { if (current?.id === req.id && stageRef.current === "menu") closeReq(); }, 0)} />;
}
