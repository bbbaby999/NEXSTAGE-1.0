import { useState } from "react";
import Button from "../Button";
import SourceCredits from "./SourceCredits";
import type { ZereiEntry } from "../../types";
import { COMPLETION_LABEL, DIFFICULTY_LABEL, fmtDay, fmtHours, fmtRating, statusLabel } from "../../lib/zerei";
import { t } from "../../lib/i18n";

/** Página de detalhes de um registro do Zerei! (arte de fundo, logo, dados e outras conclusões do mesmo jogo). */
export default function ZereiDetail({ entry, siblings, onEdit, onDelete, onOpen, onBack }: {
  entry: ZereiEntry;
  /** Outros registros do mesmo jogo (outras plataformas ou conclusões). */
  siblings: ZereiEntry[];
  onEdit: () => void; onDelete: () => void; onOpen: (id: number) => void; onBack: () => void;
}) {
  const [confirm, setConfirm] = useState(false);
  const e = entry;
  const art = e.background ?? e.cover;
  const facts: [string, string][] = [
    [t("Status"), statusLabel(e.status)],
    [t("Tipo de conclusão"), e.completionType ? t(COMPLETION_LABEL[e.completionType]) : "—"],
    [t("Data de início"), fmtDay(e.startedAt)],
    [t("Data em que zerou"), fmtDay(e.completedAt)],
    [t("Tempo jogado"), fmtHours(e.hours)],
    [t("Nota pessoal"), e.rating === null ? "—" : `${fmtRating(e.rating)} / 10`],
    [t("Dificuldade pessoal"), e.difficulty ? t(DIFFICULTY_LABEL[e.difficulty]) : "—"],
    [t("Lançamento"), e.year ? String(e.year) : "—"],
  ];

  return (
    <div className="zr-detail">
      <div className="zr-hero" style={art ? { backgroundImage: `url("${art}")` } : undefined}>
        <div className="zr-hero-shade" />
        <div className="zr-hero-body">
          <div className="cover zr-hero-cover" style={e.cover ? { backgroundImage: `url("${e.cover}")` } : undefined}>
            {!e.cover && <span>{e.title.charAt(0).toUpperCase()}</span>}
          </div>
          <div className="zr-hero-text">
            {e.logo ? <img className="zr-hero-logo" src={e.logo} alt={e.title} referrerPolicy="no-referrer" /> : <h1>{e.title}</h1>}
            <div className="zr-chips">
              <span className={`zr-chip st-${e.status}`}>{statusLabel(e.status)}</span>
              <span className="zr-chip">{e.platform}</span>
              {e.completionType && <span className={`zr-chip ${e.completionType === "full" ? "gold" : ""}`}>{t(COMPLETION_LABEL[e.completionType])}</span>}
              {e.run > 1 && <span className="zr-chip">{t("{n}ª conclusão", { n: e.run })}</span>}
            </div>
            <div className="row zr-actions" data-nav-group>
              <Button variant="primary" data-autofocus onClick={onEdit}>{t("Editar")}</Button>
              {confirm ? (
                <>
                  <Button variant="danger" onClick={onDelete}>{t("Confirmar exclusão")}</Button>
                  <Button variant="ghost" onClick={() => setConfirm(false)}>{t("Cancelar")}</Button>
                </>
              ) : <Button variant="danger" onClick={() => setConfirm(true)}>{t("Excluir")}</Button>}
              <Button variant="ghost" onClick={onBack}>{t("← Voltar")}</Button>
            </div>
          </div>
          {e.rating !== null && (
            <div className="zr-score" aria-label={t("Nota pessoal")}>
              <b>{fmtRating(e.rating)}</b><small>/ 10</small>
            </div>
          )}
        </div>
      </div>

      <div className="zr-detail-grid">
        <section className="zr-panel">
          <h3 className="section sub">{t("Registro")}</h3>
          <dl className="zr-facts">
            {facts.map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}
          </dl>
          {e.platforms.length > 0 && (
            <p className="muted zr-avail">{t("Disponível em")}: {e.platforms.join(", ")}</p>
          )}
        </section>
        <section className="zr-panel">
          <h3 className="section sub">{t("Comentários e observações")}</h3>
          {e.notes ? <p className="zr-notes">{e.notes}</p> : <p className="muted">{t("Nenhum comentário.")}</p>}
        </section>
        {siblings.length > 0 && (
          <section className="zr-panel wide">
            <h3 className="section sub">{t("Outros registros deste jogo")}</h3>
            <div className="zr-sibs" data-nav-group>
              {siblings.map((s) => (
                <button key={s.id} type="button" data-focus data-zerei-sib={s.id} className="zr-sib" onClick={() => onOpen(s.id)}>
                  <strong>{s.platform}{s.run > 1 ? ` · ${t("{n}ª conclusão", { n: s.run })}` : ""}</strong>
                  <small>{statusLabel(s.status)} · {fmtDay(s.completedAt)} · {fmtHours(s.hours)}</small>
                </button>
              ))}
            </div>
          </section>
        )}
      </div>
      <SourceCredits igdb={e.igdbId !== null} rawgId={e.rawgId} sgdbId={e.sgdbId} />
      <p className="muted zr-uid">{t("Identificador")}: {e.uid}</p>
    </div>
  );
}
