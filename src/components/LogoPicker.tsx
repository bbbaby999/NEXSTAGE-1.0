import { useEffect, useRef, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import SourceCredits from "./zerei/SourceCredits";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import type { LogoGame, WebShot } from "../types";
import { t } from "../lib/i18n";

/**
 * Logos do SteamGridDB (usado pela Biblioteca e pelo Zerei!; a pesquisa de imagens não tem logos).
 * Mesmo fluxo da pesquisa de imagens, sempre com escolha manual:
 *  1. pesquisa o nome → lista de jogos do SteamGridDB (capa, nome, ano);
 *  2. escolhido o jogo → galeria só com as logos DELE (prévia grande + miniaturas);
 *  3. o usuário marca uma logo e confirma.
 * Controle/teclado: tudo navegável por foco; Voltar na galeria volta à lista de jogos.
 */
export default function LogoPicker({ title, current, onPick, onClose }: {
  title: string; year?: number | null; sgdbId?: number | null; current: string | null;
  onPick: (url: string, sgdbId: number | null) => void; onClose: () => void;
}) {
  const { toast } = useSession();
  const [query, setQuery] = useState(title);
  const [games, setGames] = useState<LogoGame[] | null>(null);
  const [hasKey, setHasKey] = useState(true);
  const [busy, setBusy] = useState(false);
  const [game, setGame] = useState<LogoGame | null>(null);
  const [logos, setLogos] = useState<WebShot[] | null>(null);
  const [picked, setPicked] = useState<string | null>(current);
  const [focused, setFocused] = useState<string | null>(null);
  const body = useRef<HTMLDivElement>(null);
  const focusFirst = (sel: string) => requestAnimationFrame(() => body.current?.querySelector<HTMLElement>(sel)?.focus());

  async function search(q = query) {
    if (!q.trim()) return;
    setBusy(true);
    setGame(null);
    setLogos(null);
    try {
      const r = await api.sgdbGames(q);
      setHasKey(r.sgdb);
      setGames(r.games);
      focusFirst(".is-hit");
    } catch (e) {
      setGames([]);
      toast(errMsg(e), "err");
    }
    setBusy(false);
  }

  async function choose(g: LogoGame) {
    setGame(g);
    setLogos(null);
    setFocused(null);
    setBusy(true);
    try {
      const a = await api.zereiArtwork(g.name, g.year, g.id);
      setLogos(a.logos);
      if (!a.logos.some((l) => l.url === picked)) setPicked(null);
      focusFirst(".is-tile");
    } catch (e) {
      toast(errMsg(e), "err");
      setGame(null);
    }
    setBusy(false);
  }

  function back() {
    setGame(null);
    setLogos(null);
    focusFirst(".is-hit");
  }

  // abre já pesquisando o nome do jogo
  useEffect(() => { search(title); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, []);

  // Voltar (Esc/○) na galeria: volta à lista de jogos em vez de fechar tudo
  useEffect(() => {
    if (!game) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(".kb-root, .pick-root")) return;
      e.stopImmediatePropagation();
      e.preventDefault();
      back();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [game]);

  // prévia grande: a miniatura em foco (controle/teclado/mouse), senão a escolhida, senão a primeira
  const preview = focused ?? picked ?? logos?.[0]?.url ?? null;

  return (
    <Modal title={t("Escolher logo")} onClose={onClose} xl portal className="is-modal">
      <div className={`is-body ${game && logos && logos.length > 0 ? "picking" : ""}`} ref={body}>
        {!hasKey ? (
          <div className="is-loading">{t("Salve uma chave do SteamGridDB em Configurações → Pesquisa de imagens para escolher logos e artes.")}</div>
        ) : !game ? (
          <>
            <form className="is-search" onSubmit={(e) => { e.preventDefault(); search(); }}>
              <input className="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("Nome do jogo")} aria-label={t("Nome do jogo")} />
              <Button type="submit" variant="primary" disabled={busy}>{busy ? t("Pesquisando…") : t("Pesquisar")}</Button>
            </form>
            <p className="muted note">{t("Escolha o jogo correto (confira o ano). Nenhuma logo é usada sem a sua confirmação.")}</p>
            {games === null || (busy && !games.length) ? (
              <div className="is-loading">{t("Pesquisando no SteamGridDB…")}</div>
            ) : games.length === 0 ? (
              <div className="is-loading">{t("Nenhum jogo encontrado. Tente o nome original ou mais curto.")}</div>
            ) : (
              <div className="is-hits">
                {games.map((g) => (
                  <button key={g.id} type="button" data-focus className="is-hit" onClick={() => choose(g)} disabled={busy}
                    aria-label={`${g.name}${g.year ? `, ${g.year}` : ""}`}>
                    <span className="is-hit-cover">
                      {g.cover ? <img src={g.cover} alt="" loading="lazy" referrerPolicy="no-referrer" onError={(e) => { e.currentTarget.style.display = "none"; }} /> : null}
                      <b>{g.name.charAt(0).toUpperCase()}</b>
                    </span>
                    <span className="is-hit-text">
                      <strong>{g.name}</strong>
                      <small>{g.year ?? t("Ano não informado")}</small>
                      <i>SteamGridDB</i>
                    </span>
                  </button>
                ))}
              </div>
            )}
          </>
        ) : (
          <>
            <div className="is-picked-head">
              <Button variant="ghost" onClick={back}>{t("← Outros jogos")}</Button>
              <div><strong>{game.name}</strong><small>{game.year ?? ""}</small></div>
            </div>
            {!logos ? (
              <div className="is-loading">{t("Carregando artes…")}</div>
            ) : logos.length === 0 ? (
              <div className="is-loading">{t("Este jogo não tem logos no SteamGridDB. Volte e escolha outro resultado.")}</div>
            ) : (
              // jogo escolhido: prévia grande à esquerda e logos à direita (só elas rolam, se precisar)
              <div className="is-picked">
                <div className="is-stage">
                {preview && (
                  <div className="is-preview zr-logo-preview">
                    <img key={preview} src={preview} alt="" referrerPolicy="no-referrer" />
                    <span>SteamGridDB</span>
                  </div>
                )}
                </div>
                <div className="is-lists">
                <section>
                  <h3 className="section sub">{t("Logos")} <small>({logos.length})</small></h3>
                  <div className="is-grid zr-logo-grid">
                    {logos.map((s) => (
                      <button key={s.url} type="button" data-focus className={`is-tile ${picked === s.url ? "on" : ""}`}
                        onClick={() => setPicked(picked === s.url ? null : s.url)} onFocus={() => setFocused(s.url)} onMouseEnter={() => setFocused(s.url)}
                        aria-pressed={picked === s.url} aria-label={t("Logos")}>
                        <img src={s.thumb} alt="" loading="lazy" referrerPolicy="no-referrer" />
                        {picked === s.url && <i className="is-check" aria-hidden="true">✓</i>}
                      </button>
                    ))}
                  </div>
                </section>
                <SourceCredits sgdbId={game.id} sgdb />
                </div>
              </div>
            )}
          </>
        )}
      </div>
      <div className="row end is-foot">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        {game && logos && logos.length > 0 && (
          <Button variant="primary" disabled={!picked} onClick={() => { if (picked) onPick(picked, game.id); onClose(); }}>{t("Usar esta imagem")}</Button>
        )}
      </div>
    </Modal>
  );
}
