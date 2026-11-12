import GamerStatsPanel from "../components/stats/GamerStatsPanel";
import { t } from "../lib/i18n";

/** Página Estatísticas: a mesma central consolidada do Perfil (NEXSTAGE + Steam + RetroAchievements). */
export default function Stats() {
  return (
    <>
      <div className="page-head"><h1>{t("Estatísticas")}</h1></div>
      <GamerStatsPanel />
    </>
  );
}
