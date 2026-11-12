import { ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import Button from "../components/Button";
import Select from "../components/Select";
import Modal from "../components/Modal";
import { TrophyIcon } from "../components/RaUnlockToasts";
import { pct, raAgo, raDate } from "../lib/ra";
import { useSteamEvents } from "../lib/steam";
import { t, tk } from "../lib/i18n";
import type { SteamAch, SteamAchEntry, SteamAchGame, SteamAchOverview, SteamGameData } from "../types";

type Tab = "overview" | "recent" | "earned" | "pending" | "rare";
type Sort = "activity" | "progress" | "progress-asc" | "name";

const TABS: [Tab, string][] = [
  ["overview", tk("Visão geral")], ["recent", tk("Recentes")], ["earned", tk("Obtidas")], ["pending", tk("Pendentes")], ["rare", tk("Raras")],
];

/** Linha de conquista Steam: ícone (cinza se pendente), nome, descrição, data e raridade global. */
function SteamAchItem({ a, gameName, onOpenGame }: { a: SteamAch; gameName?: string; onOpenGame?: () => void }) {
  const done = a.unlockedAt !== null;
  const desc = a.description || (a.hidden && !done ? t("Conquista oculta") : t("Sem descrição."));
  const body = (
    <>
      <div className="ra-badge">{a.icon ? <img src={a.icon} alt="" loading="lazy" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : <TrophyIcon size={24} />}</div>
      <div className="ra-ach-text">
        <strong>{a.name}</strong>
        <span>{desc}</span>
        <small>
          {gameName && <>{gameName} · </>}
          {done ? (a.unlockedAt ? t("Obtida em {d}", { d: raDate(a.unlockedAt) }) : t("Obtida")) : t("Pendente")}
        </small>
      </div>
      {a.globalPct !== null && (
        <div className="ra-ach-side" title={t("{p}% dos jogadores", { p: a.globalPct.toFixed(1) })}>
          <b>{a.globalPct < 10 ? a.globalPct.toFixed(1) : Math.round(a.globalPct)}%</b><small>{t("jogadores")}</small>
        </div>
      )}
    </>
  );
  return onOpenGame
    ? <button data-focus className={`ra-ach ${done ? "done" : "locked"}`} onClick={onOpenGame}>{body}</button>
    : <div className={`ra-ach ${done ? "done" : "locked"}`}>{body}</div>;
}

/** Barra de progresso de um jogo Steam. */
function SteamProgress({ unlocked, total, compact }: { unlocked: number; total: number; compact?: boolean }) {
  const p = pct(unlocked, total);
  return (
    <div className={`ra-prog ${compact ? "compact" : ""}`}>
      <div className="ra-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={p}
        aria-label={t("{a} de {b} conquistas", { a: unlocked, b: total })}>
        <i className="soft" style={{ width: `${p}%` }} />
      </div>
      <div className="ra-prog-meta">
        <span><b>{p}%</b> · {t("{a} de {b} conquistas", { a: unlocked, b: total })}</span>
        {total > 0 && unlocked === total && <span className="ra-award mastered">{t("100% concluído")}</span>}
      </div>
    </div>
  );
}

/** Conquistas de um jogo Steam (cache; "Atualizar" consulta a API). */
function SteamGameModal({ appid, title, libraryGameId, onClose, onOpenLibrary }: {
  appid: number; title: string; libraryGameId: number | null; onClose: () => void; onOpenLibrary: (gameId: number) => void;
}) {
  const { profile, toast } = useSession();
  const p = profile!;
  const [d, setD] = useState<SteamGameData | null>(null);
  const [busy, setBusy] = useState(false);
  const [show, setShow] = useState<"all" | "done" | "pending">("all");

  const load = useCallback(async (refresh: boolean) => {
    setBusy(true);
    try {
      const [data, warn] = await api.steamGame(p.id, appid, refresh);
      setD(data);
      if (warn && refresh) toast(warn, "err");
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }, [p.id, appid, toast]);
  useEffect(() => { load(false); }, [load]);

  const list = (d?.achievements ?? []).filter((a) => show === "all" || (show === "done") === (a.unlockedAt !== null));
  return (
    <Modal title={title} onClose={onClose} wide>
      {d && d.total > 0 && <SteamProgress unlocked={d.unlocked} total={d.total} />}
      {d?.error === "private" && <p className="ra-warn">{t("A Steam não liberou as conquistas: os detalhes de jogos do perfil estão privados.")}</p>}
      <div className="row">
        <div className="seg">
          {([["all", tk("Todas")], ["done", tk("Obtidas")], ["pending", tk("Pendentes")]] as const).map(([k, l]) => (
            <button key={k} type="button" data-focus className={show === k ? "on" : ""} onClick={() => setShow(k)}>{t(l)}</button>
          ))}
        </div>
        <Button onClick={() => load(true)} disabled={busy}>{busy ? t("Atualizando…") : t("Atualizar dados da Steam")}</Button>
        <Button variant="ghost" onClick={() => openUrl(`https://steamcommunity.com/stats/${appid}/achievements`).catch(() => {})}>{t("Ver na Steam")}</Button>
        {libraryGameId !== null && <Button variant="ghost" onClick={() => onOpenLibrary(libraryGameId)}>{t("Abrir na biblioteca")}</Button>}
      </div>
      <div className="ra-ach-list">
        {d === null && <p className="muted">{t("Carregando…")}</p>}
        {d && list.length === 0 && <div className="empty"><p>{d.total ? t("Nenhuma conquista nesta lista.") : t("Conquistas ainda não carregadas.")}</p></div>}
        {list.map((a) => <SteamAchItem key={a.apiname} a={a} />)}
      </div>
    </Modal>
  );
}

/**
 * Aba Conquistas → Steam. Tudo vem do cache local (funciona offline); "Sincronizar agora" consulta a
 * Web API com a chave pessoal do usuário. Nada é inventado: sem dados, a tela diz que não há dados.
 */
export default function SteamAchievements({ switcher }: { switcher: ReactNode }) {
  const { profile, toast, setView, setOpenGameId } = useSession();
  const p = profile!;
  const [ov, setOv] = useState<SteamAchOverview | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("name"); // lista de jogos em ordem alfabética por padrão
  const [list, setList] = useState<SteamAchEntry[] | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [open, setOpen] = useState<{ appid: number; title: string; libraryGameId: number | null } | null>(null);

  const load = useCallback(() => api.steamAchOverview(p.id).then((o) => { setOv(o); setSyncing(!!o.account?.syncing); }).catch((e) => toast(errMsg(e), "err")), [p.id, toast]);
  useEffect(() => { load(); }, [load]);
  useSteamEvents(p.id, (kind, running) => { if (kind === "state") setSyncing(!!running); else load(); });

  useEffect(() => {
    if (tab === "overview") return;
    let alive = true;
    setList(null);
    const h = setTimeout(() => {
      api.steamAchList(p.id, tab, query).then((l) => alive && setList(l)).catch((e) => toast(errMsg(e), "err"));
    }, 180);
    return () => { alive = false; clearTimeout(h); };
  }, [tab, query, p.id, ov, toast]);

  const byApp = useMemo(() => new Map((ov?.games ?? []).map((g) => [g.appid, g])), [ov]);
  const games = useMemo(() => {
    const q = query.trim().toLowerCase();
    const g = (ov?.games ?? []).filter((x) => !q || x.name.toLowerCase().includes(q));
    const by: Record<Sort, (a: SteamAchGame, b: SteamAchGame) => number> = {
      activity: (a, b) => (b.lastUnlock ?? "").localeCompare(a.lastUnlock ?? "") || b.unlocked - a.unlocked,
      progress: (a, b) => pct(b.unlocked, b.total) - pct(a.unlocked, a.total) || b.unlocked - a.unlocked,
      "progress-asc": (a, b) => pct(a.unlocked, a.total) - pct(b.unlocked, b.total) || a.unlocked - b.unlocked,
      name: (a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true }),
    };
    return [...g].sort(by[sort]);
  }, [ov, query, sort]);

  async function sync() {
    setSyncing(true);
    try {
      const r = await api.steamSync(p.id);
      if (r.libraryPrivate) toast(t("A Steam não liberou a lista de jogos: os detalhes de jogos do perfil estão privados."), "err");
      else toast(r.achievementsUpdated ? t("Conquistas atualizadas em {n} jogo(s).", { n: r.achievementsUpdated }) : t("Tudo em dia: nenhum jogo com conquistas para atualizar."));
      if (r.warning) toast(r.warning, "err");
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setSyncing(false); load(); }
  }
  const openGame = (appid: number, title: string) => setOpen({ appid, title, libraryGameId: byApp.get(appid)?.libraryGameId ?? null });
  const openLibrary = (gameId: number) => { setOpen(null); setOpenGameId(gameId); };

  if (!ov) return null;
  const acc = ov.account;

  if (!acc) {
    return (
      <>
        <div className="page-head"><h1>{t("Conquistas")}</h1>{switcher}</div>
        <div className="empty ra-empty">
          <TrophyIcon size={40} />
          <h2>{t("Conecte sua conta Steam")}</h2>
          <p>{t("Veja as conquistas de todos os seus jogos da Steam, o progresso de cada um e as conquistas mais raras que você obteve.")}</p>
          <Button variant="primary" onClick={() => setView("settings")}>{t("Ir para Configurações")}</Button>
        </div>
      </>
    );
  }

  const pr = acc.profile;
  const tot = ov.totals;
  return (
    <div className="ra-page">
      <div className="page-head">
        <h1>{t("Conquistas")}</h1>
        {switcher}
        <div className="row">
          <span className={`ra-sync-state ${acc.syncError ? "err" : ""}`}>
            {syncing ? t("Sincronizando…") : acc.syncError ? acc.syncError : t("Sincronizado {ago}", { ago: raAgo(acc.syncedAt) })}
          </span>
          <Button variant="primary" onClick={sync} disabled={syncing || !acc.hasKey}>{syncing ? t("Sincronizando…") : t("Sincronizar agora")}</Button>
        </div>
      </div>
      {!acc.hasKey && (
        <p className="ra-warn">
          {acc.keyUnreadable
            ? t("A chave da Web API salva não pode ser lida neste Windows. Salve a chave novamente em Configurações. Os dados abaixo são do último sincronismo.")
            : t("As conquistas da Steam precisam da sua chave pessoal da Web API (Configurações → Contas e plataformas → Steam).")}
        </p>
      )}

      <section className="ra-hero">
        <button data-focus className="ra-user" onClick={() => openUrl(acc.profileUrl).catch(() => {})} title={t("Abrir perfil na Steam")}>
          {pr?.avatar ? <img src={pr.avatar} alt="" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : <span className="ra-user-ph"><TrophyIcon size={28} /></span>}
          <span><b>{pr?.name || acc.name}</b><small>Steam</small></span>
        </button>
        <div className="ra-stats">
          <div className="ra-stat main"><strong>{tot.unlocked.toLocaleString(undefined)}</strong><span>{t("Conquistas desbloqueadas")}</span></div>
          <div className="ra-stat"><strong>{pct(tot.unlocked, tot.total)}%</strong><span>{t("Conclusão média")}</span></div>
          <div className="ra-stat"><strong>{tot.games}</strong><span>{t("Jogos com progresso")}</span></div>
          <div className="ra-stat"><strong>{tot.perfect}</strong><span>{t("Jogos 100%")}</span></div>
        </div>
      </section>

      {tot.pendingGames > 0 && (
        <p className="ra-note">{t("Conquistas ainda não carregadas para {n} jogo(s): elas chegam aos poucos nas próximas sincronizações (ou use “Sincronizar agora”).", { n: tot.pendingGames })}</p>
      )}

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
                  <button key={`${a.appid}-${a.apiname}`} data-focus className="ra-recent-item" onClick={() => openGame(a.appid, a.gameName)} title={`${a.name} — ${a.description}`}>
                    {a.icon ? <img src={a.icon} alt="" loading="lazy" /> : <TrophyIcon size={30} />}
                    <b>{a.name}</b>
                    <small>{a.gameName}</small>
                    <em>{a.unlockedAt ? raAgo(a.unlockedAt) : t("Obtida")}{a.globalPct !== null ? ` · ${a.globalPct.toFixed(1)}%` : ""}</em>
                  </button>
                ))}
              </div>
            </>
          )}
          <h2 className="section">{t("Jogos com conquistas")}</h2>
          {games.length === 0 ? (
            <div className="empty"><p>{ov.games.length ? t("Nenhum jogo encontrado para essa pesquisa.") : acc.syncedAt ? t("Nenhuma conquista carregada ainda.") : t("Ainda não sincronizado. Use “Sincronizar agora”.")}</p></div>
          ) : (
            <div className="ra-games">
              {games.map((g) => (
                <button key={g.appid} data-focus className="ra-game" onClick={() => openGame(g.appid, g.name)}>
                  <div className="ra-game-top">
                    {g.iconUrl ? <img src={g.iconUrl} alt="" loading="lazy" /> : <span className="ra-icon-ph"><TrophyIcon /></span>}
                    <div>
                      <strong>{g.name}</strong>
                      <small>Steam{g.libraryGameId ? ` · ${t("Na biblioteca")}` : ""}</small>
                      <small>{t("Último desbloqueio: {ago}", { ago: raAgo(g.lastUnlock) })}</small>
                    </div>
                  </div>
                  <SteamProgress unlocked={g.unlocked} total={g.total} compact />
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
              tab === "pending" ? t("Nenhuma conquista pendente nos jogos já carregados.")
                : tab === "recent" ? t("Nenhuma conquista nos últimos 30 dias.")
                : t("Nenhuma conquista registrada ainda.")
            }</p></div>
          )}
          {list?.map((a) => <SteamAchItem key={`${a.appid}-${a.apiname}`} a={a} gameName={a.gameName} onOpenGame={() => openGame(a.appid, a.gameName)} />)}
        </div>
      )}

      {open && <SteamGameModal {...open} onClose={() => setOpen(null)} onOpenLibrary={openLibrary} />}
    </div>
  );
}
