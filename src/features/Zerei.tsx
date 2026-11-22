import { CSSProperties, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { onPadButton } from "../lib/nav";
import { useInputDevice } from "../lib/inputDevice";
import { play } from "../lib/sound";
import Glyph from "../components/Glyph";
import { pickOpen as openDialog, pickSave as saveDialog } from "../lib/filePicker";
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
import type { ZereiEntry, ZereiHit, ZereiInput, ZereiRefreshProgress, ZereiStatus } from "../types";
import Modal from "../components/Modal";
import { CaptureMenu, ICON, type MenuAction } from "../components/CaptureUi";
import { listen } from "@tauri-apps/api/event";
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
  // seleção de vários (excluir, mover de lista, atualizar imagens)
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [confirmDel, setConfirmDel] = useState(false);
  const [refreshing, setRefreshing] = useState<ZereiRefreshProgress | null>(null);
  /** Menu de ações da seleção pelo controle (☰ = ações, ▢ = mover para): sem subir até a barra. */
  const [selMenu, setSelMenu] = useState<"main" | "move" | null>(null);
  /** "Mais opções" de um registro: botão direito sobre o cartão. */
  const [entryMenu, setEntryMenu] = useState<{ entry: ZereiEntry; x: number; y: number; from: DOMRect } | null>(null);
  /** Confirmação antes de atualizar imagens (aviso, chaves de API e "substituir as existentes"). */
  const [askRefresh, setAskRefresh] = useState<{ ids: number[] | null; overwrite: boolean } | null>(null);
  const [keys, setKeys] = useState<{ sgdb: boolean; igdb: boolean; rawg: boolean } | null>(null);
  const openRefresh = (ids: number[] | null) => {
    // seleção: por padrão troca as imagens dos escolhidos; todos: só preenche o que falta
    setAskRefresh({ ids, overwrite: ids !== null });
    setKeys(null);
    Promise.all([
      api.getAppSetting("sgdb_key").catch(() => null),
      api.getAppSetting("rawg_key").catch(() => null),
      api.sourcesList().catch(() => []),
    ]).then(([sg, rw, src]) => setKeys({ sgdb: sg === "1", rawg: rw === "1", igdb: src.some((x) => x.id === "igdb" && x.enabled && x.configured) }));
  };
  const stopSelecting = () => { setSelecting(false); setSelected(new Set()); };
  const toggleSel = (id: number) => setSelected((x) => { const n = new Set(x); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  /** Segurar um jogo (Ⓐ ou botão do mouse por 0,5 s): abre a seleção já com ele marcado; já selecionando, marca/desmarca. */
  const selectingRef = useRef(selecting);
  selectingRef.current = selecting;
  const holdSelect = (id: number) => {
    play("confirm");
    if (selectingRef.current) toggleSel(id);
    else { setSelected(new Set([id])); setSelecting(true); }
  };
  const gridRef = useRef<HTMLDivElement>(null);
  const holdRef = useRef(holdSelect);
  holdRef.current = holdSelect;
  // controle: Ⓐ segurado chega como "nexstage:hold" no cartão em foco (os cartões têm data-hold)
  useEffect(() => {
    const el = gridRef.current;
    if (!el) return;
    const onHold = (e: Event) => {
      const id = (e.target as HTMLElement).closest<HTMLElement>("[data-zerei-id]")?.dataset.zereiId;
      if (id) holdRef.current(Number(id));
    };
    el.addEventListener("nexstage:hold", onHold);
    return () => el.removeEventListener("nexstage:hold", onHold);
  });
  // mouse/toque: segurar o botão sobre o cartão; o clique que vem ao soltar é ignorado
  const press = useRef<{ id: number; x: number; y: number; timer: ReturnType<typeof setTimeout>; fired: boolean } | null>(null);
  const pressStart = (id: number, e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const x = e.clientX, y = e.clientY;
    const timer = setTimeout(() => { if (press.current) { press.current.fired = true; holdRef.current(id); } }, 500);
    press.current = { id, x, y, timer, fired: false };
  };
  const pressMove = (e: React.PointerEvent) => {
    const pr = press.current;
    if (pr && !pr.fired && Math.abs(e.clientX - pr.x) + Math.abs(e.clientY - pr.y) > 10) { clearTimeout(pr.timer); press.current = null; }
  };
  const pressEnd = () => { const pr = press.current; if (pr && !pr.fired) { clearTimeout(pr.timer); press.current = null; } };
  /** O clique logo depois de um "segurar" do mouse não abre/alterna o jogo de novo. */
  const swallowClick = () => {
    const pr = press.current;
    if (pr?.fired) { press.current = null; return true; }
    return false;
  };

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

  // Esc/Ⓑ sai do modo de seleção antes de qualquer outra navegação (sem janela por cima)
  useEffect(() => {
    if (!selecting) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(".overlay, .ctx-root, .pick-root, .kb-root, .sm-root:not(.leaving)")) return;
      e.stopImmediatePropagation();
      e.preventDefault();
      stopSelecting();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [selecting]);

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

  // ---- Controle: L2/R2, □ e Options no cartão (só sem janela/lista/menu/teclado por cima) ----
  const device = useInputDevice();
  const pad = device !== "keyboard";
  const live = useRef({ openId, tab, shown, sort, count: all.length, selecting });
  live.current = { openId, tab, shown, sort, count: all.length, selecting };
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
    // Selecionando: ☰ abre as ações da seleção; ▢ abre "Mover para…"
    if (s.selecting && s.tab === "list" && (b === "menu" || b === "x")) {
      e.preventDefault();
      setSelMenu(b === "menu" ? "main" : "move");
      return;
    }
    // Options sobre um cartão: o mesmo menu do botão direito (capa ampliada no lugar + opções ao lado)
    if (b === "menu") {
      const card = (document.activeElement as HTMLElement | null)?.closest?.<HTMLElement>("[data-zerei-id]");
      const entry = card ? s.shown.find((x) => x.id === Number(card.dataset.zereiId)) : undefined;
      if (!card || !entry) return;
      e.preventDefault();
      const cover = card.querySelector(".cover") ?? card;
      const r = cover.getBoundingClientRect();
      setEntryMenu({ entry, x: r.left, y: r.top, from: r });
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

  /**
   * Atualizar imagens: `ids` null = todos os jogos, só preenchendo o que falta (o que o usuário escolheu
   * fica); com seleção, troca capa/logo/fundo dos escolhidos pelo primeiro resultado encontrado.
   */
  async function refreshImages(ids: number[] | null, overwrite: boolean) {
    setRefreshing({ done: 0, total: ids?.length ?? 0, updated: 0, title: "" });
    const un = await listen<ZereiRefreshProgress>("zerei-images-progress", (e) => setRefreshing(e.payload));
    try {
      const r = await api.zereiRefreshImages(p.id, ids, overwrite);
      if (r.total === 0) toast(t("Todos os jogos já têm capa, logo e plano de fundo."));
      else toast(t("Imagens atualizadas em {u} de {n} jogo(s){v}.", { u: r.updated, n: r.total, v: r.notFound ? t(" · {n} sem imagens encontradas", { n: r.notFound }) : "" }) + (r.canceled ? ` ${t("(interrompido)")}` : ""),
        r.updated === 0 && r.notFound ? "err" : undefined);
      await load();
      if (ids) stopSelecting();
    } catch (e) { toast(errMsg(e), "err"); }
    finally { un(); setRefreshing(null); }
  }

  async function moveSelected(to: ZereiStatus) {
    try {
      const n = await api.zereiBulkStatus(p.id, [...selected], to);
      toast(t("{n} jogo(s) movido(s) para “{list}”.", { n, list: statusLabel(to) }));
      await load();
      stopSelecting();
    } catch (e) { toast(errMsg(e), "err"); }
  }

  async function deleteSelected() {
    setConfirmDel(false);
    try {
      const n = await api.zereiBulkDelete(p.id, [...selected]);
      toast(t("{n} registro(s) excluído(s).", { n }));
      await load();
      stopSelecting();
    } catch (e) { toast(errMsg(e), "err"); }
  }

  async function exportCsv() {
    const path = await saveDialog({ purpose: "zerei-export", title: t("Exportar Zerei! (CSV)"), defaultPath: `zerei-${p.username}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] }).catch(() => null);
    if (!path) return;
    try { const n = await api.zereiExportCsv(p.id, path); toast(t("{n} registro(s) exportado(s).", { n })); }
    catch (e) { toast(errMsg(e), "err"); }
  }

  async function importCsv() {
    const path = await openDialog({ purpose: "zerei-import", title: t("Importar CSV para o Zerei!"), multiple: false, filters: [{ name: "CSV", extensions: ["csv", "txt"] }] }).catch(() => null);
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
      {askRefresh && (
        <Modal title={t("Atualizar imagens")} onClose={() => setAskRefresh(null)}>
          <div className="zr-ask">
            <p>{askRefresh.ids
              ? t("O NEXSTAGE vai procurar na internet a capa, a logo e o plano de fundo de {n} jogo(s) selecionado(s), usando o primeiro resultado encontrado.", { n: askRefresh.ids.length })
              : t("O NEXSTAGE vai procurar na internet a capa, a logo e o plano de fundo dos jogos do Zerei! ({n}), usando o primeiro resultado encontrado.", { n: all.length })}</p>
            <p className="muted">{t("Isso pode levar alguns minutos, conforme a quantidade de jogos e a sua conexão. Você pode cancelar a qualquer momento; o que já foi atualizado fica salvo.")}</p>
            <div className="zr-keys">
              <strong>{t("Fontes de imagens")}</strong>
              <small className="muted">{t("Para os resultados corresponderem melhor aos jogos, é recomendado configurar as chaves de API em Configurações → Pesquisa de imagens e Fontes de dados.")}</small>
              <ul>
                <li className={keys?.sgdb ? "ok" : "off"}><b>{keys ? (keys.sgdb ? "✓" : "✕") : "…"}</b> SteamGridDB <span>{t("capas, logos e planos de fundo (recomendado)")}</span></li>
                <li className={keys?.igdb ? "ok" : "off"}><b>{keys ? (keys.igdb ? "✓" : "✕") : "…"}</b> IGDB <span>{t("capas")}</span></li>
                <li className={keys?.rawg ? "ok" : "off"}><b>{keys ? (keys.rawg ? "✓" : "✕") : "…"}</b> RAWG <span>{t("capas e planos de fundo")}</span></li>
              </ul>
              {keys && !keys.sgdb && <small className="zr-keys-warn">{t("Sem a chave do SteamGridDB não é possível encontrar logos, e as capas e fundos vêm só da pesquisa pública.")}</small>}
            </div>
            <label className="zr-overwrite">
              <input type="checkbox" data-focus checked={askRefresh.overwrite}
                onChange={(e) => setAskRefresh((a) => (a ? { ...a, overwrite: e.target.checked } : a))} />
              <span>
                <b>{t("Substituir imagens existentes")}</b>
                <small>{askRefresh.overwrite
                  ? t("As imagens que os jogos já têm serão trocadas pelo primeiro resultado encontrado.")
                  : t("As imagens que já existem não serão alteradas: só os jogos sem capa, logo ou plano de fundo recebem imagens.")}</small>
              </span>
            </label>
          </div>
          <div className="row end">
            <Button variant="ghost" onClick={() => setAskRefresh(null)}>{t("Cancelar")}</Button>
            <Button variant="primary" data-autofocus onClick={() => { const a = askRefresh; setAskRefresh(null); refreshImages(a.ids, a.overwrite); }}>{t("Começar")}</Button>
          </div>
        </Modal>
      )}
      {refreshing && (
        // Esc/Ⓑ ou Cancelar interrompem (entre um jogo e outro)
        <Modal title={t("Atualizando imagens")} onClose={() => { api.zereiRefreshCancel().catch(() => {}); }}>
          <div className="zr-refresh">
            <div className={`zr-refresh-bar ${refreshing.total ? "" : "indet"}`}><i style={{ width: `${refreshing.total ? Math.round((refreshing.done / refreshing.total) * 100) : 0}%` }} /></div>
            <p className="zr-refresh-line">
              <span>{refreshing.total ? t("{d} de {n} jogo(s) · {u} atualizado(s)", { d: refreshing.done, n: refreshing.total, u: refreshing.updated }) : t("Preparando…")}</span>
              {refreshing.total > 0 && <b>{Math.round((refreshing.done / refreshing.total) * 100)}%</b>}
            </p>
            {refreshing.title && <small className="muted">{refreshing.title}</small>}
          </div>
          <div className="row end">
            <Button variant="ghost" data-autofocus onClick={() => { api.zereiRefreshCancel().catch(() => {}); }}>{t("Cancelar")}</Button>
          </div>
        </Modal>
      )}
      {entryMenu && (
        <CaptureMenu from={entryMenu.from} hero={{ cover: entryMenu.entry.cover, title: entryMenu.entry.title, sub: entryMenu.entry.platform }} onClose={() => setEntryMenu(null)} actions={[
          { label: t("Abrir"), icon: ICON.open, run: () => setOpenId(entryMenu.entry.id) },
          { label: t("Editar"), icon: ICON.edit, run: () => setEditing({ entry: entryMenu.entry, initial: toInput(entryMenu.entry) }) },
          { label: t("Selecionar"), icon: ICON.select, run: () => holdSelect(entryMenu.entry.id) },
          { label: t("Excluir"), icon: ICON.trash, danger: true, run: () => { stopSelecting(); setSelected(new Set([entryMenu.entry.id])); setConfirmDel(true); } },
        ]} />
      )}
      {selMenu && (
        <CaptureMenu onClose={() => setSelMenu(null)} actions={selMenu === "move"
          ? STATUS_ORDER.map((x): MenuAction => ({ label: t(STATUS_LABEL[x]), icon: ICON.folder, run: () => { if (selected.size) moveSelected(x); } }))
          : [
            ...(selected.size ? [
              { label: t("Mover para…"), icon: ICON.folder, run: () => setTimeout(() => setSelMenu("move"), 0) },
              { label: t("Atualizar imagens"), icon: ICON.open, run: () => openRefresh([...selected]) },
            ] as MenuAction[] : []),
            { label: t("Selecionar todos"), icon: ICON.select, run: () => setSelected(new Set(shown.map((x) => x.id))) },
            ...(selected.size ? [
              { label: t("Limpar seleção"), icon: ICON.back, run: () => setSelected(new Set()) },
              { label: t("Excluir"), icon: ICON.trash, danger: true, run: () => setConfirmDel(true) },
            ] as MenuAction[] : []),
            { label: t("Sair da seleção"), icon: ICON.back, run: stopSelecting },
          ]} />
      )}
      {confirmDel && (
        <Modal title={t("Excluir registros")} onClose={() => setConfirmDel(false)}>
          <p>{t("Excluir {n} registro(s) do Zerei!? Isso não pode ser desfeito.", { n: selected.size })}</p>
          <div className="row end">
            <Button variant="ghost" data-autofocus onClick={() => setConfirmDel(false)}>{t("Cancelar")}</Button>
            <Button variant="danger" onClick={deleteSelected}>{t("Excluir")}</Button>
          </div>
        </Modal>
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
          {tab === "list" && all.length > 0 && !selecting && <Button onClick={() => setSelecting(true)}>{t("Selecionar")}</Button>}
          {all.length > 0 && <Button onClick={() => openRefresh(null)} disabled={!!refreshing}
            title={t("Procura capa, logo e plano de fundo dos jogos que ainda não têm. As imagens que você já escolheu não mudam.")}>{t("Atualizar imagens")}</Button>}
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

          {selecting && (
            <div className="selbar zr-selbar" data-nav-group>
              <strong>{t("{n} selecionado(s)", { n: selected.size })}</strong>
              <Button onClick={() => setSelected(new Set(shown.map((e) => e.id)))}>{t("Selecionar todos")}</Button>
              <Button onClick={() => setSelected(new Set())} disabled={!selected.size}>{t("Limpar seleção")}</Button>
              <Select<ZereiStatus | ""> value="" width={220} title={t("Mover para")} disabled={!selected.size}
                onChange={(v) => { if (v) moveSelected(v); }}
                options={[[t("Mover para…"), ""], ...STATUS_ORDER.map((x): [string, ZereiStatus] => [t(STATUS_LABEL[x]), x])]} />
              <Button disabled={!selected.size || !!refreshing} onClick={() => openRefresh([...selected])}
                title={t("Troca capa, logo e plano de fundo dos jogos selecionados pelo primeiro resultado encontrado.")}>{t("Atualizar imagens")}</Button>
              <Button variant="danger" disabled={!selected.size} onClick={() => setConfirmDel(true)}>{t("Excluir")}</Button>
              <Button variant="ghost" onClick={stopSelecting}>{t("Cancelar")}</Button>
            </div>
          )}
          {shown.length === 0 ? (
            <div className="empty"><p>{t("Nenhum jogo com esses filtros.")}</p></div>
          ) : (
            <div className="grid zr-grid" ref={gridRef}>
              {shown.map((e, i) => (
                <button key={e.id} data-focus data-hold data-zerei-id={e.id} data-autofocus={i === 0 ? "" : undefined}
                  className={`card zr-card ${selecting && selected.has(e.id) ? "sel" : ""}`} style={{ "--i": Math.min(i, 12) } as CSSProperties}
                  aria-pressed={selecting ? selected.has(e.id) : undefined}
                  onClick={() => { if (swallowClick()) return; if (selecting) toggleSel(e.id); else setOpenId(e.id); }}
                  onPointerDown={(ev) => pressStart(e.id, ev)} onPointerMove={pressMove} onPointerUp={pressEnd} onPointerCancel={pressEnd}
                  onContextMenu={(ev) => {
                    ev.preventDefault();
                    if (press.current?.fired) return; // toque longo (seleção) não abre o menu
                    const cover = ev.currentTarget.querySelector(".cover") ?? ev.currentTarget;
                    setEntryMenu({ entry: e, x: ev.clientX, y: ev.clientY, from: cover.getBoundingClientRect() });
                  }} onMouseMove={tilt.onMouseMove} onMouseLeave={tilt.onMouseLeave}
                  aria-label={`${e.title}, ${e.platform}, ${statusLabel(e.status)}`}>
                  <div className="cover">
                    {e.cover && <img className="cover-img" src={e.cover} alt="" loading="lazy" decoding="async" draggable={false}
                      onError={(ev) => { ev.currentTarget.style.display = "none"; }} />}
                    {!e.cover && <span>{e.title.charAt(0).toUpperCase()}</span>}
                    <PlatformBadge platform={e.platform} />
                    <i className={`zr-status st-${e.status}`}>{statusLabel(e.status)}</i>
                    {e.completionType === "full" && <i className="zr-100">{t(COMPLETION_LABEL.full)}</i>}
                    {e.rating !== null && <i className="zr-rate">★ {fmtRating(e.rating)}</i>}
                    {selecting && <i className="zr-check" aria-hidden="true">{selected.has(e.id) ? "✓" : ""}</i>}
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
