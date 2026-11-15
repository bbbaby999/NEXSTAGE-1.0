/** Zerei!: rótulos, datas e estatísticas (compartilhados pela página, formulário e detalhes). */
import { currentLang, t, tk } from "./i18n";
import type { ZereiCompletion, ZereiDifficulty, ZereiEntry, ZereiStatus } from "../types";

export const STATUS_LABEL: Record<ZereiStatus, string> = {
  not_started: tk("Não iniciado"), playing: tk("Jogando"), paused: tk("Pausado"), completed: tk("Zerado"), wishlist: tk("Lista de desejos"),
};
export const STATUS_ORDER: ZereiStatus[] = ["completed", "playing", "paused", "wishlist", "not_started"];
export const COMPLETION_LABEL: Record<ZereiCompletion, string> = { campaign: tk("Zerado"), full: tk("Platinado") };
export const DIFFICULTY_LABEL: Record<ZereiDifficulty, string> = {
  very_easy: tk("Muito fácil"), easy: tk("Fácil"), normal: tk("Normal"), hard: tk("Difícil"), very_hard: tk("Muito difícil"),
};
export const DIFFICULTY_ORDER: ZereiDifficulty[] = ["very_easy", "easy", "normal", "hard", "very_hard"];

export const statusLabel = (s: ZereiStatus) => t(STATUS_LABEL[s]);

type Part = "d" | "m" | "y";
const ORDER_CACHE = new Map<string, { order: Part[]; sep: string }>();

/**
 * Ordem e separador da data no idioma da interface, vindos do próprio sistema (Intl): pt/es/fr/it
 * 31/12/2024, en 12/31/2024, de/ru 31.12.2024, ja/zh 2024/12/31, ko 2024.12.31.
 */
export function dateFormat(lang = currentLang()): { order: Part[]; sep: string } {
  const hit = ORDER_CACHE.get(lang);
  if (hit) return hit;
  let order: Part[] = ["d", "m", "y"];
  let sep = "/";
  try {
    const parts = new Intl.DateTimeFormat(lang, { day: "2-digit", month: "2-digit", year: "numeric" }).formatToParts(new Date(2024, 11, 31));
    const o = parts.map((p) => (p.type === "day" ? "d" : p.type === "month" ? "m" : p.type === "year" ? "y" : null)).filter((x): x is Part => !!x);
    if (o.length === 3 && new Set(o).size === 3) order = o;
    const lit = parts.find((p) => p.type === "literal")?.value.trim().charAt(0);
    if (lit && "/.-".includes(lit)) sep = lit;
  } catch { /* Intl indisponível: dia/mês/ano */ }
  const r = { order, sep };
  ORDER_CACHE.set(lang, r);
  return r;
}

/** Exemplo do formato (31/12/2024, 12/31/2024, 2024/12/31...), usado como dica no campo. */
export const dateExample = (lang = currentLang()) => isoToInput("2024-12-31", lang);

/** "2024-03-05" → "05/03/2024" / "03/05/2024" / "2024/03/05", conforme o idioma. */
export function isoToInput(iso: string | null, lang = currentLang()): string {
  if (!iso) return "";
  const [y, m, d] = iso.split("-");
  const { order, sep } = dateFormat(lang);
  return order.map((p) => (p === "d" ? d : p === "m" ? m : y)).join(sep);
}

/**
 * Texto digitado → "2024-03-05", na ordem do idioma. Aceita "/", "." ou "-" e dia/mês com 1 dígito.
 * Vazio → null; inválido → undefined.
 */
export function inputToIso(v: string, lang = currentLang()): string | null | undefined {
  const s = v.trim().replace(/\.$/, "");
  if (!s) return null;
  const nums = s.split(/\s*[/.\-]\s*/);
  if (nums.length !== 3 || !nums.every((x) => /^\d+$/.test(x))) return undefined;
  const { order } = dateFormat(lang);
  const val = (p: Part) => nums[order.indexOf(p)];
  const yStr = val("y");
  if (yStr.length !== 4 || val("d").length > 2 || val("m").length > 2) return undefined;
  const [d, mo, y] = [Number(val("d")), Number(val("m")), Number(yStr)];
  const dt = new Date(Date.UTC(y, mo - 1, d));
  if (dt.getUTCFullYear() !== y || dt.getUTCMonth() !== mo - 1 || dt.getUTCDate() !== d || y < 1970) return undefined;
  return `${y}-${String(mo).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
}

export const todayIso = () => {
  const n = new Date();
  return `${n.getFullYear()}-${String(n.getMonth() + 1).padStart(2, "0")}-${String(n.getDate()).padStart(2, "0")}`;
};

/** Data por extenso no idioma da interface ("5 de mar. de 2024"). */
export function fmtDay(iso: string | null): string {
  if (!iso) return "—";
  const d = new Date(`${iso}T12:00:00`);
  return isNaN(d.getTime()) ? "—" : d.toLocaleDateString(currentLang(), { day: "numeric", month: "short", year: "numeric" });
}

export const fmtHours = (h: number | null) => (h === null ? "—" : `${h.toLocaleString(currentLang(), { maximumFractionDigits: 1 })} h`);
export const fmtRating = (r: number | null) => (r === null ? "—" : r.toLocaleString(currentLang(), { maximumFractionDigits: 1 }));

/** Ano usado nos filtros/histórico: o ano em que zerou. */
export const doneYear = (e: ZereiEntry) => (e.status === "completed" && e.completedAt ? Number(e.completedAt.slice(0, 4)) : null);

export interface ZereiStats {
  total: number; completed: number; full: number; playing: number; paused: number; notStarted: number; wishlist: number;
  hours: number; avgRating: number | null; rated: number;
  byPlatform: [string, number][]; byYear: [number, number][]; recent: ZereiEntry[];
}

export function computeStats(list: ZereiEntry[]): ZereiStats {
  const done = list.filter((e) => e.status === "completed");
  const rated = list.filter((e) => e.rating !== null);
  const count = <K,>(keys: K[]) => {
    const m = new Map<K, number>();
    keys.forEach((k) => m.set(k, (m.get(k) ?? 0) + 1));
    return [...m.entries()];
  };
  return {
    total: list.length,
    completed: done.length,
    full: done.filter((e) => e.completionType === "full").length,
    playing: list.filter((e) => e.status === "playing").length,
    paused: list.filter((e) => e.status === "paused").length,
    notStarted: list.filter((e) => e.status === "not_started").length,
    wishlist: list.filter((e) => e.status === "wishlist").length,
    hours: list.reduce((s, e) => s + (e.hours ?? 0), 0),
    avgRating: rated.length ? rated.reduce((s, e) => s + (e.rating ?? 0), 0) / rated.length : null,
    rated: rated.length,
    // jogos zerados por plataforma (mais zerados primeiro)
    byPlatform: count(done.map((e) => e.platform)).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])),
    byYear: count(done.map(doneYear).filter((y): y is number => y !== null)).sort((a, b) => a[0] - b[0]),
    recent: done.filter((e) => e.completedAt).sort((a, b) => (b.completedAt ?? "").localeCompare(a.completedAt ?? "")).slice(0, 12),
  };
}

/** Erro de duplicado do backend: `ZEREI_DUPLICATE::<id>::<quantos>`. */
export function parseDuplicate(e: unknown): { id: number; count: number } | null {
  if (typeof e !== "string" || !e.startsWith("ZEREI_DUPLICATE::")) return null;
  const [, id, count] = e.split("::");
  return { id: Number(id), count: Number(count) };
}
