import { ReactNode } from "react";
import Modal from "./Modal";
import Button from "./Button";
import { t } from "../lib/i18n";

export interface PickItem {
  id: string | number;
  title: string;
  subtitle?: string;
  image?: string | null;
  /** Aviso (ex.: "não consta para esta plataforma"): o item fica esmaecido. */
  warning?: string | null;
  /** Destaque (ex.: "mesmo AppID da Steam"). */
  highlight?: string | null;
  /** Conteúdo à direita (ex.: nota). */
  side?: ReactNode;
}

/**
 * Confirmação de identidade de um jogo numa fonte externa (Metacritic/RAWG, IGDB, Nexus Mods).
 * Usado sempre que a correspondência não é segura: nada é vinculado sem a escolha do usuário.
 */
export default function CandidatePicker({ title, intro, items, onPick, onClose, footer }: {
  title: string; intro: string; items: PickItem[]; onPick: (id: PickItem["id"]) => void; onClose: () => void; footer?: ReactNode;
}) {
  return (
    <Modal title={title} onClose={onClose} wide>
      <p className="muted">{intro}</p>
      {footer}
      <div className="mc-cands">
        {items.length === 0 && <p className="muted">{t("Nenhum resultado.")}</p>}
        {items.map((c) => (
          <button key={c.id} data-focus type="button" className={`mc-cand ${c.warning ? "other" : ""}`} onClick={() => onPick(c.id)}>
            <span className="mc-cand-img" style={c.image ? { backgroundImage: `url(${c.image})` } : undefined} />
            <span className="mc-cand-text">
              <b>{c.title}</b>
              {c.subtitle && <small>{c.subtitle}</small>}
              {c.highlight && <small className="cand-ok">{c.highlight}</small>}
              {c.warning && <small className="ra-err">{c.warning}</small>}
            </span>
            {c.side}
          </button>
        ))}
      </div>
      <div className="row end"><Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button></div>
    </Modal>
  );
}
