import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Block from "./DpBlock";
import Button from "./Button";
import Modal from "./Modal";
import { api, errMsg } from "../lib/api";
import { BBCode, summarize } from "../lib/bbcode";
import { useSession } from "../state/session";
import { currentLang, t } from "../lib/i18n";
import type { Game, SteamGameNews as News } from "../types";

/** Publicações com menos de 3 dias ganham a indicação "Novo". */
const FRESH = 3 * 86400;

const fmtDate = (s: number) =>
  new Date(s * 1000).toLocaleString(currentLang(), { day: "2-digit", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });
function ago(s: number): string {
  const diff = s - Date.now() / 1000;
  const rtf = new Intl.RelativeTimeFormat(currentLang(), { numeric: "auto" });
  const a = Math.abs(diff);
  if (a < 3600) return rtf.format(Math.round(diff / 60), "minute");
  if (a < 86400) return rtf.format(Math.round(diff / 3600), "hour");
  if (a < 86400 * 30) return rtf.format(Math.round(diff / 86400), "day");
  if (a < 86400 * 365) return rtf.format(Math.round(diff / (86400 * 30)), "month");
  return rtf.format(Math.round(diff / (86400 * 365)), "year");
}

/**
 * "Última atualização" na página de um jogo vinculado à Steam: a nota de atualização OFICIAL mais recente
 * publicada pelo desenvolvedor (a mesma fonte da seção da Home, ISteamNews), com resumo, notas completas
 * e link original. Usa o cache compartilhado com a Home (6 h); "Atualizar" consulta de novo.
 */
export default function SteamGameNews({ game }: { game: Game }) {
  const { toast } = useSession();
  const appid = game.steamAppId!;
  const [n, setN] = useState<News | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);

  const load = useCallback(async (refresh: boolean) => {
    setBusy(true);
    try {
      const r = await api.steamGameNews(appid, refresh);
      setN(r);
      if (r.error && refresh) toast(t(r.error), "err");
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }, [appid, toast]);
  useEffect(() => { setN(null); load(false); }, [load]);

  const it = n?.item ?? null;
  const sum = it ? summarize(it.contents) : null;
  const fresh = it ? Date.now() / 1000 - it.date < FRESH : false;

  return (
    <Block title={t("Última atualização")} actions={<Button variant="ghost" disabled={busy} onClick={() => load(true)}>{busy ? t("Atualizando…") : t("Atualizar")}</Button>}>
      {!n ? (
        <div className="upd-skel gnews-skel" role="status"><span>{t("Buscando a nota de atualização na Steam…")}</span></div>
      ) : !it ? (
        <p className="muted">{n.checkedAt
          ? t("Nenhuma nota de atualização encontrada nos anúncios recentes deste jogo na Steam.")
          : n.error ? t(n.error) : t("Nota de atualização ainda não consultada.")}</p>
      ) : (
        <div className="gnews">
          <div className="upd-kicker">
            <span className={`upd-badge ${it.official ? "official" : "likely"}`}
              title={it.official ? t("Publicação marcada pelo desenvolvedor como notas de atualização.") : t("Anúncio oficial do desenvolvedor com título de atualização (sem a marcação de notas de atualização).")}>
              {it.official ? t("Notas de atualização oficiais") : t("Anúncio de atualização")}
            </span>
            {fresh && <span className="upd-badge new">{t("Novo")}</span>}
          </div>
          <p className="upd-title">{it.title}</p>
          <small className="upd-when">
            {t("Publicado em {date}", { date: fmtDate(it.date) })} · {ago(it.date)}
            {n.localUpdated ? <> · {t("Atualizado neste PC em {date}", { date: fmtDate(n.localUpdated) })}</> : null}
          </small>
          {sum!.items.length ? (
            <ul className="upd-sum">{sum!.items.map((s, i) => <li key={i}>{s}</li>)}</ul>
          ) : sum!.text ? <p className="upd-sum-text">{sum!.text}</p>
            : <p className="upd-sum-text muted">{t("A publicação não traz texto; veja os detalhes na Steam.")}</p>}
          <div className="upd-actions">
            <Button variant="primary" onClick={() => setOpen(true)}>{t("Ver notas completas")}</Button>
            <Button onClick={() => openUrl(it.url).catch(() => {})}>{t("Abrir na Steam")}</Button>
          </div>
        </div>
      )}
      {open && it && (
        <Modal title={`${game.title} — ${it.title}`} onClose={() => setOpen(false)} xl>
          <p className="muted upd-modal-meta">
            {it.official ? t("Notas de atualização oficiais") : t("Anúncio de atualização")} · {t("Publicado em {date}", { date: fmtDate(it.date) })}
            {it.author ? ` · ${it.author}` : ""}
          </p>
          <div className="upd-notes"><BBCode src={it.contents} /></div>
          <p className="muted note">{t("Texto publicado pelo desenvolvedor na Steam, no idioma original. Imagens e vídeos aparecem na publicação original.")}</p>
          <div className="row end">
            <Button variant="ghost" onClick={() => setOpen(false)}>{t("Fechar")}</Button>
            <Button variant="primary" onClick={() => openUrl(it.url).catch(() => {})}>{t("Abrir na Steam")}</Button>
          </div>
        </Modal>
      )}
    </Block>
  );
}
