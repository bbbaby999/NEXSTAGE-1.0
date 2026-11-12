import { CSSProperties, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSession } from "../state/session";
import { api } from "../lib/api";
import { BBCode, summarize } from "../lib/bbcode";
import { currentLang, t } from "../lib/i18n";
import Button from "./Button";
import Modal from "./Modal";
import type { SteamNewsItem, SteamNewsOverview } from "../types";

/**
 * "Últimas atualizações" (Home): o jogo Steam da biblioteca com a nota de atualização OFICIAL mais
 * recente (publicada pelo desenvolvedor na Steam), com resumo, notas completas e link original.
 * Lê só o cache local (não atrasa a Home); a verificação roda em segundo plano no backend
 * (steam/news.rs), respeitando o limite de requisições, a cada 6 h ou pelo botão "Atualizar".
 */
const AUTO_EVERY = 30 * 60 * 1000; // reavalia o cache vencido enquanto a Home fica aberta
const OTHERS = 4;

const fmtDate = (s: number) =>
  new Date(s * 1000).toLocaleString(currentLang(), { day: "2-digit", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });
function ago(s: number): string {
  const diff = s - Date.now() / 1000;
  const rtf = new Intl.RelativeTimeFormat(currentLang(), { numeric: "auto" });
  const a = Math.abs(diff);
  if (a < 3600) return rtf.format(Math.round(diff / 60), "minute");
  if (a < 86400) return rtf.format(Math.round(diff / 3600), "hour");
  if (a < 86400 * 30) return rtf.format(Math.round(diff / 86400), "day");
  return rtf.format(Math.round(diff / (86400 * 30)), "month");
}
const coverOf = (i: SteamNewsItem) => i.cover ?? `https://cdn.cloudflare.steamstatic.com/steam/apps/${i.appid}/library_600x900.jpg`;

export default function SteamUpdates() {
  const { profile, setView, setOpenGameId } = useSession();
  const pid = profile!.id;
  const [ov, setOv] = useState<SteamNewsOverview | null>(null);
  const [open, setOpen] = useState<SteamNewsItem | null>(null);
  /** Atualização mais recente que ainda não tinha sido vista (indicação "Novo" durante esta visita). */
  const [freshGid, setFreshGid] = useState<string | null>(null);
  const seenChecked = useRef(false);

  const load = useCallback(() => api.steamNewsOverview(pid).then(setOv).catch(() => {}), [pid]);

  useEffect(() => {
    let alive = true;
    load();
    // a verificação só começa depois que a Home já apareceu, e só consulta o que está vencido
    const kick = setTimeout(() => { if (alive) api.steamNewsRefresh(pid, false).catch(() => {}); }, 2500);
    const iv = setInterval(() => api.steamNewsRefresh(pid, false).catch(() => {}), AUTO_EVERY);
    const uns: (() => void)[] = [];
    const on = (ev: string) => listen<{ profileId: number }>(ev, (e) => { if (e.payload.profileId === pid) load(); })
      .then((f) => (alive ? uns.push(f) : f()));
    on("steam-news-updated");
    on("steam-news-state");
    return () => { alive = false; clearTimeout(kick); clearInterval(iv); uns.forEach((f) => f()); };
  }, [pid, load]);

  // "Novo": a mais recente mudou desde a última vez que o perfil viu a seção.
  const latest = ov?.items[0] ?? null;
  useEffect(() => {
    if (!latest || !ov) return;
    if (!seenChecked.current || latest.gid !== ov.seenGid) {
      if (ov.seenGid && latest.gid !== ov.seenGid) setFreshGid(latest.gid);
      seenChecked.current = true;
      if (latest.gid !== ov.seenGid) api.steamNewsSeen(pid, latest.gid).catch(() => {});
    }
  }, [latest?.gid, ov, pid]); // eslint-disable-line react-hooks/exhaustive-deps

  const refresh = () => { api.steamNewsRefresh(pid, true).then(() => load()).catch(() => {}); };
  const openGame = (i: SteamNewsItem) => { setView("library"); setOpenGameId(i.gameId); };

  const status = !ov ? "" : ov.running
    ? t("Verificando {done} de {total} jogos…", { done: ov.done, total: ov.total })
    : ov.error ? ov.error
    : ov.checkedAt ? t("Verificado {when}", { when: ago(ov.checkedAt) }) : "";

  let body: JSX.Element;
  if (!ov) {
    body = <div className="upd-skel" aria-hidden="true" />;
  } else if (!ov.connected) {
    body = (
      <div className="upd-empty">
        <p>{t("Conecte sua conta Steam para ver aqui as notas de atualização oficiais dos seus jogos.")}</p>
        <Button onClick={() => setView("settings")}>{t("Conectar a Steam")}</Button>
      </div>
    );
  } else if (ov.candidates === 0) {
    body = (
      <div className="upd-empty">
        <p>{t("Nenhum jogo da Steam na sua biblioteca ainda. Importe seus jogos da Steam para acompanhar as atualizações.")}</p>
        <Button onClick={() => setView("settings")}>{t("Importar jogos da Steam")}</Button>
      </div>
    );
  } else if (!latest) {
    body = ov.running || ov.checked === 0
      ? <div className="upd-skel" role="status"><span>{t("Buscando as notas de atualização dos seus jogos…")}</span></div>
      : <p className="upd-none">{t("Nenhuma nota de atualização encontrada nos anúncios recentes dos seus jogos da Steam.")}</p>;
  } else {
    const sum = summarize(latest.contents);
    body = (
      <>
        <article className="upd-hero" style={{ "--upd-cover": `url("${coverOf(latest)}")` } as CSSProperties}>
          <button data-focus type="button" className="upd-cover" onClick={() => openGame(latest)}
            aria-label={t("Abrir {title} na Biblioteca", { title: latest.gameTitle })}
            style={{ backgroundImage: `url("${coverOf(latest)}")` }} />
          <div className="upd-body">
            <div className="upd-kicker">
              <span className={`upd-badge ${latest.official ? "official" : "likely"}`}
                title={latest.official ? t("Publicação marcada pelo desenvolvedor como notas de atualização.") : t("Anúncio oficial do desenvolvedor com título de atualização (sem a marcação de notas de atualização).")}>
                {latest.official ? t("Notas de atualização oficiais") : t("Anúncio de atualização")}
              </span>
              {freshGid === latest.gid && <span className="upd-badge new">{t("Novo")}</span>}
            </div>
            <h3 className="upd-game">{latest.gameTitle}</h3>
            <p className="upd-title">{latest.title}</p>
            <small className="upd-when">
              {t("Publicado em {date}", { date: fmtDate(latest.date) })} · {ago(latest.date)}
              {latest.localUpdated ? <> · {t("Atualizado neste PC em {date}", { date: fmtDate(latest.localUpdated) })}</> : null}
            </small>
            {sum.items.length ? (
              <ul className="upd-sum">{sum.items.map((s, i) => <li key={i}>{s}</li>)}</ul>
            ) : sum.text ? <p className="upd-sum-text">{sum.text}</p>
              : <p className="upd-sum-text muted">{t("A publicação não traz texto; veja os detalhes na Steam.")}</p>}
            <div className="upd-actions">
              <Button variant="primary" onClick={() => setOpen(latest)}>{t("Ver notas completas")}</Button>
              <Button onClick={() => openUrl(latest.url).catch(() => {})}>{t("Abrir na Steam")}</Button>
            </div>
          </div>
        </article>
        {ov.items.length > 1 && (
          <div className="upd-others">
            <h3 className="section sub">{t("Também atualizados recentemente")}</h3>
            <div className="upd-list">
              {ov.items.slice(1, 1 + OTHERS).map((i) => (
                <button key={i.gid} data-focus type="button" className="upd-row" onClick={() => setOpen(i)}
                  aria-label={t("{game}: {title}, {when}", { game: i.gameTitle, title: i.title, when: fmtDate(i.date) })}>
                  <span className="upd-thumb" style={{ backgroundImage: `url("${coverOf(i)}")` }} />
                  <span className="upd-row-text">
                    <strong>{i.gameTitle}</strong>
                    <small>{i.title}</small>
                    <em>{ago(i.date)}{i.official ? "" : ` · ${t("anúncio")}`}</em>
                  </span>
                </button>
              ))}
            </div>
          </div>
        )}
      </>
    );
  }

  return (
    <section className="upd" aria-label={t("Últimas atualizações")}>
      <div className="section-head">
        <h2 className="section">{t("Últimas atualizações")}</h2>
        {ov?.connected && ov.candidates > 0 && (
          <div className="upd-tools">
            {status && <small className="upd-status">{ov.running && <i className="upd-spin" aria-hidden="true" />}{status}</small>}
            <Button variant="ghost" disabled={ov.running} onClick={refresh}>{t("Atualizar")}</Button>
          </div>
        )}
      </div>
      {body}
      {open && (
        <Modal title={`${open.gameTitle} — ${open.title}`} onClose={() => setOpen(null)} xl>
          <p className="muted upd-modal-meta">
            {open.official ? t("Notas de atualização oficiais") : t("Anúncio de atualização")} · {t("Publicado em {date}", { date: fmtDate(open.date) })}
            {open.author ? ` · ${open.author}` : ""}
          </p>
          <div className="upd-notes"><BBCode src={open.contents} /></div>
          <p className="muted note">{t("Texto publicado pelo desenvolvedor na Steam, no idioma original. Imagens e vídeos aparecem na publicação original.")}</p>
          <div className="row end">
            <Button variant="ghost" onClick={() => setOpen(null)}>{t("Fechar")}</Button>
            <Button variant="primary" onClick={() => openUrl(open.url).catch(() => {})}>{t("Abrir na Steam")}</Button>
          </div>
        </Modal>
      )}
    </section>
  );
}
