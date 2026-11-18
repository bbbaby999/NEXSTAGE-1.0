import { currentLang } from "./i18n";

export function formatDuration(sec: number): string {
  if (sec < 60) return sec <= 0 ? "0 min" : "< 1 min";
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  return h > 0 ? `${h} h ${m} min` : `${m} min`;
}

/** Datas do SQLite vêm em UTC no formato "YYYY-MM-DD HH:MM:SS". */
export function formatDate(s: string | null): string {
  if (!s) return "Nunca";
  const d = new Date(s.replace(" ", "T") + "Z");
  return isNaN(d.getTime()) ? "—" : d.toLocaleDateString(currentLang(), { day: "2-digit", month: "short", year: "numeric" });
}

export function formatBytes(b: number): string {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0, v = b;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(i > 1 ? 1 : 0)} ${u[i]}`;
}

export function formatDateTime(s: string | null): string {
  if (!s) return "—";
  const d = new Date(s.replace(" ", "T") + "Z");
  return isNaN(d.getTime()) ? "—" : d.toLocaleString(currentLang(), { day: "2-digit", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });
}

export function sortRecent(a: { lastPlayedAt: string | null }, b: { lastPlayedAt: string | null }) {
  return (b.lastPlayedAt ?? "").localeCompare(a.lastPlayedAt ?? "");
}

/** Tempo de jogo curto para capas e listas ("45 min", "2 h 15 min", "38 h"); `null` se nunca jogado. */
export function formatPlaytime(sec: number): string | null {
  if (sec < 60) return null;
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  if (!h) return `${m} min`;
  return h < 10 && m ? `${h} h ${m} min` : `${h} h`;
}

/**
 * Data de saída do PC Game Pass ("AAAA-MM-DD", dia do calendário, sem fuso). Só datas de hoje até daqui a
 * 120 dias são aceitas; fora disso (data passada ou distante) a saída é tratada como "em breve".
 */
export function gamePassLeaveDate(s: string | null | undefined): Date | null {
  const m = s ? /^(\d{4})-(\d{2})-(\d{2})$/.exec(s) : null;
  if (!m) return null;
  const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const days = (d.getTime() - today.getTime()) / 86400000;
  return days >= 0 && days <= 120 ? d : null;
}
