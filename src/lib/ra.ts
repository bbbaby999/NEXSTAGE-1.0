/** Utilidades da integração com o RetroAchievements (interface). */
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { currentLang, t, tk } from "./i18n";
import type { RaAchievement, RaSyncReport } from "../types";

/** Plataformas do NEXSTAGE com suporte no RetroAchievements (mesma tabela do backend: retro/hash.rs). */
export const RA_PLATFORMS = ["Playstation 1", "Playstation 2", "PSP", "Super Nintendo", "Nintendinho"];
export const raSupported = (platform: string) => RA_PLATFORMS.includes(platform);

export const RA_SITE = "https://retroachievements.org";

const AWARD: Record<string, string> = {
  mastered: tk("Masterizado"),
  completed: tk("Completo"),
  "beaten-hardcore": tk("Zerado (Hardcore)"),
  "beaten-softcore": tk("Zerado"),
};
export const awardLabel = (k: string | null) => (k && AWARD[k] ? t(AWARD[k]) : null);

export const pct = (n: number, total: number) => (total > 0 ? Math.min(100, Math.round((n / total) * 100)) : 0);

/** Datas do cache (UTC, "AAAA-MM-DD HH:MM:SS") → data local curta. */
export function raDate(s: string | null): string {
  if (!s) return "—";
  const d = new Date(s.replace(" ", "T") + "Z");
  if (isNaN(d.getTime())) return "—";
  return d.toLocaleString(currentLang(), { day: "2-digit", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" });
}

/** "há 5 min", "há 3 h", "há 2 dias" (ou a data, se for antigo). */
export function raAgo(s: string | null): string {
  if (!s) return t("nunca");
  const d = new Date(s.replace(" ", "T") + "Z").getTime();
  if (isNaN(d)) return "—";
  const m = Math.max(0, Math.round((Date.now() - d) / 60000));
  if (m < 1) return t("agora mesmo");
  if (m < 60) return t("há {n} min", { n: m });
  const h = Math.round(m / 60);
  if (h < 24) return t("há {n} h", { n: h });
  const days = Math.round(h / 24);
  if (days < 30) return t("há {n} dias", { n: days });
  return raDate(s);
}

export const unlockedAt = (a: RaAchievement) => a.unlockedHcAt ?? a.unlockedAt;
export const isUnlocked = (a: RaAchievement) => !!(a.unlockedAt || a.unlockedHcAt);

/** Recarrega quando uma sincronização termina, falha ou muda de estado (para o perfil informado). */
export function useRaEvents(profileId: number, onChange: (kind: "synced" | "failed" | "state", payload: unknown) => void) {
  useEffect(() => {
    const uns: (() => void)[] = [];
    let cancelled = false;
    const on = <T,>(ev: string, kind: "synced" | "failed" | "state") =>
      listen<T & { profileId: number }>(ev, (e) => { if (e.payload.profileId === profileId) onChange(kind, e.payload); })
        .then((f) => (cancelled ? f() : uns.push(f)));
    on<{ report: RaSyncReport }>("ra-synced", "synced");
    on<{ message: string }>("ra-sync-failed", "failed");
    on<{ running: boolean }>("ra-sync-state", "state");
    return () => { cancelled = true; uns.forEach((f) => f()); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profileId]);
}
