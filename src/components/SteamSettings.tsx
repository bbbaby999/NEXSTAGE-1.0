import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "./Button";
import Select from "./Select";
import SteamImport from "./SteamImport";
import { api, errMsg } from "../lib/api";
import { raAgo } from "../lib/ra";
import { useSteamEvents } from "../lib/steam";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { SteamAccount, SteamClientStatus } from "../types";

/**
 * Configurações → Contas e plataformas → Steam. Login apenas pelo "Entrar com a Steam" oficial
 * (o NEXSTAGE nunca vê a senha). A chave pessoal da Web API é opcional e fica no cofre do Windows.
 */
export default function SteamSettings() {
  const { profile, toast } = useSession();
  const p = profile!;
  const [acc, setAcc] = useState<SteamAccount | null | undefined>(undefined);
  const [client, setClient] = useState<SteamClientStatus | null>(null);
  const [waiting, setWaiting] = useState(false);
  const [busy, setBusy] = useState<null | "sync" | "key" | "refresh">(null);
  const [key, setKey] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [clearCache, setClearCache] = useState(false);
  const [minutes, setMinutes] = useState(60);
  const [notify, setNotify] = useState(true);
  const [importing, setImporting] = useState(false);

  const load = useCallback(() => api.steamGetAccount(p.id).then(setAcc).catch(() => setAcc(null)), [p.id]);
  useEffect(() => {
    load();
    api.steamClientStatus().then(setClient).catch(() => {});
    api.getAppSetting("steam_sync_minutes").then((v) => setMinutes(v === null ? 60 : Number(v))).catch(() => {});
    api.getAppSetting("steam_notify").then((v) => setNotify(v !== "0")).catch(() => {});
  }, [load]);
  useSteamEvents(p.id, () => load());

  async function login() {
    setWaiting(true);
    try {
      const a = await api.steamLogin(p.id);
      setAcc(a);
      toast(t("Conta Steam {name} conectada.", { name: a.name }));
      api.steamSync(p.id).catch(() => {}).finally(load);
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setWaiting(false); }
  }
  async function sync() {
    setBusy("sync");
    try {
      const r = await api.steamSync(p.id);
      if (r.libraryPrivate) toast(t("Perfil sincronizado, mas a Steam não liberou a lista de jogos (detalhes de jogos privados)."), "err");
      else if (r.owned !== null) toast(t("Steam sincronizada: {n} jogo(s) na conta.", { n: r.owned }));
      else toast(t("Perfil atualizado. Sem a chave da Web API, só os jogos instalados neste PC aparecem."));
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(null); load(); }
  }
  async function saveKey(k: string | null) {
    setBusy("key");
    try {
      setAcc(await api.steamSetKey(p.id, k));
      setKey("");
      toast(k ? t("Chave da Web API verificada e guardada com segurança.") : t("Chave da Web API removida."));
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(null); }
  }
  async function disconnect() {
    try {
      await api.steamDisconnect(p.id, clearCache);
      setConfirm(false); setClearCache(false); setAcc(null);
      toast(t("Conta Steam desconectada. Os jogos importados continuam na biblioteca."));
    } catch (e) { toast(errMsg(e), "err"); }
  }

  if (acc === undefined) return <div className="setting"><span className="muted">…</span></div>;
  const vis = acc?.profile?.visibility ?? "unknown";
  const visLabel = vis === "public" ? t("Perfil público") : vis === "private" ? t("Perfil privado") : vis === "friends" ? t("Visível só para amigos") : t("Privacidade desconhecida");

  return (
    <>
      {!acc ? (
        <div className="setting col-set">
          <div>
            <strong>{t("Conectar conta Steam")}</strong>
            <small>{t("O login acontece no site oficial da Steam, no seu navegador. O NEXSTAGE nunca pede nem vê sua senha e não guarda cookies ou tokens: recebe apenas o seu SteamID64 público, confirmado pela própria Steam.")}</small>
          </div>
          <div className="row">
            {!waiting
              ? <Button variant="primary" onClick={login}>{t("Entrar com a Steam")}</Button>
              : <>
                  <span className="muted">{t("Conclua o login na janela do navegador…")}</span>
                  <Button variant="ghost" onClick={() => api.steamCancelLogin()}>{t("Cancelar")}</Button>
                </>}
          </div>
        </div>
      ) : (
        <div className="setting ra-acc">
          <div className="ra-acc-id">
            {acc.profile?.avatar ? <img src={acc.profile.avatar} alt="" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : <span className="ra-user-ph">S</span>}
            <div>
              <strong>{acc.name} <span className={`steam-dot ${acc.syncError ? "err" : "ok"}`} title={acc.syncError ?? t("Conectado")} /></strong>
              <small>{t("SteamID64: {id}", { id: acc.steamId })} · {visLabel}</small>
              <small>
                {t("Sincronizado {ago}", { ago: raAgo(acc.syncedAt) })}
                {acc.ownedCount > 0 && <> · {acc.ownedSource === "local" ? t("{n} jogo(s) instalados neste PC", { n: acc.ownedCount }) : t("{n} jogo(s) na conta", { n: acc.ownedCount })}</>}
              </small>
              {acc.syncError && <small className="ra-err">{acc.syncError}</small>}
            </div>
          </div>
          <div className="row">
            <Button variant="primary" onClick={() => setImporting(true)}>{t("Importar jogos")}</Button>
            <Button onClick={sync} disabled={busy !== null || acc.syncing}>{busy === "sync" || acc.syncing ? t("Sincronizando…") : t("Sincronizar")}</Button>
            <Button variant="ghost" onClick={() => openUrl(acc.profileUrl).catch(() => {})}>{t("Ver perfil")}</Button>
            {!confirm && <Button variant="danger" onClick={() => setConfirm(true)}>{t("Desconectar")}</Button>}
          </div>
        </div>
      )}

      {acc && (vis !== "public" || acc.ownedSource === "local") && (
        <div className="setting col-set">
          <div><strong>{t("Dados disponíveis")}</strong>
            <small>
              {vis === "private" || vis === "friends"
                ? t("Seu perfil não é público: a Steam não libera biblioteca, tempo de jogo e conquistas para aplicativos. Para usar esses dados, deixe \"Detalhes de jogos\" como Público em Steam → Perfil → Editar perfil → Privacidade.")
                : t("Sem a chave da Web API, o NEXSTAGE mostra apenas os jogos instalados neste PC (sem tempo de jogo nem conquistas).")}
            </small>
          </div>
        </div>
      )}

      {acc && confirm && (
        <div className="setting col-set ra-confirm">
          <div><strong>{t("Desconectar a conta Steam {name}?", { name: acc.name })}</strong><small>{t("O SteamID e a chave da Web API são apagados deste computador. Os jogos importados continuam na biblioteca.")}</small></div>
          <label className="ra-check"><input type="checkbox" checked={clearCache} onChange={(e) => setClearCache(e.target.checked)} /> {t("Apagar também os dados da Steam guardados neste perfil (tempo de jogo, conquistas)")}</label>
          <div className="row">
            <Button variant="ghost" onClick={() => setConfirm(false)}>{t("Cancelar")}</Button>
            <Button variant="danger" onClick={disconnect}>{t("Confirmar desconexão")}</Button>
          </div>
        </div>
      )}

      {acc && (
        <div className="setting col-set">
          <div>
            <strong>{t("Chave da Web API (opcional)")} {acc.hasKey && <span className="steam-ok">{t("✓ guardada")}</span>}</strong>
            <small>{t("Biblioteca, tempo de jogo e conquistas vêm da Steam Web API, que exige uma chave. O NEXSTAGE não embute chave compartilhada nem usa servidor próprio: você pode gerar uma chave pessoal gratuita (domínio: localhost). Ela é guardada cifrada pelo Windows e nunca é exibida.")}</small>
            {acc.keyUnreadable && <small className="ra-err">{t("A chave salva não pode ser lida neste Windows: informe-a novamente.")}</small>}
          </div>
          <div className="row">
            <input className="search" style={{ width: 300 }} type="password" value={key} onChange={(e) => setKey(e.target.value)}
              placeholder={acc.hasKey ? t("Nova chave (substitui a atual)") : t("Chave da Web API")} aria-label={t("Chave da Web API da Steam")} autoComplete="off" spellCheck={false} />
            <Button disabled={!key.trim() || busy !== null} onClick={() => saveKey(key.trim())}>{busy === "key" ? t("Verificando…") : t("Verificar e salvar")}</Button>
            {acc.hasKey && <Button variant="ghost" disabled={busy !== null} onClick={() => saveKey(null)}>{t("Remover chave")}</Button>}
            <Button variant="ghost" onClick={() => openUrl("https://steamcommunity.com/dev/apikey").catch(() => {})}>{t("Gerar chave")}</Button>
          </div>
        </div>
      )}

      {acc && (
        <div className="setting">
          <div><strong>{t("Sincronização automática da Steam")}</strong><small>{t("Atualiza perfil, biblioteca, tempo de jogo e conquistas dos jogos jogados recentemente. Nunca na abertura do app.")}</small></div>
          <Select<number> value={minutes} width={200} title={t("Sincronização automática da Steam")}
            onChange={(v) => { setMinutes(v); api.setAppSetting("steam_sync_minutes", String(v)).catch(() => {}); }}
            options={[[t("Desligada"), 0], [t("A cada 30 min"), 30], [t("A cada 1 hora"), 60], [t("A cada 6 horas"), 360], [t("Uma vez por dia"), 1440]]} />
        </div>
      )}

      {acc && (
        <div className="setting">
          <div><strong>{t("Avisar novos desbloqueios")}</strong><small>{t("Assim que você desbloqueia uma conquista num jogo da Steam, mostra o cartão no estilo dos consoles por cima do jogo, com som. Precisa da chave da Web API.")}</small></div>
          <div className="row">
            <Button variant={notify ? "primary" : "default"} onClick={() => { const v = !notify; setNotify(v); api.setAppSetting("steam_notify", v ? "1" : "0").catch(() => {}); }}>{notify ? t("Ligado") : t("Desligado")}</Button>
          </div>
        </div>
      )}

      <div className="setting">
        <div>
          <strong>{t("Cliente Steam neste PC")}</strong>
          <small>
            {!client ? "…" : client.found
              ? t("Encontrado em {path} · {libs} biblioteca(s) · {n} jogo(s) instalados · protocolo steam:// {proto}", {
                  path: client.path ?? "", libs: client.libraries, n: client.installed, proto: client.protocol ? t("disponível") : t("indisponível"),
                })
              : t("Não encontrado. Instalar e jogar usam o cliente oficial; sem ele, o NEXSTAGE abre a página da loja.")}
          </small>
          <small>{t("Jogos da Steam são instalados e iniciados pelo próprio cliente Steam: downloads, atualizações, overlay e anti-cheat continuam com a Steam. O NEXSTAGE não injeta código nem acessa os processos dos jogos.")}</small>
        </div>
      </div>

      {importing && <SteamImport onClose={() => { setImporting(false); load(); }} />}
    </>
  );
}
