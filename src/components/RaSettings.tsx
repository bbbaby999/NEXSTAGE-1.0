import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "./Button";
import Select from "./Select";
import { TrophyIcon } from "./RaUnlockToasts";
import { api, errMsg } from "../lib/api";
import { raAgo, useRaEvents } from "../lib/ra";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { RaAccount, RaEmuSupport } from "../types";

/**
 * Configurações → RetroAchievements. A conta é por perfil local. A chave da API é enviada ao backend
 * uma única vez (ao conectar), guardada cifrada pelo Windows (DPAPI) e nunca volta para a interface.
 */
export default function RaSettings() {
  const { profile, toast, setView } = useSession();
  const p = profile!;
  const [acc, setAcc] = useState<RaAccount | null | undefined>(undefined);
  const [user, setUser] = useState("");
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState<null | "connect" | "test" | "sync">(null);
  const [confirm, setConfirm] = useState(false);
  const [clearCache, setClearCache] = useState(false);
  const [minutes, setMinutes] = useState(15);
  const [notify, setNotify] = useState(true);
  const [emus, setEmus] = useState<RaEmuSupport[]>([]);

  const load = useCallback(() => api.raGetAccount(p.id).then(setAcc).catch(() => setAcc(null)), [p.id]);
  useEffect(() => {
    load();
    api.getAppSetting("ra_sync_minutes").then((v) => setMinutes(v === null ? 15 : Number(v))).catch(() => {});
    api.getAppSetting("ra_notify").then((v) => setNotify(v !== "0")).catch(() => {});
    api.raEmulatorSupport().then(setEmus).catch(() => {});
  }, [load]);
  useRaEvents(p.id, () => load());

  const save = (k: string, v: string) => api.setAppSetting(k, v).catch((e) => toast(errMsg(e), "err"));

  async function connect() {
    setBusy("connect");
    try {
      const a = await api.raConnect(p.id, user.trim(), key.trim());
      setAcc(a);
      setKey(""); // a chave não fica na memória da interface
      toast(t("Conta {u} conectada.", { u: a.username }));
      // primeira carga em segundo plano: monta a base (sem avisos) e identifica a biblioteca
      api.raSync(p.id).then(() => api.raScanLibrary(p.id)).catch(() => {}).finally(load);
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(null); }
  }
  async function test() {
    setBusy("test");
    try { const a = await api.raTest(p.id); setAcc(a); toast(t("Conexão OK: {u} · {pts} pontos.", { u: a.username, pts: a.profile?.totalPoints ?? 0 })); }
    catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(null); }
  }
  async function syncNow() {
    setBusy("sync");
    try {
      const r = await api.raSync(p.id);
      toast(r.newUnlocks ? t("{n} novo(s) desbloqueio(s) sincronizado(s).", { n: r.newUnlocks }) : t("Sincronizado: nenhum desbloqueio novo."));
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(null); load(); }
  }
  async function disconnect() {
    try {
      await api.raDisconnect(p.id, clearCache);
      setConfirm(false); setClearCache(false); setAcc(null);
      toast(clearCache ? t("Conta desconectada e dados apagados.") : t("Conta desconectada. A chave foi apagada."));
    } catch (e) { toast(errMsg(e), "err"); }
  }

  if (acc === undefined) return <div className="setting"><span className="muted">…</span></div>;

  return (
    <>
      {!acc ? (
        <div className="setting col-set">
          <div>
            <strong>{t("Conectar conta")}</strong>
            <small>
              {t("Informe seu usuário e a \"Web API Key\" (retroachievements.org → Configurações → Chaves). A chave é testada, guardada cifrada pelo Windows apenas neste perfil e nunca é exibida novamente.")}
            </small>
          </div>
          <div className="row">
            <input className="search" style={{ width: 200 }} value={user} onChange={(e) => setUser(e.target.value)} placeholder={t("Usuário")} aria-label={t("Usuário do RetroAchievements")} autoComplete="off" spellCheck={false} />
            <input className="search" style={{ width: 280 }} type="password" value={key} onChange={(e) => setKey(e.target.value)} placeholder={t("Chave da API (Web API Key)")} aria-label={t("Chave da API do RetroAchievements")} autoComplete="off" spellCheck={false} />
            <Button variant="primary" disabled={!user.trim() || !key.trim() || busy !== null} onClick={connect}>{busy === "connect" ? t("Testando…") : t("Testar e conectar")}</Button>
            <Button variant="ghost" onClick={() => openUrl("https://retroachievements.org/settings").catch(() => {})}>{t("Onde encontro a chave?")}</Button>
          </div>
        </div>
      ) : (
        <div className="setting ra-acc">
          <div className="ra-acc-id">
            {acc.profile?.userPic ? <img src={acc.profile.userPic} alt="" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : <span className="ra-user-ph"><TrophyIcon /></span>}
            <div>
              <strong>{acc.username}</strong>
              <small>
                {t("{pts} pontos · {soft} softcore", { pts: (acc.profile?.totalPoints ?? 0).toLocaleString(undefined), soft: (acc.profile?.totalSoftcorePoints ?? 0).toLocaleString(undefined) })}
                {" · "}{t("Sincronizado {ago}", { ago: raAgo(acc.syncedAt) })}
              </small>
              {!acc.hasKey && <small className="ra-err">{t("A chave salva não pode ser lida neste Windows: desconecte e conecte novamente.")}</small>}
              {acc.syncError && <small className="ra-err">{acc.syncError}</small>}
            </div>
          </div>
          <div className="row">
            <Button onClick={test} disabled={busy !== null || !acc.hasKey}>{busy === "test" ? t("Testando…") : t("Testar conexão")}</Button>
            <Button onClick={syncNow} disabled={busy !== null || !acc.hasKey}>{busy === "sync" ? t("Sincronizando…") : t("Sincronizar agora")}</Button>
            <Button onClick={() => setView("achievements")}>{t("Abrir conquistas")}</Button>
            <Button variant="ghost" onClick={() => openUrl(acc.profileUrl).catch(() => {})}>{t("Ver perfil")}</Button>
            {!confirm ? <Button variant="danger" onClick={() => setConfirm(true)}>{t("Desconectar")}</Button> : null}
          </div>
        </div>
      )}
      {acc && confirm && (
        <div className="setting col-set ra-confirm">
          <div><strong>{t("Desconectar a conta {u}?", { u: acc.username })}</strong><small>{t("A chave da API é apagada deste computador. Associações dos jogos são mantidas.")}</small></div>
          <label className="ra-check"><input type="checkbox" checked={clearCache} onChange={(e) => setClearCache(e.target.checked)} /> {t("Apagar também o progresso e as conquistas guardadas deste perfil")}</label>
          <div className="row">
            <Button variant="ghost" onClick={() => setConfirm(false)}>{t("Cancelar")}</Button>
            <Button variant="danger" onClick={disconnect}>{t("Confirmar desconexão")}</Button>
          </div>
        </div>
      )}
      <div className="setting">
        <div><strong>{t("Sincronização automática")}</strong><small>{t("Consulta a conta em segundo plano (nunca na abertura do app). Durante jogos com emulador compatível, a cada 2 minutos.")}</small></div>
        <Select<number> value={minutes} width={200} title={t("Sincronização automática")}
          onChange={(v) => { setMinutes(v); save("ra_sync_minutes", String(v)); }}
          options={[[t("Desligada"), 0], [t("A cada 10 min"), 10], [t("A cada 15 min"), 15], [t("A cada 30 min"), 30], [t("A cada 1 hora"), 60]]} />
      </div>
      <div className="setting">
        <div><strong>{t("Avisar novos desbloqueios")}</strong><small>{t("Cartão no estilo dos consoles, embaixo no centro da tela, por cima do jogo (sem tirar o foco), com som. Conquistas raras têm estilo dourado e som próprio.")}</small></div>
        <div className="row">
          <Button variant={notify ? "primary" : "default"} onClick={() => { const v = !notify; setNotify(v); save("ra_notify", v ? "1" : "0"); }}>{notify ? t("Ligado") : t("Desligado")}</Button>
        </div>
      </div>
      <div className="setting col-set">
        <div><strong>{t("Emuladores")}</strong><small>{t("Somente leitura das configurações de cada emulador. Conquistas só são registradas se o RetroAchievements estiver ativo no próprio emulador, com a mesma conta.")}</small></div>
        {emus.length === 0 ? <p className="muted">{t("Nenhum emulador cadastrado.")}</p> : (
          <ul className="ra-emus">
            {emus.map((e) => (
              <li key={e.id} className={e.mode}>
                <b>{e.name}</b>
                <span className="ra-emu-mode">{e.mode === "realtime" ? t("Quase em tempo real") : e.mode === "polling" ? t("Por sincronização") : t("Sem suporte")}</span>
                <small>
                  {e.raCapable && e.raEnabled !== null && (e.raEnabled ? t("RetroAchievements ativo") : t("RetroAchievements desligado"))}
                  {e.raCapable && e.raUser && <> · {t("conta {u}", { u: e.raUser })}{acc && e.raUser.toLowerCase() !== acc.username.toLowerCase() && <b className="ra-err"> {t("(diferente da conectada)")}</b>}</>}
                  {e.raCapable && (e.raEnabled !== null || e.raUser) && " · "}{t(e.note)}
                </small>
              </li>
            ))}
          </ul>
        )}
      </div>
    </>
  );
}
