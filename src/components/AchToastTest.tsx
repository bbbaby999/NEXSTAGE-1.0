import Button from "./Button";
import { api } from "../lib/api";
import { t } from "../lib/i18n";

/** Mostra um cartão de conquista de exemplo (comum ou raro), com o som, pelo caminho real. */
export default function AchToastTest() {
  const show = (rare: boolean) =>
    api.previewAchievementToast(rare, rare ? t("Lenda viva") : t("Primeiros passos"), t("Aviso de teste do NEXSTAGE")).catch(() => {});
  return (
    <div className="row">
      <Button variant="ghost" onClick={() => show(false)}>{t("Testar aviso comum")}</Button>
      <Button variant="ghost" onClick={() => show(true)}>{t("Testar aviso raro")}</Button>
    </div>
  );
}
