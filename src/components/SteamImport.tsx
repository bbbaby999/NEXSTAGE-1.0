import { useEffect, useMemo, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import Select from "./Select";
import { api, errMsg } from "../lib/api";
import { installLabel, isInstalled, steamTime } from "../lib/steam";
import { raAgo } from "../lib/ra";
import { useSession } from "../state/session";
import { t, tk } from "../lib/i18n";
import type { SteamOwned, SteamOwnedList, SteamPreviewItem } from "../types";

type Filter = "all" | "installed" | "not_installed" | "recent";
type Time = "any" | "never" | "1" | "10" | "50";
type Sort = "name" | "playtime" | "recent";

const UPDATE_LABEL: Record<string, string> = {
  link: tk("Vincular ao AppID da Steam"),
  launch: tk("Instalar e jogar pelo cliente Steam"),
  playtime: tk("Tempo de jogo da Steam (dado externo; o tempo do NEXSTAGE não muda)"),
  cover: tk("Capa da Steam (o jogo não tem capa)"),
};

/**
 * Importação da Steam: o usuário escolhe exatamente quais jogos entram, revisa a prévia e confirma.
 * Nada é importado automaticamente.
 */
export default function SteamImport({ onClose, onImported }: { onClose: () => void; onImported?: () => void }) {
  const { profile, toast, setView } = useSession();
  const p = profile!;
  const [connected, setConnected] = useState<boolean | null>(null);
  const [data, setData] = useState<SteamOwnedList | null>(null);
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [time, setTime] = useState<Time>("any");
  const [sort, setSort] = useState<Sort>("name");
  const [sel, setSel] = useState<Set<number>>(new Set());
  const [preview, setPreview] = useState<SteamPreviewItem[] | null>(null);
  const [linkTitles, setLinkTitles] = useState(true);
  const [busy, setBusy] = useState<null | "sync" | "import" | "preview">(null);

  const load = () => api.steamOwned(p.id).then(setData).catch((e) => toast(errMsg(e), "err"));
  useEffect(() => {
    api.steamGetAccount(p.id).then((a) => { setConnected(!!a); if (a) load(); }).catch(() => setConnected(false));
    /* eslint-disable-next-line react-hooks/exhaustive-deps */
  }, [p.id]);

  const games = useMemo(() => {
    const term = q.trim().toLowerCase();
    const min = (g: SteamOwned) => g.playtimeForever ?? -1;
    const list = (data?.games ?? []).filter((g) => {
      if (term && !g.name.toLowerCase().includes(term)) return false;
      if (filter === "installed" && !isInstalled(g.install)) return false;
      if (filter === "not_installed" && g.install !== "not_installed") return false;
      if (filter === "recent" && !g.recent) return false;
      if (time === "never" && g.playtimeForever !== 0) return false;
      if (time !== "any" && time !== "never" && min(g) < Number(time) * 60) return false;
      return true;
    });
    const by: Record<Sort, (a: SteamOwned, b: SteamOwned) => number> = {
      name: (a, b) => a.name.localeCompare(b.name),
      playtime: (a, b) => min(b) - min(a),
      recent: (a, b) => (b.lastPlayed ?? "").localeCompare(a.lastPlayed ?? ""),
    };
    return [...list].sort(by[sort]);
  }, [data, q, filter, time, sort]);

  const toggle = (id: number) => setSel((s) => { const n = new Set(s); n.has(id) ? n.delete(id) : n.add(id); return n; });
  const local = data?.games.some((g) => g.source === "local") ?? false;
  const noTime = local || (data?.games.every((g) => g.playtimeForever === null) ?? true);

  async function refresh() {
    setBusy("sync");
    try {
      const r = await api.steamSync(p.id);
      if (r.libraryPrivate) toast(t("A Steam não liberou a lista de jogos: os detalhes de jogos do perfil estão privados."), "err");
      if (r.warning && !r.libraryPrivate) toast(r.warning, "err");
      await load();
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(null); }
  }
  async function review() {
    setBusy("preview");
    try { setPreview(await api.steamPreview(p.id, [...sel])); }
    catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(null); }
  }
  async function confirm() {
    setBusy("import");
    try {
      const r = await api.steamImport(p.id, [...sel], linkTitles);
      toast(t("Importação concluída: {a} adicionado(s), {l} vinculado(s), {u} atualizado(s), {s} ignorado(s).", { a: r.added, l: r.linked, u: r.updated, s: r.skipped }));
      onImported?.();
      onClose();
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(null); }
  }

  if (connected === false) {
    return (
      <Modal title={t("Importar da Steam")} onClose={onClose}>
        <p>{t("Conecte sua conta Steam para escolher quais jogos importar.")}</p>
        <div className="row end">
          <Button variant="ghost" onClick={onClose}>{t("Fechar")}</Button>
          <Button variant="primary" onClick={() => { onClose(); setView("settings"); }}>{t("Ir para Configurações")}</Button>
        </div>
      </Modal>
    );
  }

  if (preview) {
    const news = preview.filter((i) => i.status === "new");
    const existing = preview.filter((i) => i.status === "existing");
    const titles = preview.filter((i) => i.status === "title");
    const count = news.length + existing.length + (linkTitles ? titles.length : 0);
    return (
      <Modal title={t("Confirmar importação")} onClose={() => setPreview(null)} wide>
        <div className="steam-sum">
          <div><b>{preview.length}</b><span>{t("selecionados")}</span></div>
          <div><b>{news.length}</b><span>{t("novos na biblioteca")}</span></div>
          <div><b>{existing.length}</b><span>{t("já existem (mesmo AppID)")}</span></div>
          <div><b>{titles.length}</b><span>{t("possíveis duplicatas (mesmo nome)")}</span></div>
        </div>
        <div className="steam-prev">
          {news.length > 0 && <h3>{t("Serão adicionados")}</h3>}
          {news.map((i) => <div key={i.appid} className="steam-prev-row"><b>{i.name}</b><small>{t("Novo jogo · PC · abre pelo cliente Steam")}</small></div>)}
          {existing.length > 0 && <h3>{t("Já estão na biblioteca — serão atualizados")}</h3>}
          {existing.map((i) => (
            <div key={i.appid} className="steam-prev-row">
              <b>{i.gameTitle}</b>
              <small>{i.updates.map((u) => t(UPDATE_LABEL[u])).join(" · ")}</small>
            </div>
          ))}
          {titles.length > 0 && (
            <>
              <h3>{t("Mesmo nome de um jogo da biblioteca")}</h3>
              <label className="ra-check"><input type="checkbox" checked={linkTitles} onChange={(e) => setLinkTitles(e.target.checked)} /> {t("Vincular a esses jogos existentes (desmarcado: não importa esses itens)")}</label>
              {titles.map((i) => (
                <div key={i.appid} className={`steam-prev-row ${linkTitles ? "" : "off"}`}>
                  <b>{i.name} → {i.gameTitle}</b>
                  <small>{i.updates.map((u) => t(UPDATE_LABEL[u])).join(" · ")}</small>
                </div>
              ))}
            </>
          )}
        </div>
        <p className="muted note">{t("Nada é baixado: jogos não instalados podem ser instalados depois pelo botão Instalar, que abre o cliente Steam.")}</p>
        <div className="row end">
          <Button variant="ghost" onClick={() => setPreview(null)}>{t("Voltar")}</Button>
          <Button variant="primary" disabled={busy !== null || count === 0} onClick={confirm}>{busy === "import" ? t("Importando…") : t("Importar {n} jogo(s)", { n: count })}</Button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal title={t("Importar da Steam")} onClose={onClose} xl>
      {local && <p className="ra-warn">{t("Mostrando só os jogos instalados neste PC (cliente Steam local). Para ver a biblioteca completa, tempo de jogo e conquistas, salve sua chave da Web API em Configurações.")}</p>}
      <div className="steam-tools">
        <input className="search" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("Pesquisar jogo…")} aria-label={t("Pesquisar jogo")} />
        <div className="seg">
          {([["all", tk("Todos")], ["installed", tk("Instalados")], ["not_installed", tk("Não instalados")], ["recent", tk("Jogados recentemente")]] as [Filter, string][]).map(([k, l]) => (
            <button key={k} type="button" data-focus className={filter === k ? "on" : ""} onClick={() => setFilter(k)}>{t(l)}</button>
          ))}
        </div>
        {!noTime && (
          <Select<Time> value={time} width={190} title={t("Tempo de jogo")} onChange={setTime}
            options={[[t("Qualquer tempo"), "any"], [t("Nunca jogados"), "never"], [t("Mais de 1 h"), "1"], [t("Mais de 10 h"), "10"], [t("Mais de 50 h"), "50"]]} />
        )}
        <Select<Sort> value={sort} width={210} title={t("Ordenar")} onChange={setSort}
          options={[[t("Nome"), "name"], ...(!noTime ? [[t("Mais jogados"), "playtime"] as [string, Sort], [t("Jogados por último"), "recent"] as [string, Sort]] : [])]} />
        <span className="grow" />
        <Button onClick={refresh} disabled={busy !== null}>{busy === "sync" ? t("Atualizando…") : t("Atualizar da Steam")}</Button>
      </div>
      <div className="row steam-selrow">
        <Button variant="ghost" onClick={() => setSel((s) => new Set([...s, ...games.map((g) => g.appid)]))} disabled={!games.length}>{t("Selecionar todos os exibidos")}</Button>
        <Button variant="ghost" onClick={() => setSel(new Set())} disabled={!sel.size}>{t("Limpar seleção")}</Button>
        <span className="muted">{t("{n} de {total} exibidos", { n: games.length, total: data?.games.length ?? 0 })}</span>
      </div>

      <div className="steam-grid">
        {!data && <p className="muted">{t("Carregando…")}</p>}
        {data && data.games.length === 0 && <p className="muted">{t("Nenhum jogo encontrado. Use “Atualizar da Steam”.")}</p>}
        {games.map((g) => (
          <button key={g.appid} data-focus className={`steam-card ${sel.has(g.appid) ? "on" : ""}`} onClick={() => toggle(g.appid)} aria-pressed={sel.has(g.appid)}>
            <div className="steam-cover">
              <img src={g.coverUrl} alt="" loading="lazy" onError={(e) => { const i = e.currentTarget; if (i.src !== g.headerUrl) { i.src = g.headerUrl; i.classList.add("wide"); } else i.style.visibility = "hidden"; }} />
              <i className="steam-check" aria-hidden="true">✓</i>
              <span className={`steam-inst ${g.install}`}>{installLabel(g.install)}</span>
            </div>
            <strong>{g.name}</strong>
            <small>{t("Steam · PC")}{g.playtimeForever !== null && <> · {steamTime(g.playtimeForever)}</>}</small>
            {g.lastPlayed && <small>{t("Jogado {ago}", { ago: raAgo(g.lastPlayed) })}</small>}
            {g.inLibrary && <em className="steam-lib">{g.inLibrary.how === "title" ? t("Mesmo nome na biblioteca") : t("Já na biblioteca")}</em>}
          </button>
        ))}
      </div>

      <div className="row end">
        <span className="grow muted">{t("{n} selecionado(s)", { n: sel.size })}</span>
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="primary" disabled={!sel.size || busy !== null} onClick={review}>{busy === "preview" ? t("Preparando…") : t("Revisar importação")}</Button>
      </div>
    </Modal>
  );
}
