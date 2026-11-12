import { useCallback, useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { useSession } from "../state/session";
import { useLibrary } from "../state/useLibrary";
import { api, errMsg } from "../lib/api";
import { formatDate } from "../lib/format";
import Button from "../components/Button";
import Seg from "../components/Seg";
import Select from "../components/Select";
import Skeleton from "../components/Skeleton";
import Viewer, { itemSrc } from "../components/Viewer";
import { AboutCapture, CaptureMenu, ConfirmDelete, ICON, MenuAction } from "../components/CaptureUi";
import type { GalleryItem } from "../types";
import { t } from "../lib/i18n";

type Filter = "all" | "recent" | "old" | "fav" | "game";
const MONTH = 30 * 24 * 3600 * 1000;
const ts = (i: GalleryItem) => new Date((i.takenAt ?? i.addedAt).replace(" ", "T") + "Z").getTime();

/**
 * Galeria: capturas copiadas para a pasta do NEXSTAGE. Abrir, favoritar, menu de contexto,
 * "sobre", seleção múltipla e exclusão (somente a cópia gerenciada; originais nunca são apagados).
 */
export default function Gallery() {
  const { profile, toast, galleryIntent, setGalleryIntent } = useSession();
  const p = profile!;
  const { games } = useLibrary(p.id);
  const [items, setItems] = useState<GalleryItem[]>([]);
  const [filter, setFilter] = useState<Filter>("all");
  const [gameFilter, setGameFilter] = useState<number | "">("");
  const [target, setTarget] = useState<number | "">("");
  const [viewer, setViewer] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [menu, setMenu] = useState<{ item: GalleryItem; x?: number; y?: number } | null>(null);
  const [about, setAbout] = useState<GalleryItem | null>(null);
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [confirmIds, setConfirmIds] = useState<number[] | null>(null);
  const [pendingOpen, setPendingOpen] = useState<number | null>(null);

  const load = useCallback(async () => {
    try { setItems(await api.listGallery(p.id)); }
    catch (e) { toast(errMsg(e), "err"); }
    finally { setLoading(false); }
  }, [p.id, toast]);
  useEffect(() => { load(); }, [load]);
  // Captura feita por atalho (inclusive durante um jogo): aparece aqui sem reabrir a página.
  useEffect(() => {
    let un: (() => void) | undefined;
    let cancelled = false;
    listen("screenshot-taken", () => load()).then((f) => (cancelled ? f() : (un = f)));
    return () => { cancelled = true; un?.(); };
  }, [load]);

  // Vindo da Home: já filtra pelo jogo e, se pedido, abre a captura escolhida.
  useEffect(() => {
    if (!galleryIntent) return;
    setFilter("game");
    setGameFilter(galleryIntent.gameId);
    setPendingOpen(galleryIntent.itemId ?? null);
    setGalleryIntent(null);
  }, [galleryIntent, setGalleryIntent]);

  const list = useMemo(() => {
    let l = [...items];
    if (filter === "recent") { const lim = Date.now() - MONTH; l = l.filter((i) => ts(i) >= lim); }
    if (filter === "fav") l = l.filter((i) => i.favorite);
    if (filter === "game") {
      l = l.filter((i) => gameFilter === "" || i.gameId === gameFilter);
      l.sort((a, b) => (a.gameTitle ?? "~").localeCompare(b.gameTitle ?? "~", "pt") || ts(b) - ts(a));
    } else l.sort((a, b) => (filter === "old" ? ts(a) - ts(b) : ts(b) - ts(a)));
    return l;
  }, [items, filter, gameFilter]);

  useEffect(() => {
    if (pendingOpen === null || loading) return;
    const idx = list.findIndex((i) => i.id === pendingOpen);
    if (idx >= 0) setViewer(idx);
    setPendingOpen(null);
  }, [pendingOpen, list, loading]);

  // Mantém o visualizador válido quando a lista muda (ex.: após excluir).
  useEffect(() => {
    if (viewer !== null && viewer >= list.length) setViewer(list.length ? list.length - 1 : null);
  }, [list.length, viewer]);
  // Remove da seleção o que deixou de existir.
  useEffect(() => {
    setSelected((s) => { const ids = new Set(items.map((i) => i.id)); const n = new Set([...s].filter((x) => ids.has(x))); return n.size === s.size ? s : n; });
  }, [items]);

  // ESC sai do modo de seleção antes de qualquer outra navegação (só se não houver janela por cima).
  useEffect(() => {
    if (!selecting) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(".overlay, .ctx-root, .pick-root, .kb-root, .sm-root")) return;
      e.stopImmediatePropagation();
      setSelecting(false);
      setSelected(new Set());
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [selecting]);

  // Botão Menu/Options do controle: menu de contexto da captura em foco.
  useEffect(() => {
    const onCtx = () => {
      if (viewer !== null || menu || about || confirmIds) return;
      const el = document.activeElement as HTMLElement | null;
      const id = Number(el?.closest<HTMLElement>("[data-capture-id]")?.dataset.captureId);
      const item = items.find((i) => i.id === id);
      if (item) setMenu({ item });
    };
    window.addEventListener("nexstage:context", onCtx);
    return () => window.removeEventListener("nexstage:context", onCtx);
  }, [viewer, menu, about, confirmIds, items]);

  async function importFiles() {
    try {
      const sel = await open({ multiple: true, filters: [{ name: "Imagens", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "gif"] }] });
      const paths = Array.isArray(sel) ? sel : typeof sel === "string" ? [sel] : [];
      if (!paths.length) return;
      const r = await api.importScreenshots(p.id, target === "" ? null : target, paths);
      toast(t("{imported} importada(s){v1}{v2}.", { imported: r.imported, v1: r.duplicates ? `, ${r.duplicates} já existia(m)` : "", v2: r.failed.length ? `, ${r.failed.length} com erro` : "" }), r.failed.length && !r.imported ? "err" : "ok");
      if (r.failed.length) toast(r.failed[0], "err");
      await load();
    } catch (e) { toast(errMsg(e), "err"); }
  }

  const fav = async (i: GalleryItem) => {
    try { await api.setGalleryFavorite(p.id, i.id, !i.favorite); await load(); } catch (e) { toast(errMsg(e), "err"); }
  };
  const toggleSel = (id: number) => setSelected((s) => { const n = new Set(s); n.has(id) ? n.delete(id) : n.add(id); return n; });
  const removeIds = async (ids: number[]) => {
    try {
      const n = await api.deleteGalleryItems(p.id, ids);
      toast(t("{n} captura(s) excluída(s). Os arquivos originais foram mantidos.", { n }));
      setSelected(new Set());
      setSelecting(false);
      await load();
    } catch (e) { toast(errMsg(e), "err"); }
  };

  const menuActions = (item: GalleryItem): MenuAction[] => [
    { label: t("Abrir captura"), icon: ICON.open, run: () => setViewer(list.findIndex((i) => i.id === item.id)) },
    { label: item.favorite ? t("Desfavoritar") : t("Favoritar"), icon: ICON.star, run: () => fav(item) },
    { label: selected.has(item.id) ? t("Desmarcar") : t("Selecionar"), icon: ICON.select, run: () => { setSelecting(true); toggleSel(item.id); } },
    { label: t("Sobre esta captura"), icon: ICON.info, run: () => setAbout(item) },
    { label: t("Excluir captura"), icon: ICON.trash, danger: true, run: () => setConfirmIds([item.id]) },
  ];

  if (loading) return <Skeleton count={8} />;

  return (
    <>
      <div className="page-head">
        <h1>{t("Galeria")}</h1>
        <div className="row">
          <Select<number | ""> value={target} width={240} title={t("Importar para o jogo")} onChange={setTarget}
            options={[[t("Importar sem jogo"), ""], ...games.map((g): [string, number] => [g.title, g.id])]} />
          <Button variant="primary" onClick={importFiles}>{t("Importar capturas")}</Button>
          <Button onClick={() => api.openCapturesFolder().catch((e) => toast(errMsg(e), "err"))}>{t("Pasta de capturas")}</Button>
          {!selecting && items.length > 0 && <Button onClick={() => setSelecting(true)}>{t("Selecionar")}</Button>}
        </div>
      </div>
      <div className="row" style={{ marginBottom: 18 }}>
        <Seg<Filter> value={filter} onChange={setFilter}
          opts={[[t("Todas"), "all"], [t("Recentes"), "recent"], [t("Antigas"), "old"], [t("Favoritas"), "fav"], [t("Por jogo"), "game"]]} />
        {filter === "game" && (
          <Select<number | ""> value={gameFilter} width={240} title={t("Filtrar por jogo")} onChange={setGameFilter}
            options={[[t("Todos os jogos (A–Z)"), ""], ...games.filter((g) => items.some((i) => i.gameId === g.id)).map((g): [string, number] => [g.title, g.id])]} />
        )}
      </div>

      {selecting && (
        <div className="selbar">
          <strong>{selected.size} selecionada{selected.size === 1 ? "" : "s"}</strong>
          <Button onClick={() => setSelected(new Set(list.map((i) => i.id)))}>{t("Selecionar todas")}</Button>
          <Button onClick={() => setSelected(new Set())} disabled={!selected.size}>{t("Limpar seleção")}</Button>
          <Button variant="danger" disabled={!selected.size} onClick={() => setConfirmIds([...selected])}>{t("Excluir selecionadas")}</Button>
          <Button variant="ghost" onClick={() => { setSelecting(false); setSelected(new Set()); }}>{t("Cancelar")}</Button>
        </div>
      )}

      {items.length === 0 ? (
        <div className="empty"><p>{t("Nenhuma captura ainda. Importe imagens: o NEXSTAGE guarda uma cópia e mantém seus arquivos originais intactos.")}</p></div>
      ) : list.length === 0 ? (
        <div className="empty"><p>{t("Nenhuma captura neste filtro.")}</p></div>
      ) : (
        <div className="thumbs">
          {list.map((i, idx) => (
            <button key={i.id} data-focus data-capture-id={i.id} data-autofocus={idx === 0 ? "" : undefined} className={`thumb ${selected.has(i.id) ? "sel" : ""}`}
              aria-pressed={selecting ? selected.has(i.id) : undefined}
              onClick={() => (selecting ? toggleSel(i.id) : setViewer(idx))}
              onContextMenu={(e) => {
                e.preventDefault();
                const r = e.currentTarget.getBoundingClientRect();
                setMenu(e.clientX || e.clientY ? { item: i, x: e.clientX, y: e.clientY } : { item: i, x: r.left + 40, y: r.top + 40 });
              }}>
              {i.exists ? <img src={itemSrc(i)} alt="" loading="lazy" /> : <div className="thumb-missing">{t("Arquivo ausente")}</div>}
              {i.favorite && <b className="fav">★</b>}
              {selecting && <i className="check" aria-hidden="true">{selected.has(i.id) ? "✓" : ""}</i>}
              <span>{i.gameTitle ?? t("Sem jogo")}</span>
              <small>{formatDate(i.takenAt ?? i.addedAt)}</small>
            </button>
          ))}
        </div>
      )}

      {viewer !== null && list[viewer] && (
        <Viewer items={list} index={viewer} onIndex={setViewer} onClose={() => setViewer(null)} onFavorite={fav}
          onDelete={(i) => removeIds([i.id])} />
      )}
      {menu && <CaptureMenu actions={menuActions(menu.item)} x={menu.x} y={menu.y} onClose={() => setMenu(null)} />}
      {about && <AboutCapture item={about} onClose={() => setAbout(null)} />}
      {confirmIds && <ConfirmDelete count={confirmIds.length} onClose={() => setConfirmIds(null)}
        onConfirm={() => { const ids = confirmIds; setConfirmIds(null); removeIds(ids); }} />}
    </>
  );
}
