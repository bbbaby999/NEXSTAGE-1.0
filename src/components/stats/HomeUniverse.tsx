import { CSSProperties, ReactNode } from "react";
import { useSession } from "../../state/session";
import { hours, sourceLabel, useGamerStats } from "../../lib/gamerStats";
import { raAgo } from "../../lib/ra";
import { t } from "../../lib/i18n";
import type { StatsTab } from "../../types";

const I = {
  star: "M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z",
  fire: "M12 3c1 3 4 4.5 4 8.5A4 4 0 0 1 12 16a4 4 0 0 1-4-4.5c0-1.6.8-2.8 1.6-3.7C10 9.6 11 10 11 10s-.4-4 1-7ZM7 17.5A5.5 5.5 0 0 0 17 17",
  clock: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18ZM12 7v5l3 2",
  trophy: "M8 4h8v5a4 4 0 0 1-8 0V4ZM8 6H5a3 3 0 0 0 3 4M16 6h3a3 3 0 0 1-3 4M12 13v4M8.5 20h7M10 17h4",
  ring: "M12 21a9 9 0 1 0 0-18M12 3a9 9 0 0 1 0 18",
  grid: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
  play: "M8 5v14l11-7z",
  list: "M5 6h14M5 12h14M5 18h9",
  pad: "M7 8h10a4 4 0 0 1 4 4v1.5a3.5 3.5 0 0 1-6.3 2.1L14 14.5h-4l-.7 1.1A3.5 3.5 0 0 1 3 13.5V12a4 4 0 0 1 4-4Z",
};

function Card({ icon, label, value, sub, empty, tab, i, children }: {
  icon: string; label: string; value?: ReactNode; sub?: ReactNode; empty: string; tab: StatsTab; i: number; children?: ReactNode;
}) {
  const { setView, setStatsIntent } = useSession();
  const has = value !== undefined && value !== null && value !== "";
  return (
    <button data-focus className={`hu-card ${has ? "" : "empty"}`} style={{ "--i": i } as CSSProperties}
      onClick={() => { setStatsIntent(tab); setView("profile"); }} aria-label={`${label}: ${has ? String(value) : empty}`}>
      {/* animação de entrada no elemento interno: no botão, a regra global de foco a reiniciava ao sair dele e o card sumia */}
      <span className="hu-inner">
      <span className="hu-icon"><svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={icon} /></svg></span>
      <span className="hu-label">{label}</span>
      {has ? <strong className="hu-value">{value}</strong> : <em className="hu-empty">{empty}</em>}
      {has && sub && <small className="hu-sub">{sub}</small>}
      {children}
      </span>
    </button>
  );
}

/**
 * "Seu universo gamer": cards compactos abaixo do carrossel "Volte a jogar" (não substituem o destaque,
 * o botão Jogar nem a navegação). Selecionar um card abre a aba correspondente das Estatísticas no Perfil.
 */
export default function HomeUniverse() {
  const { profile } = useSession();
  const { stats: s } = useGamerStats(profile!.id);
  if (!s) return null;
  const steamOn = s.sources.some((x) => x.id === "steam");
  const anyAch = s.sources.some((x) => x.id !== "local");
  const a = s.achievements;
  const noTime = steamOn ? t("Jogue ou sincronize a Steam para descobrir") : t("Sincronize sua conta Steam para descobrir");
  const noAch = anyAch ? t("Atualize suas conquistas para descobrir") : t("Conecte a Steam ou o RetroAchievements para descobrir");
  const p = a.total > 0 ? a.pct : null;

  return (
    <section className="hu" aria-label={t("Seu universo gamer")}>
      <h2 className="section">{t("Seu universo gamer")}</h2>
      <div className="hu-grid">
        <Card i={0} tab="summary" icon={I.star} label={t("Jogo favorito")} value={s.favorite?.title} sub={s.favorite && hours(s.favorite.totalSeconds)} empty={noTime} />
        <Card i={1} tab="top" icon={I.fire} label={t("Mais jogado")} value={s.mostPlayedRecent?.title}
          sub={s.mostPlayedRecent && t("{t} nas últimas 2 semanas", { t: hours(s.mostPlayedRecent.recentSeconds) })} empty={t("Nenhuma atividade nas últimas 2 semanas")} />
        <Card i={2} tab="summary" icon={I.clock} label={t("Horas jogadas")} value={s.totalSeconds > 0 ? hours(s.totalSeconds) : null}
          sub={s.steamSeconds !== null ? t("NEXSTAGE + Steam, sem dupla contagem") : t("Registradas pelo NEXSTAGE")} empty={noTime} />
        <Card i={3} tab="achievements" icon={I.trophy} label={t("Conquistas desbloqueadas")} value={a.total > 0 ? a.unlocked.toLocaleString() : null}
          sub={a.platforms.map((x) => `${sourceLabel(x.source)} ${x.unlocked}`).join(" · ")} empty={noAch} />
        <Card i={4} tab="achievements" icon={I.ring} label={t("Progresso de conquistas")} value={p !== null ? `${p}%` : null}
          sub={p !== null ? t("{a} de {b}", { a: a.unlocked, b: a.total }) : null} empty={noAch}>
          {p !== null && <span className="hu-bar"><i style={{ width: `${p}%` }} /></span>}
        </Card>
        <Card i={5} tab="summary" icon={I.grid} label={t("Jogos na biblioteca")} value={s.libraryCount} empty="" />
        <Card i={6} tab="history" icon={I.play} label={t("Último jogo")} value={s.lastGame?.title} sub={s.lastGame?.lastActivity && raAgo(s.lastGame.lastActivity)} empty={t("Nenhuma atividade registrada")} />
        <Card i={7} tab="history" icon={I.list} label={t("Jogos recentes")} value={s.recentGames.length ? s.recentGames.slice(0, 3).map((g) => g.title).join(" · ") : null}
          empty={t("Nenhuma atividade registrada")} />
        <Card i={8} tab="summary" icon={I.pad} label={t("Plataforma favorita")} value={s.favoritePlatform?.platform} sub={s.favoritePlatform && hours(s.favoritePlatform.seconds)}
          empty={t("Ainda não há dados suficientes")} />
      </div>
    </section>
  );
}
