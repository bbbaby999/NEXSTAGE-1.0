import { CSSProperties, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { onPadButton } from "../lib/nav";
import { useInputDevice } from "../lib/inputDevice";
import { play } from "../lib/sound";
import Glyph from "../components/Glyph";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import Button from "../components/Button";
import Select from "../components/Select";
import Seg from "../components/Seg";
import PlatformBadge from "../components/PlatformBadge";
import ZereiAdd from "../components/zerei/ZereiAdd";
import ZereiForm from "../components/zerei/ZereiForm";
import ZereiDetail from "../components/zerei/ZereiDetail";
import ZereiStats from "../components/zerei/ZereiStats";
import { tiltHandlers } from "../lib/tilt";
import type { ZereiEntry, ZereiHit, ZereiInput, ZereiStatus } from "../types";
import { COMPLETION_LABEL, STATUS_LABEL, STATUS_ORDER, doneYear, fmtHours, fmtRating, statusLabel } from "../lib/zerei";
import { t, tk } from "../lib/i18n";

type Tab = "list" | "stats";
type Sort = "recent" | "title" | "rating" | "completed" | "hours";
type RatingFilter = "all" | "9" | "7" | "5" | "low" | "none";
type Editing = { entry?: ZereiEntry; initial: ZereiInput } | null;

const tilt = tiltHandlers<HTMLButtonElement>((el) => el.querySelector<HTMLElement>(".cover"));
const SORTS: [string, Sort][] = [
  [tk("Nome"), "title"], [tk("Mais recentes"), "recent"], [tk("Data em que zerou"), "completed"], [tk("Maior nota"), "rating"], [tk("Mais horas"), "hours"],
];
/** Ordem do filtro de status (L2/R2 percorrem nesta ordem). */
const STATUS_FILTERS: (ZereiStatus | "all")[] = ["all", ...STATUS_ORDER];
// Unicode: vale para qualquer alfabeto (japonês, chinês, coreano, cirílico...), não só a-z. Tira só os
// acentos latinos (é → e); marcas de outros alfabetos (ex.: o dakuten de ゼ) fazem parte do nome.
const key = (s: string) => s.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").normalize("NFC")
  .replace(/['’™®]/g, "").replace(/[^\p{L}\p{N}]+/gu, " ").trim();

const blank = (title = ""): ZereiInput => ({
  title, platform: "PC", cover: null, logo: null, background: null, year: null, platforms: [], igdbId: null, rawgId: null, sgdbId: null,
  status: "completed", completionType: null, difficulty: null, startedAt: null, completedAt: null, hours: null, rating: null, notes: "",
});

/**
 * Zerei!: coleção pessoal de jogos zerados/jogando/pausados de cada perfil, independente da biblioteca
 * (o jogo não precisa estar instalado nem cadastrado). Cards com capa, filtros, detalhes, estatísticas
 * e importação/exportação em CSV.
 */
export default function Zerei() {
  const { profile, toast } = useSession();
  const p = profile!;
  const [list, setList] = useState<ZereiEntry[] | null>(null);
  const [tab, setTab] = useState<Tab>("list");
  const [openId, setOpenId] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<Editing>(null);
  const [q, setQ] = useState("");
  const [status, setStatus] = useState<ZereiStatus | "all">("all");
  const [platform, setPlatform] = useState("all");
  const [year, setYear] = useState<number | "all" | "none">("all");
  const [rating, setRating] = useState<RatingFilter>("all");
  const [sort, setSort] = useState<Sort>("title");

  const load = useCallback(() => api.zereiList(p.id).then(setList).catch((e) => { setList([]); toast(errMsg(e), "err"); }), [p.id, toast]);
  useEffect(() => { load(); }, [load]);

  const focusLater = (sel: string) => setTimeout(() => {
    const el = document.querySelector<HTMLElement>(sel);
    el?.scrollIntoView({ block: "center", behavior: "auto" });
    el?.focus({ preventScroll: true });
  }, 80);

  const closeDetail = useCallback(() => {
    const id = openId;
    setOpenId(null);
    if (id !== null) focusLater(`[data-zerei-id="${id}"]`);
  }, [openId]);

  // Voltar (Esc/Círculo) na página de detalhes: volta à coleção em vez de sair da página
  useEffect(() => {
    if (openId === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(".overlay, .pick-root, .kb-root, .sm-root:not(.leaving)")) return;
      const tg = e.target as HTMLElement;
      if (["INPUT", "TEXTAREA"].includes(tg.tagName)) return;
      e.stopImmediatePropagation();
      e.preventDefault();
      closeDetail();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [openId, closeDetail]);

  const all = list ?? [];
  const platforms = useMemo(() => [...new Set(all.map((e) => e.platform))].sort((a, b) => a.localeCompare(b)), [all]);
  const years = useMemo(() => [...new Set(all.map(doneYear).filter((y): y is number => y !== null))].sort((a, b) => b - a), [all]);

  const shown = useMemo(() => {
    const k = key(q);
    const r = all.filter((e) =>
      (!k || key(e.title).includes(k)) &&
      (status === "all" || e.status === status) &&
      (platform === "all" || e.platform === platform) &&
      (year === "all" || (year === "none" ? doneYear(e) === null : doneYear(e) === year)) &&
      (rating === "all" || (rating === "none" ? e.rating === null : rating === "low" ? e.rating !== null && e.rating < 5 : e.rating !== null && e.rating >= Number(rating))));
    const by: Record<Sort, (a: ZereiEntry, b: ZereiEntry) => number> = {
      recent: (a, b) => b.updatedAt.localeCompare(a.updatedAt),
      title: (a, b) => a.title.localeCompare(b.title) || a.platform.localeCompare(b.platform) || a.run - b.run,
      rating: (a, b) => (b.rating ?? -1) - (a.rating ?? -1),
      completed: (a, b) => (b.completedAt ?? "").localeCompare(a.completedAt ?? ""),
      hours: (a, b) => (b.hours ?? -1) - (a.hours ?? -1),
    };
    return [...r].sort(by[sort]);
  }, [all, q, status, platform, year, rating, sort]);

  // ---- Controle: L2/R2, □ e Options (só sem janela/lista/menu/teclado por cima) ----
  const device = useInputDevice();
  const pad = device !== "keyboard";
  const live = useRef({ openId, tab, shown, sort, count: all.length });
  live.current = { openId, tab, shown, sort, count: all.length };
  useEffect(() => onPadButton((b, e) => {
    if (e.defaultPrevented || !["l2", "r2", "x", "menu"].includes(b)) return;
    if (document.querySelector(".overlay, .pick-root, .kb-root, .sm-root:not(.leaving), .ctx-root")) return;
    const s = live.current;
    const d = b === "r2" ? 1 : -1;
    // Detalhes: L2/R2 = registro anterior/próximo, na ordem da coleção (para nas pontas)
    if (s.openId !== null) {
      if (b !== "l2" && b !== "r2") return;
      e.preventDefault();
      const i = s.shown.findIndex((x) => x.id === s.openId);
      const next = s.shown[i < 0 ? 0 : Math.max(0, Math.min(s.shown.length - 1, i + d))];
      if (next && next.id !== s.openId) { play("move"); setOpenId(next.id); }
      return;
    }
    // Options: Coleção ↔ Estatísticas
    if (b === "menu") {
      e.preventDefault();
      play("move");
      setTab((x) => (x === "list" ? "stats" : "list"));
      return;
    }
    if (s.tab !== "list" || !s.count) return;
    e.preventDefault();
    if (b === "x") {
      const i = SORTS.findIndex(([, k]) => k === s.sort);
      const [label, k] = SORTS[(i + 1) % SORTS.length];
      setSort(k);
      toast(t("Ordenado por {v}", { v: t(label).toLowerCase() }));
      return;
    }
    // L2/R2: filtro de status anterior/próximo (para nas pontas)
    setStatus((cur) => {
      const i = STATUS_FILTERS.indexOf(cur);
      const next = STATUS_FILTERS[Math.max(0, Math.min(STATUS_FILTERS.length - 1, i + d))];
      if (next !== cur) play("move");
      return next;
    });
  }), [toast]);

  const filtered = q || status !== "all" || platform !== "all" || year !== "all" || rating !== "all";
  const clearFilters = () => { setQ(""); setStatus("all"); setPlatform("all"); setYear("all"); setRating("all"); };
  const current = openId !== null ? all.find((e) => e.id === openId) ?? null : null;
  const siblings = current ? all.filter((e) => e.id !== current.id && (key(e.title) === key(current.title) || (current.igdbId !== null && e.igdbId === current.igdbId))) : [];

  const fromHit = (h: ZereiHit): ZereiInput => ({
    ...blank(h.title), cover: h.cover, background: h.background, year: h.year, platforms: h.platforms,
    platform: h.platforms[0] ?? "PC", igdbId: h.igdbId, rawgId: h.rawgId,
  });
  const toInput = (e: ZereiEntry): ZereiInput => {
    const { id: _i, uid: _u, run: _r, createdAt: _c, updatedAt: _up, ...rest } = e;
    return rest;
  };

  const onSaved = (e: ZereiEntry, created: boolean) => {
    setEditing(null);
    setList((l) => [e, ...(l ?? []).filter((x) => x.id !== e.id)]);
    toast(created ? t("“{title}” adicionado ao Zerei!.", { title: e.title }) : t("Registro atualizado."));
    if (created) setOpenId(e.id);
  };

  async function remove(e: ZereiEntry) {
    try {
      await api.zereiDelete(p.id, e.id);
      setList((l) => (l ?? []).filter((x) => x.id !== e.id));
      setOpenId(null);
      toast(t("Registro excluído."));
    } catch (err) { toast(errMsg(err), "err"); }
  }

  async function exportCsv() {
    const path = await saveDialog({ title: t("Exportar Zerei! (CSV)"), defaultPath: `zerei-${p.username}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] }).catch(() => null);
    if (!path) return;
    try { const n = await api.zereiExportCsv(p.id, path); toast(t("{n} registro(s) exportado(s).", { n })); }
    catch (e) { toast(errMsg(e), "err"); }
  }

  async function importCsv() {
    const path = await openDialog({ title: t("Importar CSV para o Zerei!"), multiple: false, filters: [{ name: "CSV", extensions: ["csv", "txt"] }] }).catch(() => null);
    if (!path || Array.isArray(path)) return;
    try {
      const r = await api.zereiImportCsv(p.id, path);
      await load();
      toast(t("Importação: {a} novo(s), {u} atualizado(s), {c} completado(s) com dados novos, {d} duplicado(s) ignorado(s).", { a: r.added, u: r.updated, c: r.completed, d: r.duplicates }), r.errors.length ? "err" : undefined);
      if (r.errors.length) toast(r.errors.slice(0, 3).map((x) => t("Linha {n}: {msg}", { n: x.line, msg: t(x.message) })).join(" · "), "err");
    } catch (e) { toast(errMsg(e), "err"); }
  }

  if (list === null) return null;

  const modals = (
    <>
      {adding && (
        <ZereiAdd onClose={() => setAdding(false)}
          onPick={(h) => { setAdding(false); setEditing({ initial: fromHit(h) }); }}
          onManual={(title) => { setAdding(false); setEditing({ initial: blank(title) }); }} />
      )}
      {editing && (
        <ZereiForm entry={editing.entry} initial={editing.initial} onClose={() => setEditing(null)} onSaved={onSaved}
          onOpenExisting={(id) => { setEditing(null); setTab("list"); setOpenId(id); }} />
      )}
    </>
  );

  if (current) {
    return (
      <>
        <ZereiDetail entry={current} siblings={siblings} onBack={closeDetail} onOpen={setOpenId}
          onEdit={() => setEditing({ entry: current, initial: toInput(current) })} onDelete={() => remove(current)} />
        {modals}
      </>
    );
  }

  return (
    <div className="zr-page">
      <div className="page-head">
        <div className="zr-title">
          <h1>{t("Zerei!")}</h1>
          <small className="muted">{t("{n} jogo(s) · {c} zerado(s)", { n: all.length, c: all.filter((e) => e.status === "completed").length })}</small>
        </div>
        <div className="row" data-nav-group>
          <Seg<Tab> value={tab} onChange={setTab} opts={[[t("Coleção"), "list"], [t("Estatísticas"), "stats"]]} />
          <Button variant="primary" data-autofocus={all.length === 0 ? "" : undefined} onClick={() => setAdding(true)}>{t("+ Adicionar jogo")}</Button>
          <Button onClick={importCsv}>{t("Importar CSV")}</Button>
          <Button onClick={exportCsv} disabled={!all.length}>{t("Exportar CSV")}</Button>
        </div>
      </div>

      {tab === "stats" ? (
        <ZereiStats list={all} onOpen={(id) => { setTab("list"); setOpenId(id); }} />
      ) : all.length === 0 ? (
        <div className="empty">
          <p>{t("Sua coleção do Zerei! está vazia. Pesquise um jogo pelo nome e registre quando começou, quando zerou, as horas e a sua nota.")}</p>
          <Button variant="primary" onClick={() => setAdding(true)}>{t("+ Adicionar jogo")}</Button>
        </div>
      ) : (
        <>
          <div className="zr-filters" data-nav-group>
            <input className="search" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("Filtrar pelo nome")} aria-label={t("Filtrar pelo nome")} />
            <span className="zr-status-filter">
              {pad && <Glyph k="l2" />}
              <Seg<ZereiStatus | "all"> value={status} onChange={setStatus}
                opts={STATUS_FILTERS.map((s): [string, ZereiStatus | "all"] => [s === "all" ? t("Todos") : t(STATUS_LABEL[s]), s])} />
              {pad && <Glyph k="r2" />}
            </span>
            <Select<string> value={platform} title={t("Plataforma")} onChange={setPlatform}
              options={[[t("Todas as plataformas"), "all"], ...platforms.map((x): [string, string] => [x, x])]} />
            <Select<number | "all" | "none"> value={year} title={t("Ano em que zerou")} onChange={setYear}
              options={[[t("Todos os anos"), "all"], ...years.map((y): [string, number] => [String(y), y]), [t("Sem data"), "none"]]} />
            <Select<RatingFilter> value={rating} title={t("Avaliação")} onChange={setRating}
              options={[[t("Todas as notas"), "all"], [t("Nota 9 ou mais"), "9"], [t("Nota 7 ou mais"), "7"], [t("Nota 5 ou mais"), "5"], [t("Abaixo de 5"), "low"], [t("Sem nota"), "none"]]} />
            <Select<Sort> value={sort} title={t("Ordenar por")} onChange={setSort}
              options={SORTS.map(([l, k]): [string, Sort] => [t(l), k])} />
            {filtered && <Button variant="ghost" onClick={clearFilters}>{t("Limpar filtros")}</Button>}
          </div>

          {shown.length === 0 ? (
            <div className="empty"><p>{t("Nenhum jogo com esses filtros.")}</p></div>
          ) : (
            <div className="grid zr-grid">
              {shown.map((e, i) => (
                <button key={e.id} data-focus data-zerei-id={e.id} data-autofocus={i === 0 ? "" : undefined}
                  className="card zr-card" style={{ "--i": Math.min(i, 12) } as CSSProperties}
                  onClick={() => setOpenId(e.id)} onMouseMove={tilt.onMouseMove} onMouseLeave={tilt.onMouseLeave}
                  aria-label={`${e.title}, ${e.platform}, ${statusLabel(e.status)}`}>
                  <div className="cover" style={e.cover ? { backgroundImage: `url("${e.cover}")` } : undefined}>
                    {!e.cover && <span>{e.title.charAt(0).toUpperCase()}</span>}
                    <PlatformBadge platform={e.platform} />
                    <i className={`zr-status st-${e.status}`}>{statusLabel(e.status)}</i>
                    {e.completionType === "full" && <i className="zr-100">{t(COMPLETION_LABEL.full)}</i>}
                    {e.rating !== null && <i className="zr-rate">★ {fmtRating(e.rating)}</i>}
                  </div>
                  <div className="card-meta">
                    <strong>{e.title}</strong>
                    <small>{[e.platform, e.run > 1 ? t("{n}ª conclusão", { n: e.run }) : null, e.hours !== null ? fmtHours(e.hours) : null, doneYear(e)].filter(Boolean).join(" · ")}</small>
                  </div>
                </button>
              ))}
            </div>
          )}
        </>
      )}
      {modals}
    </div>
  );
}
