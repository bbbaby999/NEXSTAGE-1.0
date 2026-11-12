import { useEffect, useRef, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import type { GameHit, ImageSet, WebShot } from "../types";
import { t } from "../lib/i18n";

export type SearchMode = "screenshots" | "cover" | "background";

const SOURCE: Record<string, string> = {
  steam: "Steam", libretro: "libretro", wikipedia: "Wikipédia", steamgriddb: "SteamGridDB", rawg: "RAWG",
};
const TITLE: Record<SearchMode, string> = {
  screenshots: "Pesquisar imagens na internet", cover: "Pesquisar capa na internet", background: "Pesquisar fundo na internet",
};

/**
 * Pesquisa inteligente de imagens, sempre com escolha manual:
 *  1. pesquisa o título → lista de jogos (capa, título, plataforma, ano) — nada é adicionado;
 *  2. escolhido o jogo → galeria só com as imagens DELE (capas, fundos, screenshots);
 *  3. o usuário marca as imagens e confirma. Em "screenshots" escolhe várias; em "cover"/"background",
 *     uma (e quem chamou só a aplica ao salvar o formulário).
 * Controle/teclado: tudo é navegável por foco; Voltar fecha (ou volta aos resultados).
 */
export default function ImageSearch({ mode, term, platform, onConfirm, onClose }: {
  mode: SearchMode; term: string; platform: string; onConfirm: (shots: WebShot[]) => Promise<void> | void; onClose: () => void;
}) {
  const { toast } = useSession();
  const [query, setQuery] = useState(term);
  const [hits, setHits] = useState<GameHit[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [game, setGame] = useState<GameHit | null>(null);
  const [images, setImages] = useState<ImageSet | null>(null);
  const [picked, setPicked] = useState<WebShot[]>([]);
  const [preview, setPreview] = useState<WebShot | null>(null);
  const [saving, setSaving] = useState(false);
  const body = useRef<HTMLDivElement>(null);
  const multi = mode === "screenshots";

  const focusFirst = (sel: string) => requestAnimationFrame(() => body.current?.querySelector<HTMLElement>(sel)?.focus());

  async function search(q = query) {
    if (!q.trim()) return;
    setBusy(true);
    setGame(null);
    setImages(null);
    setPicked([]);
    try {
      const r = await api.searchGames(q, platform);
      setHits(r);
      focusFirst(".is-hit");
    } catch (e) {
      setHits([]);
      toast(errMsg(e), "err");
    }
    setBusy(false);
  }

  async function choose(h: GameHit) {
    setGame(h);
    setImages(null);
    setPicked([]);
    setPreview(null);
    setBusy(true);
    try {
      const set = await api.gameImages(h.sources);
      setImages(set);
      const first = order(set)[0]?.[1][0] ?? null;
      setPreview(first);
      focusFirst(".is-tile");
    } catch (e) {
      toast(errMsg(e), "err");
      setGame(null);
    }
    setBusy(false);
  }

  // abre já pesquisando o título do jogo
  useEffect(() => { search(term); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, []);

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

  function back() {
    setGame(null);
    setImages(null);
    setPicked([]);
    focusFirst(".is-hit");
  }

  /** Seções na ordem que faz sentido para o uso (capa → capas primeiro; fundo → fundos; galeria → screenshots). */
  function order(set: ImageSet): [string, WebShot[]][] {
    const s: [string, WebShot[]][] =
      mode === "cover" ? [["Capas", set.covers], ["Fundos", set.backgrounds], ["Screenshots", set.screenshots]]
      : mode === "background" ? [["Fundos", set.backgrounds], ["Screenshots", set.screenshots], ["Capas", set.covers]]
      : [["Screenshots", set.screenshots], ["Capas", set.covers], ["Fundos", set.backgrounds]];
    return s.filter((x) => x[1].length > 0);
  }

  const isPicked = (s: WebShot) => picked.some((p) => p.url === s.url);
  const toggle = (s: WebShot) => {
    setPreview(s);
    if (multi) setPicked((p) => (isPicked(s) ? p.filter((x) => x.url !== s.url) : [...p, s]));
    else setPicked(isPicked(s) ? [] : [s]);
  };

  async function confirm() {
    if (!picked.length) return;
    setSaving(true);
    try {
      await onConfirm(picked);
      onClose();
    } catch (e) {
      toast(errMsg(e), "err");
    }
    setSaving(false);
  }

  const meta = (h: GameHit) => [h.platform, h.year].filter(Boolean).join(" · ");
  const sources = (h: GameHit) => [...new Set(h.sources.map((s) => SOURCE[s.source] ?? s.source))].join(" · ");

  return (
    <Modal title={TITLE[mode]} onClose={onClose} wide>
      <div className="is-body" ref={body}>
        {!game ? (
          <>
            <form className="is-search" onSubmit={(e) => { e.preventDefault(); search(); }}>
              <input className="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("Nome do jogo")} aria-label={t("Nome do jogo")} />
              <Button type="submit" variant="primary" disabled={busy}>{busy ? t("Pesquisando…") : t("Pesquisar")}</Button>
            </form>
            <p className="muted note">{t("Escolha o jogo correto (confira plataforma e ano). Nenhuma imagem é adicionada sem a sua confirmação.")}</p>
            {hits === null || (busy && !hits.length) ? (
              <div className="is-loading">{t("Pesquisando em bancos de dados de jogos…")}</div>
            ) : hits.length === 0 ? (
              <div className="is-loading">{t("Nenhum jogo encontrado. Tente o nome original ou mais curto.")}</div>
            ) : (
              <div className="is-hits">
                {hits.map((h, i) => (
                  <button key={`${h.title}-${i}`} type="button" data-focus className="is-hit" onClick={() => choose(h)} disabled={busy}
                    aria-label={`${h.title}${meta(h) ? `, ${meta(h)}` : ""}`}>
                    <span className="is-hit-cover">
                      {h.cover ? <img src={h.cover} alt="" loading="lazy" referrerPolicy="no-referrer" onError={(e) => { e.currentTarget.style.display = "none"; }} /> : null}
                      <b>{h.title.charAt(0).toUpperCase()}</b>
                    </span>
                    <span className="is-hit-text">
                      <strong>{h.title}</strong>
                      <small>{meta(h) || t("Plataforma e ano não informados")}</small>
                      <i>{sources(h)}</i>
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
              <div><strong>{game.title}</strong><small>{meta(game)}</small></div>
            </div>
            {!images ? (
              <div className="is-loading">{t("Carregando as imagens deste jogo…")}</div>
            ) : (
              <>
                {preview && (
                  <div className="is-preview" style={{ backgroundImage: `url("${preview.thumb}")` }}>
                    <img key={preview.url} src={preview.url} alt="" referrerPolicy="no-referrer"
                      onError={(e) => { const i = e.currentTarget; if (i.src !== preview.thumb) i.src = preview.thumb; }} />
                    <span>{SOURCE[preview.source] ?? preview.source}{preview.title && preview.title !== SOURCE[preview.source] ? ` · ${preview.title}` : ""}</span>
                  </div>
                )}
                {order(images).map(([label, list]) => (
                  <section key={label}>
                    <h3 className="section sub">{label} <small>({list.length})</small></h3>
                    <div className={`is-grid ${label === "Capas" ? "tall" : ""}`}>
                      {list.map((s) => (
                        <button key={s.url} type="button" data-focus className={`is-tile ${isPicked(s) ? "on" : ""}`}
                          onClick={() => toggle(s)} onFocus={() => setPreview(s)} aria-pressed={isPicked(s)}
                          aria-label={`${label}: ${SOURCE[s.source] ?? s.source}${isPicked(s) ? ", selecionada" : ""}`}>
                          <img src={s.thumb} alt="" loading="lazy" referrerPolicy="no-referrer" />
                          {isPicked(s) && <i className="is-check" aria-hidden="true">✓</i>}
                        </button>
                      ))}
                    </div>
                  </section>
                ))}
              </>
            )}
          </>
        )}
      </div>
      <div className="row end is-foot">
        <small className="muted">
          {game && images ? (multi ? t("{length} selecionada(s)", { length: picked.length }) : picked.length ? t("1 imagem escolhida") : t("Escolha uma imagem")) : ""}
        </small>
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        {game && images && (
          <Button variant="primary" disabled={!picked.length || saving} onClick={confirm}>
            {saving ? t("Aplicando…") : multi ? (picked.length ? t("Adicionar {n} à galeria", { n: picked.length }) : t("Adicionar à galeria")) : t("Usar esta imagem")}
          </Button>
        )}
      </div>
    </Modal>
  );
}
