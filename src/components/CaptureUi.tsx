import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import Modal from "./Modal";
import Button from "./Button";
import { play } from "../lib/sound";
import { formatDateTime } from "../lib/format";
import type { GalleryItem } from "../types";
import { t } from "../lib/i18n";

export interface MenuAction { label: string; icon: string; danger?: boolean; run: () => void }

/** Ícone SVG simples (traço) usado nos controles da Galeria. */
export function Icon({ d, size = 20 }: { d: string; size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d={d} />
    </svg>
  );
}
export const ICON = {
  prev: "M15 5l-7 7 7 7",
  next: "M9 5l7 7-7 7",
  star: "M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z",
  back: "M10 6L4 12l6 6M4 12h16",
  more: "M5 12h.01M12 12h.01M19 12h.01",
  open: "M4 5h16v14H4zM4 15l5-5 4 4 3-3 4 4",
  select: "M5 12l4 4 10-10",
  info: "M12 8h.01M11 12h1v5h1M12 3a9 9 0 100 18 9 9 0 000-18z",
  folder: "M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z",
  trash: "M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3",
  full: "M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5",
  exitFull: "M9 4v5H4M20 9h-5V4M15 20v-5h5M4 15h5v5",
  edit: "M4 20h4L19 9l-4-4L4 16v4zM14 6l4 4",
};

/**
 * Menu de contexto (botão direito, tecla de menu, Shift+F10 ou botão Menu/Options do controle).
 * Segue a mesma camada/navegação das listas: foco no primeiro item, Voltar fecha sem executar nada.
 */
export function CaptureMenu({ actions, x, y, onClose }: { actions: MenuAction[]; x?: number; y?: number; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    play("open");
    ref.current?.querySelector<HTMLElement>(".ctx-item")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopImmediatePropagation();
      e.preventDefault();
      play("back");
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => { window.removeEventListener("keydown", onKey, true); play("close"); prev?.focus?.(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const style = x === undefined || y === undefined
    ? undefined
    : { left: Math.max(8, Math.min(x, innerWidth - 280)), top: Math.max(8, Math.min(y, innerHeight - actions.length * 52 - 24)) };
  return createPortal(
    <div className="ctx-root" ref={ref}>
      <div className="ctx-backdrop" onClick={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }} />
      <div className={`ctx-panel ${style ? "at" : "center"}`} style={style} role="menu">
        {actions.map((a) => (
          <button key={a.label} type="button" data-focus role="menuitem" className={`ctx-item ${a.danger ? "danger" : ""}`}
            onClick={() => { onClose(); a.run(); }}>
            <Icon d={a.icon} size={18} /><span>{a.label}</span>
          </button>
        ))}
      </div>
    </div>,
    document.body
  );
}

/** "Sobre esta captura": só mostra o que realmente existe. */
export function AboutCapture({ item, onClose }: { item: GalleryItem; onClose: () => void }) {
  const managedName = item.path.split(/[\\/]/).pop() ?? item.path;
  return (
    <Modal title={t("Sobre esta captura")} onClose={onClose}>
      <dl>
        <dt>{t("Nome do arquivo")}</dt><dd className="path">{item.originalName || managedName}</dd>
        <dt>{t("Jogo")}</dt><dd>{item.gameTitle ?? t("Sem jogo associado")}</dd>
        <dt>{t("Data de criação")}</dt><dd>{item.takenAt ? formatDateTime(item.takenAt) : t("Não disponível")}</dd>
        <dt>{t("Importada em")}</dt><dd>{formatDateTime(item.addedAt)}</dd>
        <dt>{t("Armazenada em")}</dt><dd className="path">{item.path}</dd>
        <dt>{t("Arquivo")}</dt><dd>{item.exists ? t("Presente (cópia gerenciada pelo NEXSTAGE)") : t("Ausente no disco")}</dd>
      </dl>
      <p className="muted note">{t("O arquivo original, no local de onde foi importado, não é alterado nem apagado.")}</p>
      <div className="row end"><Button variant="primary" onClick={onClose}>{t("Fechar")}</Button></div>
    </Modal>
  );
}

/** Confirmação de exclusão com a quantidade. Só as cópias do NEXSTAGE são removidas. */
export function ConfirmDelete({ count, onConfirm, onClose }: { count: number; onConfirm: () => void; onClose: () => void }) {
  return (
    <Modal title={count === 1 ? t("Excluir 1 captura?") : t("Excluir {count} capturas?", { count })} onClose={onClose}>
      <p>{count === 1 ? t("Esta captura será removida da Galeria do NEXSTAGE.") : t("Estas {count} capturas serão removidas da Galeria do NEXSTAGE.", { count })}</p>
      <p className="muted note">{t("Somente as cópias gerenciadas pelo NEXSTAGE são apagadas. Os arquivos originais, nas pastas de onde foram importados, permanecem intactos.")}</p>
      <div className="row end">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="danger" onClick={onConfirm}>{t("Excluir")}{" "}{count === 1 ? "captura" : t("{count} capturas", { count })}</Button>
      </div>
    </Modal>
  );
}
