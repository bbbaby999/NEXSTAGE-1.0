import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import { t } from "../lib/i18n";
import { dlcDate, dlcPrice } from "./SteamDlcPanel";
import type { SteamNewDlc } from "../types";

const AUTO_EVERY = 30 * 60 * 1000; // reavalia o cache vencido enquanto a Home fica aberta

/**
 * "Novas DLCs" (Home): DLCs que apareceram recentemente na loja da Steam para os jogos da biblioteca.
 * Lê só o cache local; a verificação roda em segundo plano no backend (steam/dlc.rs), no máximo uma vez
 * por dia para cada jogo. Sem DLC nova, a seção não aparece.
 */
export default function SteamNewDlcs() {
  const { profile, toast } = useSession();
  const pid = profile!.id;
  const [list, setList] = useState<SteamNewDlc[]>([]);

  const load = useCallback(() => api.steamDlcNew(pid).then(setList).catch(() => {}), [pid]);

  useEffect(() => {
    let alive = true;
    load();
    // começa depois das notas de atualização (mesma fila de baixa prioridade, sem atrasar a Home)
    const kick = setTimeout(() => { if (alive) api.steamDlcRefresh(pid).catch(() => {}); }, 8000);
    const iv = setInterval(() => api.steamDlcRefresh(pid).catch(() => {}), AUTO_EVERY);
    let un: (() => void) | undefined;
    listen<{ profileId: number }>("steam-dlc-updated", (e) => { if (e.payload.profileId === pid) load(); })
      .then((f) => (alive ? (un = f) : f()));
    return () => { alive = false; clearTimeout(kick); clearInterval(iv); un?.(); };
  }, [pid, load]);

  if (!list.length) return null;

  return (
    <section className="upd ndlc" aria-label={t("Novas DLCs")}>
      <div className="section-head">
        <h2 className="section">{t("Novas DLCs")}</h2>
      </div>
      <div className="upd-list">
        {list.map((d) => (
          <button key={`${d.appid}-${d.id}`} data-focus data-focus-key={`ndlc-${d.id}`} type="button" className="upd-row ndlc-row"
            onClick={() => api.steamOpenStore(d.id).catch((e) => toast(errMsg(e), "err"))}
            aria-label={t("{name}, DLC de {game}: abrir na loja da Steam", { name: d.name, game: d.gameTitle })}>
            <span className="ndlc-thumb" style={d.image ? { backgroundImage: `url("${d.image}")` } : d.cover ? { backgroundImage: `url("${d.cover}")` } : undefined} />
            <span className="upd-row-text">
              <strong>{d.name}</strong>
              <small>{d.gameTitle}</small>
              <em>
                {d.release ? dlcDate(d.release) : t("DLC nova")}
                {d.price ? ` · ${dlcPrice(d.price)}${d.price.discount > 0 ? ` (−${d.price.discount}%)` : ""}` : ""}
              </em>
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}
