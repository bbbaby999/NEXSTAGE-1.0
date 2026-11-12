import { CSSProperties, useEffect, useRef, useState } from "react";
import { useSession } from "../state/session";
import { useLibrary } from "../state/useLibrary";
import { api, errMsg } from "../lib/api";
import { formatDate, formatDateTime, formatPlaytime } from "../lib/format";
import { launchWithGameboot } from "../lib/gameboot";
import { useAccent } from "../lib/accent";
import { tiltHandlers } from "../lib/tilt";
import { recentGames } from "../lib/recent";
import { useTaggedImage } from "../lib/useImage";
import { homeBgImage, useHomeBg } from "../lib/homeBg";
import Button from "../components/Button";
import HomeBackdrop from "../components/HomeBackdrop";
import Skeleton from "../components/Skeleton";
import PlatformBadge from "../components/PlatformBadge";
import { CaptureMenu, ICON, MenuAction } from "../components/CaptureUi";
import { itemSrc } from "../components/Viewer";
import HomeUniverse from "../components/stats/HomeUniverse";
import SteamUpdates from "../components/SteamUpdates";
import type { Game, GalleryItem } from "../types";
import { t } from "../lib/i18n";
import { steamHero } from "../lib/steam";

// Parallax 3D das capas: o mesmo da Biblioteca (mesma inclinação, suavidade e reflexo).
const coverTilt = tiltHandlers<HTMLButtonElement>();
const cardTilt = tiltHandlers<HTMLButtonElement>((el) => el.querySelector<HTMLElement>(".replay-cover"));

/** Miniaturas na faixa abaixo das ações (como na composição de referência). */
const THUMBS = 3;
const MAX_TAGS = 4;
/** Jogos no carrossel "Volte a jogar" (mais recente → mais antigo). */
const REPLAY = 8;

type Thumb = { key: string; src: string; label: string; open: () => void };

/**
 * Tela inicial: somente o ÚLTIMO jogo realmente iniciado (last_played_at é gravado pelo backend
 * no lançamento; abrir a página de detalhes não altera). Arte do jogo em tela cheia (leve parallax),
 * capa à esquerda e, ao lado, título, etiquetas, ações e uma faixa de capturas.
 */
export default function Home() {
  const { profile, setView, setOpenGameId, setGalleryIntent, toast } = useSession();
  const p = profile!;
  const { games, loading, reload } = useLibrary(p.id);
  const [caps, setCaps] = useState<GalleryItem[]>([]);
  const [netShots, setNetShots] = useState<string[]>([]);
  const [menu, setMenu] = useState(false);
  // Jogo em foco (controle) ou sob o mouse no carrossel "Volte a jogar": troca só o plano de fundo.
  const [preview, setPreview] = useState<Game | null>(null);
  const [shown, setShown] = useState<Game | null>(null);
  useEffect(() => {
    // pequeno atraso: ao varrer o carrossel com o mouse, o fundo não pisca a cada capa
    const t = setTimeout(() => setShown(preview), preview ? 120 : 60);
    return () => clearTimeout(t);
  }, [preview]);

  const recent = recentGames(games, REPLAY);
  const game = recent[0];
  // cor de destaque tirada da capa (tinge brilhos e detalhes da Home; legível sobre qualquer arte)
  const accent = useAccent(game?.cover);
  const bgGame = (shown && games.some((g) => g.id === shown.id) ? shown : null) ?? game;
  // A imagem vem junto com o jogo DONO dela: enquanto a do jogo novo carrega, a antiga continua com os
  // próprios ajustes (posição, capa suavizada), em vez de herdar os do jogo novo.
  // Modo do plano de fundo (Configurações → Aparência): arte do jogo, só as ondas ou imagem própria.
  const homeBg = useHomeBg();
  const gameMode = homeBg.mode === "game";
  const { src: bg, tag: bgOwner } = useTaggedImage(
    gameMode ? [bgGame?.background, bgGame?.steamId ? steamHero(bgGame.steamId) : null, bgGame?.cover] : [],
    gameMode ? bgGame ?? null : null);
  // Imagem própria ou captura da Galeria (sem imagem salva, fica só com as ondas — como em "Sem fundo").
  const ownImage = homeBgImage(homeBg);
  const customBg = ownImage ? <HomeBackdrop src={ownImage} x={50} y={50} soft={false} /> : null;

  useEffect(() => {
    if (!game) { setCaps([]); setNetShots([]); return; }
    let dead = false;
    api.listGallery(p.id)
      .then((l) => { if (!dead) setCaps(l.filter((i) => i.gameId === game.id && i.exists)); })
      .catch(() => {});
    // Sem capturas próprias, a faixa usa as screenshots já buscadas (Steam ou internet).
    api.getGameInfo(p.id, game.id)
      .then((i) => { if (!dead) setNetShots([...(i?.screenshots ?? []), ...(i?.webScreenshots ?? []).map((w) => w.thumb)]); })
      .catch(() => {});
    return () => { dead = true; };
  }, [p.id, game?.id]);


  // Botão Menu/Options do controle (ou tecla de menu) abre as opções extras.
  useEffect(() => {
    const onCtx = () => { if (game && !document.querySelector(".overlay, .ctx-root, .sm-root:not(.leaving)")) setMenu(true); };
    window.addEventListener("nexstage:context", onCtx);
    return () => window.removeEventListener("nexstage:context", onCtx);
  }, [game]);

  if (loading) return <Skeleton count={4} />;

  if (!game) {
    return (
      <div className="ps-home stage empty-stage">
        {customBg}
        <h1 className="sr-only">{t("Tela inicial")}</h1>
        <div className="empty">
          <h2>{games.length ? t("Nenhum jogo iniciado ainda") : t("Sua biblioteca está vazia")}</h2>
          <p>{games.length
            ? t("O último jogo que você iniciar pelo NEXSTAGE aparece aqui.")
            : t("Adicione o executável de um jogo ou de um emulador para começar.")}</p>
          <Button variant="primary" onClick={() => setView("library")}>
            {games.length ? t("Abrir biblioteca") : t("Adicionar jogo")}
          </Button>
        </div>
      </div>
    );
  }

  const openInLibrary = () => { setView("library"); setOpenGameId(game.id); };
  const openGallery = (itemId?: number) => { setGalleryIntent({ gameId: game.id, itemId }); setView("gallery"); };
  const play = async () => {
    try {
      await launchWithGameboot(game, () => api.launchGame(p.id, game.id), () => api.checkLaunch(p.id, game.id));
      toast(t("Iniciando {title}…", { title: game.title }));
      reload();
    } catch (e) { toast(errMsg(e), "err"); }
  };
  const toggleFav = async () => {
    try { await api.setFavorite(p.id, game.id, !game.favorite); reload(); } catch (e) { toast(errMsg(e), "err"); }
  };
  const owner = (bgOwner && games.find((g) => g.id === bgOwner.id)) ?? bgOwner ?? bgGame;
  const usingCover = !!bg && bg === owner.cover && bg !== owner.background;
  const usingBg = !!bg && bg === owner.background;

  const thumbs: Thumb[] = caps.length
    ? caps.slice(0, THUMBS).map((c) => ({ key: `c${c.id}`, src: itemSrc(c), label: t("Abrir captura na Galeria"), open: () => openGallery(c.id) }))
    : netShots.slice(0, THUMBS).map((s) => ({ key: s, src: s, label: t("Ver screenshots na página do jogo"), open: openInLibrary }));
  const extra = caps.length > THUMBS ? caps.length - THUMBS : 0;
  const tags = [game.platform, ...game.genres.filter((g) => g !== game.platform)].slice(0, MAX_TAGS + 1);

  const actions: MenuAction[] = [
    { label: t("Capturas de tela{v0}", { v0: caps.length ? ` (${caps.length})` : "" }), icon: ICON.open, run: () => openGallery() },
    { label: game.favorite ? t("Remover dos favoritos") : t("Favoritar"), icon: ICON.star, run: toggleFav },
    { label: t("Abrir na Biblioteca"), icon: ICON.folder, run: openInLibrary },
  ];

  return (
    <div className={`ps-home stage ${accent ? "tinted" : ""}`} style={accent ? ({ "--game-rgb": accent } as CSSProperties) : undefined}>
      {gameMode ? <HomeBackdrop src={bg} x={usingBg ? owner.bgX : 50} y={usingBg ? owner.bgY : 50} soft={usingCover} /> : customBg}
      <h1 className="sr-only">{t("Tela inicial")}</h1>
      <div className="stage-top">
      <div className="stage-hero" key={game.id} data-parallax>
        <button data-focus className="stage-cover" onClick={openInLibrary} aria-label={t("Abrir {title} na Biblioteca", { title: game.title })}
          onMouseMove={coverTilt.onMouseMove} onMouseLeave={coverTilt.onMouseLeave}
          style={game.cover ? { backgroundImage: `url(${game.cover})` } : undefined}>
          {!game.cover && <span>{game.title.charAt(0).toUpperCase()}</span>}
          {/* favorito: no canto da capa (como na Biblioteca), longe do nome */}
          {game.favorite && <i className="fav" title={t("Favorito")} aria-label={t("Favorito")}>★</i>}
        </button>

        <div className="stage-info">
          <div className="stage-head">
            <h2 className="stage-title">{game.title}</h2>
            <div className="stage-tags">
              {tags.map((tag) => <span key={tag}>{t(tag)}</span>)}
            </div>
            <small className="stage-when">{t("Jogado por último em {date}", { date: formatDateTime(game.lastPlayedAt) })}{formatPlaytime(game.playtimeSeconds) && <> · <b>{t("{time} de jogo", { time: formatPlaytime(game.playtimeSeconds)! })}</b></>}</small>
          </div>

          <div className="stage-actions">
            <Button variant="primary" className="stage-play" data-autofocus /* foco inicial da Home (lib/pageFocus.ts) */ onClick={play}>{t("Jogar")}</Button>
            <Button className="stage-open" onClick={openInLibrary}>{t("Página do jogo")}</Button>
            <button data-focus type="button" className="stage-more" onClick={() => setMenu(true)} aria-label={t("Mais opções")} title={t("Mais opções")}>
              <svg viewBox="0 0 24 24" width="22" height="22" fill="currentColor" aria-hidden="true">
                <circle cx="5.5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="18.5" cy="12" r="1.7" />
              </svg>
            </button>
          </div>

          {thumbs.length > 0 && (
            <div className="stage-shots">
              {thumbs.map((t) => (
                <button key={t.key} data-focus className="stage-shot" aria-label={t.label} onClick={t.open}>
                  <img src={t.src} alt="" loading="lazy" decoding="async" referrerPolicy="no-referrer" />
                </button>
              ))}
              {extra > 0 && <button data-focus className="stage-shot more" onClick={() => openGallery()} aria-label={t("Mais {extra} capturas", { extra })}>+{extra}</button>}
            </div>
          )}
        </div>
      </div>
      </div>
      <Replay games={recent} onPreview={setPreview} onOpen={(g) => { setView("library"); setOpenGameId(g.id); }} />
      {/* complementa a Home: fica abaixo do destaque e do "Volte a jogar" */}
      <HomeUniverse />
      {/* notas de atualização oficiais dos jogos Steam (carrega sozinha, sem atrasar a Home) */}
      <SteamUpdates />
      {menu && <CaptureMenu actions={actions} onClose={() => setMenu(false)} />}
    </div>
  );
}

/**
 * "Volte a jogar": carrossel horizontal com os últimos jogos iniciados. Controle e teclado navegam
 * pelo foco (o item focado é centralizado); com o mouse, a roda sobre o carrossel rola na horizontal.
 */
function Replay({ games, onOpen, onPreview }: { games: Game[]; onOpen: (g: Game) => void; onPreview: (g: Game | null) => void }) {
  const track = useRef<HTMLDivElement>(null);
  const [edge, setEdge] = useState({ start: true, end: true });

  const measure = () => {
    const t = track.current;
    if (!t) return;
    setEdge({ start: t.scrollLeft <= 4, end: t.scrollLeft + t.clientWidth >= t.scrollWidth - 4 });
  };
  useEffect(() => {
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [games.length]);

  // Sem setas: a roda do mouse sobre o carrossel rola na horizontal. Nas pontas, a roda segue para a
  // página normalmente (listener não passivo, para poder impedir a rolagem vertical só quando rola o carrossel).
  useEffect(() => {
    const t = track.current;
    if (!t) return;
    const onWheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return; // touchpad horizontal: comportamento nativo
      const max = t.scrollWidth - t.clientWidth;
      if (max <= 0) return;
      const atEdge = (e.deltaY < 0 && t.scrollLeft <= 0) || (e.deltaY > 0 && t.scrollLeft >= max - 1);
      if (atEdge) return;
      e.preventDefault();
      t.scrollBy({ left: e.deltaY * 1.4 });
    };
    t.addEventListener("wheel", onWheel, { passive: false });
    return () => t.removeEventListener("wheel", onWheel);
  }, [games.length]);

  if (!games.length) return null;
  // Saindo do carrossel (mouse ou foco), o fundo volta ao jogo em destaque — ou ao card que ainda tem o foco.
  const focusedGame = () => {
    const id = Number((document.activeElement as HTMLElement | null)?.closest?.(".replay-card")?.getAttribute("data-game-id"));
    return games.find((g) => g.id === id) ?? null;
  };

  return (
    <section className="replay" aria-label={t("Volte a jogar")} data-parallax>
      <div className="replay-head">
        <h2 className="section">{t("Volte a jogar")}</h2>
      </div>
      <div className={`replay-track ${edge.start ? "at-start" : ""} ${edge.end ? "at-end" : ""}`} ref={track} onScroll={measure}
        onMouseLeave={() => onPreview(focusedGame())}
        onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) onPreview(null); }}>
        {games.map((g, i) => (
          <button key={g.id} data-focus className="replay-card" style={{ "--i": i } as CSSProperties} data-game-id={g.id}
            onFocus={() => onPreview(g)} onMouseEnter={() => onPreview(g)}
            onMouseMove={cardTilt.onMouseMove} onMouseLeave={cardTilt.onMouseLeave}
            onClick={() => onOpen(g)} aria-label={t("{title}, jogado em {v1}", { title: g.title, v1: formatDate(g.lastPlayedAt) })}>
            {/* A animação de entrada fica no elemento interno: a regra global de foco ([data-focus]:focus
                { animation: none }) a reiniciava no botão ao perder o foco, e a capa sumia (opacidade 0). */}
            <span className="replay-inner">
              <span className="replay-cover" style={g.cover ? { backgroundImage: `url(${g.cover})` } : undefined}>
                {!g.cover && <b>{g.title.charAt(0).toUpperCase()}</b>}
                <PlatformBadge platform={g.platform} />
              </span>
              <strong>{g.title}</strong>
              <small>{[formatDate(g.lastPlayedAt), formatPlaytime(g.playtimeSeconds)].filter(Boolean).join(" · ")}</small>
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}
