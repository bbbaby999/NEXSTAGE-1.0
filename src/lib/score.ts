/**
 * Pontuação de conquistas (escala dos troféus do PlayStation: bronze 15, prata 30, ouro 90, platina 180).
 * Espelho de `src-tauri/src/score.rs`, que soma o total do perfil: mude os dois juntos.
 */
export const COMPLETION_BONUS = 180;

export function trophyValue(grade: string): number {
  return grade === "P" ? 180 : grade === "G" ? 90 : grade === "S" ? 30 : 15;
}

/** Steam: pela raridade global (% de jogadores); desconhecida vale o mínimo. */
export function steamValue(globalPct: number | null | undefined): number {
  if (globalPct == null) return 15;
  return globalPct < 3 ? 90 : globalPct < 10 ? 60 : globalPct < 30 ? 30 : 15;
}

/** RetroAchievements: pelos pontos da conquista; Hardcore vale +50%. */
export function raValue(points: number, hardcore: boolean): number {
  const base = points >= 25 ? 90 : points >= 10 ? 60 : points >= 5 ? 30 : 15;
  return hardcore ? Math.floor((base * 3) / 2) : base;
}
