import { useEffect, useState } from "react";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import Button from "./Button";
import Modal from "./Modal";
import Select from "./Select";
import type { Profile } from "../types";
import { t } from "../lib/i18n";

/** Configurações da conta: entrada automática de um perfil ao abrir o NEXSTAGE. */
export default function AccountSettings({ onClose }: { onClose: () => void }) {
  const { toast } = useSession();
  const [profiles, setProfiles] = useState<Profile[] | null>(null);
  const [auto, setAuto] = useState(0);

  useEffect(() => {
    Promise.all([api.listProfiles(), api.getAppSetting("auto_profile")])
      .then(([list, a]) => { setProfiles(list); setAuto(list.some((x) => x.id === Number(a)) ? Number(a) : 0); })
      .catch((e) => toast(errMsg(e), "err"));
  }, [toast]);

  return (
    <Modal title={t("Configurações da conta")} onClose={onClose} wide>
      {!profiles ? <p className="muted">…</p> : (
        <div className="settings">
          <div className="setting">
            <div><strong>{t("Entrada automática do perfil")}</strong><small>{t("Ao abrir o NEXSTAGE, carrega este perfil direto na Home.")}</small></div>
            <Select<number> value={auto} width={240} title={t("Entrada automática do perfil")}
              onChange={(v) => { setAuto(v); api.setAppSetting("auto_profile", String(v)).catch((e) => toast(errMsg(e), "err")); }}
              options={[[t("Desligada"), 0], ...profiles.map((x): [string, number] => [x.displayName, x.id])]} />
          </div>
        </div>
      )}
      <div className="row end" style={{ marginTop: 18 }}><Button variant="ghost" onClick={onClose}>{t("Fechar")}</Button></div>
    </Modal>
  );
}
