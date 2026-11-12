/**
 * Plano de fundo da Home (por perfil):
 * - `game`    → arte do jogo em destaque (padrão; troca ao navegar pelo "Volte a jogar");
 * - `waves`   → sem imagem: só o fundo animado de ondas do app;
 * - `custom`  → uma imagem escolhida pelo usuário no computador;
 * - `gallery` → uma captura escolhida na Galeria do perfil.
 * Sem imagem salva para o modo escolhido, a Home fica só com as ondas.
 *
 * Preferências: `profile_settings.home_bg_mode`, `home_bg_image` e `home_bg_capture` (arquivos em
 * `<app_data>/images`, gravados pelo backend — ver `set_home_background`). Cada modo guarda a sua
 * imagem: alternar entre eles não perde a escolha do outro.
 */
import { useSyncExternalStore } from "react";
import { api } from "./api";

export type HomeBgMode = "game" | "waves" | "custom" | "gallery";
export interface HomeBg { mode: HomeBgMode; image: string | null; capture: string | null }

let state: HomeBg = { mode: "game", image: null, capture: null };
const subs = new Set<() => void>();
const subscribe = (cb: () => void) => { subs.add(cb); return () => { subs.delete(cb); }; };

export const parseHomeBgMode = (v: string | null): HomeBgMode =>
  (v === "waves" || v === "custom" || v === "gallery" ? v : "game");
export const getHomeBg = () => state;
export function setHomeBg(next: Partial<HomeBg>) {
  state = { ...state, ...next };
  subs.forEach((f) => f());
}
export const useHomeBg = () => useSyncExternalStore(subscribe, getHomeBg);

/** Imagem do modo atual (null = só as ondas ou arte do jogo). */
export const homeBgImage = (bg: HomeBg) =>
  (bg.mode === "custom" ? bg.image : bg.mode === "gallery" ? bg.capture : null);

/** Carrega as preferências do perfil (na troca de perfil). */
export async function loadHomeBg(profileId: number) {
  const [mode, imgs] = await Promise.all([
    api.getProfileSetting(profileId, "home_bg_mode").catch(() => null),
    api.getHomeBackground(profileId).catch(() => ({ image: null, capture: null })),
  ]);
  setHomeBg({ mode: parseHomeBgMode(mode), ...imgs });
}
