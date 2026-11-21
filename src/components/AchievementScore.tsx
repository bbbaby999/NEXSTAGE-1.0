import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import Button from "./Button";
import { t } from "../lib/i18n";
import type { ProfileScore } from "../types";

/** Pontuação do perfil, recarregada quando chegam conquistas novas (Steam, RetroAchievements, troféus). */
export function useProfileScore(profileId: number): ProfileScore | null {
  const [s, setS] = useState<ProfileScore | null>(null);
  const load = useCallback(() => { api.profileScore(profileId).then(setS).catch(() => {}); }, [profileId]);
  useEffect(() => {
    load();
    const uns: (() => void)[] = [];
    let dead = false;
    for (const ev of ["steam-synced", "ra-synced", "trophies-changed"]) listen(ev, () => load()).then((f) => (dead ? f() : uns.push(f)));
    return () => { dead = true; uns.forEach((f) => f()); };
  }, [load]);
  return s;
}

/** Selo de nível (cabeçalho do perfil). */
export function LevelBadge({ score }: { score: ProfileScore }) {
  return (
    <span className="lvl-badge" title={t("Pontuação de conquistas")}>
      <b>{score.level}</b><span>{t("Nível")}</span><em>{t("{n} pts", { n: score.total.toLocaleString(undefined) })}</em>
    </span>
  );
}

/** Seção do perfil: nível, barra até o próximo nível e a pontuação de cada fonte. */
export default function AchievementScore({ score }: { score: ProfileScore | null }) {
  const [how, setHow] = useState(false);
  if (!score) return null;
  const span = score.nextLevelAt - score.levelFloor;
  const pct = span > 0 ? Math.min(100, Math.round(((score.total - score.levelFloor) / span) * 100)) : 0;
  const rows: [string, ProfileScore["ra"], string][] = [
    ["Steam", score.steam, t("{n} jogo(s) 100%", { n: score.steam.completed })],
    ["RetroAchievements", score.ra, t("{n} jogo(s) completo(s)", { n: score.ra.completed })],
    [t("Troféus"), score.trophies, t("{n} platina(s)", { n: score.trophies.completed })],
    ["NexStage", score.nexstage, t("{n} jogo(s) 100%", { n: score.nexstage.completed })],
  ];
  return (
    <div className="score-panel">
      <div className="score-level">
        <div className="score-ring"><b>{score.level}</b><span>{t("Nível")}</span></div>
        <div className="score-main">
          <strong>{t("{n} pts", { n: score.total.toLocaleString(undefined) })}</strong>
          <div className="ra-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}><i className="soft" style={{ width: `${pct}%` }} /></div>
          <small className="muted">{t("Faltam {n} pts para o nível {l}", { n: (score.nextLevelAt - score.total).toLocaleString(undefined), l: score.level + 1 })}</small>
        </div>
      </div>
      <div className="score-sources">
        {rows.map(([name, s, extra]) => (
          <div key={name} className="ra-stat">
            <strong>{s.points.toLocaleString(undefined)}</strong>
            <span>{name}</span>
            <small className="muted">{t("{n} conquista(s)", { n: s.achievements.toLocaleString(undefined) })} · {extra}</small>
          </div>
        ))}
      </div>
      <Button variant="ghost" onClick={() => setHow(!how)}>{how ? t("Ocultar como funciona") : t("Como a pontuação funciona")}</Button>
      {how && (
        <div className="score-how muted">
          <p>{t("Cada conquista vale pontos na escala dos troféus do PlayStation, somados de todas as fontes:")}</p>
          <ul>
            <li>{t("Troféus: bronze 15, prata 30, ouro 90, platina 180 (só jogos que estão na sua biblioteca).")}</li>
            <li>{t("Steam, pela % de jogadores que têm a conquista: 30% ou mais 15, de 10% a 30% 30, de 3% a 10% 60, menos de 3% 90. Jogo 100%: +180.")}</li>
            <li>{t("RetroAchievements, pelos pontos da conquista: 1 a 4 vale 15, 5 a 9 vale 30, 10 a 24 vale 60, 25 ou mais vale 90; Hardcore +50%. Jogo completo: +180.")}</li>
            <li>{t("Do nível N para o N+1 são precisos 250 × N pontos. Tudo é recalculado a partir das conquistas guardadas.")}</li>
          </ul>
        </div>
      )}
    </div>
  );
}
