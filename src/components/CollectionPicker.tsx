import { useCallback, useEffect, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import type { GameCollection } from "../types";
import { t } from "../lib/i18n";

/** Adiciona/remove um jogo de coleções e permite criar uma nova na hora. */
export default function CollectionPicker({ profileId, gameId, onClose }: {
  profileId: number; gameId: number; onClose: () => void;
}) {
  const { toast } = useSession();
  const [cols, setCols] = useState<GameCollection[]>([]);
  const [name, setName] = useState("");

  const load = useCallback(
    () => api.listCollections(profileId).then(setCols).catch((e) => toast(errMsg(e), "err")),
    [profileId, toast]
  );
  useEffect(() => { load(); }, [load]);

  async function toggle(c: GameCollection) {
    try {
      await api.setInCollection(profileId, c.id, gameId, !c.gameIds.includes(gameId));
      await load();
    } catch (e) { toast(errMsg(e), "err"); }
  }
  async function create() {
    try {
      const c = await api.createCollection(profileId, name);
      await api.setInCollection(profileId, c.id, gameId, true);
      setName("");
      await load();
    } catch (e) { toast(errMsg(e), "err"); }
  }

  return (
    <Modal title={t("Coleções")} onClose={onClose}>
      <div className="coll-list">
        {cols.length === 0 && <p className="muted">{t("Você ainda não tem coleções. Crie a primeira abaixo.")}</p>}
        {cols.map((c) => {
          const on = c.gameIds.includes(gameId);
          return (
            <button key={c.id} data-focus className={`coll-row ${on ? "on" : ""}`} onClick={() => toggle(c)}>
              <span>{c.name}</span><i>{on ? t("✓ Na coleção") : t("Adicionar")}</i>
            </button>
          );
        })}
      </div>
      <div className="row" style={{ marginTop: 16 }}>
        <input placeholder={t("Nova coleção (ex.: Terror, Retrô)")} value={name} maxLength={40}
          onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && create()}
          style={{ flex: 1 }} />
        <Button onClick={create}>{t("Criar e adicionar")}</Button>
      </div>
      <div className="row end"><Button variant="primary" onClick={onClose}>{t("Concluir")}</Button></div>
    </Modal>
  );
}
