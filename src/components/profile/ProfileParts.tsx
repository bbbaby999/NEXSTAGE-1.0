import { CSSProperties, useEffect, useMemo, useState } from "react";
import { api, imgUrl } from "../../lib/api";
import { formatDate, formatDateTime, formatDuration } from "../../lib/format";
import { raAgo } from "../../lib/ra";
import { sourceLabel } from "../../lib/gamerStats";
import { raValue, steamValue, trophyValue } from "../../lib/score";
import { gradeLabel } from "../TrophyGamePanel";
import { TrophyIcon } from "../RaUnlockToasts";
import { t } from "../../lib/i18n";
import type { Activity, Game, GamerStats, ProfileScore, TrophySetSummary } from "../../types";

/** Faixa de cor pelo nível (moldura do avatar, anel do nível): bronze → prata → ouro → esmeralda → safira → ametista. */
export function tierOf(level: number): { name: string; rgb: string } {
  if (level >= 75) return { name: "amethyst", rgb: "176,124,255" };
  if (level >= 50) return { name: "sapphire", rgb: "74,168,255" };
  if (level >= 30) return { name: "emerald", rgb: "61,220,151" };
  if (level >= 20) return { name: "gold", rgb: "242,193,78" };
  if (level >= 10) return { name: "silver", rgb: "201,209,219" };
  return { name: "bronze", rgb: "197,138,90" };
}

/** Ícone de conquista; se a imagem falhar (endereço antigo/removido), mostra o troféu genérico. */
function AchIcon({ src, size }: { src: string | null; size: number }) {
  const [bad, setBad] = useState(false);
  const url = imgUrl(src);
  return url && !bad ? <img src={url} alt="" loading="lazy" onError={() => setBad(true)} /> : <TrophyIcon size={size} />;
}

/** Arte larga do jogo (fundo próprio, senão a capa). */
const artOf = (g: Game) => imgUrl(g.background ?? g.cover);

/** Anel do nível com o progresso até o próximo. */
export function LevelRing({ score, size = 120 }: { score: ProfileScore; size?: number }) {
  const span = score.nextLevelAt - score.levelFloor;
  const pct = span > 0 ? Math.min(100, ((score.total - score.levelFloor) / span) * 100) : 0;
  return (
    <div className="pf2-ring" style={{ "--p": `${pct}%`, width: size, height: size } as CSSProperties}
      title={t("Faltam {n} pts para o nível {l}", { n: (score.nextLevelAt - score.total).toLocaleString(undefined), l: score.level + 1 })}>
      <div className="pf2-ring-in"><b>{score.level}</b><span>{t("Nível")}</span></div>
    </div>
  );
}

/** Vitrine: o 1º favorito em destaque (arte larga), os demais em capas. */
export function Showcase({ games, onOpen }: { games: Game[]; onOpen: (g: Game) => void }) {
  const [first, ...rest] = games;
  if (!first) return null;
  const art = artOf(first);
  return (
    <div className="pf2-show">
      <button type="button" data-focus className="pf2-feature" onClick={() => onOpen(first)}>
        {art && <span className="pf2-feature-art" style={{ backgroundImage: `url(${art})`, backgroundPosition: `${first.bgX}% ${first.bgY}%` }} />}
        <span className="pf2-feature-shade" />
        <span className="pf2-feature-text">
          <small>{t("Favorito nº 1")}</small>
          <strong>{first.title}</strong>
          <em>{first.platform}{first.playtimeSeconds > 0 ? ` · ${formatDuration(first.playtimeSeconds)}` : ""}{first.completed ? ` · ✓ ${t("Concluído")}` : ""}</em>
        </span>
      </button>
      {rest.length > 0 && (
        <div className="pf2-show-rest">
          {rest.map((g, i) => (
            <button key={g.id} type="button" data-focus className="pf2-cover" onClick={() => onOpen(g)} title={g.title}>
              {g.cover ? <img src={imgUrl(g.cover) ?? undefined} alt="" loading="lazy" /> : <b>{g.title.charAt(0)}</b>}
              <span className="pf2-cover-rank">#{i + 2}</span>
              <span className="pf2-cover-name">{g.title}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** Atividade recente: um cartão largo por jogo (arte, tempo nas sessões recentes, total e última sessão). */
export function RecentActivity({ activity, games, onOpen }: { activity: Activity; games: Game[]; onOpen: (g: Game) => void }) {
  const byGame = useMemo(() => {
    const m = new Map<number, { secs: number; last: string; n: number }>();
    for (const s of activity.sessions) {
      const cur = m.get(s.gameId) ?? { secs: 0, last: s.startedAt, n: 0 };
      cur.secs += s.durationSeconds; cur.n += 1;
      if (s.startedAt > cur.last) cur.last = s.startedAt;
      m.set(s.gameId, cur);
    }
    return [...m.entries()].sort((a, b) => b[1].last.localeCompare(a[1].last)).slice(0, 4);
  }, [activity]);
  return (
    <div className="pf2-act">
      <div className="pf2-act-head">
        <strong>{formatDuration(activity.month.seconds)}</strong>
        <span>{t("Este mês · {sessions} sessão(ões) em {games} jogo(s)", { sessions: activity.month.sessions, games: activity.month.games })}</span>
      </div>
      {byGame.map(([id, a]) => {
        const g = games.find((x) => x.id === id);
        const title = g?.title ?? activity.sessions.find((s) => s.gameId === id)?.title ?? "";
        const art = g ? artOf(g) : null;
        return (
          <button key={id} type="button" data-focus className="pf2-act-row" onClick={() => g && onOpen(g)} disabled={!g}>
            {art && <span className="pf2-act-art" style={{ backgroundImage: `url(${art})` }} />}
            <span className="pf2-act-cover">{g?.cover ? <img src={imgUrl(g.cover) ?? undefined} alt="" loading="lazy" /> : <b>{title.charAt(0)}</b>}</span>
            <span className="pf2-act-text">
              <strong>{title}</strong>
              <small>{t("Última sessão: {d}", { d: formatDateTime(a.last) })}</small>
            </span>
            <span className="pf2-act-time">
              <b>{formatDuration(a.secs)}</b><small>{t("{n} sessão(ões) recente(s)", { n: a.n })}</small>
              {g && g.playtimeSeconds > 0 && <small>{t("{t} no total", { t: formatDuration(g.playtimeSeconds) })}</small>}
            </span>
          </button>
        );
      })}
      {activity.completed.length > 0 && (
        <div className="pf2-done">
          {activity.completed.slice(0, 6).map((c) => (
            <span key={c.gameId} className="pf2-done-chip" title={t("Concluído em") + " " + formatDate(c.completedAt)}>✓ {c.title}</span>
          ))}
        </div>
      )}
    </div>
  );
}

type Unlock = { key: string; name: string; game: string; icon: string | null; at: number; source: string; value: number };

/** Conquistas recentes de todas as fontes (Steam/RetroAchievements pelas estatísticas, troféus pelos emuladores). */
function useRecentUnlocks(profileId: number, stats: GamerStats | null): Unlock[] {
  const [sets, setSets] = useState<TrophySetSummary[]>([]);
  useEffect(() => { api.trophyOverview(profileId).then(setSets).catch(() => {}); }, [profileId]);
  return useMemo(() => {
    const out: Unlock[] = [];
    for (const u of stats?.achievements.recent ?? []) {
      const at = u.at ? Date.parse(u.at.replace(" ", "T") + (u.at.includes("Z") || u.at.includes("+") ? "" : "Z")) / 1000 : 0;
      const value = u.source === "retroachievements" ? raValue(u.points ?? 0, u.hardcore) : steamValue(u.rarityPct);
      out.push({ key: `${u.source}-${u.game}-${u.name}`, name: u.name, game: u.game, icon: u.icon, at, source: sourceLabel(u.source), value });
    }
    for (const s of sets) for (const a of s.trophies) {
      if (a.unlocked) out.push({ key: `${s.commId}-${a.id}`, name: a.name, game: s.title, icon: a.icon, at: a.unlockedAt ?? 0, source: `${s.emulator} · ${gradeLabel(a.grade)}`, value: trophyValue(a.grade) });
    }
    return out.sort((a, b) => b.at - a.at).slice(0, 12);
  }, [stats, sets]);
}

/** Coluna lateral: nível e pontuação, números do perfil, conquistas recentes e as mais raras. */
export function ProfileSidebar({ profileId, score, showScore, stats, games }: {
  profileId: number; score: ProfileScore | null; showScore: boolean; stats: GamerStats | null; games: Game[];
}) {
  const recent = useRecentUnlocks(profileId, stats);
  const completed = games.filter((g) => g.completed).length;
  const hours = stats?.totalSeconds ?? games.reduce((s, g) => s + g.playtimeSeconds, 0);
  const perfect = score ? score.steam.completed + score.ra.completed + score.trophies.completed : 0;
  return (
    <aside className="pf2-side">
      {showScore && score && (
        <div className="pf2-card pf2-levelcard">
          <LevelRing score={score} size={96} />
          <div>
            <strong>{t("{n} pts", { n: score.total.toLocaleString(undefined) })}</strong>
            <small>{t("Faltam {n} pts para o nível {l}", { n: (score.nextLevelAt - score.total).toLocaleString(undefined), l: score.level + 1 })}</small>
            <div className="pf2-split">
              {([["Steam", score.steam.points], ["RA", score.ra.points], [t("Troféus"), score.trophies.points]] as const).map(([n, v]) => (
                <span key={n}><b>{v.toLocaleString(undefined)}</b>{n}</span>
              ))}
            </div>
          </div>
        </div>
      )}
      <div className="pf2-card pf2-nums">
        <div><b>{(stats?.libraryCount ?? games.length).toLocaleString(undefined)}</b><span>{t("Jogos")}</span></div>
        <div><b>{formatDuration(hours)}</b><span>{t("Tempo de jogo")}</span></div>
        <div><b>{completed}</b><span>{t("Concluídos")}</span></div>
        <div><b>{(stats?.achievements.unlocked ?? 0).toLocaleString(undefined)}</b><span>{t("Conquistas")}</span></div>
        {score && <div><b>{perfect}</b><span>{t("100% / platinas")}</span></div>}
        {stats?.favoritePlatform && <div><b className="pf2-small">{stats.favoritePlatform.platform}</b><span>{t("Plataforma mais jogada")}</span></div>}
      </div>
      {recent.length > 0 && (
        <div className="pf2-card">
          <h3>{t("Conquistas recentes")}</h3>
          <div className="pf2-badges">
            {recent.map((u) => (
              <span key={u.key} className="pf2-badge" title={`${u.name} — ${u.game}\n${u.source}${u.at ? ` · ${raAgo(new Date(u.at * 1000).toISOString().slice(0, 19).replace("T", " "))}` : ""} · +${u.value}`}>
                <AchIcon src={u.icon} size={22} />
                <i>+{u.value}</i>
              </span>
            ))}
          </div>
        </div>
      )}
      {(stats?.achievements.rare.length ?? 0) > 0 && (
        <div className="pf2-card">
          <h3>{t("Conquistas mais raras")}</h3>
          <ul className="pf2-rare">
            {stats!.achievements.rare.slice(0, 4).map((u) => (
              <li key={`${u.source}-${u.game}-${u.name}`}>
                <AchIcon src={u.icon} size={20} />
                <span><b>{u.name}</b><small>{u.game}</small></span>
                {u.rarityPct !== null && <em>{u.rarityPct < 10 ? u.rarityPct.toFixed(1) : Math.round(u.rarityPct)}%</em>}
              </li>
            ))}
          </ul>
        </div>
      )}
    </aside>
  );
}
