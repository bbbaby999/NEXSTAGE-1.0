import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { raAgo } from "../lib/ra";
import { useSession } from "../state/session";
import { t, tk } from "../lib/i18n";
import type { SourceId, SourceView } from "../types";

const META: Record<SourceId, { name: string; role: string; help: string; link: string; linkLabel: string }> = {
  igdb: {
    name: "IGDB", role: tk("Informações, imagens, trailers e tempo para zerar"),
    help: tk("Crie um aplicativo gratuito no console de desenvolvedores da Twitch (dev.twitch.tv/console/apps) e copie o Client ID e o Client Secret. O segredo é guardado cifrado pelo Windows e nunca volta para a interface."),
    link: "https://dev.twitch.tv/console/apps", linkLabel: tk("Abrir console da Twitch"),
  },
  nexus: {
    name: "Nexus Mods", role: tk("Descoberta de mods"),
    help: tk("Copie a sua chave pessoal em nexusmods.com → Configurações → Chaves de API. Ela é guardada cifrada pelo Windows. O NEXSTAGE só consulta mods: nada é baixado nem instalado."),
    link: "https://www.nexusmods.com/settings/api-keys", linkLabel: tk("Abrir chaves de API do Nexus"),
  },
};

/**
 * Configurações → Fontes de dados. Cada fonte é independente: ativação, credenciais (só enviadas ao
 * backend, cifradas pela DPAPI), teste de conexão, cache próprio e última sincronização.
 */
export default function DataSources() {
  const [list, setList] = useState<SourceView[] | null>(null);
  const load = useCallback(() => api.sourcesList().then(setList).catch(() => setList([])), []);
  useEffect(() => { load(); }, [load]);
  if (!list) return <div className="setting"><span className="muted">…</span></div>;
  return <>{list.filter((s) => s.id in META).map((s) => <SourceCard key={s.id} s={s} onChange={load} />)}</>;
}

function SourceCard({ s, onChange }: { s: SourceView; onChange: () => void }) {
  const { toast } = useSession();
  const m = META[s.id];
  const [f, setF] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<null | string>(null);
  const [editing, setEditing] = useState(false);
  const run = async (what: string, fn: () => Promise<unknown>, ok: string) => {
    setBusy(what);
    try { await fn(); toast(ok); setF({}); setEditing(false); }
    catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(null); onChange(); }
  };
  const fields: [string, string, boolean][] = s.id === "igdb"
    ? [["clientId", t("Client ID"), false], ["clientSecret", t("Client Secret"), true]]
    : s.id === "nexus" ? [["apiKey", t("Chave da API (pessoal)"), true]] : [];
  const status = !s.authorized ? t("Sem acesso autorizado")
    : !s.configured ? t("Não configurado")
    : s.status === "error" ? t("Erro na última conexão")
    : s.enabled ? t("Conectado") : t("Desativado");
  const dot = !s.authorized || !s.configured ? "off" : s.status === "error" ? "err" : s.enabled ? "ok" : "off";
  const info = s.info as { clientId?: string; user?: string; premium?: boolean };

  return (
    <div className="setting col-set ds-card">
      <div className="ds-head">
        <div>
          <strong>{m.name} <span className={`ds-dot ${dot}`} /> <span className="ds-status">{status}</span></strong>
          <small>{t(m.role)}</small>
          <small>
            {s.lastSync ? t("Última sincronização {ago}", { ago: raAgo(s.lastSync) }) : t("Nunca sincronizado")}
            {s.cacheEntries > 0 && <> · {t("{n} itens em cache", { n: s.cacheEntries })}</>}
            {info.clientId && <> · {t("Client ID {id}", { id: info.clientId })}</>}
            {info.user && <> · {t("conta {u}", { u: info.user })}{info.premium ? " (Premium)" : ""}</>}
          </small>
          {s.lastError && <small className="ra-err">{s.lastError}</small>}
        </div>
        {s.authorized && s.configured && (
          <Button variant={s.enabled ? "primary" : "default"} disabled={busy !== null}
            onClick={() => run("toggle", () => api.sourceSetEnabled(s.id, !s.enabled), s.enabled ? t("{n} desativado.", { n: m.name }) : t("{n} ativado.", { n: m.name }))}>
            {s.enabled ? t("Ativado") : t("Desativado")}</Button>
        )}
      </div>
      <small className="muted">{t(m.help)}</small>
      {s.authorized && (!s.configured || editing) && (
        <div className="row">
          {fields.map(([k, label, secret]) => (
            <input key={k} className="search" style={{ width: 260 }} type={secret ? "password" : "text"} value={f[k] ?? ""} placeholder={label} aria-label={`${m.name}: ${label}`}
              autoComplete="off" spellCheck={false} onChange={(e) => setF({ ...f, [k]: e.target.value })} />
          ))}
          <Button variant="primary" disabled={busy !== null || fields.some(([k]) => !(f[k] ?? "").trim())}
            onClick={() => run("connect", () => api.sourceConnect(s.id, f), t("{n} conectado e ativado.", { n: m.name }))}>
            {busy === "connect" ? t("Testando…") : t("Testar e salvar")}</Button>
          {editing && <Button variant="ghost" onClick={() => { setEditing(false); setF({}); }}>{t("Cancelar")}</Button>}
        </div>
      )}
      <div className="row">
        {s.authorized && s.configured && !editing && (
          <>
            <Button disabled={busy !== null} onClick={() => run("test", () => api.sourceTest(s.id), t("Conexão com {n} OK.", { n: m.name }))}>{busy === "test" ? t("Testando…") : t("Testar conexão")}</Button>
            <Button variant="ghost" onClick={() => setEditing(true)}>{t("Trocar credenciais")}</Button>
            <Button variant="ghost" disabled={busy !== null || !s.cacheEntries} onClick={() => run("cache", () => api.sourceClearCache(s.id), t("Cache limpo."))}>{t("Limpar cache")}</Button>
            <Button variant="danger" disabled={busy !== null} onClick={() => run("off", () => api.sourceDisconnect(s.id), t("Credenciais de {n} apagadas.", { n: m.name }))}>{t("Desconectar")}</Button>
          </>
        )}
        <Button variant="ghost" onClick={() => openUrl(m.link).catch(() => {})}>{t(m.linkLabel)}</Button>
      </div>
    </div>
  );
}
