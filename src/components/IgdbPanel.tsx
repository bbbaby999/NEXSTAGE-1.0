import { Fragment, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Block from "./DpBlock";
import Button from "./Button";
import CandidatePicker from "./CandidatePicker";
import IgdbSlideshow, { type Slide } from "./IgdbSlideshow";
import { api, errMsg } from "../lib/api";
import { formatDate } from "../lib/format";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { Game, GameInfo, IgdbCandidate } from "../types";

/**
 * Dados do IGDB na página do jogo (API oficial). Busca sozinho uma vez por jogo se a fonte estiver
 * ativa; depois usa o cache. Só exibe: as imagens do IGDB nunca alteram a capa ou o fundo do jogo.
 */
export default function IgdbPanel({ game, info, loaded, term, onInfo, onGameChanged }: {
  game: Game; info: GameInfo | null; loaded: boolean; term: string; onInfo: (i: GameInfo) => void; onGameChanged: () => void;
}) {
  const { profile, toast, setView } = useSession();
  const p = profile!;
  const [busy, setBusy] = useState(false);
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [choices, setChoices] = useState<IgdbCandidate[] | null>(null);
  const [story, setStory] = useState(false);
  const [reid, setReid] = useState(false);
  const tried = useRef<number | null>(null);
  const d = info?.igdb ?? null;

  useEffect(() => { api.sourcesList().then((l) => setEnabled(!!l.find((s) => s.id === "igdb")?.enabled)).catch(() => setEnabled(false)); }, []);

  const fetchData = async (silent: boolean, igdbId: number | null = null, reidentify = false) => {
    setBusy(true);
    setReid(reidentify);
    try {
      const r = await api.igdbFetch(p.id, game.id, term || game.title, igdbId, reidentify);
      onInfo(r.info);
      if (r.status === "disabled") { setEnabled(false); return; }
      if (r.status === "choose") { setChoices(r.candidates); setNote(t("Escolha qual destes é o seu jogo.")); return; }
      setChoices(null);
      const msg = r.status === "found" ? null : t("Jogo não encontrado no IGDB com esse nome.");
      setNote(msg);
      if (!silent) toast(msg ?? t("Dados do IGDB atualizados."), msg ? "err" : "ok");
    } catch (e) {
      if (!silent) toast(errMsg(e), "err"); else setNote(errMsg(e));
    } finally { setBusy(false); }
  };

  // primeira visita com a fonte ativa: busca uma única vez
  useEffect(() => {
    if (!loaded || enabled !== true || tried.current === game.id || d) return;
    tried.current = game.id;
    fetchData(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, enabled, game.id, d]);

  if (enabled === false && !d) {
    return (
      <Block title={t("IGDB")}>
        <p className="muted">{t("Ative o IGDB para ver descrição, empresas, franquia, classificação, imagens e trailers do jogo.")}</p>
        <div className="row"><Button onClick={() => setView("settings")}>{t("Configurar fontes de dados")}</Button></div>
      </Block>
    );
  }

  // apresentação: trailers primeiro, depois artes e capturas
  const slides: Slide[] = d ? [
    ...d.videos.map((v) => ({ kind: "video" as const, id: v.id, name: v.name })),
    ...d.artworks.map((url) => ({ kind: "artwork" as const, url })),
    ...d.screenshots.map((url) => ({ kind: "shot" as const, url })),
  ] : [];
  const facts: [string, string | null][] = d ? [
    [t("Desenvolvedora"), d.developers.join(", ") || null],
    [t("Publicadora"), d.publishers.join(", ") || null],
    [t("Lançamento"), d.releaseDate],
    [t("Gêneros"), d.genres.join(", ") || null],
    [t("Plataformas"), d.platforms.join(", ") || null],
    [t("Franquia"), d.franchises.join(", ") || null],
    [t("Série"), d.series.join(", ") || null],
    [t("Classificação indicativa"), d.ageRatings.join(" · ") || null],
  ] : [];

  return (
    <>
      <Block title={t("IGDB")}>
        {!d && <p className="muted">{busy ? t("Buscando no IGDB…") : note ?? t("Sem dados do IGDB para este jogo.")}</p>}
        {d && (
          <>
            <div className="igdb-head">
              {d.cover && (
                <div className="igdb-cover">
                  <img src={d.cover} alt="" loading="lazy" />
                </div>
              )}
              <div className="igdb-text">
                <strong>{d.name}</strong>
                {d.summary && <p className="desc">{d.summary}</p>}
                {d.storyline && (
                  <>
                    <button data-focus type="button" className="igdb-more" onClick={() => setStory(!story)}>{story ? t("Ocultar história") : t("Ler a história")}</button>
                    {story && <p className="desc">{d.storyline}</p>}
                  </>
                )}
              </div>
            </div>
            <dl className="dp-info">
              {facts.filter(([, v]) => v).map(([k, v]) => <Fragment key={k}><dt>{k}</dt><dd>{v}</dd></Fragment>)}
            </dl>
            {slides.length > 0 && <IgdbSlideshow slides={slides} />}
            <p className="muted note">{t("Fonte: IGDB · atualizado em {d}", { d: formatDate(d.fetchedAt) })}</p>
          </>
        )}
        {/* ações na parte inferior do bloco */}
        <div className="row igdb-actions">
          {d?.url && <Button variant="ghost" onClick={() => openUrl(d.url!).catch(() => {})}>{t("Ver no IGDB")}</Button>}
          <Button variant="ghost" disabled={busy || enabled === false} onClick={() => fetchData(false)}>{busy ? t("Buscando…") : d ? t("Atualizar") : t("Buscar no IGDB")}</Button>
          {d && <Button variant="ghost" disabled={busy || enabled === false} onClick={() => fetchData(false, null, true)}>{t("Não é este jogo?")}</Button>}
        </div>
      </Block>
      {choices && (
        <CandidatePicker title={t("Qual destes é o seu jogo?")} onClose={() => setChoices(null)}
          intro={reid
            ? t("Resultados do IGDB para “{name}”. A escolha fica salva.", { name: term || game.title })
            : t("Encontramos mais de um jogo parecido com “{name}” no IGDB. A escolha fica salva.", { name: term || game.title })}
          onPick={(id) => { setChoices(null); fetchData(false, Number(id)); }}
          items={choices.map((c) => ({
            id: c.id, title: `${c.name}${c.year ? ` (${c.year})` : ""}`, image: c.image,
            subtitle: c.platforms.slice(0, 5).join(" · ") || t("Plataformas não informadas"),
            highlight: c.steamMatch ? t("Mesmo AppID da Steam") : null,
            warning: c.onPlatform ? null : t("Não consta para {p} no IGDB", { p: game.platform }),
          }))} />
      )}
    </>
  );
}
