import { awardLabel, pct } from "../lib/ra";
import { t } from "../lib/i18n";
import type { RaGame } from "../types";

/** Barra de progresso de um jogo: total obtido (claro) e Hardcore (destaque), pontos e selo de conclusão. */
export default function RaProgress({ game: g, compact }: { game: RaGame; compact?: boolean }) {
  const total = g.numAchievements;
  const soft = pct(g.awarded, total);
  const hc = pct(g.awardedHc, total);
  const award = awardLabel(g.awardKind);
  return (
    <div className={`ra-prog ${compact ? "compact" : ""}`}>
      <div className="ra-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={soft}
        aria-label={t("{a} de {b} conquistas", { a: g.awarded, b: total })}>
        <i className="soft" style={{ width: `${soft}%` }} />
        <i className="hc" style={{ width: `${hc}%` }} />
      </div>
      <div className="ra-prog-meta">
        <span><b>{soft}%</b> · {t("{a} de {b} conquistas", { a: g.awarded, b: total })}</span>
        {/* pontos por jogo só com a lista completa em cache (sem ela, a soma ficaria incompleta) */}
        {!compact && g.hasDetails && <span>{t("{a} / {b} pontos", { a: g.pointsEarned, b: g.pointsTotal })}</span>}
        {g.awardedHc > 0 && <span className="ra-hc-txt">{t("{n} em Hardcore", { n: g.awardedHc })}</span>}
        {award && <span className={`ra-award ${g.awardKind}`}>{award}</span>}
      </div>
    </div>
  );
}
