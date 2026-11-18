import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useSession } from "../state/session";
import { useLibrary } from "../state/useLibrary";
import { api, errMsg } from "../lib/api";
import { formatBytes, gamePassLeaveDate } from "../lib/format";
import { launchWithGameboot } from "../lib/gameboot";
import { currentLang, t, tk } from "../lib/i18n";
import { useInputDevice } from "../lib/inputDevice";
import Button from "../components/Button";
import Glyph from "../components/Glyph";
import ImageViewer from "../components/ImageViewer";
import Block from "../components/DpBlock";
import TrailerPlayer, { type TrailerSource } from "../components/TrailerPlayer";
import AgeRatingBadge from "../components/AgeRatingBadge";
import type { GamePassDetailView, GamePassItem, GamePassList } from "../types";

/** Categorias da Microsoft Store (em inglês) → gêneros que o app já traduz. "Other" fica de fora. */
const CATEGORY: Record<string, string[]> = {
  "Action & adventure": [tk("Ação"), tk("Aventura")],
  "Strategy": [tk("Estratégia")],
  "Shooter": [tk("Tiro")],
  "Sports": [tk("Esporte")],
  "Simulation": [tk("Simulação")],
  "Role playing": [tk("RPG")],
  "Platformer": [tk("Plataforma")],
  "Puzzle & trivia": [tk("Quebra-cabeça")],
  "Fighting": [tk("Luta")],
  "Racing & flying": [tk("Corrida")],
  "Card & board": [tk("Cartas e tabuleiro")],
  "Family & kids": [tk("Família e infantil")],
  "Multi-player Online Battle Arena": ["MOBA"],
  "Other": [],
};
const genresOf = (cats: string[]) => [...new Set(cats.flatMap((c) => CATEGORY[c] ?? [c]))];

/** Recursos no PC (chaves do backend, gamepass.rs → feature_key). */
const FEATURE: Record<string, string> = {
  single: tk("Um jogador"),
  coop_online: tk("Cooperativo online"),
  coop_local: tk("Cooperativo local"),
  coop_cross: tk("Cooperativo entre plataformas"),
  mp_online: tk("Multijogador online"),
  mp_local: tk("Multijogador local"),
  mp_cross: tk("Multijogador entre plataformas"),
  split: tk("Tela dividida"),
  xpa: "Xbox Play Anywhere",
  achievements: tk("Conquistas do Xbox"),
  cloud_saves: tk("Salvamento na nuvem"),
  hdr: "HDR",
  vrr: tk("Taxa de atualização variável (VRR)"),
  ray_tracing: "Ray tracing",
  spatial: tk("Som espacial"),
  atmos: "Dolby Atmos",
  dtsx: "DTS:X",
};
const RATING_SYSTEM: Record<string, string> = { DJCTQ: "ClassInd", "COB-AU": "ACB" };

const fmtDate = (s: number) => new Date(s * 1000).toLocaleDateString(currentLang(), { day: "2-digit", month: "long", year: "numeric" });
const isFuture = (s: number | null) => s !== null && s * 1000 > Date.now();

/**
 * Página de um jogo do catálogo do PC Game Pass (aberta pelos carrosséis da Home), no mesmo desenho da página
 * de jogo da Biblioteca. Dados e imagens vêm do catálogo público da Microsoft Store (cache de 24 h; sem
 * internet, mostra o que já foi buscado ou, na falta, o resumo do carrossel). Instalar continua no app Xbox;
 * o jogo pode ir para a Biblioteca antes ou depois de instalado e, instalado, é jogado direto daqui.
 */
export default function GamePassDetail({ item, list }: { item: GamePassItem; list: GamePassList }) {
  const { profile, toast, setOpenGamePass } = useSession();
  const p = profile!;
  const device = useInputDevice();
  const { games, reload } = useLibrary(p.id);
  const [v, setV] = useState<GamePassDetailView | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [shot, setShot] = useState(0);
  const [viewer, setViewer] = useState<number | null>(null);
  // descrição: até 12 linhas; "Ler mais" só aparece quando o texto passa disso
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const descRef = useRef<HTMLParagraphElement>(null);
  // trailers do YouTube pelo IGDB: só quando o jogo não tem trailer do Xbox que toque no app
  const [yt, setYt] = useState<{ available: boolean; videos: { id: string; name: string }[] } | null>(null);

  const load = useCallback(async (refresh: boolean) => {
    setBusy(true);
    try {
      const r = await api.gamePassDetail(p.id, item.id, refresh);
      setV(r);
      setFailed(null);
      if (r.error && refresh) toast(t(r.error), "err");
    } catch (e) { setFailed(errMsg(e)); }
    finally { setBusy(false); }
  }, [p.id, item.id, toast]);
  useEffect(() => { load(false); }, [load]);
  const needYoutube = v !== null && v.detail.trailers.length === 0;
  useEffect(() => {
    if (!needYoutube) return;
    let alive = true;
    api.gamePassVideos(item.id).then((r) => { if (alive) setYt(r); }).catch(() => { if (alive) setYt({ available: true, videos: [] }); });
    return () => { alive = false; };
  }, [needYoutube, item.id]);
  // ao abrir, o foco vai para a ação principal (controle/teclado)
  useEffect(() => {
    const r = requestAnimationFrame(() => document.querySelector<HTMLElement>(".gpd .dp-play")?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(r);
  }, [v !== null]); // eslint-disable-line react-hooks/exhaustive-deps

  const d = v?.detail;
  const title = d?.title ?? item.title;
  const hero = d?.hero ?? null;
  const poster = d?.poster ?? item.image;
  const release = d ? d.release : item.release;
  const genres = d ? genresOf(d.categories) : [];
  const shots = d?.screenshots ?? [];
  const cur = shots.length ? shots[Math.min(shot, shots.length - 1)] : null;
  const description = d?.description || item.description || "";
  const libGame = v?.libraryGameId != null ? games.find((g) => g.id === v.libraryGameId) ?? null : null;

  // mede se a descrição passa de 12 linhas (com o texto recolhido)
  useLayoutEffect(() => {
    const el = descRef.current;
    if (!el || expanded) return;
    const check = () => setOverflows(el.scrollHeight > el.clientHeight + 2);
    check();
    const ro = new ResizeObserver(check);
    ro.observe(el);
    return () => ro.disconnect();
  }, [description, expanded]);

  const leaves = list === "leaving" ? gamePassLeaveDate(d?.leavesOn ?? item.leavesOn) : null;
  const status = list === "leaving" ? (leaves
      ? t("Sai do PC Game Pass em {date} (data prevista no catálogo da Microsoft).", { date: leaves.toLocaleDateString(currentLang(), { day: "2-digit", month: "long", year: "numeric" }) })
      : t("Este jogo vai sair do PC Game Pass em breve."))
    : list === "added" ? t("Adicionado recentemente ao PC Game Pass.")
    : release === null ? t("Chega ao PC Game Pass em breve. Data de lançamento ainda não anunciada.")
    : isFuture(release) ? t("Chega ao PC Game Pass. Lançamento: {date}.", { date: fmtDate(release) })
    : t("Chega ao PC Game Pass em breve.");
  const statusChip = list === "leaving" ? (leaves ? t("Sai em {date}", { date: leaves.toLocaleDateString(currentLang(), { day: "2-digit", month: "short" }) }) : t("Saindo")) : list === "added" ? t("Adicionados recentemente") : t("Em breve no Game Pass");

  const close = () => setOpenGamePass(null);
  const openXbox = () => api.gamePassOpen(item.id)
    .then((how) => { if (how === "store") toast(t("App Xbox não encontrado neste PC: a página do jogo foi aberta na Microsoft Store.")); })
    .catch((e) => toast(errMsg(e), "err"));
  const addToLibrary = async () => {
    try {
      await api.gamePassAddToLibrary(p.id, item.id);
      toast(v?.installed ? t("{title} adicionado à biblioteca.", { title }) : t("{title} adicionado à biblioteca. Instale pelo app Xbox para jogar.", { title }));
      reload();
      load(false);
    } catch (e) { toast(errMsg(e), "err"); }
  };
  const playGame = async () => {
    if (!libGame) return;
    try {
      await launchWithGameboot(libGame, () => api.launchGame(p.id, libGame.id), () => api.checkLaunch(p.id, libGame.id));
      toast(t("Iniciando {title}…", { title: libGame.title }));
      reload();
    } catch (e) { toast(errMsg(e), "err"); }
  };

  // ação principal: fora da biblioteca → adicionar; na biblioteca e instalado → jogar; na biblioteca e
  // não instalado → instalar pelo app Xbox. "Abrir no app Xbox" fica sempre ao lado (exceto quando já é a principal).
  const canPlay = !!v?.inLibrary && v.installed && !!libGame;
  const installFirst = !!v?.inLibrary && !v.installed;
  const primary = !v ? null
    : !v.inLibrary ? <Button variant="primary" className="dp-play stage-play" onClick={addToLibrary}>{t("Adicionar à biblioteca")}</Button>
    : canPlay ? <Button variant="primary" className="dp-play stage-play" onClick={playGame}>{t("Jogar")}</Button>
    : <Button variant="primary" className="dp-play stage-play" onClick={openXbox}>{t("Instalar pelo app Xbox")}</Button>;

  return (
    <div className="dp gpd">
      <header className="dp-hero" data-parallax>
        {hero ? <img className="dp-hero-img" src={hero} alt="" decoding="async" referrerPolicy="no-referrer" style={{ objectPosition: "50% 30%" }} />
          : <div className="dp-hero-soft" style={poster ? { backgroundImage: `url("${poster}")` } : undefined} />}
        <div className="dp-shade" />
        <div className="chips dp-meta">
          <span className="plat-tag">PC Game Pass</span>
          <span className={`gpd-status ${list}`}>{statusChip}</span>
          {genres.slice(0, 3).map((g) => <span key={g}>{t(g)}</span>)}
        </div>
        {/* mesmo selo da página da Biblioteca (ClassInd, PEGI ou ESRB; outros sistemas só na coluna de informações) */}
        {d?.ageRating && <AgeRatingBadge ratings={[`${d.ageRating.system === "DJCTQ" ? "CLASS_IND" : d.ageRating.system}:${d.ageRating.value}`]} />}
        <Button className="dp-back" onClick={close} aria-label={t("Voltar")}>
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M15 5l-7 7 7 7" /></svg>
          <span>{t("Voltar")}</span>
          {device !== "keyboard" && <Glyph k="back" />}
        </Button>
        <div className="dp-head">
          <div className="dp-title">
            {/* jogos do Game Pass: sempre o título escrito (o "Logo" do catálogo é o ícone da loja, não a arte do nome) */}
            <h1>{title}</h1>
            <div className="dp-actions">
              {primary}
              {!installFirst && <Button className="dp-act" onClick={openXbox}>{t("Abrir no app Xbox")}</Button>}
            </div>
          </div>
        </div>
      </header>

      <div className="dp-grid">
        <div className="dp-main" data-nav-group>
          {(() => {
            const sources: TrailerSource[] = d && d.trailers.length
              ? d.trailers.map((tr, i) => ({ kind: "xbox", key: `x${i}`, title: tr.title, hls: tr.hls, poster: tr.poster }))
              : (yt?.videos ?? []).map((vid) => ({ kind: "youtube", key: `y${vid.id}`, title: vid.name || t("Trailer"), id: vid.id }));
            if (sources.length) return <Block title={t("Trailers")}><TrailerPlayer sources={sources} /></Block>;
            // sem trailer do Xbox e sem IGDB: explica como ter trailers do YouTube
            if (needYoutube && yt && !yt.available) {
              return (
                <Block title={t("Trailers")}>
                  <p className="muted">{t("Este jogo não tem trailer do Xbox que possa tocar no app. Ative o IGDB em Configurações → Fontes de dados para ver trailers do YouTube.")}</p>
                </Block>
              );
            }
            return null;
          })()}

          <Block title={t("Sobre o jogo")}>
            {!v && !failed && <div className="upd-skel gnews-skel" role="status"><span>{t("Buscando os detalhes na Microsoft Store…")}</span></div>}
            {(v || failed) && (
              <>
                <p ref={descRef} className={`desc gpd-desc ${expanded ? "open" : ""}`}>{description || t("Descrição não encontrada.")}</p>
                {(overflows || expanded) && (
                  <Button variant="ghost" className="gpd-more" aria-expanded={expanded} onClick={() => setExpanded((x) => !x)}>
                    {expanded ? t("Ler menos") : t("Ler mais")}
                  </Button>
                )}
              </>
            )}
            {failed && !v && <p className="ra-warn">{t(failed)}</p>}
          </Block>

          {shots.length > 0 && cur && (
            <Block title={t("Capturas de tela")}>
              <button data-focus type="button" className="shot-big-btn" onClick={() => setViewer(Math.min(shot, shots.length - 1))} aria-label={t("Abrir captura em tela cheia")}>
                <img className="shot-big" src={cur.full} alt={t("Captura {v0}", { v0: shot + 1 })} referrerPolicy="no-referrer" decoding="async" />
              </button>
              <div className="shots">
                {shots.map((s, i) => (
                  <button key={s.thumb} data-focus className={`shot ${i === shot ? "on" : ""}`} onClick={() => setShot(i)} onFocus={() => setShot(i)}>
                    <img src={s.thumb} alt="" loading="lazy" referrerPolicy="no-referrer" />
                  </button>
                ))}
              </div>
            </Block>
          )}
        </div>

        <aside className="dp-side" data-nav-group>
          <Block title={t("PC Game Pass")}>
            <p className="gpd-when">{status}</p>
            {v?.installed && <p className="muted">{v.inLibrary ? t("Instalado neste PC e já na sua biblioteca.") : t("Instalado neste PC.")}</p>}
            <p className="muted note">{t("Informações do catálogo público do site do Xbox. Disponibilidade e datas podem mudar; confira na Microsoft Store.")}</p>
          </Block>

          <Block title={t("Informações")} actions={<Button variant="ghost" disabled={busy} onClick={() => load(true)}>{busy ? t("Atualizando…") : t("Atualizar")}</Button>}>
            <dl className="dp-info">
              <dt>{t("Desenvolvedora")}</dt><dd>{d?.developer || t("Não informado")}</dd>
              <dt>{t("Editora")}</dt><dd>{d?.publisher || item.publisher || t("Não informado")}</dd>
              <dt>{t("Lançamento")}</dt><dd>{release === null ? t("Data a anunciar") : fmtDate(release)}</dd>
              {genres.length > 0 && <><dt>{t("Gênero")}</dt><dd>{genres.map((g) => t(g)).join(", ")}</dd></>}
              {d?.ageRating && <><dt>{t("Classificação indicativa")}</dt><dd>{RATING_SYSTEM[d.ageRating.system] ?? d.ageRating.system} {d.ageRating.value}</dd></>}
              {d?.rating != null && <><dt>{t("Avaliação")}</dt><dd>★ {d.rating.toLocaleString(currentLang(), { maximumFractionDigits: 1 })} · {t("{n} avaliações na Microsoft Store", { n: d.ratingCount.toLocaleString(currentLang()) })}</dd></>}
              {d?.size != null && <><dt>{t("Tamanho")}</dt><dd>{formatBytes(d.size)}</dd></>}
              {d && <><dt>{t("Complementos")}</dt><dd>{d.hasAddOns ? t("Tem DLCs ou complementos na Microsoft Store") : t("Nenhum")}</dd></>}
            </dl>
          </Block>

          {d && d.features.length > 0 && (
            <Block title={t("Recursos no PC")}>
              <div className="chips gpd-feats">{d.features.map((f) => <span key={f}>{FEATURE[f] ? t(FEATURE[f]) : f}</span>)}</div>
            </Block>
          )}
          {v && v.fetchedAt > 0 && <p className="muted note gpd-src">{t("Dados da Microsoft Store de {date}.", { date: fmtDate(v.fetchedAt) })}{v.error ? ` ${t(v.error)}` : ""}</p>}
        </aside>
      </div>

      {/* mesmo visualizador da Galeria (imagem grande, anterior/próxima, tela cheia) */}
      {viewer !== null && shots[viewer] && (
        <ImageViewer images={shots.map((s) => s.full)} index={viewer} caption={title} backLabel={t("Voltar para o jogo")}
          onIndex={(i) => { setViewer(i); setShot(i); }} onClose={() => setViewer(null)} />
      )}
    </div>
  );
}
