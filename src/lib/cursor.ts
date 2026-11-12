import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onPadActivity } from "./inputDevice";

/** Movimento mínimo (px, somado) para o cursor reaparecer: ignora tremidas e eventos sintéticos sem deslocamento. */
const REVEAL_PX = 6;

/**
 * Cursor do mouse invisível enquanto o controle está em uso; reaparece ao mexer, clicar ou rolar o mouse.
 * - Esconde no nível da janela (o Windows deixa de desenhar o cursor sobre o NEXSTAGE) e, por garantia,
 *   também via CSS (`html.cursor-hidden`).
 * - Escondido, a página não recebe "hover": um cursor parado e invisível não dispara efeitos quando o
 *   conteúdo rola por baixo dele (ex.: o fundo da Home trocando ao rolar o carrossel com o controle).
 */
export function useAutoHideCursor() {
  useEffect(() => {
    const root = document.documentElement;
    const win = (() => { try { return getCurrentWindow(); } catch { return null; } })();
    let hidden = false, moved = 0, lx = -1, ly = -1;

    const set = (hide: boolean) => {
      if (hide === hidden) return;
      hidden = hide;
      moved = 0;
      root.classList.toggle("cursor-hidden", hide);
      win?.setCursorVisible(!hide).catch(() => {}); // sem permissão/indisponível: fica só o CSS
    };
    const onMove = (e: MouseEvent) => {
      if (lx >= 0) moved += Math.abs(e.clientX - lx) + Math.abs(e.clientY - ly);
      lx = e.clientX; ly = e.clientY;
      if (hidden && moved >= REVEAL_PX) set(false);
    };
    const reveal = () => set(false);
    const offPad = onPadActivity(() => set(true));

    window.addEventListener("mousemove", onMove, true);
    window.addEventListener("mousedown", reveal, true);
    window.addEventListener("wheel", reveal, true);
    return () => {
      offPad();
      window.removeEventListener("mousemove", onMove, true);
      window.removeEventListener("mousedown", reveal, true);
      window.removeEventListener("wheel", reveal, true);
      set(false);
    };
  }, []);
}
