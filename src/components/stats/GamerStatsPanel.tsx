import { useEffect, useState } from "react";
import StatCover from "./StatCover";
import Button from "../Button";
import Seg from "../Seg";
import StatChart, { lastMonths, monthLabel } from "./StatChart";
import { TrophyIcon } from "../RaUnlockToasts";
import { hours, pctOf, sourceColor, sourceLabel, useGamerStats } from "../../lib/gamerStats";
import { raAgo, raDate } from "../../lib/ra";
import { useSession } from "../../state/session";
import { onPadButton } from "../../lib/nav";
import { useInputDevice } from "../../lib/inputDevice";
import Glyph from "../Glyph";
import { t, tk } from "../../lib/i18n";
import type { GamerStats, StatGame, StatGameAch, StatSource, StatsTab, StatUnlock } from "../../types";


const TABS: [StatsTab, string][] = [
  ["summary", tk("Resumo gamer")], ["top", tk("Jogos mais jogados")], ["achievements", tk("Conquistas")], ["history", tk("Histórico")], ["steam", tk("Steam")],
];

export function SourceTag({ s }: { s: StatSource }) {
  return <span className={`gs-src ${s}`} style={{ borderColor: sourceColor(s) }}>{sourceLabel(s)}</span>;
}

/**
 * Estatísticas gamer completas. `data` (versão pública já filtrada) desliga atualização e links.
 * Tudo vem do cache local consolidado (`gamer_stats`); "Atualizar" sincroniza as contas conectadas.
 */
/** `inProfile`: versão do perfil, sem a lista "Jogos recentes" (o perfil já mostra a Atividade recente) e sem a nota de rodapé. */
export default function GamerStatsPanel({ initialTab, data, inProfile }: { initialTab?: StatsTab | null; data?: GamerStats | null; inProfile?: boolean }) {
  const { profile, toast, setView, setOpenGameId } = useSession();
  const own = useGamerStats(profile!.id);
  const s = data !== undefined ? data : own.stats;
  const readOnly = data !== undefined;
  const [tab, setTab] = useState<StatsTab>(initialTab ?? "summary");
  useEffect(() => { if (initialTab) setTab(initialTab); }, [initialTab]);
  const device = useInputDevice();
  // Controle: L2 = aba anterior, R2 = próxima (para nas pontas). Só sem janela/lista/menu por cima,
  // como na página do jogo e nas Conquistas.
  useEffect(() => onPadButton((b, e) => {
    if (e.defaultPrevented || (b !== "l2" && b !== "r2")) return;
    if (document.querySelector(".overlay, .pick-root, .kb-root, .sm-root:not(.leaving), .ctx-root")) return;
    e.preventDefault();
    setTab((cur) => {
      const i = TABS.findIndex(([k]) => k === cur);
      const j = Math.max(0, Math.min(TABS.length - 1, i + (b === "r2" ? 1 : -1)));
      return TABS[j][0];
    });
  }), []);

  if (!s) return <p className="muted">{t("Carregando…")}</p>;
  const open = (id: number | null) => { if (id && !readOnly) { setView("library"); setOpenGameId(id); } };
  const steamSrc = s.sources.find((x) => x.id === "steam");
  const raSrc = s.sources.find((x) => x.id === "retroachievements");

  async function refresh() {
    const errs = await own.refresh();
    if (errs.length) toast(errs[0], "err"); else toast(t("Estatísticas atualizadas."));
  }

  return (
    <div className="gs" id="gamer-stats">
      {!readOnly && (
        <div className="gs-head">
          <div className="gs-sources">
            <span className="gs-src local">{t("NEXSTAGE · sempre atualizado")}</span>
            <span className={`gs-src steam ${steamSrc ? "" : "off"}`}>
              {steamSrc ? t("Steam · {when}", { when: steamSrc.syncedAt ? raAgo(steamSrc.syncedAt) : t("não sincronizado") }) : t("Steam não conectada")}
            </span>
            <span className={`gs-src retroachievements ${raSrc ? "" : "off"}`}>
              {raSrc ? t("RetroAchievements · {when}", { when: raSrc.syncedAt ? raAgo(raSrc.syncedAt) : t("não sincronizado") }) : t("RetroAchievements não conectado")}
            </span>
          </div>
          <Button onClick={refresh} disabled={own.refreshing}>{own.refreshing ? t("Atualizando…") : t("Atualizar estatísticas")}</Button>
        </div>
      )}
      <div className="gs-tabs">
        {device !== "keyboard" && <Glyph k="l2" />}
        <Seg<StatsTab> value={tab} onChange={setTab} opts={TABS.map(([k, l]) => [t(l), k])} />
        {device !== "keyboard" && <Glyph k="r2" />}
      </div>
      <div className="gs-body" key={tab}>
        {tab === "summary" && <Summary s={s} open={open} hideRecent={inProfile} />}
        {tab === "top" && <Top s={s} open={open} />}
        {tab === "achievements" && <Achs s={s} open={open} />}
        {tab === "history" && <History s={s} />}
        {tab === "steam" && <SteamTab s={s} open={open} />}
      </div>
      {!inProfile && <p className="muted note">
        {t("Tempo de jogo: quando um jogo está vinculado a mais de uma origem (NEXSTAGE e Steam), vale o maior valor, sem somar. Conquistas ficam separadas por plataforma; o total soma apenas conjuntos distintos (Steam + RetroAchievements). Nada é estimado.")}
      </p>}
    </div>
  );
}

function Tile({ label, value, sub, empty }: { label: string; value?: string | number | null; sub?: string | null; empty?: string }) {
  const has = value !== undefined && value !== null && value !== "";
  return (
    <div className={`gs-tile ${has ? "" : "empty"}`}>
      <span>{label}</span>
      {has ? <strong>{value}</strong> : <em>{empty ?? t("Sem dados suficientes")}</em>}
      {has && sub && <small>{sub}</small>}
    </div>
  );
}

function breakdown(g: StatGame): string {
  const parts: string[] = [];
  if (g.localSeconds) parts.push(`${t("NEXSTAGE")} ${hours(g.localSeconds)}`);
  if (g.steamSeconds !== null) parts.push(`${t("Steam")} ${hours(g.steamSeconds)}`);
  return parts.length > 1 ? parts.join(" · ") : "";
}

function Summary({ s, open, hideRecent }: { s: GamerStats; open: (id: number | null) => void; hideRecent?: boolean }) {
  const a = s.achievements;
  const steamTime = s.steamSeconds;
  return (
    <>
      <div className="gs-tiles">
        <Tile label={t("Total de horas jogadas")} value={s.totalSeconds > 0 ? hours(s.totalSeconds) : null}
          sub={steamTime !== null ? t("NEXSTAGE {a} · Steam {b} (sem dupla contagem)", { a: hours(s.localSeconds), b: hours(steamTime) }) : t("Registrado pelo NEXSTAGE")}
          empty={t("Jogue pelo NEXSTAGE ou sincronize a Steam para descobrir")} />
        <Tile label={t("Jogos na biblioteca")} value={s.libraryCount} />
        <Tile label={t("Conquistas desbloqueadas")} value={a.total > 0 ? a.unlocked.toLocaleString() : null}
          sub={a.platforms.map((p) => `${sourceLabel(p.source)} ${p.unlocked}`).join(" · ")}
          empty={t("Conecte a Steam ou o RetroAchievements para descobrir")} />
        <Tile label={t("Percentual de conclusão")} value={a.total > 0 ? `${a.pct}%` : null}
          sub={a.total > 0 ? t("{a} de {b} conquistas disponíveis nos jogos com dados", { a: a.unlocked, b: a.total }) : null} />
        <Tile label={t("Jogo favorito")} value={s.favorite?.title} sub={s.favorite ? `${hours(s.favorite.totalSeconds)} · ${s.favorite.platform}` : null} />
        <Tile label={t("Plataforma mais utilizada")} value={s.favoritePlatform?.platform} sub={s.favoritePlatform ? hours(s.favoritePlatform.seconds) : null}
          empty={t("Ainda não há tempo de jogo suficiente")} />
        <Tile label={t("Últimas 2 semanas")} value={s.recentSeconds > 0 ? hours(s.recentSeconds) : null} sub={s.mostPlayedRecent ? t("Mais jogado: {g}", { g: s.mostPlayedRecent.title }) : null}
          empty={t("Nenhuma atividade recente")} />
        <Tile label={t("Último jogo")} value={s.lastGame?.title} sub={s.lastGame?.lastActivity ? raAgo(s.lastGame.lastActivity) : null} />
      </div>
      {!hideRecent && s.recentGames.length > 0 && (
        <>
          <h3 className="gs-h3">{t("Jogos recentes")}</h3>
          <div className="gs-recent">
            {s.recentGames.map((g) => (
              <button key={g.key} data-focus className="gs-mini" onClick={() => open(g.libraryGameId)} aria-disabled={!g.libraryGameId}>
                <StatCover className="gs-mini-cover" cover={g.cover} icon={g.icon} title={g.title} />
                <b>{g.title}</b><small>{g.lastActivity ? raAgo(g.lastActivity) : ""}</small>
              </button>
            ))}
          </div>
        </>
      )}
    </>
  );
}

function Rank({ games, open }: { games: StatGame[]; open: (id: number | null) => void }) {
  const max = Math.max(1, ...games.map((g) => g.totalSeconds));
  return (
    <ol className="gs-rank">
      {games.map((g, i) => (
        <li key={g.key}>
          <button data-focus className="gs-rank-row" onClick={() => open(g.libraryGameId)} aria-disabled={!g.libraryGameId}>
            <b className="gs-pos">{i + 1}</b>
            <StatCover className="gs-rank-cover" cover={g.cover} icon={g.icon} title={g.title} />
            <span className="gs-rank-main">
              <strong>{g.title}</strong>
              <small>{g.platform} · {g.sources.map(sourceLabel).join(" + ")}</small>
              <span className="gs-hbar"><i style={{ width: `${(g.totalSeconds / max) * 100}%` }} /></span>
            </span>
            <span className="gs-rank-time"><b>{hours(g.totalSeconds)}</b>{breakdown(g) && <small>{breakdown(g)}</small>}</span>
          </button>
        </li>
      ))}
    </ol>
  );
}

/** Jogos mais jogados: sempre o Top 10. */
function Top({ s, open }: { s: GamerStats; open: (id: number | null) => void }) {
  if (!s.topGames.length) return <div className="empty"><p>{t("Jogue pelo NEXSTAGE ou sincronize sua conta Steam para descobrir seus jogos mais jogados.")}</p></div>;
  return <Rank games={s.topGames.slice(0, 10)} open={open} />;
}

function UnlockGrid({ list, rare }: { list: StatUnlock[]; rare?: boolean }) {
  return (
    <div className="gs-unlocks">
      {list.map((u, i) => (
        <div key={`${u.source}-${u.name}-${i}`} className="gs-unlock" data-focus tabIndex={0}
          aria-label={`${u.name} · ${u.game} · ${sourceLabel(u.source)}${u.at ? ` · ${raDate(u.at)}` : ""}`}>
          <span className="gs-unlock-icon">{u.icon ? <img src={u.icon} alt="" loading="lazy" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : <TrophyIcon />}</span>
          <span className="gs-unlock-text">
            <b>{u.name}</b>
            <small>{u.game}</small>
            <small>{rare && u.rarityPct !== null ? t("{p}% dos jogadores", { p: u.rarityPct.toFixed(1) }) : u.at ? raDate(u.at) : ""}{u.hardcore ? " · HC" : ""}{u.points ? ` · ${u.points} ${t("pts")}` : ""}</small>
          </span>
          <SourceTag s={u.source} />
        </div>
      ))}
    </div>
  );
}

function GameAchList({ list, open }: { list: StatGameAch[]; open: (id: number | null) => void }) {
  return (
    <div className="gs-gach">
      {list.map((g, i) => (
        <button key={`${g.source}-${g.title}-${i}`} data-focus className="gs-gach-row" onClick={() => open(g.libraryGameId)} aria-disabled={!g.libraryGameId}>
          <StatCover className="gs-mini-cover sq" cover={g.icon} title={g.title} />
          <span className="gs-rank-main">
            <strong>{g.title}</strong>
            <span className="ra-bar"><i className="soft" style={{ width: `${g.pct}%`, background: sourceColor(g.source) }} /></span>
            <small>{g.unlocked}/{g.total} · {g.pct}%</small>
          </span>
          <SourceTag s={g.source} />
        </button>
      ))}
    </div>
  );
}

function Achs({ s, open }: { s: GamerStats; open: (id: number | null) => void }) {
  const a = s.achievements;
  if (!a.platforms.length) return <div className="empty"><p>{t("Conecte sua conta Steam (com a chave da Web API) ou o RetroAchievements em Configurações para ver suas conquistas.")}</p></div>;
  return (
    <>
      <div className="gs-tiles">
        <Tile label={t("Progresso geral")} value={a.total > 0 ? `${a.pct}%` : null} sub={t("{a} de {b} (Steam + RetroAchievements, conjuntos distintos)", { a: a.unlocked, b: a.total })} />
        {a.platforms.map((p) => (
          <Tile key={p.source} label={sourceLabel(p.source)} value={`${p.unlocked}/${p.total} · ${p.pct}%`}
            sub={p.source === "retroachievements" && a.ra
              ? t("{hc} em Hardcore · {m} masterizado(s)/completo(s)", { hc: a.ra.unlockedHc, m: a.ra.mastered })
              : t("{n} jogo(s) com dados · {c} com 100%", { n: p.games, c: p.gamesComplete })} />
        ))}
        {a.ra && <Tile label={t("Pontuação RetroAchievements")} value={a.ra.points !== null ? a.ra.points.toLocaleString() : null} sub={a.ra.pointsSoftcore !== null ? t("{n} softcore", { n: a.ra.pointsSoftcore.toLocaleString() }) : null} />}
      </div>
      <h3 className="gs-h3">{t("Conquistas recentes")}</h3>
      {a.recent.length ? <UnlockGrid list={a.recent} /> : <p className="muted">{t("Nenhum desbloqueio registrado ainda.")}</p>}
      <h3 className="gs-h3">{t("Conquistas raras")}</h3>
      {a.rare.length ? <UnlockGrid list={a.rare} rare /> : <p className="muted">{t("A raridade vem das porcentagens globais da Steam; o RetroAchievements não as informa no cache. Atualize as conquistas de um jogo Steam para ver.")}</p>}
      <div className="gs-cols">
        <div>
          <h3 className="gs-h3">{t("Jogos 100% concluídos")}</h3>
          {a.completed.length ? <GameAchList list={a.completed} open={open} /> : <p className="muted">{t("Nenhum jogo com todas as conquistas ainda.")}</p>}
        </div>
        <div>
          <h3 className="gs-h3">{t("Mais perto de completar")}</h3>
          {a.closest.length ? <GameAchList list={a.closest} open={open} /> : <p className="muted">{t("Sem jogos em andamento com dados de conquistas.")}</p>}
        </div>
      </div>
    </>
  );
}

function History({ s }: { s: GamerStats }) {
  const [period, setPeriod] = useState<6 | 12 | 24>(12);
  const months = lastMonths(period);
  const byMonth = new Map(s.history.map((h) => [h.month, h]));
  const has = s.history.some((h) => months.includes(h.month) && (h.localSeconds || Object.values(h.unlocks).some((n) => n > 0)));
  const achSources = s.sources.filter((x) => x.hasAchievements).map((x) => x.id);
  const labels = months.map(monthLabel);
  const secs = months.map((m) => byMonth.get(m)?.localSeconds ?? 0);
  let acc = 0;
  const cumulative = secs.map((v) => (acc += v) / 3600);
  const fmtH = (v: number) => hours(Math.round(v));
  return (
    <>
      <div className="row"><Seg<6 | 12 | 24> value={period} onChange={setPeriod} opts={[[t("6 meses"), 6], [t("12 meses"), 12], [t("24 meses"), 24]]} /></div>
      {!has ? (
        <div className="empty"><p>{t("Ainda não há atividade suficiente neste período. Jogue pelo NEXSTAGE ou sincronize suas conquistas para ver os gráficos.")}</p></div>
      ) : (
        <div className="gs-charts">
          <section>
            <h3 className="gs-h3">{t("Horas jogadas por mês")}</h3>
            <StatChart kind="bar" title={t("Horas jogadas por mês")} labels={labels} format={fmtH}
              series={[{ key: "local", label: t("NEXSTAGE"), color: sourceColor("local"), values: secs }]} />
            <p className="muted note">{t("Sessões registradas pelo NEXSTAGE. A Steam informa só o total acumulado, não o tempo por mês.")}</p>
          </section>
          <section>
            <h3 className="gs-h3">{t("Conquistas desbloqueadas por mês")}</h3>
            <StatChart kind="bar" title={t("Conquistas desbloqueadas por mês")} labels={labels} format={(v) => String(Math.round(v))}
              series={achSources.map((src) => ({ key: src, label: sourceLabel(src), color: sourceColor(src), values: months.map((m) => byMonth.get(m)?.unlocks[src] ?? 0) }))} />
          </section>
          <section>
            <h3 className="gs-h3">{t("Evolução do tempo de jogo")}</h3>
            <StatChart kind="line" title={t("Evolução do tempo de jogo")} labels={labels} format={(v) => hours(Math.round(v * 3600))}
              series={[{ key: "cum", label: t("Acumulado no período"), color: sourceColor("local"), values: cumulative }]} />
          </section>
        </div>
      )}
    </>
  );
}

function SteamTab({ s, open }: { s: GamerStats; open: (id: number | null) => void }) {
  const st = s.steam;
  if (!st) return <div className="empty"><p>{t("Sincronize sua conta Steam para descobrir suas estatísticas da Steam (Configurações → Contas e plataformas).")}</p></div>;
  const timed = s.sources.find((x) => x.id === "steam")?.hasPlaytime;
  return (
    <>
      <div className="gs-tiles">
        <Tile label={t("Total na Steam")} value={timed ? hours(st.totalSeconds) : null} empty={t("Salve a chave da Web API para ver o tempo de jogo")} />
        <Tile label={t("Últimas 2 semanas (Steam)")} value={timed ? hours(st.recentSeconds) : null} />
        <Tile label={t("Jogos na conta")} value={st.owned} sub={timed ? t("{n} já jogado(s)", { n: st.played }) : null} />
        <Tile label={t("Conquistas Steam")} value={st.achTotal > 0 ? `${st.achUnlocked}/${st.achTotal}` : null} sub={st.achTotal > 0 ? `${pctOf(st.achUnlocked, st.achTotal)}%` : null}
          empty={t("Atualize os dados de um jogo Steam para ver as conquistas")} />
      </div>
      {st.ranking.length > 0 && (<><h3 className="gs-h3">{t("Ranking pessoal por tempo de jogo")}</h3><Rank games={st.ranking} open={open} /></>)}
      {st.recent.length > 0 && (
        <>
          <h3 className="gs-h3">{t("Jogados recentemente")}</h3>
          <div className="gs-recent">
            {st.recent.map((g) => (
              <button key={g.key} data-focus className="gs-mini" onClick={() => open(g.libraryGameId)} aria-disabled={!g.libraryGameId}>
                <StatCover className="gs-mini-cover" cover={g.cover} icon={g.icon} title={g.title} />
                <b>{g.title}</b><small>{t("{t} nas últimas 2 semanas", { t: hours(g.recentSeconds) })}</small>
              </button>
            ))}
          </div>
        </>
      )}
      {st.achGames.length > 0 && (<><h3 className="gs-h3">{t("Conquistas por jogo")}</h3><GameAchList list={st.achGames} open={open} /></>)}
      <p className="muted note">{t("Dados oficiais da Steam Web API, sujeitos à privacidade do perfil. A raridade é a porcentagem global de jogadores.")}</p>
    </>
  );
}
