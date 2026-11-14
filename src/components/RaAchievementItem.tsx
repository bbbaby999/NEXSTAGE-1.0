import { openUrl } from "@tauri-apps/plugin-opener";
import { isUnlocked, raDate } from "../lib/ra";
import { t } from "../lib/i18n";
import { raValue } from "../lib/score";
import type { RaAchievement } from "../types";
import { TrophyIcon } from "./RaUnlockToasts";

/** Linha de conquista: ícone (cinza se pendente), nome, descrição, pontos, data e selo Hardcore. */
export default function RaAchievementItem({ a, showGame = true }: { a: RaAchievement; showGame?: boolean }) {
  const done = isUnlocked(a);
  const kindLabel = a.kind === "progression" ? t("Progressão") : a.kind === "win_condition" ? t("Vitória") : a.kind === "missable" ? t("Perdível") : null;
  return (
    <button data-focus className={`ra-ach ${done ? "done" : "locked"}`} onClick={() => openUrl(a.url).catch(() => {})}
      title={t("Abrir no RetroAchievements")}>
      <div className="ra-badge">
        {a.badgeUrl ? <img src={a.badgeUrl} alt="" loading="lazy" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : <TrophyIcon size={24} />}
      </div>
      <div className="ra-ach-text">
        <strong>{a.title}</strong>
        <span>{a.description || t("Sem descrição.")}</span>
        <small>
          {showGame && a.gameTitle && <>{a.gameTitle} · </>}
          {done ? (a.unlockedHcAt ? t("Hardcore em {d}", { d: raDate(a.unlockedHcAt) }) : t("Obtida em {d}", { d: raDate(a.unlockedAt) })) : t("Pendente")}
          {kindLabel && <> · {kindLabel}</>}
          {" · "}<i className="ns-score" title={t("Pontuação de conquistas")}>+{raValue(a.points, !!a.unlockedHcAt)}</i>
        </small>
      </div>
      <div className="ra-ach-side">
        <b>{a.points}</b><small>{t("pts")}</small>
        {a.unlockedHcAt && <i className="ra-hc">HC</i>}
      </div>
    </button>
  );
}
