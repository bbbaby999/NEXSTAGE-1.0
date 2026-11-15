import { ReactNode, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { play } from "../lib/sound";
import { markLayerClosed } from "../lib/nav";

/** Modal com Escape (ou botão B do controle) para fechar; só o mais recente reage. */
export default function Modal({ title, onClose, children, wide, xl, full, portal, className }: {
  title: string; onClose: () => void; children: ReactNode; wide?: boolean;
  /** Janela larga para telas de resultados (ex.: importação da Steam). */
  xl?: boolean;
  /**
   * Tela cheia, renderizada direto no <body>. Necessário quando a janela abre DE DENTRO de outra:
   * a animação 3D (transform) do modal de fora prenderia o `position: fixed` desta à caixa dele (cortada).
   */
  full?: boolean;
  /**
   * Janela normal, mas renderizada direto no <body> (mesmo motivo de `full`): para janelas abertas de
   * dentro de outra (ex.: pesquisa de imagens a partir de "Editar jogo"), que não podem ficar presas à
   * caixa — e à rolagem — da janela de fora.
   */
  portal?: boolean;
  /** Classe extra na caixa da janela (layout próprio). */
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const prevFocus = document.activeElement as HTMLElement | null;
    play("open");
    // foco inicial: o item principal da janela ([data-autofocus]), senão o primeiro navegável
    const first = ref.current?.querySelector<HTMLElement>("[data-autofocus]") ?? ref.current?.querySelector<HTMLElement>("[data-focus]:not([disabled]), input, textarea, select");
    first?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const all = document.querySelectorAll(".overlay");
      if (all[all.length - 1] === ref.current) {
        play("back");
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      markLayerClosed();
      play("close");
      prevFocus?.focus?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (full) return createPortal(
    <div className="overlay full" ref={ref} role="dialog" aria-modal="true" aria-label={title}>
      <div className={`modal full ${wide ? "wide" : ""}`}>
        <div className="modal-full-body">
          <h2>{title}</h2>
          {children}
        </div>
      </div>
    </div>,
    document.body
  );
  const box = (
    <div className="overlay" ref={ref} role="dialog" aria-modal="true" aria-label={title}>
      <div className={`modal ${wide ? "wide" : ""} ${xl ? "xl" : ""} ${className ?? ""}`}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
  return portal ? createPortal(box, document.body) : box;
}
