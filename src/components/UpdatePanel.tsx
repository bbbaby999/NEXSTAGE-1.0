import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { check, Update } from "@tauri-apps/plugin-updater";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { idleBusy } from "../lib/idle";
import { t } from "../lib/i18n";

type State = "idle" | "checking" | "none" | "available" | "installing" | "unconfigured" | "error";

/** Atualização pelo app: verifica a última versão publicada, baixa, valida a assinatura e instala. */
export default function UpdatePanel() {
  const [version, setVersion] = useState("");
  const [state, setState] = useState<State>("idle");
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [msg, setMsg] = useState("");
  const [auto, setAuto] = useState(true);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
    api.getAppSetting("auto_update").then((v) => setAuto(v !== "0")).catch(() => {});
  }, []);

  async function verify() {
    setState("checking");
    setMsg("");
    try {
      const u = await check();
      if (u) { setUpdate(u); setState("available"); } else setState("none");
    } catch (e) {
      const m = errMsg(e);
      if (/updater|plugin|config/i.test(m) && !/network|connect|dns|timeout|request/i.test(m)) setState("unconfigured");
      else { setMsg(m); setState("error"); }
    }
  }

  async function install() {
    if (!update) return;
    setState("installing");
    let total = 0, got = 0;
    try {
      await idleBusy(update.downloadAndInstall((ev) => {
        if (ev.event === "Started") { total = ev.data.contentLength ?? 0; setProgress(0); }
        else if (ev.event === "Progress") { got += ev.data.chunkLength; if (total) setProgress(Math.round((got / total) * 100)); }
        else if (ev.event === "Finished") setProgress(100);
      }));
      // No Windows o app fecha sozinho durante a instalação e reabre já atualizado.
    } catch (e) {
      setMsg(errMsg(e));
      setState("error");
    }
  }

  return (
    <div className="settings">
      <div className="setting">
        <div>
          <strong>{t("Versão instalada:")}{" "}{version || "…"}</strong>
          <small>
            {state === "idle" && t("Clique em Verificar para procurar uma versão nova.")}
            {state === "checking" && t("Verificando…")}
            {state === "none" && t("Você já está na versão mais recente.")}
            {state === "available" && t("Nova versão disponível: {version}.{v1}", { version: update?.version ?? "", v1: update?.body ? ` ${update.body}` : "" })}
            {state === "installing" && t("Baixando e instalando{v0}… o NEXSTAGE vai fechar e reabrir sozinho.", { v0: progress !== null ? ` (${progress}%)` : "" })}
            {state === "unconfigured" && t("A atualização automática ainda não foi configurada neste app (veja scripts/setup-updater.mjs).")}
            {state === "error" && t("Não foi possível atualizar: {msg}", { msg })}
          </small>
        </div>
        {state === "available" ? (
          <Button variant="primary" onClick={install}>{t("Baixar e instalar")}</Button>
        ) : (
          <Button onClick={verify} disabled={state === "checking" || state === "installing"}>{t("Verificar atualizações")}</Button>
        )}
      </div>
      <div className="setting">
        <div><strong>{t("Verificar ao abrir")}</strong><small>{t("Mostra um aviso quando houver versão nova (nunca instala sozinho).")}</small></div>
        <Button variant={auto ? "primary" : "default"} onClick={() => { const n = !auto; setAuto(n); api.setAppSetting("auto_update", n ? "1" : "0").catch(() => {}); }}>
          {auto ? t("Ligado") : t("Desligado")}
        </Button>
      </div>
    </div>
  );
}
