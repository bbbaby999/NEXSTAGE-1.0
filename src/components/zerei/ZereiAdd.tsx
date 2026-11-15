import { useRef, useState } from "react";
import Modal from "../Modal";
import Button from "../Button";
import { api, errMsg } from "../../lib/api";
import { useSession } from "../../state/session";
import type { ZereiHit } from "../../types";
import SourceCredits from "./SourceCredits";
import { t } from "../../lib/i18n";

/**
 * Pesquisa de jogos para o Zerei! (IGDB + RAWG; sem eles, pesquisa pública). Mostra capa, nome, ano e
 * plataformas; o usuário escolhe o resultado certo (ou cadastra à mão) e segue para o formulário.
 * Nada depende da biblioteca principal: o jogo não precisa estar instalado nem cadastrado.
 */
export default function ZereiAdd({ onPick, onManual, onClose }: {
  onPick: (h: ZereiHit) => void; onManual: (title: string) => void; onClose: () => void;
}) {
  const { toast } = useSession();
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<ZereiHit[] | null>(null);
  const [fallback, setFallback] = useState(false);
  const [busy, setBusy] = useState(false);
  const body = useRef<HTMLDivElement>(null);

  async function search() {
    if (!query.trim()) return;
    setBusy(true);
    try {
      const r = await api.zereiSearch(query);
      setHits(r.hits);
      setFallback(r.fallback);
      requestAnimationFrame(() => body.current?.querySelector<HTMLElement>(".is-hit")?.focus());
    } catch (e) {
      setHits([]);
      toast(errMsg(e), "err");
    }
    setBusy(false);
  }

  const plats = (h: ZereiHit) => (h.platforms.length > 4 ? `${h.platforms.slice(0, 4).join(", ")} +${h.platforms.length - 4}` : h.platforms.join(", "));

  return (
    <Modal title={t("Adicionar ao Zerei!")} onClose={onClose} wide>
      <div className="is-body" ref={body}>
        <form className="is-search" onSubmit={(e) => { e.preventDefault(); search(); }}>
          <input className="search" data-autofocus value={query} onChange={(e) => setQuery(e.target.value)}
            placeholder={t("Pesquisar jogo pelo nome")} aria-label={t("Pesquisar jogo pelo nome")} />
          <Button type="submit" variant="primary" disabled={busy || !query.trim()}>{busy ? t("Pesquisando…") : t("Pesquisar")}</Button>
        </form>
        {fallback && (
          <p className="muted note">{t("Resultados da pesquisa pública. Para dados mais completos (ano e todas as plataformas), configure o IGDB em Configurações → Fontes de dados ou a chave do RAWG em Pesquisa de imagens.")}</p>
        )}
        {hits === null ? (
          <p className="muted note">{t("Pesquise pelo nome e escolha o resultado certo. O jogo não precisa estar instalado nem na biblioteca.")}</p>
        ) : busy && !hits.length ? (
          <div className="is-loading">{t("Pesquisando em bancos de dados de jogos…")}</div>
        ) : hits.length === 0 ? (
          <div className="is-loading">{t("Nenhum jogo encontrado. Tente o nome original ou mais curto.")}</div>
        ) : (
          <div className="is-hits">
            {hits.map((h, i) => (
              <button key={`${h.title}-${h.igdbId ?? h.rawgId ?? i}`} type="button" data-focus className="is-hit zr-hit" onClick={() => onPick(h)}
                aria-label={[h.title, h.year, plats(h)].filter(Boolean).join(", ")}>
                <span className="is-hit-cover">
                  {h.cover || h.background ? <img src={h.cover ?? h.background ?? ""} alt="" loading="lazy" referrerPolicy="no-referrer" onError={(e) => { e.currentTarget.style.display = "none"; }} /> : null}
                  <b>{h.title.charAt(0).toUpperCase()}</b>
                </span>
                <span className="is-hit-text">
                  <strong>{h.title}</strong>
                  <small>{h.year ?? t("Ano não informado")}</small>
                  <small className="zr-hit-plats">{plats(h) || t("Plataformas não informadas")}</small>
                  <i>{h.sources.join(" · ")}</i>
                </span>
              </button>
            ))}
          </div>
        )}
      </div>
      {hits && hits.length > 0 && (
        <SourceCredits igdb={hits.some((h) => h.sources.includes("IGDB"))} rawg={hits.some((h) => h.sources.includes("RAWG"))}
          sgdb={hits.some((h) => h.sources.includes("SteamGridDB"))} />
      )}
      <div className="row end is-foot">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button onClick={() => onManual(query.trim())}>{t("Cadastrar manualmente")}</Button>
      </div>
    </Modal>
  );
}
