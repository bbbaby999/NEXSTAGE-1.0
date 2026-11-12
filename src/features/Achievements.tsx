import { ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import Button from "../components/Button";
import Select from "../components/Select";
import RaAchievementItem from "../components/RaAchievementItem";
import RaGameModal from "../components/RaGameModal";
import RaProgress from "../components/RaProgress";
import { TrophyIcon } from "../components/RaUnlockToasts";
import { pct, raAgo, useRaEvents } from "../lib/ra";
import { onPadButton } from "../lib/nav";
import { useInputDevice } from "../lib/inputDevice";
import Glyph from "../components/Glyph";
import { t, tk } from "../lib/i18n";
import SteamAchievements from "./SteamAchievements";
import type { RaAchievement, RaEmuSupport, RaOverview } from "../types";

type Source = "ra" | "steam";
const SOURCE_KEY = "nexstage.achievementsSource";
const readSource = (): Source | null => { try { const v = localStorage.getItem(SOURCE_KEY); return v === "ra" || v === "steam" ? v : null; } catch { return null; } };
const saveSource = (v: Source) => { try { localStorage.setItem(SOURCE_KEY, v); } catch { /* sem armazenamento: só não lembra */ } };

/**
 * Aba Conquistas: RetroAchievements ou Steam (seletor no topo). Sem escolha salva, abre na fonte que
 * tem conta conectada (RetroAchievements primeiro).
 */
export default function Achievements() {
  const { profile } = useSession();
  const p = profile!;
  const [source, setSource] = useState<Source | null>(readSource);
  const device = useInputDevice();
  const pick = useCallback((v: Source) => { setSource(v); saveSource(v); }, []);
  // Controle: L2 = RetroAchievements, R2 = Steam (só sem janela/lista/menu por cima, como na página do jogo)
  useEffect(() => onPadButton((b, e) => {
    if (e.defaultPrevented || (b !== "l2" && b !== "r2")) return;
    if (document.querySelector(".overlay, .pick-root, .kb-root, .sm-root:not(.leaving), .ctx-root")) return;
    e.preventDefault();
    pick(b === "r2" ? "steam" : "ra");
  }), [pick]);
  useEffect(() => {
    if (source) return;
    Promise.all([api.raGetAccount(p.id).catch(() => null), api.steamGetAccount(p.id).catch(() => null)])
      .then(([ra, steam]) => setSource(!ra && steam ? "steam" : "ra"));
  }, [source, p.id]);
  if (!source) return null;
  const switcher = (
    <div className="seg ach-source" role="tablist" aria-label={t("Fonte das conquistas")}>
      <button type="button" role="tab" aria-selected={source === "ra"} data-focus className={source === "ra" ? "on" : ""} onClick={() => pick("ra")}>
        {device !== "keyboard" && <Glyph k="l2" />}RetroAchievements
      </button>
      <button type="button" role="tab" aria-selected={source === "steam"} data-focus className={source === "steam" ? "on" : ""} onClick={() => pick("steam")}>
        Steam{device !== "keyboard" && <Glyph k="r2" />}
      </button>
    </div>
  );
  return source === "steam" ? <SteamAchievements switcher={switcher} /> : <RaAchievements switcher={switcher} />;
}

type Tab = "overview" | "recent" | "earned" | "pending" | "hardcore";
type Sort = "activity" | "progress" | "progress-asc" | "name";

const TABS: [Tab, string][] = [
  ["overview", tk("Visão geral")], ["recent", tk("Recentes")], ["earned", tk("Obtidas")], ["pending", tk("Pendentes")], ["hardcore", tk("Hardcore")],
];

/**
 * Central de conquistas do RetroAchievements. Tudo vem do cache local (funciona offline);
 * "Sincronizar" consulta a API oficial. Nada é inventado: sem dados, a tela diz que não há dados.
 */
function RaAchievements({ switcher }: { switcher: ReactNode }) {
  const { profile, toast, setView, setOpenGameId } = useSession();
  const p = profile!;
  const [ov, setOv] = useState<RaOverview | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("name"); // lista de jogos em ordem alfabética por padrão
  const [list, setList] = useState<RaAchievement[] | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [openGame, setOpenGame] = useState<number | null>(null);
  const [emus, setEmus] = useState<RaEmuSupport[]>([]);

  const load = useCallback(() => api.raOverview(p.id).then((o) => { setOv(o); setSyncing(!!o.account?.syncing); }).catch((e) => toast(errMsg(e), "err")), [p.id, toast]);
  useEffect(() => { load(); api.raEmulatorSupport().then(setEmus).catch(() => {}); }, [load]);
  useRaEvents(p.id, (kind, payload) => {
    if (kind === "state") setSyncing(!!(payload as { running: boolean }).running);
    else load();
  });

  // listas de conquistas (abas que não são a visão geral)
  useEffect(() => {
    if (tab === "overview") return;
    let alive = true;
    const h = setTimeout(() => {
      api.raListAchievements(p.id, tab, query).then((l) => alive && setList(l)).catch((e) => toast(errMsg(e), "err"));
    }, 180);
    return () => { alive = false; clearTimeout(h); };
  }, [tab, query, p.id, ov, toast]);

  const games = useMemo(() => {
    const q = query.trim().toLowerCase();
    const g = (ov?.games ?? []).filter((x) => !q || x.title.toLowerCase().includes(q) || x.consoleName.toLowerCase().includes(q));
    const prog = (x: { awarded: number; numAchievements: number }) => pct(x.awarded, x.numAchievements);
    const by: Record<Sort, (a: typeof g[0], b: typeof g[0]) => number> = {
      activity: (a, b) => (b.lastAwardAt ?? "").localeCompare(a.lastAwardAt ?? ""),
      progress: (a, b) => prog(b) - prog(a) || b.awarded - a.awarded,
      "progress-asc": (a, b) => prog(a) - prog(b) || a.awarded - b.awarded,
      name: (a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true }),
    };
    return [...g].sort(by[sort]);
  }, [ov, query, sort]);

  async function sync() {
    setSyncing(true);
    try {
      const r = await api.raSync(p.id);
      toast(r.baseline ? t("Primeira sincronização concluída. A partir de agora, novos desbloqueios serão avisados.")
        : r.newUnlocks ? t("{n} novo(s) desbloqueio(s) sincronizado(s).", { n: r.newUnlocks }) : t("Tudo em dia: nenhum desbloqueio novo."));
      if (r.warning) toast(r.warning, "err");
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setSyncing(false); load(); }
  }
  async function scan() {
    setScanning(true);
    try {
      const r = await api.raScanLibrary(p.id);
      toast(t("{c} jogo(s) verificado(s): {l} associado(s) pelo hash oficial, {s} aguardando confirmação.", { c: r.checked, l: r.linked, s: r.suggested }));
      if (r.errors.length) toast(r.errors[0], "err");
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setScanning(false); load(); }
  }
  const openLibrary = (gameId: number) => { setOpenGame(null); setOpenGameId(gameId); };

  if (!ov) return null;
  const acc = ov.account;

  if (!acc) {
    return (
      <>
        <div className="page-head"><h1>{t("Conquistas")}</h1>{switcher}</div>
        <div className="empty ra-empty">
          <TrophyIcon size={40} />
          <h2>{t("Conecte sua conta do RetroAchievements")}</h2>
          <p>{t("Veja pontos, conquistas e progresso dos seus jogos retrô, receba avisos de novos desbloqueios e acompanhe tudo na página de cada jogo.")}</p>
          <Button variant="primary" onClick={() => setView("settings")}>{t("Ir para Configurações")}</Button>
        </div>
      </>
    );
  }

  const pr = acc.profile;
  const realtime = emus.filter((e) => e.mode === "realtime");
  return (
    <div className="ra-page">
      <div className="page-head">
        <h1>{t("Conquistas")}</h1>
        {switcher}
        <div className="row">
          <span className={`ra-sync-state ${acc.syncError ? "err" : ""}`}>
            {syncing ? t("Sincronizando…") : acc.syncError ? acc.syncError : t("Sincronizado {ago}", { ago: raAgo(acc.syncedAt) })}
          </span>
          <Button onClick={scan} disabled={scanning}>{scanning ? t("Identificando…") : t("Identificar jogos da biblioteca")}</Button>
          <Button variant="primary" onClick={sync} disabled={syncing || !acc.hasKey}>{syncing ? t("Sincronizando…") : t("Sincronizar agora")}</Button>
        </div>
      </div>
      {!acc.hasKey && <p className="ra-warn">{t("A chave salva não pode ser lida neste Windows. Conecte a conta novamente em Configurações. Os dados abaixo são do último sincronismo.")}</p>}

      <section className="ra-hero">
        <button data-focus className="ra-user" onClick={() => openUrl(acc.profileUrl).catch(() => {})} title={t("Abrir perfil no RetroAchievements")}>
          {pr?.userPic ? <img src={pr.userPic} alt="" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : <span className="ra-user-ph"><TrophyIcon size={28} /></span>}
          <span><b>{acc.username}</b><small>{pr?.richPresence || (pr?.memberSince ? t("Membro desde {d}", { d: pr.memberSince.slice(0, 10) }) : "RetroAchievements")}</small></span>
        </button>
        <div className="ra-stats">
          <div className="ra-stat main"><strong>{(pr?.totalPoints ?? 0).toLocaleString(undefined)}</strong><span>{t("Pontos (Hardcore)")}</span></div>
          <div className="ra-stat"><strong>{(pr?.totalSoftcorePoints ?? 0).toLocaleString(undefined)}</strong><span>{t("Pontos (Softcore)")}</span></div>
          <div className="ra-stat"><strong>{ov.totals.unlocked.toLocaleString(undefined)}</strong><span>{t("Conquistas desbloqueadas")}</span></div>
          <div className="ra-stat"><strong>{ov.totals.games}</strong><span>{t("Jogos com progresso")}</span></div>
          <div className="ra-stat"><strong>{ov.totals.mastered + ov.totals.completed}</strong><span>{t("Masterizados / completos")}</span></div>
        </div>
      </section>

      <p className="ra-note">
        {realtime.length
          ? t("Avisos quase em tempo real com {list} (desbloqueio lido no log do emulador e confirmado pela API). Nos demais casos, a sincronização automática pode atrasar os avisos.", { list: realtime.map((e) => e.name).join(", ") })
          : t("Os avisos de desbloqueio dependem da sincronização com a conta e podem ter atraso (a cada 2 min durante jogos compatíveis; fora deles, no intervalo escolhido em Configurações).")}
        {ov.totals.gamesWithoutDetails > 0 && <> {t("Lista completa de conquistas ainda não carregada para {n} jogo(s): ela chega aos poucos nas próximas sincronizações.", { n: ov.totals.gamesWithoutDetails })}</>}
      </p>

      <div className="ra-toolbar">
        <div className="seg">
          {TABS.map(([k, l]) => <button key={k} type="button" data-focus className={tab === k ? "on" : ""} onClick={() => setTab(k)}>{t(l)}</button>)}
        </div>
        <input className="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("Pesquisar jogo…")} aria-label={t("Pesquisar jogo")} />
        {tab === "overview" && (
          <Select<Sort> value={sort} width={230} title={t("Ordenar")} onChange={setSort}
            options={[[t("Nome"), "name"], [t("Atividade recente"), "activity"], [t("Maior progresso"), "progress"], [t("Menor progresso"), "progress-asc"]]} />
        )}
      </div>

      {tab === "overview" ? (
        <>
          {ov.recent.length > 0 && !query && (
            <>
              <h2 className="section">{t("Conquistas recentes")}</h2>
              <div className="ra-recent">
                {ov.recent.slice(0, 12).map((a) => (
                  <button key={a.id} data-focus className="ra-recent-item" onClick={() => openUrl(a.url).catch(() => {})} title={`${a.title} — ${a.description}`}>
                    {a.badgeUrl ? <img src={a.badgeUrl} alt="" loading="lazy" /> : <TrophyIcon size={30} />}
                    <b>{a.title}</b>
                    <small>{a.gameTitle}</small>
                    <em>{raAgo(a.unlockedHcAt ?? a.unlockedAt)} · {a.points} {t("pts")}{a.unlockedHcAt ? " · HC" : ""}</em>
                  </button>
                ))}
              </div>
            </>
          )}
          <h2 className="section">{t("Jogos com conquistas")}</h2>
          {games.length === 0 ? (
            <div className="empty"><p>{ov.games.length ? t("Nenhum jogo encontrado para essa pesquisa.") : acc.syncedAt ? t("Nenhum jogo com progresso nesta conta.") : t("Ainda não sincronizado. Use “Sincronizar agora”.")}</p></div>
          ) : (
            <div className="ra-games">
              {games.map((g) => (
                <button key={g.id} data-focus className="ra-game" onClick={() => setOpenGame(g.id)}>
                  <div className="ra-game-top">
                    {g.iconUrl ? <img src={g.iconUrl} alt="" loading="lazy" /> : <span className="ra-icon-ph"><TrophyIcon /></span>}
                    <div>
                      <strong>{g.title}</strong>
                      <small>{g.consoleName}{g.libraryGameId ? ` · ${t("Na biblioteca")}` : ""}</small>
                      <small>{t("Último desbloqueio: {ago}", { ago: raAgo(g.lastAwardAt) })}</small>
                    </div>
                  </div>
                  <RaProgress game={g} compact />
                </button>
              ))}
            </div>
          )}
        </>
      ) : (
        <div className="ra-ach-list">
          {list === null && <p className="muted">{t("Carregando…")}</p>}
          {list?.length === 0 && (
            <div className="empty"><p>{
              tab === "pending" ? t("Nenhuma conquista pendente nos jogos com lista completa carregada.")
                : tab === "recent" ? t("Nenhuma conquista nos últimos 30 dias.")
                : tab === "hardcore" ? t("Nenhuma conquista em Hardcore registrada.")
                : t("Nenhuma conquista registrada ainda.")
            }</p></div>
          )}
          {list?.map((a) => <RaAchievementItem key={a.id} a={a} />)}
        </div>
      )}

      {openGame !== null && <RaGameModal raGameId={openGame} onClose={() => setOpenGame(null)} onOpenLibrary={openLibrary} />}
    </div>
  );
}
