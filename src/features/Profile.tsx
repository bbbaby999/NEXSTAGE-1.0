import { CSSProperties, useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useSession } from "../state/session";
import { useLibrary } from "../state/useLibrary";
import { api, errMsg, imgUrl } from "../lib/api";
import { formatDate } from "../lib/format";
import { DEFAULT_CFG, Layout, PageCfg, SectionId, normalizeCfg, normalizeShowcase, showcaseValid } from "../lib/profileStats";
import GamerStatsPanel from "../components/stats/GamerStatsPanel";
import Avatar from "../components/Avatar";
import Button from "../components/Button";
import Modal from "../components/Modal";
import ProfileForm from "../components/ProfileForm";
import AccountSettings from "../components/AccountSettings";
import Seg from "../components/Seg";
import Skeleton from "../components/Skeleton";
import { useProfileScore } from "../components/AchievementScore";
import { LevelRing, ProfileSidebar, RecentActivity, Showcase, tierOf } from "../components/profile/ProfileParts";
import type { Activity, Game, GamerStats } from "../types";
import { t, tk } from "../lib/i18n";

const LABEL: Record<SectionId, string> = { stats: tk("Estatísticas"), score: tk("Pontuação de conquistas"), favorites: tk("Jogos favoritos"), activity: tk("Atividade recente") };
const ACCENTS = ["#2f6bff", "#8a3dff", "#ff3b5c", "#12b886", "#ffb020", "#18c5ff", "#ff6fb5"];

const hexRgb = (h: string): [number, number, number] => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16)) as [number, number, number];
/** Cor de destaque do perfil: sobrescreve as variáveis do tema só dentro desta página. */
function accentVars(hex: string | null): CSSProperties | undefined {
  if (!hex) return undefined;
  const [r, g, b] = hexRgb(hex);
  const l = [r, g, b].map((v) => Math.round(v + (255 - v) * 0.45));
  return {
    "--a": hex, "--b": `rgb(${l.join(",")})`, "--ar": `${r},${g},${b}`, "--br": l.join(","),
    "--lab": `color-mix(in srgb, ${hex} 62%, white)`,
  } as CSSProperties;
}

/** Página "Meu perfil": cabeçalho com arte, avatar e nível; coluna principal (vitrine, atividade, estatísticas) e
 *  coluna lateral (pontuação, números, conquistas recentes e raras). Personalização salva por perfil. */
export default function Profile() {
  const { profile, setProfile, replaceProfile, setView, setOpenGameId, setStartManage, statsIntent, setStatsIntent, toast } = useSession();
  const p = profile!;
  const { games, loading } = useLibrary(p.id);
  const [cfg, setCfg] = useState<PageCfg>(DEFAULT_CFG);
  const [showcase, setShowcase] = useState<number[]>([]);
  const [activity, setActivity] = useState<Activity | null>(null);
  const [editing, setEditing] = useState(false);
  const [customizing, setCustomizing] = useState(false);
  const [pickFav, setPickFav] = useState(false);
  const [account, setAccount] = useState(false);
  const [ready, setReady] = useState(false);
  const score = useProfileScore(p.id);
  const [stats, setStats] = useState<GamerStats | null>(null);
  useEffect(() => { api.gamerStats(p.id).then(setStats).catch(() => {}); }, [p.id]);

  const gameIds = useMemo(() => games.map((g) => g.id), [games]);
  useEffect(() => {
    let dead = false;
    Promise.all([api.getProfileSetting(p.id, "profile_page"), api.getProfileSetting(p.id, "showcase")])
      .then(([c, s]) => { if (!dead) { setCfg(normalizeCfg(c)); setShowcase(normalizeShowcase(s, gameIds)); setReady(true); } })
      .catch(() => setReady(true));
    return () => { dead = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p.id, loading]);

  const loadActivity = useCallback(() => api.profileActivity(p.id).then(setActivity).catch((e) => toast(errMsg(e), "err")), [p.id, toast]);
  useEffect(() => {
    loadActivity();
    let un: (() => void) | undefined;
    let cancelled = false;
    listen("game-session-ended", () => loadActivity()).then((f) => (cancelled ? f() : (un = f)));
    return () => { cancelled = true; un?.(); };
  }, [loadActivity]);

  const saveCfg = (next: PageCfg) => {
    setCfg(next);
    api.setProfileSetting(p.id, "profile_page", JSON.stringify(next)).catch((e) => toast(errMsg(e), "err"));
  };
  const saveShowcase = async (ids: number[]) => {
    try { await api.setProfileSetting(p.id, "showcase", JSON.stringify(ids)); setShowcase(ids); setPickFav(false); toast(t("Favoritos do perfil salvos.")); }
    catch (e) { toast(errMsg(e), "err"); }
  };
  const openGame = (g: Game) => { setView("library"); setOpenGameId(g.id); };
  // Vindo de um card da Home: abre a aba pedida e rola até as Estatísticas (mesmo se a seção estiver oculta).
  const [intent] = useState(statsIntent);
  useEffect(() => {
    if (!intent || !ready) return;
    setStatsIntent(null);
    const r = requestAnimationFrame(() => document.getElementById("gamer-stats")?.scrollIntoView({ behavior: "smooth", block: "start" }));
    return () => cancelAnimationFrame(r);
  }, [intent, ready, setStatsIntent]);
  const favGames = showcase.map((id) => games.find((g) => g.id === id)).filter((g): g is Game => !!g);

  if (loading || !ready) return <Skeleton count={4} />;
  const tier = tierOf(score?.level ?? 1);
  // arte do cabeçalho: banner do perfil; sem banner, a arte do 1º favorito
  const heroArt = p.banner ?? (favGames[0] ? imgUrl(favGames[0].background ?? favGames[0].cover) : null);

  const move = (id: SectionId, d: number) => {
    const o = [...cfg.order]; const i = o.indexOf(id); const j = i + d;
    if (j < 0 || j >= o.length) return;
    [o[i], o[j]] = [o[j], o[i]];
    saveCfg({ ...cfg, order: o });
  };
  const toggleHidden = (id: SectionId) =>
    saveCfg({ ...cfg, hidden: cfg.hidden.includes(id) ? cfg.hidden.filter((s) => s !== id) : [...cfg.hidden, id] });

  const section = (id: SectionId) => {
    if (id === "stats") return (
      <section key={id} className="pf-section">
        <h2 className="section">{t("Estatísticas")}</h2>
        <GamerStatsPanel initialTab={intent} inProfile />
      </section>
    );
    // "score" fica na coluna lateral (ProfileSidebar)
    if (id === "favorites") return (
      <section key={id} className="pf-section">
        <div className="pf-head"><h2 className="section">{t("Jogos favoritos")}</h2><Button onClick={() => setPickFav(true)}>{favGames.length ? t("Editar favoritos") : t("Escolher favoritos")}</Button></div>
        {favGames.length ? <Showcase games={favGames} onOpen={openGame} />
          : <div className="empty"><p>{t("Escolha de 3 a 5 jogos da sua Biblioteca para destacar aqui.")}</p></div>}
      </section>
    );
    return (
      <section key={id} className="pf-section">
        <h2 className="section">{t("Atividade recente")}</h2>
        {!activity || (!activity.sessions.length && !activity.completed.length) ? (
          <div className="empty"><p>{t("Ainda não há atividade registrada. Inicie um jogo pelo NEXSTAGE para ver o histórico.")}</p></div>
        ) : (
          <RecentActivity activity={activity} games={games} onOpen={openGame} />
        )}
      </section>
    );
  };

  return (
    <div className={`profile-page pf2 pf-${cfg.layout}`} style={{ ...accentVars(cfg.accent), "--tier": tier.rgb } as CSSProperties}>
      <header className={`pf2-hero tier-${tier.name}`}>
        <div className="pf2-hero-art" style={heroArt ? { backgroundImage: `url(${heroArt})` } : undefined} />
        <div className="pf-shade" />
        <div className="pf2-id">
          <div className="pf2-frame"><Avatar name={p.displayName} src={p.avatar} size={cfg.layout === "compact" ? 96 : 136} /></div>
          <div className="pf-text">
            <h1>{p.displayName}</h1>
            <p className="muted">@{p.username} {t("· Perfil criado em")}{" "}{formatDate(p.createdAt)}</p>
            {p.bio && <p className="pf-bio">{p.bio}</p>}
            {(p.favoriteGame || p.favoritePlatform) && (
              <div className="pf2-tags">
                {p.favoriteGame && <span>★ {p.favoriteGame}</span>}
                {p.favoritePlatform && <span>{p.favoritePlatform}</span>}
              </div>
            )}
          </div>
          {score && (
            <div className="pf2-level">
              <LevelRing score={score} size={cfg.layout === "compact" ? 92 : 116} />
              <strong>{t("{n} pts", { n: score.total.toLocaleString(undefined) })}</strong>
            </div>
          )}
        </div>
        <div className="pf2-actions">
          <Button variant="primary" onClick={() => setEditing(true)}>{t("Editar perfil")}</Button>
          <Button onClick={() => setAccount(true)}>{t("Configurações da conta")}</Button>
          <Button onClick={() => setCustomizing((c) => !c)}>{customizing ? t("Fechar personalização") : t("Personalizar")}</Button>
          <Button variant="ghost" onClick={() => { setStartManage(true); setProfile(null); }}>{t("Gerenciar perfis")}</Button>
        </div>
      </header>

      {customizing && (
        <div className="pf-custom">
          <div className="row"><strong>{t("Layout")}</strong>
            <Seg<Layout> value={cfg.layout} onChange={(l) => saveCfg({ ...cfg, layout: l })}
              opts={[[t("Cinematográfico"), "cinematic"], [t("Compacto"), "compact"], [t("Focado em estatísticas"), "stats"]]} /></div>
          <div className="pf-order">
            {cfg.order.map((id, i) => (
              <div className="pf-order-row" key={id}>
                <span className={cfg.hidden.includes(id) ? "muted" : ""}>{t(LABEL[id])}{cfg.hidden.includes(id) ? t(" (oculta)") : ""}</span>
                <div className="row">
                  <Button onClick={() => move(id, -1)} disabled={i === 0} aria-label={t("Subir {v0}", { v0: t(LABEL[id]) })}>▲</Button>
                  <Button onClick={() => move(id, 1)} disabled={i === cfg.order.length - 1} aria-label={t("Descer {v0}", { v0: t(LABEL[id]) })}>▼</Button>
                  <Button onClick={() => toggleHidden(id)}>{cfg.hidden.includes(id) ? t("Exibir") : t("Ocultar")}</Button>
                </div>
              </div>
            ))}
          </div>
          <div className="row"><strong>{t("Cor de destaque")}</strong>
            <button data-focus type="button" className={`swatch ${cfg.accent === null ? "on" : ""}`} onClick={() => saveCfg({ ...cfg, accent: null })} title={t("Usar a cor do tema")}>{t("Tema")}</button>
            {ACCENTS.map((c) => (
              <button key={c} data-focus type="button" className={`swatch ${cfg.accent === c ? "on" : ""}`} style={{ background: c }}
                aria-label={t("Cor {c}", { c })} onClick={() => saveCfg({ ...cfg, accent: c })} />
            ))}
            <Button variant="ghost" onClick={() => saveCfg(DEFAULT_CFG)}>{t("Restaurar padrão")}</Button>
          </div>
        </div>
      )}

      <div className="pf2-grid">
        <div className="pf-body">
          {cfg.order.filter((id) => id !== "score" && (!cfg.hidden.includes(id) || (id === "stats" && intent))).map(section)}
          {cfg.hidden.length === cfg.order.length && <div className="empty"><p>{t("Todas as seções estão ocultas. Use “Personalizar” para exibi-las.")}</p></div>}
        </div>
        <ProfileSidebar profileId={p.id} score={score} showScore={!cfg.hidden.includes("score")} stats={stats} games={games} />
      </div>

      {editing && (
        <ProfileForm profile={p} onClose={() => setEditing(false)} onDone={async (deleted) => {
          setEditing(false);
          if (deleted) return setProfile(null);
          const fresh = (await api.listProfiles()).find((x) => x.id === p.id);
          if (fresh) replaceProfile(fresh);
        }} />
      )}
      {account && <AccountSettings onClose={() => setAccount(false)} />}
      {pickFav && <ShowcaseEditor games={games} initial={showcase} onSave={saveShowcase} onClose={() => setPickFav(false)} />}
    </div>
  );
}

/** Escolhe de 3 a 5 jogos e define a ordem. Só habilita Salvar com 0 (limpar) ou 3 a 5 jogos. */
function ShowcaseEditor({ games, initial, onSave, onClose }: { games: Game[]; initial: number[]; onSave: (ids: number[]) => void; onClose: () => void }) {
  const [ids, setIds] = useState<number[]>(initial);
  const title = (id: number) => games.find((g) => g.id === id)?.title ?? "—";
  const swap = (i: number, d: number) => setIds((a) => { const n = [...a]; const j = i + d; if (j < 0 || j >= n.length) return a; [n[i], n[j]] = [n[j], n[i]]; return n; });
  return (
    <Modal title={t("Jogos favoritos do perfil")} onClose={onClose} wide>
      <p className="muted">{t("Escolha de 3 a 5 jogos e defina a ordem. Selecionados:")}{" "}{ids.length}/5.</p>
      <div className="pf-pick">
        <div>
          <h3 className="section">{t("Ordem")}</h3>
          {ids.length === 0 && <p className="muted">{t("Nenhum jogo escolhido.")}</p>}
          {ids.map((id, i) => (
            <div className="pf-order-row" key={id}>
              <span>#{i + 1} {title(id)}</span>
              <div className="row">
                <Button onClick={() => swap(i, -1)} disabled={i === 0}>▲</Button>
                <Button onClick={() => swap(i, 1)} disabled={i === ids.length - 1}>▼</Button>
                <Button variant="danger" onClick={() => setIds(ids.filter((x) => x !== id))}>{t("Remover")}</Button>
              </div>
            </div>
          ))}
        </div>
        <div>
          <h3 className="section">{t("Biblioteca")}</h3>
          <div className="pick-list" style={{ maxHeight: 280 }}>
            {games.filter((g) => !ids.includes(g.id)).map((g) => (
              <button key={g.id} data-focus type="button" className="opt" disabled={ids.length >= 5} onClick={() => setIds([...ids, g.id])}>
                <span>{g.title}</span><i>+</i>
              </button>
            ))}
          </div>
        </div>
      </div>
      {!showcaseValid(ids.length) && <p className="muted note">{t("Escolha de 3 a 5 jogos (ou nenhum, para limpar).")}</p>}
      <div className="row end">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="primary" disabled={!showcaseValid(ids.length)} onClick={() => onSave(ids)}>{t("Salvar")}</Button>
      </div>
    </Modal>
  );
}
