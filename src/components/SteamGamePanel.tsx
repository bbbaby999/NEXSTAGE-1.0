import { Fragment, useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { installLabel, isInstalled, steamTime, useSteamEvents } from "../lib/steam";
import { pct, raDate } from "../lib/ra";
import { formatBytes } from "../lib/format";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { Game, SteamAppStatus, SteamGameData } from "../types";

/**
 * Bloco "Steam" na página de um jogo com AppID (importado ou vinculado). Tempo de jogo da Steam é
 * exibido como dado externo; nada aqui altera o histórico local do NEXSTAGE.
 */
export default function SteamGamePanel({ game, status, onInstall, onChanged }: {
  game: Game; status: SteamAppStatus | null; onInstall: () => void; onChanged: () => void;
}) {
  const { profile, toast } = useSession();
  const p = profile!;
  const appid = game.steamAppId!;
  const [d, setD] = useState<SteamGameData | null>(null);
  const [busy, setBusy] = useState(false);
  const [all, setAll] = useState(false);

  const load = useCallback(async (refresh: boolean) => {
    setBusy(refresh);
    try {
      const [data, warn] = await api.steamGame(p.id, appid, refresh);
      setD(data);
      if (warn && refresh) toast(warn, "err");
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }, [p.id, appid, toast]);
  useEffect(() => { load(false); }, [load]);
  useSteamEvents(p.id, (k) => { if (k === "synced") load(false); });

  const o = d?.owned;
  const achs = d?.achievements ?? [];
  const shown = all ? achs : achs.slice(0, 6);
  const err = d?.error;

  return (
    <>
      <h2 className="section steam-sec">{t("Steam")}</h2>
      <div className="ra-panel">
        <div className="steam-status">
          <span className={`steam-inst ${status?.install ?? "unknown"}`}>{status ? installLabel(status.install) : "…"}</span>
          {status && status.sizeOnDisk > 0 && <small className="muted">{formatBytes(status.sizeOnDisk)}</small>}
        </div>
        {status && !isInstalled(status.install) && status.install !== "installing" && (
          <Button variant="primary" onClick={onInstall}>{status.protocol ? t("Instalar pela Steam") : t("Abrir na loja Steam")}</Button>
        )}
        {status?.install === "installing" && <p className="muted">{t("Download em andamento no cliente Steam.")}</p>}

        <dl className="steam-dl">
          <dt>{t("Tempo na Steam")}</dt><dd>{o ? steamTime(o.playtimeForever) : t("Não informado")}</dd>
          <dt>{t("Últimas 2 semanas")}</dt><dd>{o?.playtime2weeks ? steamTime(o.playtime2weeks) : o?.playtimeForever !== null && o ? steamTime(0) : t("Não informado")}</dd>
          <dt>{t("Última vez (Steam)")}</dt><dd>{o?.lastPlayed ? raDate(o.lastPlayed) : t("Não informado")}</dd>
        </dl>

        <h3 className="steam-h3">{t("Conquistas")}</h3>
        {err === "private" && <p className="ra-warn">{t("A Steam não liberou as conquistas: os detalhes de jogos do perfil estão privados.")}</p>}
        {err === "nostats" && <p className="muted">{t("Este jogo não tem conquistas na Steam.")}</p>}
        {err && err !== "private" && err !== "nostats" && <p className="ra-warn">{err}</p>}
        {achs.length > 0 && (
          <>
            <div className="ra-prog">
              <div className="ra-bar"><i className="soft" style={{ width: `${pct(d!.unlocked, d!.total)}%` }} /></div>
              <div className="ra-prog-meta"><span><b>{pct(d!.unlocked, d!.total)}%</b> · {t("{a} de {b} conquistas", { a: d!.unlocked, b: d!.total })}</span></div>
            </div>
            <div className="ra-ach-list mini">
              {shown.map((a) => {
                const done = a.unlockedAt !== null;
                const desc = a.description || (a.hidden && !done ? t("Conquista oculta") : t("Sem descrição."));
                return (
                  <div key={a.apiname} className={`ra-ach ${done ? "done" : "locked"}`}>
                    <div className="ra-badge">{a.icon ? <img src={a.icon} alt="" loading="lazy" /> : null}</div>
                    <div className="ra-ach-text">
                      <strong>{a.name}</strong>
                      <span>{desc}</span>
                      <small>
                        {done ? (a.unlockedAt ? t("Obtida em {d}", { d: raDate(a.unlockedAt) }) : t("Obtida")) : t("Pendente")}
                        {a.globalPct !== null && <> · {t("{p}% dos jogadores", { p: a.globalPct.toFixed(1) })}</>}
                      </small>
                    </div>
                  </div>
                );
              })}
            </div>
            {achs.length > 6 && <Button variant="ghost" onClick={() => setAll(!all)}>{all ? t("Mostrar menos") : t("Ver todas ({n})", { n: achs.length })}</Button>}
          </>
        )}
        {d && achs.length === 0 && !err && <p className="muted">{d.fetchedAt ? t("Nenhuma conquista encontrada.") : t("Conquistas ainda não carregadas.")}</p>}

        {(d?.stats.length ?? 0) > 0 && (
          <>
            <h3 className="steam-h3">{t("Estatísticas públicas")}</h3>
            <dl className="steam-dl">
              {d!.stats.slice(0, 12).map((s) => (<Fragment key={s.name}><dt>{s.label ?? s.name}</dt><dd>{Number(s.value).toLocaleString()}</dd></Fragment>))}
            </dl>
          </>
        )}

        <div className="row">
          <Button onClick={() => load(true)} disabled={busy}>{busy ? t("Atualizando…") : t("Atualizar dados da Steam")}</Button>
          <Button variant="ghost" onClick={() => api.steamOpenPage(appid).catch((e) => toast(errMsg(e), "err"))}>{t("Abrir no cliente Steam")}</Button>
          <Button variant="ghost" onClick={() => openUrl(`https://store.steampowered.com/app/${appid}/`).catch(() => {})}>{t("Loja")}</Button>
          {game.source !== "steam" && (
            <Button variant="ghost" onClick={() => api.steamSetLink(p.id, game.id, null).then(() => { toast(t("Vínculo com a Steam removido.")); onChanged(); }).catch((e) => toast(errMsg(e), "err"))}>{t("Desvincular da Steam")}</Button>
          )}
        </div>
      </div>
    </>
  );
}
