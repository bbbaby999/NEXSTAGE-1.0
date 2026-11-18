import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "./Button";
import Select from "./Select";
import XboxImport from "./XboxImport";
import { api, errMsg } from "../lib/api";
import { useXboxSynced } from "../lib/xbox";
import { raAgo } from "../lib/ra";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { XboxStatus } from "../types";

const AZURE_APPS = "https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade";

/**
 * Configurações → Contas e plataformas → Xbox. Login só na página oficial da Microsoft (o NEXSTAGE nunca
 * vê a senha); a sessão fica cifrada pelo Windows. Conquistas e tempo de jogo aparecem na página de cada
 * jogo encontrado no histórico da conta.
 */
export default function XboxSettings() {
  const { profile, toast } = useSession();
  const p = profile!;
  const [st, setSt] = useState<XboxStatus | null>(null);
  const [waiting, setWaiting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [cid, setCid] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [clearCache, setClearCache] = useState(false);
  const [minutes, setMinutes] = useState(60);
  const [notify, setNotify] = useState(true);
  const [importing, setImporting] = useState(false);
  const [gamePass, setGamePass] = useState(true);

  const load = useCallback(() => api.xboxStatus(p.id).then(setSt).catch(() => {}), [p.id]);
  useEffect(() => {
    load();
    api.getAppSetting("xbox_sync_minutes").then((v) => setMinutes(v === null ? 60 : Number(v))).catch(() => {});
    api.getAppSetting("xbox_notify").then((v) => setNotify(v !== "0")).catch(() => {});
    api.getAppSetting("gamepass_home").then((v) => setGamePass(v !== "0")).catch(() => {});
  }, [load]);
  // sincronização automática concluída em segundo plano
  useXboxSynced(p.id, load);

  async function sync(quiet = false) {
    setSyncing(true);
    try {
      const r = await api.xboxSync(p.id);
      if (!quiet) toast(t("Xbox sincronizado: {n} jogo(s) no histórico da conta.", { n: r.titles }));
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setSyncing(false); load(); }
  }
  async function login() {
    setWaiting(true);
    try {
      const a = await api.xboxLogin(p.id);
      toast(t("Conta Xbox {name} conectada.", { name: a.gamertag }));
      load();
      sync(true);
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setWaiting(false); }
  }
  async function saveCid(v: string | null) {
    try {
      await api.xboxSetClientId(v);
      setCid("");
      toast(v ? t("Client ID salvo.") : t("Client ID removido."));
      load();
    } catch (e) { toast(errMsg(e), "err"); }
  }
  async function disconnect() {
    try {
      await api.xboxDisconnect(p.id, clearCache);
      setConfirm(false); setClearCache(false);
      toast(t("Conta Xbox desconectada. Os jogos continuam na biblioteca."));
      load();
    } catch (e) { toast(errMsg(e), "err"); }
  }

  if (!st) return <div className="setting"><span className="muted">…</span></div>;
  const acc = st.account;
  const ready = st.embedded || !!st.clientId;
  const loginRow = (
    <div className="row">
      {!waiting
        ? <Button variant="primary" onClick={login} disabled={!ready}>{t("Entrar com a conta Microsoft")}</Button>
        : <>
            <span className="muted">{t("Conclua o login na janela do navegador…")}</span>
            <Button variant="ghost" onClick={() => api.xboxCancelLogin()}>{t("Cancelar")}</Button>
          </>}
    </div>
  );

  return (
    <>
      {!st.embedded && (
        <div className="setting col-set">
          <div>
            <strong>{t("Aplicativo do Azure (Client ID)")} {st.clientId && <span className="steam-ok">{t("✓ guardado")}</span>}</strong>
            <small>{t("O login da Microsoft exige um aplicativo registrado no Azure (gratuito). Cole aqui o \"ID do aplicativo (cliente)\". Não é uma senha: nenhum segredo é necessário.")}</small>
            {st.clientId && <small className="path">{st.clientId}</small>}
          </div>
          <div className="row">
            <input className="search" style={{ width: 320 }} value={cid} onChange={(e) => setCid(e.target.value)}
              placeholder="00000000-0000-0000-0000-000000000000" aria-label={t("Client ID do aplicativo do Azure")} autoComplete="off" spellCheck={false} />
            <Button disabled={!cid.trim()} onClick={() => saveCid(cid.trim())}>{t("Salvar")}</Button>
            {st.clientId && <Button variant="danger" onClick={() => saveCid(null)}>{t("Remover")}</Button>}
            <Button variant="ghost" onClick={() => openUrl(AZURE_APPS).catch(() => {})}>{t("Abrir o portal do Azure")}</Button>
          </div>
        </div>
      )}

      {!acc ? (
        <div className="setting col-set">
          <div>
            <strong>{t("Conectar conta Xbox")}</strong>
            <small>{t("O login acontece no site oficial da Microsoft, no seu navegador: o NEXSTAGE nunca vê sua senha. Só a sessão é guardada, cifrada pelo Windows, e o acesso é somente leitura (conquistas, gamerscore e tempo de jogo).")}</small>
            {!ready && <small className="ra-err">{t("Informe o Client ID acima antes de entrar.")}</small>}
          </div>
          {loginRow}
        </div>
      ) : (
        <div className="setting ra-acc">
          <div className="ra-acc-id">
            {acc.gamerpic ? <img src={acc.gamerpic} alt="" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : <span className="ra-user-ph">X</span>}
            <div>
              <strong>{acc.gamertag} <span className={`steam-dot ${acc.syncError || acc.needsLogin ? "err" : "ok"}`} title={acc.syncError ?? t("Conectado")} /></strong>
              <small>{acc.gamerscore !== null && <>{t("Gamerscore: {n}", { n: acc.gamerscore.toLocaleString() })} · </>}{t("{n} jogo(s) no histórico", { n: acc.titles })}</small>
              <small>{t("Sincronizado {ago}", { ago: raAgo(acc.syncedAt) })}</small>
              {acc.needsLogin && <small className="ra-err">{t("A sessão da conta Xbox não pode ser lida neste Windows: entre novamente.")}</small>}
              {acc.syncError && !acc.needsLogin && <small className="ra-err">{acc.syncError}</small>}
            </div>
          </div>
          <div className="row">
            {acc.needsLogin
              ? loginRow
              : <Button onClick={() => sync()} disabled={syncing}>{syncing ? t("Sincronizando…") : t("Sincronizar")}</Button>}
            <Button variant="ghost" onClick={() => openUrl(`https://account.xbox.com/profile?gamertag=${encodeURIComponent(acc.gamertag)}`).catch(() => {})}>{t("Ver perfil")}</Button>
            {!confirm && <Button variant="danger" onClick={() => setConfirm(true)}>{t("Desconectar")}</Button>}
          </div>
        </div>
      )}

      {acc && confirm && (
        <div className="setting col-set ra-confirm">
          <div><strong>{t("Desconectar a conta Xbox {name}?", { name: acc.gamertag })}</strong><small>{t("A sessão é apagada deste computador. Para revogar o acesso de vez, remova o NEXSTAGE em account.live.com/consent/Manage. Os jogos continuam na biblioteca.")}</small></div>
          <label className="ra-check"><input type="checkbox" checked={clearCache} onChange={(e) => setClearCache(e.target.checked)} /> {t("Apagar também os dados do Xbox guardados neste perfil (tempo de jogo, conquistas)")}</label>
          <div className="row">
            <Button variant="ghost" onClick={() => setConfirm(false)}>{t("Cancelar")}</Button>
            <Button variant="danger" onClick={disconnect}>{t("Confirmar desconexão")}</Button>
          </div>
        </div>
      )}

      {acc && (
        <div className="setting">
          <div><strong>{t("Sincronização automática do Xbox")}</strong><small>{t("Atualiza gamerscore, histórico de jogos e tempo de jogo do Xbox. Nunca na abertura do app.")}</small></div>
          <Select<number> value={minutes} width={200} title={t("Sincronização automática do Xbox")}
            onChange={(v) => { setMinutes(v); api.setAppSetting("xbox_sync_minutes", String(v)).catch(() => {}); }}
            options={[[t("Desligada"), 0], [t("A cada 30 min"), 30], [t("A cada 1 hora"), 60], [t("A cada 6 horas"), 360], [t("Uma vez por dia"), 1440]]} />
        </div>
      )}

      {acc && (
        <div className="setting">
          <div><strong>{t("Avisar novos desbloqueios")}</strong><small>{t("Com um jogo do Xbox aberto, as conquistas dele são conferidas a cada 45 s; cada nova aparece no cartão por cima do jogo, com som. O jogo precisa estar no histórico da conta (sincronize depois de jogá-lo pela primeira vez).")}</small></div>
          <div className="row">
            <Button variant={notify ? "primary" : "default"} onClick={() => { const v = !notify; setNotify(v); api.setAppSetting("xbox_notify", v ? "1" : "0").catch(() => {}); }}>{notify ? t("Ligado") : t("Desligado")}</Button>
          </div>
        </div>
      )}

      <div className="setting">
        <div><strong>{t("Jogos do Xbox instalados neste PC")}</strong><small>{t("Adiciona de uma vez os jogos do app Xbox / Microsoft Store instalados, sem procurar atalho por atalho. Não precisa de conta.")}</small></div>
        <div className="row"><Button onClick={() => setImporting(true)}>{t("Importar jogos")}</Button></div>
      </div>

      <div className="setting">
        <div><strong>{t("Catálogo do PC Game Pass na Home (experimental)")}</strong><small>{t("Mostra na Home os jogos que estão chegando, os adicionados recentemente e os que vão sair do PC Game Pass. Usa o catálogo público do site do Xbox, sem conta e sem dados seus; como não é um serviço oficial para outros apps, pode parar de funcionar sem aviso — nesse caso a seção some da Home.")}</small></div>
        <div className="row">
          <Button variant={gamePass ? "primary" : "default"} onClick={() => { const v = !gamePass; setGamePass(v); api.setAppSetting("gamepass_home", v ? "1" : "0").catch(() => {}); }}>{gamePass ? t("Ligado") : t("Desligado")}</Button>
        </div>
      </div>

      {acc && (
        <div className="setting col-set">
          <div>
            <strong>{t("Como os jogos são reconhecidos")}</strong>
            <small>{t("Jogos adicionados pelo atalho do app Xbox são ligados automaticamente; os demais, pelo nome igual ao do Xbox. Conquistas e tempo de jogo do Xbox aparecem na página de cada jogo e não alteram o tempo registrado pelo NEXSTAGE.")}</small>
          </div>
        </div>
      )}
      {importing && <XboxImport onClose={() => setImporting(false)} />}
    </>
  );
}
