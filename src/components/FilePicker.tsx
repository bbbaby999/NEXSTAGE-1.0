import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import Select from "./Select";
import { api, errMsg } from "../lib/api";
import { finishPick, IMAGE_EXTS, usePickRequest, type PickRequest } from "../lib/filePicker";
import { formatBytes } from "../lib/format";
import { LAYER_SEL } from "../lib/nav";
import { play } from "../lib/sound";
import { currentLang, t, tk } from "../lib/i18n";
import type { FsEntry, FsListing, FsPlaces } from "../types";
import { ALL_FORMATS } from "../data/emulatorPresets";

/* =====================================================================================================
 * Navegador de arquivos do NEXSTAGE (substitui o seletor do Windows; ver lib/filePicker.ts).
 *
 * Layout do Explorador: barra superior (Voltar, Pasta acima, caminho), lateral (Acesso rápido, Este
 * computador, Recentes), conteúdo em lista ou grade (miniaturas nas imagens) e rodapé (tipo de arquivo,
 * nome ao salvar, Cancelar/confirmar).
 *
 * Controle: direcional move · Ⓐ abre a pasta / escolhe o arquivo · Ⓑ pasta acima (em "Este computador",
 * fecha; segurado, cancela de uma vez) · Ⓧ/▢ favorita a pasta em foco (ou a atual) · Ⓨ/△ vai aos botões do
 * rodapé (Cancelar / Selecionar esta pasta) e volta · L1/R1 lateral ↔ conteúdo · L2/R2 página · ☰ confirma.
 * O rodapé é um painel à parte: pelo direcional só se chega a ele saindo da última linha da lista, por isso Ⓨ.
 * Cada tela (purpose) lembra a última pasta aberta; as favoritas aparecem no Acesso rápido.
 * Teclado: Backspace = pasta acima; Esc igual ao Ⓑ.
 * ===================================================================================================== */

const ROOT = ""; // "Este computador"
const PLACE_LABEL: Record<string, string> = {
  desktop: tk("Área de trabalho"), documents: tk("Documentos"), downloads: tk("Downloads"), pictures: tk("Imagens"),
  videos: tk("Vídeos"), music: tk("Músicas"), home: tk("Pasta pessoal"),
};

/* ---------- Memória (por computador: localStorage, fora do backup — os caminhos são desta máquina) ---------- */
const LS_LAST = "nx.fp.last.";
const LS_RECENT = "nx.fp.recent";
const LS_VIEW = "nx.fp.view";
const LS_FAVS = "nx.fp.favs";
const lsGet = (k: string) => { try { return localStorage.getItem(k); } catch { return null; } };
const lsSet = (k: string, v: string) => { try { localStorage.setItem(k, v); } catch { /* sem armazenamento */ } };
function recentList(): string[] {
  try { const v = JSON.parse(lsGet(LS_RECENT) ?? "[]"); return Array.isArray(v) ? v.filter((x) => typeof x === "string").slice(0, 6) : []; } catch { return []; }
}
const parseFavs = (raw: string | null | undefined): string[] => {
  try { const v = JSON.parse(raw ?? "[]"); return Array.isArray(v) ? v.filter((x) => typeof x === "string") : []; } catch { return []; }
};
/** Cópia local (abre o navegador já com as favoritas, sem esperar o backend). */
function favList(): string[] { return parseFavs(lsGet(LS_FAVS)); }
/**
 * Favoritas: UMA lista para todas as telas que usam o navegador de arquivos, guardada nas configurações do
 * app (`app_settings.file_picker_favs`, entra no backup). O localStorage fica só como cópia para abrir rápido;
 * favoritas antigas, que só existiam nele, são levadas para as configurações na primeira abertura.
 */
const FAVS_KEY = "file_picker_favs";
async function loadFavs(): Promise<string[]> {
  const saved = await api.getAppSetting(FAVS_KEY).catch(() => undefined);
  if (saved === undefined) return favList(); // backend indisponível: fica a cópia local
  const local = favList();
  let list = saved === null ? [] : parseFavs(saved);
  const missing = local.filter((f) => !list.some((x) => samePath(x, f)));
  if (saved === null && missing.length) { list = [...list, ...missing]; saveFavs(list); }
  lsSet(LS_FAVS, JSON.stringify(list));
  return list;
}
function saveFavs(list: string[]) {
  lsSet(LS_FAVS, JSON.stringify(list));
  api.setAppSetting(FAVS_KEY, JSON.stringify(list)).catch(() => { /* fica a cópia local */ });
}
const samePath = (a: string, b: string) => a.replace(/[\\/]+$/, "").toLowerCase() === b.replace(/[\\/]+$/, "").toLowerCase();
function remember(purpose: string | undefined, dir: string) {
  if (!dir) return;
  if (purpose) lsSet(LS_LAST + purpose, dir);
  lsSet(LS_RECENT, JSON.stringify([dir, ...recentList().filter((d) => d.toLowerCase() !== dir.toLowerCase())].slice(0, 6)));
}

/* ---------- Caminhos (Windows) ---------- */
const join = (dir: string, name: string) => (/[\\/]$/.test(dir) ? dir + name : `${dir}\\${name}`);
const dirOf = (p: string) => { const i = Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/")); return i <= 2 ? p.slice(0, 3) : p.slice(0, i); };
const baseOf = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
/** Trechos clicáveis do caminho: "D:\Jogos\X" → [["D:\", "D:\"], ["Jogos", "D:\Jogos"], ["X", "D:\Jogos\X"]]. */
function crumbs(path: string): [string, string][] {
  if (!path) return [];
  const unc = path.startsWith("\\\\");
  const parts = path.split(/[\\/]/).filter(Boolean);
  const out: [string, string][] = [];
  let acc = "";
  parts.forEach((part, i) => {
    if (i === 0) acc = unc ? `\\\\${part}` : `${part}\\`;
    else acc = join(acc, part);
    out.push([i === 0 && !unc ? `${part}\\` : part, acc]);
  });
  return out;
}

/* ---------- Ícones ---------- */
type IconKind = "star" | "folder" | "file" | "image" | "exe" | "disc" | "drive" | "usb" | "pc" | "up" | "back" | "list" | "grid" | "check" | "place";
const ICON_PATH: Record<IconKind, string> = {
  star: "M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9L12 3.5Z",
  folder: "M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2h8.5A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5v-11Z",
  file: "M6 3h8l4 4v14H6V3ZM14 3v4h4",
  image: "M4 5h16v14H4V5ZM4 16l5-5 4 4 3-3 4 4M15.5 9.5h0",
  exe: "M4 5h16v14H4V5ZM4 9h16M8 13l2 2-2 2M12 17h4",
  disc: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18ZM12 14a2 2 0 1 0 0-4 2 2 0 0 0 0 4Z",
  drive: "M3 14h18v5H3v-5ZM5 14l2-8h10l2 8M17 16.5h0",
  usb: "M9 3h6v6H9V3ZM7 9h10v12H7V9ZM11 5h0M13 5h0",
  pc: "M3 5h18v11H3V5ZM8 20h8M12 16v4",
  up: "M12 19V5M5 12l7-7 7 7",
  back: "M19 12H5M12 19l-7-7 7-7",
  list: "M8 6h13M8 12h13M8 18h13M3 6h0M3 12h0M3 18h0",
  grid: "M4 4h7v7H4V4ZM13 4h7v7h-7V4ZM4 13h7v7H4v-7ZM13 13h7v7h-7v-7Z",
  check: "M5 12.5l4.5 4.5L19 7.5",
  place: "M12 21s-7-6.1-7-11a7 7 0 1 1 14 0c0 4.9-7 11-7 11ZM12 12.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z",
};
function Icon({ k }: { k: IconKind }) {
  return <svg className={`fp-ico fp-ico-${k}`} viewBox="0 0 24 24" aria-hidden="true"><path d={ICON_PATH[k]} /></svg>;
}
const DISC_EXTS = new Set<string>([...ALL_FORMATS, "cso", "img"]);
const EXE_EXTS = new Set(["exe", "bat", "cmd", "lnk"]);
const iconFor = (e: FsEntry): IconKind => (e.dir ? "folder" : IMAGE_EXTS.includes(e.ext) ? "image" : EXE_EXTS.has(e.ext) ? "exe" : DISC_EXTS.has(e.ext) ? "disc" : "file");

/* ---------- Miniaturas: carregadas quando aparecem, no máximo 4 por vez, com cache ---------- */
const thumbCache = new Map<string, string | null>();
const thumbQueue: (() => void)[] = [];
let thumbBusy = 0;
function runThumbs() {
  while (thumbBusy < 4 && thumbQueue.length) { thumbBusy++; thumbQueue.shift()!(); }
}
function Thumb({ e }: { e: FsEntry }) {
  const key = `${e.path}|${e.modified}`;
  const [src, setSrc] = useState<string | null | undefined>(thumbCache.get(key));
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (src !== undefined || !ref.current) return;
    let dead = false, queued = false;
    const job = () => {
      if (dead) { thumbBusy--; runThumbs(); return; }
      api.fsThumb(e.path, 320).then((d) => d, () => null).then((d) => {
        if (thumbCache.size > 400) thumbCache.delete(thumbCache.keys().next().value!);
        thumbCache.set(key, d);
        if (!dead) setSrc(d);
        thumbBusy--; runThumbs();
      });
    };
    const io = new IntersectionObserver(([en]) => {
      if (!en.isIntersecting || queued) return;
      queued = true; io.disconnect(); thumbQueue.push(job); runThumbs();
    }, { rootMargin: "200px" });
    io.observe(ref.current);
    return () => { dead = true; io.disconnect(); const i = thumbQueue.indexOf(job); if (i >= 0) thumbQueue.splice(i, 1); };
  }, [key, src, e.path]);
  return (
    <span className="fp-thumb" ref={ref}>
      {src ? <img src={src} alt="" draggable={false} /> : <Icon k="image" />}
    </span>
  );
}

const fmtDate = (s: number) => (s ? new Date(s * 1000).toLocaleDateString(currentLang(), { day: "2-digit", month: "2-digit", year: "numeric" }) : "");

/* ===================================================================================================== */

export default function FilePickerHost() {
  const req = usePickRequest();
  return req ? <FilePicker key={req.id} req={req} /> : null;
}

function FilePicker({ req }: { req: PickRequest }) {
  const { opts, mode } = req;
  const dirMode = !!opts.directory && mode === "open";
  const multiple = !!opts.multiple && mode === "open" && !dirMode;
  const browse = !!opts.browse && mode === "open";
  const filters = opts.filters?.length ? opts.filters : [{ name: t("Todos os arquivos"), extensions: ["*"] }];
  const imagesOnly = filters[0].extensions.every((x) => IMAGE_EXTS.includes(x.toLowerCase()));
  const title = opts.title ?? (browse ? t("Explorar pasta") : dirMode ? t("Escolha uma pasta") : mode === "save" ? t("Salvar como") : multiple ? t("Escolha os arquivos") : t("Escolha um arquivo"));

  const [places, setPlaces] = useState<FsPlaces | null>(null);
  const [path, setPath] = useState<string | null>(null); // null = decidindo a pasta inicial
  const [listing, setListing] = useState<FsListing | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filterIdx, setFilterIdx] = useState(0);
  const [view, setView] = useState<"list" | "grid">(() => (imagesOnly ? "grid" : lsGet(LS_VIEW) === "grid" ? "grid" : "list"));
  const [showHidden, setShowHidden] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [name, setName] = useState(() => (mode === "save" && opts.defaultPath ? baseOf(opts.defaultPath) : ""));
  const [typing, setTyping] = useState<string | null>(null);
  const [overwrite, setOverwrite] = useState<string | null>(null);
  const [recent] = useState(recentList);
  const [favs, setFavs] = useState(favList);
  useEffect(() => { let live = true; loadFavs().then((l) => { if (live) setFavs(l); }); return () => { live = false; }; }, []);
  const back = useRef<string[]>([]);
  const focusAfter = useRef<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const mainRef = useRef<HTMLDivElement>(null);
  const lastMain = useRef<string | null>(null);

  const exts = filters[Math.min(filterIdx, filters.length - 1)].extensions.map((x) => x.toLowerCase());

  /* ---------- última pasta aberta, por tela: gravada a cada pasta visitada (mesmo se cancelar) ---------- */
  useEffect(() => {
    if (path && opts.purpose) lsSet(LS_LAST + opts.purpose, path);
  }, [path, opts.purpose]);

  /* ---------- favoritas (Acesso rápido) ---------- */
  const isFav = (p: string) => favs.some((f) => samePath(f, p));
  const toggleFav = (p: string) => {
    if (!p) return;
    const on = !isFav(p);
    const next = on ? [...favs, p] : favs.filter((f) => !samePath(f, p));
    setFavs(next);
    saveFavs(next);
    play(on ? "confirm" : "back");
  };
  /** Ⓧ/▢: a pasta em foco (no conteúdo ou na lateral); sem pasta em foco, a pasta atual. */
  const favTarget = (): string | null => {
    const k = document.activeElement instanceof HTMLElement ? document.activeElement.dataset.focusKey : undefined;
    if (k?.startsWith("side:")) return k.slice(5) || null;
    const e = k ? listing?.entries.find((x) => x.path === k) : undefined;
    if (e?.dir) return e.path;
    if (k && path === ROOT) return k; // unidade/pasta especial em "Este computador"
    return path || null;
  };

  /* ---------- pasta inicial ---------- */
  useEffect(() => {
    let dead = false;
    (async () => {
      const pl = await api.fsPlaces().catch(() => ({ places: [], drives: [] }) as FsPlaces);
      if (dead) return;
      setPlaces(pl);
      const tryDir = async (p: string | null | undefined) => (p && (await api.fsPathInfo(p).catch(() => null))?.dir ? p : null);
      let start: string | null = null;
      if (opts.defaultPath) {
        const info = await api.fsPathInfo(opts.defaultPath).catch(() => null);
        if (info?.dir) start = opts.defaultPath;
        else if (/[\\/]/.test(opts.defaultPath)) {
          start = await tryDir(dirOf(opts.defaultPath));
          if (start && info?.exists) focusAfter.current = opts.defaultPath;
        }
      }
      start ??= await tryDir(opts.purpose ? lsGet(LS_LAST + opts.purpose) : null);
      start ??= imagesOnly ? pl.places.find((p) => p.id === "pictures")?.path ?? null : null;
      start ??= mode === "save" ? pl.places.find((p) => p.id === "documents")?.path ?? null : null;
      if (!dead) setPath(start ?? ROOT);
    })();
    return () => { dead = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /* ---------- conteúdo da pasta ---------- */
  useEffect(() => {
    if (path === null) return;
    setError(null);
    if (path === ROOT) { setListing(null); setLoading(false); return; }
    let dead = false;
    setLoading(true);
    api.fsList(path, exts, dirMode, showHidden)
      .then((l) => { if (!dead) setListing(l); })
      .catch((e) => { if (!dead) { setListing(null); setError(errMsg(e)); } })
      .finally(() => { if (!dead) setLoading(false); });
    return () => { dead = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, filterIdx, showHidden]);

  /* ---------- foco depois de carregar: o item pedido (ex.: a pasta de onde viemos) ou o 1º ---------- */
  useEffect(() => {
    if (loading || path === null) return;
    const id = requestAnimationFrame(() => {
      const main = mainRef.current;
      if (!main) return;
      const want = focusAfter.current;
      focusAfter.current = null;
      // quem está no rodapé (nome do arquivo, tipo de arquivo) continua lá; o resto vai para o conteúdo
      const act = document.activeElement;
      if (!want && act instanceof HTMLElement && (act.closest(".fp-foot") || act.tagName === "INPUT")) return;
      const el = (want ? main.querySelector<HTMLElement>(`[data-focus-key="${CSS.escape(want)}"]`) : null)
        ?? main.querySelector<HTMLElement>("[data-focus]")
        ?? rootRef.current?.querySelector<HTMLElement>(".fp-side [data-focus]");
      el?.focus({ preventScroll: true });
      el?.scrollIntoView({ block: "nearest" });
    });
    return () => cancelAnimationFrame(id);
  }, [loading, listing, path]);

  const go = useCallback((p: string, opt: { focus?: string; push?: boolean } = {}) => {
    if (path !== null && opt.push !== false && p !== path) back.current.push(path);
    focusAfter.current = opt.focus ?? null;
    setSelected((s) => (multiple ? s : []));
    setTyping(null);
    setPath(p);
  }, [path, multiple]);

  const up = useCallback(() => {
    if (path === null || path === ROOT) return false;
    go(listing?.parent ?? (crumbs(path).length > 1 ? dirOf(path) : ROOT), { focus: path });
    play("back");
    return true;
  }, [path, listing, go]);

  const goBack = () => {
    const p = back.current.pop();
    if (p !== undefined) { focusAfter.current = path; setPath(p); play("back"); }
  };

  /* ---------- confirmar ---------- */
  const done = (paths: string[] | null, dir?: string) => {
    if (paths) remember(opts.purpose, dir ?? (path || ""));
    finishPick(paths);
  };
  const cancel = () => done(null);

  const saveTarget = (): string | null => {
    const n = name.trim();
    if (!n || !path) return null;
    const ext0 = exts.find((x) => x !== "*");
    const withExt = ext0 && !n.toLowerCase().endsWith(`.${ext0}`) && !exts.some((x) => n.toLowerCase().endsWith(`.${x}`)) ? `${n}.${ext0}` : n;
    return /[\\/]/.test(withExt) ? withExt : join(path, withExt);
  };

  async function confirm() {
    if (path === null || browse) return;
    if (dirMode) { if (path) done([path]); return; }
    if (mode === "save") {
      const target = saveTarget();
      if (!target) return;
      const info = await api.fsPathInfo(target).catch(() => null);
      if (info?.dir) { go(target); return; }
      if (info?.exists && overwrite !== target) { setOverwrite(target); return; }
      done([target], dirOf(target));
      return;
    }
    if (multiple) {
      if (selected.length) { done(selected); return; }
      const f = document.activeElement instanceof HTMLElement ? document.activeElement.dataset.focusKey : undefined;
      const e = listing?.entries.find((x) => x.path === f && !x.dir);
      if (e) done([e.path]);
    }
  }

  function activate(e: FsEntry) {
    if (e.dir) { go(e.path); return; }
    if (browse) return; // só navegação: arquivos não são escolhidos nem abertos
    if (mode === "save") { setName(e.name); setOverwrite(null); return; }
    if (multiple) { setSelected((s) => (s.includes(e.path) ? s.filter((x) => x !== e.path) : [...s, e.path])); return; }
    done([e.path]);
  }

  /* ---------- digitar caminho ---------- */
  async function submitTyped() {
    const v = (typing ?? "").trim().replace(/^"|"$/g, "");
    if (!v) { setTyping(null); return; }
    const info = await api.fsPathInfo(v).catch(() => null);
    if (info?.dir) { go(v); return; }
    if (info?.exists && !dirMode) {
      if (mode === "save") { go(dirOf(v)); setName(baseOf(v)); return; }
      done([v], dirOf(v));
      return;
    }
    setError(t("Caminho não encontrado: {path}", { path: v }));
    setTyping(null);
  }

  /* ---------- controle e teclado ---------- */
  const topmost = () => {
    const ov = rootRef.current?.closest(".overlay");
    const layers = document.querySelectorAll(LAYER_SEL);
    return !!ov && layers[layers.length - 1] === ov;
  };
  const focusArea = (area: "side" | "main" | "foot") => {
    const root = rootRef.current;
    if (!root) return;
    if (area === "foot") {
      // botão principal do rodapé (Selecionar esta pasta / Salvar / Abrir), senão Cancelar/Fechar
      const btns = Array.from(root.querySelectorAll<HTMLElement>(".fp-actions button:not([disabled])"));
      (btns[btns.length - 1] ?? null)?.focus();
    } else if (area === "main") {
      const main = mainRef.current;
      const el = (lastMain.current ? main?.querySelector<HTMLElement>(`[data-focus-key="${CSS.escape(lastMain.current)}"]`) : null) ?? main?.querySelector<HTMLElement>("[data-focus]");
      el?.focus();
    } else {
      const el = root.querySelector<HTMLElement>(".fp-side [aria-current='true']") ?? root.querySelector<HTMLElement>(".fp-side [data-focus]");
      el?.focus();
    }
    play("move");
  };
  const page = (dir: 1 | -1) => {
    const main = mainRef.current;
    if (!main) return;
    const items = Array.from(main.querySelectorAll<HTMLElement>("[data-focus]"));
    if (!items.length) return;
    const cur = items.indexOf(document.activeElement as HTMLElement);
    const top0 = items[0].offsetTop;
    const cols = Math.max(1, items.filter((el) => el.offsetTop === top0).length);
    const rows = Math.max(1, Math.floor(main.clientHeight / Math.max(1, items[0].offsetHeight)) - 1);
    const next = items[Math.min(items.length - 1, Math.max(0, (cur < 0 ? 0 : cur) + dir * rows * cols))];
    next.focus({ preventScroll: true });
    next.scrollIntoView({ block: dir > 0 ? "end" : "start" });
    play("move");
  };

  const handlers = useRef({ up, confirm, page, focusArea, cancel, toggleFav, favTarget });
  handlers.current = { up, confirm, page, focusArea, cancel, toggleFav, favTarget };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!topmost()) return;
      const inField = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (e.key === "Escape") {
        e.preventDefault(); e.stopImmediatePropagation();
        if (inField) { setTyping(null); (e.target as HTMLElement).blur(); handlers.current.focusArea("main"); return; }
        if (!handlers.current.up()) { play("back"); handlers.current.cancel(); }
      } else if (e.key === "Backspace" && !inField) {
        e.preventDefault();
        handlers.current.up();
      }
    };
    const onBtn = (ev: Event) => {
      if (!topmost()) return;
      const b = (ev as CustomEvent<string>).detail;
      const h = handlers.current;
      if (b === "x") { const target = h.favTarget(); if (target) h.toggleFav(target); }
      else if (b === "y") {
        // Ⓨ/△: alterna entre o conteúdo e os botões do rodapé
        const inFoot = document.activeElement instanceof Element && !!document.activeElement.closest(".fp-foot");
        h.focusArea(inFoot ? "main" : "foot");
      }
      else if (b === "l1") h.focusArea("side");
      else if (b === "r1") h.focusArea("main");
      else if (b === "l2") h.page(-1);
      else if (b === "r2") h.page(1);
      else if (b === "menu") { h.confirm(); }
      else return;
      ev.preventDefault();
    };
    // segurar Ⓑ/○: cancela de uma vez (o toque só sobe uma pasta)
    const onBackHold = (ev: Event) => {
      if (!topmost()) return;
      ev.preventDefault();
      play("back");
      handlers.current.cancel();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("nexstage:button", onBtn);
    window.addEventListener("nexstage:back-hold", onBackHold);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("nexstage:button", onBtn);
      window.removeEventListener("nexstage:back-hold", onBackHold);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /* ---------- desenho ---------- */
  const entries = listing?.entries ?? [];
  const here = path ?? ROOT;
  const placeFor = useMemo(() => {
    const p = here.toLowerCase();
    return places?.places.find((x) => x.path.toLowerCase() === p)?.path
      ?? places?.drives.find((d) => d.mount.toLowerCase() === p)?.mount
      ?? favs.find((f) => samePath(f, here)) ?? null;
  }, [here, places, favs]);
  const confirmLabel = dirMode ? t("Selecionar esta pasta") : mode === "save" ? t("Salvar") : multiple
    ? (selected.length ? t("Abrir ({n})", { n: selected.length }) : t("Abrir")) : t("Abrir");
  const canConfirm = dirMode ? !!path : mode === "save" ? !!path && !!name.trim() : multiple ? selected.length > 0 : false;

  const sideItem = (key: string, label: string, icon: IconKind, target: string, sub?: string) => (
    <button key={key} type="button" data-focus data-focus-key={`side:${target}`} className={`fp-side-item ${placeFor === target || (target === ROOT && here === ROOT) ? "on" : ""}`}
      aria-current={placeFor === target || (target === ROOT && here === ROOT) ? "true" : undefined} onClick={() => go(target)}>
      <Icon k={icon} /><span><b>{label}</b>{sub && <small>{sub}</small>}</span>
    </button>
  );

  const driveTitle = (d: FsPlaces["drives"][number]) => (d.label ? `${d.label} (${d.mount.replace(/\\$/, "")})` : d.mount.replace(/\\$/, ""));

  return (
    <Modal title={title} onClose={cancel} xl portal className="fp-modal">
      <div className="fp" ref={rootRef}>
        {/* ---------- barra superior ---------- */}
        <div className="fp-bar" data-nav-group>
          <button type="button" data-focus className="fp-tool" onClick={goBack} disabled={!back.current.length} aria-label={t("Voltar")} title={t("Voltar")}><Icon k="back" /></button>
          <button type="button" data-focus className="fp-tool" onClick={() => up()} disabled={here === ROOT} aria-label={t("Pasta acima")} title={t("Pasta acima")}><Icon k="up" /></button>
          {typing !== null ? (
            <form className="fp-path-edit" onSubmit={(e) => { e.preventDefault(); submitTyped(); }}>
              <input autoFocus value={typing} onChange={(e) => setTyping(e.target.value)} spellCheck={false} aria-label={t("Caminho")}
                placeholder={t("Digite o caminho (ex.: D:\\Jogos) e pressione Enter")} />
            </form>
          ) : (
            <div className="fp-crumbs">
              <button type="button" data-focus className="fp-crumb" onClick={() => go(ROOT)}><Icon k="pc" />{t("Este computador")}</button>
              {crumbs(here).map(([label, p]) => (
                <span key={p} className="fp-crumb-wrap"><i aria-hidden="true">›</i>
                  <button type="button" data-focus className="fp-crumb" onClick={() => go(p)}>{label}</button>
                </span>
              ))}
            </div>
          )}
          <button type="button" data-focus className="fp-tool wide" onClick={() => setTyping(here)}>{t("Digitar caminho")}</button>
          <button type="button" data-focus className={`fp-tool fp-fav-tool ${here && isFav(here) ? "on" : ""}`} disabled={!here} aria-pressed={!!here && isFav(here)}
            onClick={() => toggleFav(here)} title={here && isFav(here) ? t("Remover dos favoritos") : t("Favoritar pasta")}>
            <Icon k="star" />{here && isFav(here) ? t("Favorita") : t("Favoritar")}</button>
          <button type="button" data-focus className={`fp-tool ${showHidden ? "on" : ""}`} aria-pressed={showHidden} onClick={() => setShowHidden((v) => !v)}>{t("Ocultos")}</button>
          <button type="button" data-focus className="fp-tool" aria-label={view === "grid" ? t("Lista") : t("Grade")} title={view === "grid" ? t("Lista") : t("Grade")}
            onClick={() => setView((v) => { const n = v === "grid" ? "list" : "grid"; lsSet(LS_VIEW, n); return n; })}><Icon k={view === "grid" ? "list" : "grid"} /></button>
        </div>

        <div className="fp-body">
          {/* ---------- lateral ---------- */}
          <aside className="fp-side" data-nav-group aria-label={t("Locais")}>
            <h4>{t("Acesso rápido")}</h4>
            {places?.places.map((p) => sideItem(p.id, t(PLACE_LABEL[p.id]), "place", p.path))}
            {favs.map((f) => sideItem(`fav:${f}`, baseOf(f), "star", f, f))}
            <h4>{t("Este computador")}</h4>
            {places?.drives.map((d) => sideItem(d.mount, driveTitle(d), d.removable ? "usb" : "drive", d.mount,
              d.totalBytes ? t("{free} livres de {total}", { free: formatBytes(d.freeBytes), total: formatBytes(d.totalBytes) }) : undefined))}
            {recent.length > 0 && <h4>{t("Recentes")}</h4>}
            {recent.map((r) => sideItem(`r:${r}`, baseOf(r), "folder", r, r))}
          </aside>

          {/* ---------- conteúdo ---------- */}
          <div className={`fp-main ${view}`} ref={mainRef} data-nav-group
            onFocus={(e) => { const k = (e.target as HTMLElement).dataset?.focusKey; if (k) lastMain.current = k; }}>
            {path === null || (loading && !listing) ? (
              <p className="fp-msg">{t("Carregando…")}</p>
            ) : error ? (
              <div className="fp-msg err"><p>{error}</p><Button onClick={() => up() || go(ROOT)}>{t("Pasta acima")}</Button></div>
            ) : here === ROOT ? (
              <div className="fp-root">
                <h4>{t("Unidades")}</h4>
                <div className="fp-drives">
                  {places?.drives.map((d) => {
                    const used = d.totalBytes ? 1 - d.freeBytes / d.totalBytes : 0;
                    return (
                      <button key={d.mount} type="button" data-focus data-focus-key={d.mount} className="fp-drive" onClick={() => go(d.mount)}>
                        <Icon k={d.removable ? "usb" : "drive"} />
                        <span className="fp-drive-text">
                          <b>{driveTitle(d)}</b>
                          {d.totalBytes > 0 && <><span className={`fp-bar-use ${used > 0.9 ? "full" : ""}`}><i style={{ width: `${Math.round(used * 100)}%` }} /></span>
                            <small>{t("{free} livres de {total}", { free: formatBytes(d.freeBytes), total: formatBytes(d.totalBytes) })}</small></>}
                        </span>
                      </button>
                    );
                  })}
                </div>
                {favs.length > 0 && <>
                  <h4>{t("Favoritas")}</h4>
                  <div className="fp-drives">
                    {favs.map((f) => (
                      <button key={f} type="button" data-focus data-focus-key={f} className="fp-drive place fp-fav" onClick={() => go(f)}>
                        <Icon k="star" /><span className="fp-drive-text"><b>{baseOf(f)}</b><small>{f}</small></span>
                      </button>
                    ))}
                  </div>
                </>}
                <h4>{t("Pastas")}</h4>
                <div className="fp-drives">
                  {places?.places.map((p) => (
                    <button key={p.id} type="button" data-focus data-focus-key={p.path} className="fp-drive place" onClick={() => go(p.path)}>
                      <Icon k="folder" /><span className="fp-drive-text"><b>{t(PLACE_LABEL[p.id])}</b><small>{p.path}</small></span>
                    </button>
                  ))}
                </div>
              </div>
            ) : entries.length === 0 ? (
              <p className="fp-msg">{dirMode ? t("Nenhuma subpasta aqui.") : t("Nenhum arquivo compatível nesta pasta.")}</p>
            ) : (
              <>
                {view === "list" && (
                  <div className="fp-head" aria-hidden="true"><span>{t("Nome")}</span><span>{t("Modificado")}</span><span>{t("Tamanho")}</span></div>
                )}
                {entries.map((e) => {
                  const isSel = selected.includes(e.path) || (mode === "save" && !e.dir && e.name === name);
                  const thumb = view === "grid" && !e.dir && IMAGE_EXTS.includes(e.ext) && e.ext !== "avif";
                  return (
                    <button key={e.path} type="button" data-focus data-focus-key={e.path} className={`fp-item ${e.dir ? "dir" : "file"} ${isSel ? "sel" : ""}`}
                      onClick={() => activate(e)} aria-pressed={multiple && !e.dir ? isSel : undefined} title={e.name}>
                      {thumb ? <Thumb e={e} /> : <span className="fp-icon"><Icon k={iconFor(e)} /></span>}
                      <span className="fp-name">{e.name}{e.link && <span className="fp-link-badge" title={t("Atalho")}>↗</span>}{e.dir && isFav(e.path) && <span className="fp-fav-badge" aria-label={t("Favorita")}><Icon k="star" /></span>}</span>
                      {view === "list" && <><span className="fp-date">{fmtDate(e.modified)}</span><span className="fp-size">{e.dir ? "" : formatBytes(e.size)}</span></>}
                      {multiple && !e.dir && isSel && <span className="fp-check"><Icon k="check" /></span>}
                    </button>
                  );
                })}
                {!!listing?.truncated && <p className="fp-msg small">{t("Mais {n} itens não exibidos. Digite o caminho ou escolha um tipo de arquivo para filtrar.", { n: listing.truncated })}</p>}
              </>
            )}
          </div>
        </div>

        {/* ---------- rodapé ---------- */}
        <div className="fp-foot" data-nav-group>
          {mode === "save" && (
            <label className="fp-name-field">
              <span>{t("Nome do arquivo")}</span>
              <input value={name} onChange={(e) => { setName(e.target.value); setOverwrite(null); }} spellCheck={false}
                onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); confirm(); } }} />
            </label>
          )}
          {!dirMode && filters.length > 1 && (
            <Select<number> value={filterIdx} width={300} title={t("Tipo de arquivo")} onChange={setFilterIdx}
              options={filters.map((f, i): [string, number] => [f.extensions.includes("*") ? f.name : `${f.name} (${f.extensions.map((x) => `.${x}`).join(", ")})`, i])} />
          )}
          <span className="fp-status">
            {overwrite ? t("“{name}” já existe. Salvar de novo para substituir.", { name: baseOf(overwrite) })
              : dirMode || browse ? (here ? here : t("Escolha uma unidade ou pasta."))
              : multiple ? (selected.length ? t("{n} selecionado(s)", { n: selected.length }) : t("Selecione os arquivos e confirme."))
              : ""}
          </span>
          <div className="fp-actions">
            <Button variant={browse ? "primary" : "ghost"} onClick={cancel}>{browse ? t("Fechar") : t("Cancelar")}</Button>
            {(dirMode || mode === "save" || multiple) && (
              <Button variant={overwrite ? "danger" : "primary"} disabled={!canConfirm} onClick={confirm}>{overwrite ? t("Substituir") : confirmLabel}</Button>
            )}
          </div>
        </div>
      </div>
    </Modal>
  );
}
