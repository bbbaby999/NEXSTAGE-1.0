import { CSSProperties, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { pickOpen as openDialog } from "../lib/filePicker";
import { useSession } from "../state/session";
import { useLibrary } from "../state/useLibrary";
import { api, errMsg } from "../lib/api";
import { formatDate, formatDuration } from "../lib/format";
import Button from "../components/Button";
import Glyph from "../components/Glyph";
import { useInputDevice } from "../lib/inputDevice";
import { onPadButton } from "../lib/nav";
import Modal from "../components/Modal";
import GameForm from "../components/GameForm";
import CollectionPicker from "../components/CollectionPicker";
import ShotManager from "../components/ShotManager";
import ImageSearch from "../components/ImageSearch";
import { launchWithGameboot } from "../lib/gameboot";
import Viewer, { itemSrc } from "../components/Viewer";
import { useFirstImage } from "../lib/useImage";
import { useAccent } from "../lib/accent";
import { rememberClosedGame } from "../lib/returnFocus";
import RaGamePanel from "../components/RaGamePanel";
import Block from "../components/DpBlock";
import IgdbPanel from "../components/IgdbPanel";
import ModsPanel from "../components/ModsPanel";
import TimeToBeatBlock from "../components/TimeToBeatBlock";
import AgeRatingBadge from "../components/AgeRatingBadge";
import MetacriticPanel from "../components/MetacriticPanel";
import SteamGamePanel from "../components/SteamGamePanel";
import SteamDlcPanel from "../components/SteamDlcPanel";
import SteamGameNews from "../components/SteamGameNews";
import TrophyGamePanel from "../components/TrophyGamePanel";
import XboxGamePanel from "../components/XboxGamePanel";
import { isInstalled } from "../lib/steam";
import { TrophyIcon } from "../components/RaUnlockToasts";
import type { GalleryItem, GameInfo, SteamAppStatus, StoreHit, WebShot } from "../types";
import CandidatePicker from "../components/CandidatePicker";
import { CaptureMenu, ICON, type MenuAction } from "../components/CaptureUi";
import { t, tk } from "../lib/i18n";

const KIND: Record<string, string> = { snap: tk("Gameplay"), title: tk("Tela de título"), cover: tk("Capa"), background: tk("Fundo"), image: tk("Imagem") };
const SOURCE: Record<string, string> = { steam: "Steam", libretro: "libretro-thumbnails", wikipedia: "Wikipédia", steamgriddb: "SteamGridDB", rawg: "RAWG", wikimedia: "Wikimedia Commons" };

/**
 * Página individual do jogo. Dados externos vêm da loja pública da Steam (jogos de PC) e
 * ficam em cache no SQLite (funcionam offline depois de buscados). Nada é inventado:
 * campos sem dado mostram "não encontrado". Descrição e gêneros podem ser editados à mão.
 */
export default function GameDetail({ gameId }: { gameId: number }) {
  const { profile, toast, setOpenGameId, setView, setGalleryIntent } = useSession();
  const device = useInputDevice();
  const p = profile!;
  const { games, loading, reload } = useLibrary(p.id);
  const game = games.find((g) => g.id === gameId);
  const [info, setInfo] = useState<GameInfo | null>(null);
  const [infoLoaded, setInfoLoaded] = useState(false);
  const [storeHits, setStoreHits] = useState<StoreHit[] | null>(null);
  const [term, setTerm] = useState("");
  const [busy, setBusy] = useState(false);
  const [shot, setShot] = useState(0);
  const [wshot, setWshot] = useState(0);
  const [searching, setSearching] = useState(false);
  const [dlg, setDlg] = useState<null | "collections" | "edit" | "notes" | "hltb" | "shots">(null);
  // abas da página: visão geral, DLCs (jogos da Steam) e mods (Nexus Mods)
  type Tab = "overview" | "dlc" | "mods";
  const [tab, setTab] = useState<Tab>("overview");
  const hasDlcTab = game?.steamAppId != null;
  const tabs: Tab[] = hasDlcTab ? ["overview", "dlc", "mods"] : ["overview", "mods"];
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  // jogo sem AppID da Steam (ou vínculo removido): a aba DLCs some e a página volta para a Visão geral
  useEffect(() => { if (!hasDlcTab) setTab((cur) => (cur === "dlc" ? "overview" : cur)); }, [hasDlcTab]);
  // Controle: R2 vai para a próxima aba e L2 para a anterior (só sem janela/lista/menu por cima)
  useEffect(() => onPadButton((b, e) => {
    if (e.defaultPrevented || (b !== "l2" && b !== "r2")) return;
    if (document.querySelector(".overlay, .pick-root, .kb-root, .sm-root:not(.leaving), .ctx-root")) return;
    e.preventDefault();
    setTab((cur) => {
      const l = tabsRef.current;
      const i = Math.max(0, l.indexOf(cur));
      return l[Math.min(l.length - 1, Math.max(0, i + (b === "r2" ? 1 : -1)))];
    });
  }), []);
  const [hv, setHv] = useState({ main: "", extra: "", comp: "", all: "" });
  const [confirm, setConfirm] = useState(false);
  // "Mais opções" (Zerado, Capturas, Coleções, Editar jogo, Remover): também pelo botão Menu/Options do controle, como na Home
  const [menu, setMenu] = useState(false);
  useEffect(() => {
    const onCtx = () => { if (!document.querySelector(".overlay, .ctx-root, .pick-root, .sm-root:not(.leaving)")) setMenu(true); };
    window.addEventListener("nexstage:context", onCtx);
    return () => window.removeEventListener("nexstage:context", onCtx);
  }, []);
  const [caps, setCaps] = useState<GalleryItem[]>([]);
  const [viewer, setViewer] = useState<number | null>(null);
  const [capSel, setCapSel] = useState(0);
  const [desc, setDesc] = useState("");
  const [genres, setGenres] = useState("");
  const [steamSt, setSteamSt] = useState<SteamAppStatus | null>(null);

  // Jogos da Steam: estado de instalação lido do cliente local. Reconfere ao voltar para a janela e,
  // enquanto não estiver instalado, a cada 15 s (o download acontece no cliente Steam).
  const steamAppId = game?.steamAppId ?? null;
  useEffect(() => {
    if (steamAppId === null) { setSteamSt(null); return; }
    let alive = true;
    let installed = false;
    const check = () => api.steamAppStatus(steamAppId).then((s) => { if (alive) { installed = isInstalled(s.install); setSteamSt(s); } }).catch(() => {});
    check();
    const iv = setInterval(() => { if (!installed) check(); }, 15000);
    window.addEventListener("focus", check);
    return () => { alive = false; clearInterval(iv); window.removeEventListener("focus", check); };
  }, [steamAppId]);

  useEffect(() => {
    setInfoLoaded(false);
    api.getGameInfo(p.id, gameId).then((i) => { setInfo(i); setInfoLoaded(true); }).catch(() => setInfoLoaded(true));
  }, [p.id, gameId]);
  useEffect(() => { if (game && !term) setTerm(game.title); }, [game, term]);
  useEffect(() => { if (!loading && !game) setOpenGameId(null); }, [loading, game, setOpenGameId]);
  // Ao abrir a página, o foco vai direto para o botão Jogar (controle/teclado).
  const hasGame = !!game;
  useEffect(() => {
    if (!hasGame) return;
    const r = requestAnimationFrame(() => document.querySelector<HTMLElement>(".dp-play")?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(r);
  }, [gameId, hasGame]);
  // Ao sair da página, a Biblioteca devolve o foco a este jogo.
  useEffect(() => () => rememberClosedGame(gameId), [gameId]);
  const loadCaps = () =>
    api.listGallery(p.id).then((l) => setCaps(l.filter((i) => i.gameId === gameId))).catch(() => {});
  useEffect(() => { loadCaps(); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [p.id, gameId]);
  const heroSrc = useFirstImage([
    game?.background,
    info?.externalId ? `https://cdn.cloudflare.steamstatic.com/steam/apps/${info.externalId}/library_hero.jpg` : null,
  ]);
  // mesma cor de destaque (tirada da capa) do selo de plataforma do jogo em destaque da Home
  const accent = useAccent(game?.cover);

  if (!game) return null;

  const run = async (fn: () => Promise<unknown>, ok?: string) => {
    try { await fn(); if (ok) toast(ok); } catch (e) { toast(errMsg(e), "err"); }
  };
  const link = (url: string) => run(() => openUrl(url));
  // Instalar: pedido ao cliente oficial Steam (steam://install); sem ele, a loja. O NEXSTAGE não baixa nada.
  const install = () => run(async () => {
    const how = await api.steamInstall(game.steamAppId!);
    toast(how === "client" ? t("Instalação solicitada ao cliente Steam. O download acontece na Steam.") : t("Cliente Steam não encontrado: página da loja aberta."));
  });
  const steamOnly = game.source === "steam" && !game.executablePath.trim();
  const needInstall = steamOnly && steamSt !== null && !isInstalled(steamSt.install);

  // "Buscar na Steam": mostra os jogos encontrados para o usuário escolher (como na pesquisa de imagens)
  async function search() {
    setBusy(true);
    try {
      const hits = await api.steamStoreSearch(term || game!.title);
      if (!hits.length) toast(t("Nenhum jogo encontrado na Steam com esse nome. Tente outro nome."), "err");
      else setStoreHits(hits);
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }
  async function pickStore(appid: number) {
    setStoreHits(null);
    setBusy(true);
    await run(async () => setInfo(await api.fetchGameInfo(p.id, game!.id, term, appid)), t("Informações atualizadas."));
    setBusy(false);
  }

  const genreList = (info?.genres ?? []).filter((g) => g !== game.platform);
  // capturas próprias (arquivos do PC) para a galeria deste jogo
  const importCaps = () => run(async () => {
    const sel = await openDialog({ purpose: "game-captures", multiple: true, filters: [{ name: "Imagens", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "gif"] }] });
    const paths = Array.isArray(sel) ? sel : typeof sel === "string" ? [sel] : [];
    if (!paths.length) return;
    const r = await api.importScreenshots(p.id, game.id, paths);
    toast(t("{imported} importada(s){v1}{v2}", { imported: r.imported, v1: r.duplicates ? t(", {n} já existia(m)", { n: r.duplicates }) : "", v2: r.failed.length ? t(". Erro: {msg}", { msg: r.failed[0] }) : "" }), r.failed.length && !r.imported ? "err" : "ok");
    await loadCaps();
  });
  const moreActions: MenuAction[] = [
    // zerado = registro "zerado" no Zerei! ligado a este jogo; editar ou desfazer é lá
    game.completed
      ? { label: t("✓ Zerado"), icon: ICON.select, run: () => { setOpenGameId(null); setView("zerei"); } }
      : { label: t("Marcar como zerado"), icon: ICON.select,
          run: () => run(async () => { await api.zereiMarkGame(p.id, game.id); reload(); }, t("Marcado como zerado: o jogo foi incluído no Zerei!.")) },
    // capturas deste jogo na Galeria (como no ⋯ da Home); voltar à Biblioteca retorna a esta página
    { label: t("Capturas de tela{v0}", { v0: caps.length ? ` (${caps.length})` : "" }), icon: ICON.open,
      run: () => { setGalleryIntent({ gameId: game.id }); setView("gallery"); } },
    { label: t("Coleções"), icon: ICON.folder, run: () => setDlg("collections") },
    // pasta onde o jogo está (atalho → arquivo original; Steam → pasta de instalação)
    { label: t("Abrir pasta de arquivos"), icon: ICON.folder, run: () => { api.revealGameFolder(p.id, game.id).catch((e) => toast(errMsg(e), "err")); } },
    { label: t("Editar jogo"), icon: ICON.edit, run: () => setDlg("edit") },
    { label: t("Remover"), icon: ICON.trash, danger: true, run: () => setConfirm(true) },
  ];
  const shots = info?.screenshots ?? [];
  const web = info?.webScreenshots ?? [];
  const cur = web.length ? web[Math.min(wshot, web.length - 1)] : null;

  // Só as imagens escolhidas e confirmadas na pesquisa entram na galeria do jogo.
  async function addFromWeb(shots: WebShot[]) {
    const r = await api.addWebScreenshots(p.id, game!.id, shots);
    setInfo(r);
    setWshot(Math.max(0, (r.webScreenshots?.length ?? 1) - 1));
    toast(t("{length} imagem(ns) adicionada(s) à galeria do jogo.", { length: shots.length }));
  }

  return (
    <div className="dp">
      <header className="dp-hero" data-parallax>
        {heroSrc ? (
          <img className="dp-hero-img" src={heroSrc} alt="" decoding="async"
            style={{ objectPosition: game.background ? `${game.bgX}% ${game.bgY}%` : "50% 30%" }} />
        ) : (
          <div className="dp-hero-soft" style={game.cover ? { backgroundImage: `url(${game.cover})` } : undefined} />
        )}
        <div className="dp-shade" />
        {/* canto superior direito: plataforma · gêneros · Steam · RetroAchievements (gêneros além de 3 no "+N", nome completo na dica) */}
        <div className="chips dp-meta">
          <span className="plat-tag" style={accent ? ({ "--game-rgb": accent } as CSSProperties) : undefined}>{game.platform}</span>
          {genreList.slice(0, 3).map((g) => <span key={g}>{t(g)}</span>)}
          {genreList.length > 3 && <span className="dp-more-genres" title={genreList.slice(3).map((g) => t(g)).join(", ")}>+{genreList.length - 3}</span>}
          {game.steamAppId !== null && <span className="steam-chip-tag" title={t("Vinculado à Steam")}>Steam</span>}
          {game.raGameId !== null && <span className="ra-chip-tag" title={t("Compatível com RetroAchievements")}><TrophyIcon size={12} /> RA</span>}
        </div>
        {/* classificação etária (IGDB): canto inferior direito, abaixo dos selos */}
        <AgeRatingBadge ratings={info?.igdb?.ageRatings} />
        {/* canto superior esquerdo da arte: vidro escuro (legível sobre qualquer imagem) + botão do controle */}
        <Button className="dp-back" onClick={() => setOpenGameId(null)} aria-label={t("Voltar")}>
          <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M15 5l-7 7 7 7" /></svg>
          <span>{t("Voltar")}</span>
          {device !== "keyboard" && <Glyph k="back" />}
        </Button>
        {/* sem capa no cabeçalho (ela continua sendo a do jogo na Biblioteca e em "Editar jogo") */}
        <div className="dp-head">
          <div className="dp-title">
            {/* com logo, ela substitui o título escrito (o nome continua no texto alternativo) */}
            {game.logo ? (
              <h1 className="dp-logo-title"><img src={game.logo} alt={game.title} referrerPolicy="no-referrer" /></h1>
            ) : <h1>{game.title}</h1>}
            {/* Jogar/Instalar em destaque · Favoritar · Mais opções (Zerado, Capturas, Coleções, Editar, Remover) */}
            <div className="dp-actions">
              {needInstall ? (
                <Button variant="primary" className="dp-play stage-play" onClick={install} disabled={steamSt!.install === "installing"}>
                  {steamSt!.install === "installing" ? t("Instalando na Steam…") : t("Instalar")}</Button>
              ) : (
                <Button variant="primary" className="dp-play stage-play" onClick={() => run(async () => { await launchWithGameboot(game, () => api.launchGame(p.id, game.id), () => api.checkLaunch(p.id, game.id)); reload(); }, `Iniciando ${game.title}…`)}>{t("Jogar")}</Button>
              )}
              <Button className={`dp-act ${game.favorite ? "on" : ""}`} aria-pressed={game.favorite}
                onClick={() => run(async () => { await api.setFavorite(p.id, game.id, !game.favorite); reload(); })}>
                {game.favorite ? t("★ Favorito") : t("☆ Favoritar")}
              </Button>
              <Button className="dp-act" aria-haspopup="menu" onClick={() => setMenu(true)}>{t("Mais opções")}</Button>
            </div>
          </div>
        </div>
      </header>

      <div className="dp-tabs" data-nav-group role="tablist" aria-label={t("Seções do jogo")}>
        <button data-focus type="button" role="tab" aria-selected={tab === "overview"} className={tab === "overview" ? "on" : ""} onClick={() => setTab("overview")}>{device !== "keyboard" && <Glyph k="l2" />}{t("Visão geral")}</button>
        {hasDlcTab && <button data-focus type="button" role="tab" aria-selected={tab === "dlc"} className={tab === "dlc" ? "on" : ""} onClick={() => setTab("dlc")}>{t("DLCs")}</button>}
        <button data-focus type="button" role="tab" aria-selected={tab === "mods"} className={tab === "mods" ? "on" : ""} onClick={() => setTab("mods")}>{t("Mods")}{device !== "keyboard" && <Glyph k="r2" />}</button>
      </div>

      {tab === "dlc" && hasDlcTab ? <div className="dp-mods"><SteamDlcPanel game={game} /></div> : tab === "mods" ? <div className="dp-mods"><ModsPanel game={game} /></div> : (
      <div className="dp-grid">
        <div className="dp-main" data-nav-group>
          {/* jogos vinculados à Steam: nota de atualização oficial mais recente, antes de tudo */}
          {game.steamAppId !== null && <SteamGameNews key={game.steamAppId} game={game} />}
          {/* IGDB: primeiro bloco da página (logo depois da última atualização, nos jogos da Steam) */}
          <IgdbPanel game={game} info={info} loaded={infoLoaded} term={term} onInfo={setInfo} />

          {/* descrição em destaque; busca de dados/imagens e edição ficam no rodapé do bloco */}
          <Block title={t("Sobre o jogo")}>
            <p className="desc">{info?.description || t("Descrição não encontrada.")}</p>
            <div className="dp-tools">
              <div className="row dp-search">
                <input className="search" value={term} onChange={(e) => setTerm(e.target.value)} placeholder={t("Nome para buscar na Steam")} aria-label={t("Nome para buscar na Steam")} />
                <Button onClick={search} disabled={busy}>{busy ? t("Buscando…") : t("Buscar na Steam")}</Button>
                <Button onClick={() => setSearching(true)}>{t("Buscar na internet")}</Button>
              </div>
              <div className="row">
                <Button variant="ghost" onClick={() => { setDesc(info?.description ?? ""); setGenres((info?.genres ?? []).join(", ")); setDlg("notes"); }}>{t("Editar descrição e gêneros")}</Button>
                {info?.externalId && <Button variant="ghost" onClick={() => run(async () => { await api.applySteamCover(p.id, game.id); reload(); }, "Capa atualizada.")}>{t("Usar capa da Steam")}</Button>}
                {info && (shots.length > 0 || web.length > 0 || (info.removedScreenshots?.length ?? 0) > 0) && (
                  <Button variant="danger" className="shots-manage" onClick={() => setDlg("shots")}>{t("Remover screenshots…")}</Button>
                )}
              </div>
            </div>
          </Block>

          {/* sem capturas (Steam ou internet), o bloco não aparece — nenhuma mensagem de "não encontrado" */}
          {(shots.length > 0 || web.length > 0) && (
          <Block title={t("Capturas de tela")}>
            {shots.length ? (
              <>
                <img className="shot-big" src={shots[Math.min(shot, shots.length - 1)]} alt={t("Captura {v0}", { v0: shot + 1 })} />
                <div className="shots">
                  {shots.map((s, i) => (
                    <button key={s} data-focus className={`shot ${i === shot ? "on" : ""}`} onClick={() => setShot(i)} onFocus={() => setShot(i)}>
                      <img src={s} alt="" loading="lazy" />
                    </button>
                  ))}
                </div>
              </>
            ) : null}

            {web.length > 0 && (
              <div className="web-shots">
                <h3 className="section sub">{t("Screenshots da internet")}</h3>
                <div className="web-big" style={{ "--img": `url("${cur!.thumb}")` } as CSSProperties}>
                  <img key={cur!.url} src={cur!.url} alt={cur!.title} referrerPolicy="no-referrer" decoding="async"
                    onError={(e) => { const i = e.currentTarget; if (i.src !== cur!.thumb) i.src = cur!.thumb; }} />
                  <div className="web-cap">
                    <span><b>{t(KIND[cur!.kind] ?? "Imagem")}</b>{cur!.title}</span>
                    <small>{SOURCE[cur!.source] ?? cur!.source}</small>
                  </div>
                </div>
                <div className="shots">
                  {web.map((s, i) => (
                    <button key={s.url} data-focus className={`shot ${i === wshot ? "on" : ""}`} onClick={() => setWshot(i)} onFocus={() => setWshot(i)}
                      aria-label={`${t(KIND[s.kind] ?? "Imagem")}: ${s.title}`}>
                      <img src={s.thumb} alt="" loading="lazy" referrerPolicy="no-referrer" />
                    </button>
                  ))}
                </div>
                <div className="row">
                  <Button variant="ghost" onClick={() => link(`https://www.google.com/search?tbm=isch&q=${encodeURIComponent(`${term || game.title} ${game.platform} screenshot`)}`)}>{t("Ver mais no navegador")}</Button>
                </div>
              </div>
            )}
          </Block>
          )}

          <Block title={t("Capturas do NEXSTAGE")} actions={<Button onClick={importCaps}>{t("Importar capturas")}</Button>}>
            {caps.length ? (() => {
              // mesmo esquema das capturas da Steam/IGDB: imagem grande 16:9 + faixa de miniaturas;
              // a miniatura escolhe a captura em destaque e a imagem grande abre o visualizador em tela cheia
              const ci = Math.min(capSel, caps.length - 1);
              const cur = caps[ci];
              return (
                <>
                  <button data-focus type="button" className="shot-big-btn" onClick={() => setViewer(ci)} aria-label={t("Abrir captura em tela cheia")}>
                    {cur.exists ? <img className="shot-big" src={itemSrc(cur)} alt={t("Captura {v0}", { v0: ci + 1 })} />
                      : <span className="shot-big thumb-missing">{t("Ausente")}</span>}
                  </button>
                  <div className="shots">
                    {caps.map((c, i) => (
                      <button key={c.id} data-focus className={`shot ${i === ci ? "on" : ""}`} onClick={() => setCapSel(i)} onFocus={() => setCapSel(i)}>
                        {c.exists ? <img src={itemSrc(c)} alt="" loading="lazy" /> : <span className="thumb-missing">{t("Ausente")}</span>}
                      </button>
                    ))}
                  </div>
                </>
              );
            })() : <p className="muted">{t("Nenhuma captura importada para este jogo.")}</p>}
          </Block>

        </div>

        <aside className="dp-side" data-nav-group>
          {/* nota do Metacritic: a primeira coisa da coluna */}
          <MetacriticPanel game={game} info={info} loaded={infoLoaded} term={term} onInfo={setInfo} />

          {game.steamAppId !== null && <SteamGamePanel game={game} status={steamSt} onInstall={install} onChanged={reload} />}
          <XboxGamePanel game={game} />
          <RaGamePanel game={game} onChanged={reload} />
          <TrophyGamePanel game={game} />

          <TimeToBeatBlock game={game} info={info} onEdit={() => {
            const h = info?.hltb?.source === "manual" ? info.hltb : null;
            const toH = (s?: number | null) => (s ? String(Math.round(s / 360) / 10) : "");
            setHv({ main: toH(h?.mainSeconds), extra: toH(h?.extraSeconds), comp: toH(h?.completionistSeconds), all: toH(h?.allSeconds) });
            setDlg("hltb");
          }} />

          <Block title={t("Informações")}>
            <dl className="dp-info">
              <dt>{t("Desenvolvedora")}</dt><dd>{info?.developers?.join(", ") || t("Não informado")}</dd>
              <dt>{t("Lançamento")}</dt><dd>{info?.releaseDate || t("Não informado")}</dd>
              <dt>{t("Plataforma")}</dt><dd>{game.platform}</dd>
              <dt>{t("Tempo de jogo")}</dt><dd>{formatDuration(game.playtimeSeconds)}</dd>
              <dt>{t("Último acesso")}</dt><dd>{formatDate(game.lastPlayedAt)}</dd>
              {game.isoPath && (<><dt>{t("Arquivo ISO")}</dt><dd className="path">{game.isoPath}</dd></>)}
              {game.romPath && (<><dt>{t("Arquivo")}</dt><dd className="path">{game.romPath}</dd></>)}
              {(game.isoPath || game.romPath || game.emulatorName) && (<><dt>{t("Emulador")}</dt><dd>{game.emulatorName ?? t("Nenhum associado (use Editar jogo)")}</dd></>)}
              <dt>{t("Fonte")}</dt><dd>{info ? (info.source === "steam" ? t("Steam · {matchedName}", { matchedName: info.matchedName }) : t("Manual")) : t("Sem dados")}</dd>
            </dl>
          </Block>

          <Block title={t("Links")}>
            <div className="row">
              {info?.storeUrl && <Button onClick={() => link(info.storeUrl!)}>{t("Página na Steam")}</Button>}
              <Button onClick={() => run(async () => {
                const name = info?.source === "steam" && info.matchedName ? info.matchedName : game.title;
                const r = await api.fandomLink(name);
                await openUrl(r.url);
                if (!r.direct) toast(t("Sem wiki exata identificada: abrindo a busca do Fandom."));
              })}>{t("Fandom")}</Button>
            </div>
          </Block>
        </aside>
      </div>
      )}

      {menu && <CaptureMenu actions={moreActions} onClose={() => setMenu(false)} />}
      {confirm && (
        <Modal title={t("Remover {title} da biblioteca?", { title: game.title })} onClose={() => setConfirm(false)}>
          <p className="muted">{t("O jogo e o histórico de sessões dele saem da biblioteca deste perfil. Os arquivos do jogo no computador não são apagados.")}</p>
          <div className="row end">
            <Button variant="ghost" onClick={() => setConfirm(false)}>{t("Cancelar")}</Button>
            <Button variant="danger" onClick={() => run(async () => { await api.removeGame(p.id, game.id); setOpenGameId(null); }, t("Jogo removido da biblioteca."))}>{t("Confirmar remoção")}</Button>
          </div>
        </Modal>
      )}
      {viewer !== null && caps[viewer] && (
        <Viewer items={caps} index={viewer} onIndex={setViewer} onClose={() => setViewer(null)}
          onFavorite={(i) => run(async () => { await api.setGalleryFavorite(p.id, i.id, !i.favorite); await loadCaps(); })}
          onDelete={(i) => run(async () => {
            await api.deleteGalleryItem(p.id, i.id);
            await loadCaps();
          }, t("Cópia excluída. O arquivo original foi mantido."))} />
      )}
      {storeHits && (
        <CandidatePicker title={t("Jogos encontrados na Steam")} onClose={() => setStoreHits(null)}
          intro={t("Resultados para “{name}”. Escolha o jogo certo para buscar descrição, gêneros, notas e capturas.", { name: term || game.title })}
          onPick={(id) => pickStore(Number(id))}
          items={storeHits.map((h) => ({
            id: h.appid, title: h.name, image: h.image,
            subtitle: [`AppID ${h.appid}`, h.platforms.join(" · "), h.price].filter(Boolean).join(" · "),
            highlight: info?.externalId === String(h.appid) ? t("Jogo atual") : null,
          }))} />
      )}
      {searching && (
        <ImageSearch mode="screenshots" term={term || game.title} platform={game.platform} onConfirm={addFromWeb} onClose={() => setSearching(false)} />
      )}
      {dlg === "shots" && info && (
        <ShotManager profileId={p.id} gameId={game.id} info={info} onChange={setInfo} onClose={() => setDlg(null)} />
      )}
      {dlg === "collections" && <CollectionPicker profileId={p.id} gameId={game.id} onClose={() => setDlg(null)} />}
      {dlg === "edit" && <GameForm profileId={p.id} game={game} onClose={() => setDlg(null)} onSaved={() => { setDlg(null); reload(); }} />}
      {dlg === "hltb" && (
        <Modal title={t("Tempo para zerar (em horas)")} onClose={() => setDlg(null)}>
          <div className="fields">
            {([["main", tk("História principal")], ["extra", tk("Principal + extras")], ["comp", tk("Completista")], ["all", tk("Todos os estilos")]] as const).map(([k, label]) => (
              <label key={k}>{t(label)}
                <input type="number" min="0" step="0.5" value={hv[k]} onChange={(e) => setHv({ ...hv, [k]: e.target.value })} />
              </label>
            ))}
          </div>
          <div className="row end">
            <Button variant="ghost" onClick={() => setDlg(null)}>{t("Cancelar")}</Button>
            <Button variant="primary" onClick={() => {
              const n = (s: string) => (s.trim() === "" ? null : Number(s.replace(",", ".")));
              run(async () => { setInfo(await api.saveManualHltb(p.id, game.id, n(hv.main), n(hv.extra), n(hv.comp), n(hv.all))); setDlg(null); }, "Tempos salvos.");
            }}>{t("Salvar")}</Button>
          </div>
        </Modal>
      )}
      {dlg === "notes" && (
        <Modal title={t("Descrição e gêneros")} onClose={() => setDlg(null)} wide>
          <div className="fields">
            <label>{t("Descrição")}<textarea rows={6} value={desc} maxLength={2000} onChange={(e) => setDesc(e.target.value)} /></label>
            <label>{t("Gêneros (separados por vírgula)")}<input value={genres} onChange={(e) => setGenres(e.target.value)} /></label>
          </div>
          <div className="row end">
            <Button variant="ghost" onClick={() => setDlg(null)}>{t("Cancelar")}</Button>
            <Button variant="primary" onClick={() => run(async () => { setInfo(await api.saveManualInfo(p.id, game.id, desc, genres.split(","))); setDlg(null); }, t("Informações salvas."))}>{t("Salvar")}</Button>
          </div>
        </Modal>
      )}
    </div>
  );
}
