import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Block from "./DpBlock";
import Button from "./Button";
import Modal from "./Modal";
import Select from "./Select";
import Seg from "./Seg";
import CandidatePicker from "./CandidatePicker";
import IgdbSlideshow, { type Slide } from "./IgdbSlideshow";
import { api, errMsg } from "../lib/api";
import { formatBytes } from "../lib/format";
import { currentLang, t, tk } from "../lib/i18n";
import { useSession } from "../state/session";
import type { Game, ModSort, NexusGame, NexusLinkStatus, NexusMod, NexusModDetail } from "../types";

const SORTS: [ModSort, string][] = [
  ["popular", tk("Popularidade")], ["downloads", tk("Mais baixados")], ["endorsements", tk("Mais endossados")], ["updated", tk("Atualizados recentemente")], ["new", tk("Novos")],
];
const num = (n: number | null) => (n === null ? "—" : n.toLocaleString(currentLang()));
const date = (s: string | number | null) => {
  if (s === null) return "—";
  const d = typeof s === "number" ? new Date(s * 1000) : new Date(s);
  return isNaN(d.getTime()) ? "—" : d.toLocaleDateString(currentLang(), { day: "2-digit", month: "short", year: "numeric" });
};

/**
 * Aba "Mods" (Nexus Mods, API oficial). Descoberta e consulta apenas: nada é baixado nem instalado;
 * "Abrir no Nexus Mods" leva à página oficial. O jogo do Nexus é sempre confirmado pelo usuário.
 */
export default function ModsPanel({ game }: { game: Game }) {
  const { profile, toast, setView } = useSession();
  const p = profile!;
  const [st, setSt] = useState<NexusLinkStatus | null>(null);
  const [picking, setPicking] = useState<NexusGame[] | null>(null);
  const [searchTerm, setSearchTerm] = useState("");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<ModSort>("popular");
  const [category, setCategory] = useState("");
  const [cats, setCats] = useState<string[]>([]);
  const [mods, setMods] = useState<NexusMod[] | null>(null);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [favs, setFavs] = useState<NexusMod[]>([]);
  const [onlyFavs, setOnlyFavs] = useState(false);
  const [open, setOpen] = useState<NexusMod | null>(null);
  const [loading, setLoading] = useState(false);
  const link = st?.link ?? null;

  const loadStatus = useCallback(() => api.nexusLinkStatus(p.id, game.id).then(setSt).catch((e) => toast(errMsg(e), "err")), [p.id, game.id, toast]);
  useEffect(() => { loadStatus(); }, [loadStatus]);
  useEffect(() => {
    if (!link) return;
    api.nexusCategories(link.domain).then(setCats).catch(() => setCats([]));
    api.nexusFavorites(p.id, link.domain).then(setFavs).catch(() => {});
  }, [link, p.id]);

  // pesquisa (com pequeno atraso ao digitar)
  useEffect(() => {
    if (!link || onlyFavs) return;
    let alive = true;
    const h = setTimeout(async () => {
      setLoading(true);
      try {
        const r = await api.nexusMods(p.id, game.id, query, sort, category, offset);
        if (alive) { setMods((cur) => (offset > 0 && cur ? [...cur, ...r.mods] : r.mods)); setTotal(r.total); }
      } catch (e) { if (alive) toast(errMsg(e), "err"); }
      finally { if (alive) setLoading(false); }
    }, 300);
    return () => { alive = false; clearTimeout(h); };
  }, [link, query, sort, category, offset, onlyFavs, p.id, game.id, toast]);
  useEffect(() => { setOffset(0); }, [query, sort, category]);

  const confirm = async (g: NexusGame | null) => {
    try { await api.nexusSetLink(p.id, game.id, g); setPicking(null); setMods(null); await loadStatus(); if (g) toast(t("Jogo do Nexus Mods confirmado: {name}.", { name: g.name })); }
    catch (e) { toast(errMsg(e), "err"); }
  };
  const isFav = (m: NexusMod) => favs.some((f) => f.modId === m.modId && f.domain === m.domain);
  const toggleFav = async (m: NexusMod) => {
    try {
      const on = await api.nexusToggleFavorite(p.id, m);
      setFavs((cur) => (on ? [m, ...cur] : cur.filter((f) => !(f.modId === m.modId && f.domain === m.domain))));
    } catch (e) { toast(errMsg(e), "err"); }
  };

  if (!st) return <p className="muted">{t("Carregando…")}</p>;
  if (!st.enabled) {
    return (
      <Block title={t("Mods")}>
        <p className="muted">{t("Ative o Nexus Mods com a sua chave pessoal para descobrir mods deste jogo.")}</p>
        <div className="row"><Button onClick={() => setView("settings")}>{t("Configurar fontes de dados")}</Button></div>
      </Block>
    );
  }

  if (!link) {
    return (
      <>
        <Block title={t("Mods")}>
          <p>{t("Qual destes jogos do Nexus Mods corresponde a “{name}”? O nome sozinho não basta (há edições e versões de console diferentes), por isso a escolha é sua.", { name: game.title })}</p>
          {st.error && <p className="ra-warn">{st.error}</p>}
          <div className="ra-cands">
            {st.candidates.map((c) => (
              <div key={c.id} className="ra-cand">
                <span className="ra-icon-ph">N</span>
                <div><strong>{c.name}</strong><small>{t("{n} mods · nexusmods.com/{d}", { n: num(c.modCount), d: c.domain })}</small></div>
                <Button variant="primary" onClick={() => confirm(c)}>{t("Confirmar")}</Button>
              </div>
            ))}
            {!st.candidates.length && !st.error && <p className="muted">{t("Nenhum jogo encontrado com esse nome. Pesquise manualmente.")}</p>}
          </div>
          <div className="row">
            <input className="search" value={searchTerm} onChange={(e) => setSearchTerm(e.target.value)} placeholder={t("Nome do jogo no Nexus Mods")} aria-label={t("Nome do jogo no Nexus Mods")} />
            <Button disabled={!searchTerm.trim()} onClick={() => api.nexusSearchGames(searchTerm).then(setPicking).catch((e) => toast(errMsg(e), "err"))}>{t("Pesquisar")}</Button>
          </div>
        </Block>
        {picking && (
          <CandidatePicker title={t("Jogos no Nexus Mods")} onClose={() => setPicking(null)} intro={t("Escolha o jogo correspondente.")}
            onPick={(id) => confirm(picking.find((g) => g.id === id) ?? null)}
            items={picking.map((g) => ({ id: g.id, title: g.name, subtitle: t("{n} mods · nexusmods.com/{d}", { n: num(g.modCount), d: g.domain }) }))} />
        )}
      </>
    );
  }

  const list = onlyFavs ? favs : mods ?? [];
  return (
    <>
      <Block title={t("Mods · {name}", { name: link.name })} actions={
        <>
          <Button variant="ghost" onClick={() => openUrl(`https://www.nexusmods.com/${link.domain}/mods/`).catch(() => {})}>{t("Abrir no Nexus Mods")}</Button>
          <Button variant="ghost" onClick={() => confirm(null)}>{t("Trocar jogo")}</Button>
        </>
      }>
        <div className="mods-tools">
          <input className="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("Pesquisar mods…")} aria-label={t("Pesquisar mods")} disabled={onlyFavs} />
          <Seg<ModSort> value={sort} onChange={setSort} opts={SORTS.map(([k, l]) => [t(l), k])} />
          <Select<string> value={category} width={210} title={t("Categoria")} onChange={setCategory} disabled={onlyFavs}
            options={[[t("Todas as categorias"), ""], ...cats.map((c): [string, string] => [c, c])]} />
          <Button variant={onlyFavs ? "primary" : "default"} onClick={() => setOnlyFavs(!onlyFavs)}>{t("★ Favoritos ({n})", { n: favs.length })}</Button>
        </div>
        <p className="muted note">{onlyFavs ? t("Seus mods favoritos deste jogo.") : t("{n} mods encontrados · conteúdo adulto oculto · nada é baixado ou instalado pelo NEXSTAGE.", { n: num(total) })}</p>
        <div className="mods-grid">
          {list.map((m) => (
            <button key={`${m.domain}-${m.modId}`} data-focus type="button" className="mod-card" onClick={() => setOpen(m)}>
              <span className="mod-img" style={m.thumbnail ? { backgroundImage: `url(${m.thumbnail})` } : undefined}>
                {isFav(m) && <i className="fav" aria-label={t("Favorito")}>★</i>}
                {m.supportsVortex && <em className="mod-vortex">Vortex</em>}
              </span>
              <span className="mod-body">
                <strong>{m.name}</strong>
                <small>{m.summary}</small>
                <span className="mod-meta">
                  <span title={t("Downloads")}>⬇ {num(m.downloads)}</span>
                  <span title={t("Endossos")}>👍 {num(m.endorsements)}</span>
                  {m.category && <span>{m.category}</span>}
                </span>
                <small className="muted">{t("por {a} · atualizado em {d}", { a: m.author || m.uploader, d: date(m.updatedAt) })}</small>
              </span>
            </button>
          ))}
        </div>
        {!onlyFavs && mods && mods.length === 0 && !loading && <p className="muted">{t("Nenhum mod encontrado com esses filtros.")}</p>}
        {loading && <p className="muted">{t("Carregando…")}</p>}
        {!onlyFavs && mods && mods.length < total && !loading && (
          <div className="row"><Button onClick={() => setOffset(mods.length)}>{t("Carregar mais")}</Button></div>
        )}
      </Block>
      {open && <ModDetailModal mod={open} gameId={link.id} fav={isFav(open)} onFav={() => toggleFav(open)} onClose={() => setOpen(null)} />}
    </>
  );
}

/** Aba "Files" da página oficial do mod. Nada é baixado pelo NEXSTAGE. */
function filesUrl(mod: NexusMod) {
  let u: URL;
  try { u = new URL(mod.url); } catch { u = new URL(`https://www.nexusmods.com/${mod.domain}/mods/${mod.modId}`); }
  u.searchParams.set("tab", "files");
  return u.toString();
}

/**
 * Mantém só imagens com cara de screenshot: carrega cada uma e descarta as pequenas (ícones, botões)
 * e as muito alongadas (banners, divisórias). Ordem original preservada; a imagem principal vem primeiro.
 */
function useScreenshots(urls: string[]) {
  const key = urls.join("|");
  const [ok, setOk] = useState<string[] | null>(null);
  useEffect(() => {
    let alive = true;
    const list = key ? key.split("|") : [];
    const keep = new Array<boolean | null>(list.length).fill(null);
    let done = false;
    const flush = () => { if (alive && !done) { done = true; setOk(list.filter((_, i) => keep[i])); } };
    const publish = () => { if (keep.every((k) => k !== null)) flush(); };
    const timer = window.setTimeout(flush, 6000);
    setOk(null);
    list.forEach((u, i) => {
      const img = new Image();
      img.referrerPolicy = "no-referrer";
      img.onload = () => {
        const r = img.naturalWidth / Math.max(1, img.naturalHeight);
        keep[i] = img.naturalWidth >= 480 && img.naturalHeight >= 240 && r >= 0.9 && r <= 2.6;
        publish();
      };
      img.onerror = () => { keep[i] = false; publish(); };
      img.src = u;
    });
    return () => { alive = false; clearTimeout(timer); };
  }, [key]);
  return ok;
}

function ModDetailModal({ mod, gameId, fav, onFav, onClose }: { mod: NexusMod; gameId: number; fav: boolean; onFav: () => void; onClose: () => void }) {
  const [d, setD] = useState<NexusModDetail | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => { api.nexusModDetail(gameId, mod.modId).then(setD).catch((e) => setErr(errMsg(e))); }, [gameId, mod.modId]);
  const candidates = [...(mod.picture ? [mod.picture] : []), ...(d?.images ?? []).filter((u) => u !== mod.picture)];
  const shots = useScreenshots(d ? candidates : []);
  // até a análise terminar, mostra a imagem principal (sem "pular" a janela)
  const slides: Slide[] = (shots && shots.length ? shots : mod.picture ? [mod.picture] : []).map((url) => ({ kind: "shot" as const, url }));
  return (
    <Modal title={mod.name} onClose={onClose} wide>
      {slides.length > 0 && (
        <div className="mod-gallery">
          <IgdbSlideshow slides={slides} auto={false} />
        </div>
      )}
      <p className="desc">{mod.summary || t("Sem descrição.")}</p>
      <dl className="dp-info">
        <dt>{t("Autor")}</dt><dd>{mod.author || mod.uploader || "—"}</dd>
        <dt>{t("Versão")}</dt><dd>{mod.version ?? "—"}</dd>
        <dt>{t("Categoria")}</dt><dd>{mod.category ?? "—"}</dd>
        <dt>{t("Atualizado em")}</dt><dd>{date(mod.updatedAt)}</dd>
        <dt>{t("Downloads")}</dt><dd>{num(mod.downloads)}</dd>
        <dt>{t("Endossos")}</dt><dd>{num(mod.endorsements)}</dd>
        <dt>{t("Vortex")}</dt><dd>{mod.supportsVortex ? t("Compatível (instalação feita pelo Vortex, fora do NEXSTAGE)") : t("Não informado")}</dd>
      </dl>
      {err && <p className="ra-warn">{err}</p>}
      {!d && !err && <p className="muted">{t("Carregando…")}</p>}
      {d && (
        <>
          <h3 className="section sub">{t("Requisitos")}</h3>
          {d.requirements.length ? (
            <ul className="mod-reqs">{d.requirements.map((r, i) => <li key={i}><b>{r.name}</b>{r.notes && <small> · {r.notes}</small>}</li>)}</ul>
          ) : <p className="muted">{t("Nenhum requisito informado.")}</p>}
          <h3 className="section sub">{t("Versões disponíveis")}</h3>
          {d.files.length ? (
            <table className="hltb mod-files">
              <tbody>{d.files.slice(0, 12).map((f, i) => (
                <tr key={i}><td>{f.name}{f.primary ? " ★" : ""}<small className="muted"> · {f.category}</small></td><td>{f.version || "—"}</td><td>{date(f.date)}</td><td>{f.sizeKb ? formatBytes(f.sizeKb * 1024) : "—"}</td></tr>
              ))}</tbody>
            </table>
          ) : <p className="muted">{t("Nenhum arquivo disponível.")}</p>}
        </>
      )}
      <p className="muted note">{t("O download acontece no site do Nexus Mods, com a sua conta. O NEXSTAGE não baixa nem instala arquivos.")}</p>
      <div className="row end">
        <Button onClick={onFav}>{fav ? t("★ Remover dos favoritos") : t("☆ Favoritar")}</Button>
        <Button onClick={() => openUrl(filesUrl(mod)).catch(() => {})} title={t("Abre a aba de arquivos do mod no navegador")}>{t("Download")}</Button>
        <Button variant="primary" onClick={() => openUrl(mod.url).catch(() => {})}>{t("Abrir no Nexus Mods")}</Button>
        <Button variant="ghost" onClick={onClose}>{t("Fechar")}</Button>
      </div>
    </Modal>
  );
}
