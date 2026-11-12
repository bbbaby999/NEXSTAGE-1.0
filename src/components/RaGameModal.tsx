import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Modal from "./Modal";
import Button from "./Button";
import RaAchievementItem from "./RaAchievementItem";
import RaProgress from "./RaProgress";
import { api, errMsg } from "../lib/api";
import { isUnlocked } from "../lib/ra";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { RaGameDetail } from "../types";

/** Conquistas de um jogo do RetroAchievements (cache local; "Atualizar" consulta a API). */
export default function RaGameModal({ raGameId, onClose, onOpenLibrary }: {
  raGameId: number; onClose: () => void; onOpenLibrary?: (gameId: number) => void;
}) {
  const { profile, toast } = useSession();
  const p = profile!;
  const [d, setD] = useState<RaGameDetail | null>(null);
  const [busy, setBusy] = useState(false);
  const [show, setShow] = useState<"all" | "done" | "pending">("all");

  const load = async (refresh: boolean) => {
    setBusy(true);
    try {
      const r = await api.raGame(p.id, raGameId, refresh);
      setD(r);
      if (r.warning && refresh) toast(r.warning, "err");
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  };
  useEffect(() => { load(false); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [raGameId]);

  const g = d?.game;
  const list = (d?.achievements ?? []).filter((a) => show === "all" || (show === "done") === isUnlocked(a));
  return (
    <Modal title={g?.title ?? t("Conquistas")} onClose={onClose} wide>
      {g && (
        <div className="ra-gm-head">
          {g.iconUrl && <img className="ra-gm-icon" src={g.iconUrl} alt="" />}
          <div className="ra-gm-info">
            <span className="muted">{g.consoleName}</span>
            <RaProgress game={g} />
          </div>
        </div>
      )}
      {d?.warning && <p className="ra-warn">{d.warning}</p>}
      <div className="row ra-gm-tools">
        <div className="seg">
          {([["all", t("Todas")], ["done", t("Obtidas")], ["pending", t("Pendentes")]] as const).map(([k, l]) => (
            <button key={k} type="button" data-focus className={show === k ? "on" : ""} onClick={() => setShow(k)}>{l}</button>
          ))}
        </div>
        <span className="grow" />
        <Button onClick={() => load(true)} disabled={busy}>{busy ? t("Atualizando…") : t("Atualizar")}</Button>
        {g && <Button onClick={() => openUrl(g.url).catch(() => {})}>{t("Abrir no RetroAchievements")}</Button>}
        {g?.libraryGameId && onOpenLibrary && <Button variant="primary" onClick={() => onOpenLibrary(g.libraryGameId!)}>{t("Abrir na biblioteca")}</Button>}
      </div>
      <div className="ra-ach-list">
        {!d && <p className="muted">{t("Carregando…")}</p>}
        {d && list.length === 0 && (
          <p className="muted">{d.achievements.length ? t("Nenhuma conquista neste filtro.") : t("As conquistas deste jogo ainda não foram carregadas. Use Atualizar (requer internet).")}</p>
        )}
        {list.map((a) => <RaAchievementItem key={a.id} a={a} showGame={false} />)}
      </div>
      <div className="row end"><Button variant="ghost" onClick={onClose}>{t("Fechar")}</Button></div>
    </Modal>
  );
}
