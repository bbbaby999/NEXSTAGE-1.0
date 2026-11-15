import { useEffect, useState } from "react";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";

/**
 * Configurações → Pesquisa de imagens: logos automáticas da biblioteca (SteamGridDB). Jogos sem logo
 * recebem a primeira logo do primeiro resultado da pesquisa; escolher outra (ou remover) em "Editar jogo"
 * continua valendo e não é desfeito pela busca automática.
 */
export default function AutoLogosSetting({ hasKey }: { hasKey: boolean }) {
  const { profile, toast } = useSession();
  const [on, setOn] = useState(true);
  const [busy, setBusy] = useState(false);
  useEffect(() => { api.getAppSetting("auto_logos").then((v) => setOn(v !== "0")).catch(() => {}); }, []);

  async function run(retry: boolean) {
    setBusy(true);
    try {
      const r = await api.autoLogos(profile!.id, retry);
      const msg = r.filled || r.notFound
        ? t("Logos: {a} adicionada(s), {b} jogo(s) sem logo no SteamGridDB.", { a: r.filled, b: r.notFound })
        : t("Todos os jogos já têm logo (ou já foram pesquisados).");
      toast(r.interrupted ? `${msg} ${t("A busca parou por falha de conexão; o restante será tentado depois.")}` : msg, r.interrupted ? "err" : undefined);
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }

  return (
    <div className={`setting ${hasKey ? "" : "disabled"}`}>
      <div>
        <strong>{t("Logos automáticas na biblioteca")}</strong>
        <small>{t("Jogos sem logo recebem a primeira logo do primeiro resultado do SteamGridDB, inclusive os adicionados depois. Você pode escolher outra (ou remover) em “Editar jogo”: sua escolha não é substituída. Precisa da chave do SteamGridDB.")}</small>
      </div>
      <div className="row">
        <Button variant={on ? "primary" : "default"} disabled={!hasKey}
          onClick={() => { const v = !on; setOn(v); api.setAppSetting("auto_logos", v ? "1" : "0").catch(() => {}); }}>{on ? t("Ligado") : t("Desligado")}</Button>
        <Button disabled={!hasKey || busy} onClick={() => run(false)}>{busy ? t("Buscando…") : t("Buscar agora")}</Button>
        <Button variant="ghost" disabled={!hasKey || busy} onClick={() => run(true)}>{t("Tentar de novo os sem logo")}</Button>
      </div>
    </div>
  );
}
