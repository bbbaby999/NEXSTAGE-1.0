import { useCallback, useEffect, useState } from "react";
import Button from "./Button";
import Seg from "./Seg";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import { currentLang, t } from "../lib/i18n";
import type { Game, SteamDlcItem, SteamDlcPrice, SteamDlcView } from "../types";

/** Preço da Steam (centavos) no formato do idioma do app. */
export function dlcPrice(p: SteamDlcPrice): string {
  try {
    return new Intl.NumberFormat(currentLang(), { style: "currency", currency: p.currency }).format(p.final / 100);
  } catch {
    return `${(p.final / 100).toFixed(2)} ${p.currency}`;
  }
}
export const dlcDate = (s: number) => new Date(s * 1000).toLocaleDateString(currentLang(), { day: "2-digit", month: "short", year: "numeric" });

type Filter = "all" | "installed" | "new";

/**
 * Aba "DLCs" da página de um jogo da Steam: lista pública da loja (nome, imagem, preço na região da
 * conexão, lançamento), com "Instalada" quando o manifesto local indica conteúdo baixado e "Nova" para
 * DLCs que apareceram recentemente. A Steam não informa quais DLCs o usuário comprou.
 */
export default function SteamDlcPanel({ game }: { game: Game }) {
  const { toast } = useSession();
  const appid = game.steamAppId!;
  const [v, setV] = useState<SteamDlcView | null>(null);
  const [busy, setBusy] = useState(false);
  const [filter, setFilter] = useState<Filter>("all");

  const load = useCallback(async (refresh: boolean) => {
    setBusy(true);
    try {
      const r = await api.steamDlc(appid, refresh);
      setV(r);
      if (r.error && refresh) toast(t(r.error), "err");
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }, [appid, toast]);
  useEffect(() => { load(false); }, [load]);

  const open = (d: SteamDlcItem) => api.steamOpenStore(d.id).catch((e) => toast(errMsg(e), "err"));

  const items = v?.items ?? [];
  const installed = items.filter((i) => i.installed).length;
  const fresh = items.filter((i) => i.new).length;
  const shown = filter === "installed" ? items.filter((i) => i.installed) : filter === "new" ? items.filter((i) => i.new) : items;

  return (
    <section className="dlc" aria-label={t("DLCs")}>
      <div className="section-head">
        <h2 className="section">{t("DLCs")}</h2>
        <div className="dlc-tools">
          {v?.fetchedAt ? <small className="muted">{t("Consultado em {date}", { date: dlcDate(v.fetchedAt) })}</small> : null}
          <Button variant="ghost" disabled={busy} onClick={() => load(true)}>{busy ? t("Atualizando…") : t("Atualizar")}</Button>
        </div>
      </div>

      {!v && <div className="upd-skel" role="status"><span>{t("Buscando as DLCs na Steam…")}</span></div>}
      {v && v.error && !v.fetchedAt && <p className="ra-warn">{t(v.error)}</p>}
      {v && v.fetchedAt !== null && items.length === 0 && <p className="muted">{t("Este jogo não tem DLCs na loja da Steam.")}</p>}

      {items.length > 0 && (
        <>
          <div className="dlc-summary">
            <span>{t("{n} DLC(s) na loja", { n: items.length })}{installed > 0 && <> · <b>{t("{n} instalada(s) neste PC", { n: installed })}</b></>}</span>
            {(installed > 0 || fresh > 0) && (
              <Seg<Filter> value={filter} onChange={setFilter}
                opts={[[t("Todas"), "all"], ...(installed > 0 ? [[t("Instaladas"), "installed"] as [string, Filter]] : []), ...(fresh > 0 ? [[t("Novas"), "new"] as [string, Filter]] : [])]} />
            )}
          </div>
          <div className="dlc-grid" data-nav-group>
            {shown.map((d) => (
              <button key={d.id} type="button" data-focus data-focus-key={`dlc-${d.id}`} className="dlc-card" onClick={() => open(d)}
                aria-label={t("{name}: abrir na loja da Steam", { name: d.name })}>
                <span className="dlc-img">
                  {d.image ? <img src={d.image} alt="" loading="lazy" referrerPolicy="no-referrer" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : null}
                  <span className="dlc-badges">
                    {d.new && <i className="upd-badge new">{t("Nova")}</i>}
                    {d.upcoming && <i className="upd-badge official">{t("Em breve")}</i>}
                    {d.installed && <i className="upd-badge likely">{t("Instalada")}</i>}
                  </span>
                </span>
                <strong>{d.name}</strong>
                <small>
                  {d.release ? (d.upcoming ? t("Lançamento: {date}", { date: dlcDate(d.release) }) : dlcDate(d.release)) : t("Data não informada")}
                  {d.price && (
                    <> · {d.price.discount > 0 && <s className="muted">{dlcPrice({ ...d.price, final: d.price.initial })}</s>}{" "}
                      <b className={d.price.discount > 0 ? "dlc-off" : ""}>{dlcPrice(d.price)}{d.price.discount > 0 ? ` (−${d.price.discount}%)` : ""}</b></>
                  )}
                </small>
              </button>
            ))}
          </div>
        </>
      )}
      <p className="muted note">{t("Lista pública da loja da Steam, com preços da região da sua conexão. A Steam não informa quais DLCs você comprou; “Instalada” aparece para DLCs com conteúdo baixado neste PC (DLCs que só desbloqueiam itens não deixam arquivos).")}</p>
    </section>
  );
}
