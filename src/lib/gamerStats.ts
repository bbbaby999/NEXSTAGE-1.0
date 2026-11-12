/** Estatísticas gamer consolidadas (interface): um único carregamento compartilhado por Home, Perfil e Estatísticas. */
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errMsg } from "./api";
import { formatDuration } from "./format";
import { t, tk } from "./i18n";
import type { GamerStats, StatSource } from "../types";

/**
 * Registro das origens na interface. Para uma nova plataforma (Epic Games, Xbox...), acrescente o rótulo
 * e uma cor de gráfico VALIDADA para o fundo escuro (skill dataviz: scripts/validate_palette.js) — nunca
 * uma cor gerada. Sem cor registrada, a origem aparece em cinza neutro, com rótulo e legenda.
 */
const SOURCE: Record<string, { label: string; color: string }> = {
  local: { label: tk("NEXSTAGE"), color: "var(--b)" },
  steam: { label: tk("Steam"), color: "#3b95cf" },
  retroachievements: { label: tk("RetroAchievements"), color: "#b8841f" },
};
export const sourceLabel = (s: StatSource) => (SOURCE[s] ? t(SOURCE[s].label) : s);
export const sourceColor = (s: StatSource) => SOURCE[s]?.color ?? "#7d8597";

/** Horas legíveis ("12 h 30 min"); 0 → "0 min". */
export const hours = (sec: number) => formatDuration(sec);
export const pctOf = (a: number, b: number) => (b > 0 ? Math.round((a / b) * 1000) / 10 : 0);

/**
 * Carrega as estatísticas do cache local e recarrega sozinho quando um jogo termina ou quando uma
 * sincronização (Steam/RetroAchievements) conclui. `refresh` pede às fontes conectadas uma
 * sincronização manual — falhas não apagam nada (o cache continua sendo exibido).
 */
export function useGamerStats(profileId: number) {
  const [stats, setStats] = useState<GamerStats | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const load = useCallback(() => api.gamerStats(profileId).then(setStats).catch(() => {}), [profileId]);

  useEffect(() => {
    load();
    const uns: (() => void)[] = [];
    let cancelled = false;
    for (const ev of ["game-session-ended", "steam-synced", "ra-synced"]) {
      listen(ev, () => load()).then((f) => (cancelled ? f() : uns.push(f)));
    }
    return () => { cancelled = true; uns.forEach((f) => f()); };
  }, [load]);

  const refresh = useCallback(async (): Promise<string[]> => {
    setRefreshing(true);
    const errors: string[] = [];
    try {
      const [steam, ra] = await Promise.all([api.steamGetAccount(profileId).catch(() => null), api.raGetAccount(profileId).catch(() => null)]);
      await Promise.all([
        steam ? api.steamSync(profileId).catch((e) => { errors.push(errMsg(e)); }) : null,
        ra?.hasKey ? api.raSync(profileId).catch((e) => { errors.push(errMsg(e)); }) : null,
      ]);
    } finally {
      await load();
      setRefreshing(false);
    }
    return errors;
  }, [profileId, load]);

  return { stats, refreshing, refresh, reload: load };
}
