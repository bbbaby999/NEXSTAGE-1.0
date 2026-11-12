/** Utilidades da integração Steam (interface). */
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { formatDuration } from "./format";
import { t, tk } from "./i18n";

/** Arte horizontal ("hero") do jogo na Steam: plano de fundo quando o jogo não tem um cadastrado. */
export const steamHero = (id: string) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/library_hero.jpg`;
import type { SteamInstall } from "../types";

/** Minutos da Steam → "12 h 30 min". `null` = a Steam não informou (nada é inventado). */
export const steamTime = (min: number | null | undefined) => (min === null || min === undefined ? t("Não informado") : formatDuration(min * 60));

const INSTALL: Record<SteamInstall, string> = {
  installed: tk("Instalado"),
  updating: tk("Atualização pendente"),
  installing: tk("Instalando"),
  not_installed: tk("Não instalado"),
  unknown: tk("Cliente Steam não encontrado"),
};
export const installLabel = (s: SteamInstall) => t(INSTALL[s]);
export const isInstalled = (s: SteamInstall) => s === "installed" || s === "updating";

/** Recarrega quando a sincronização Steam do perfil termina ou muda de estado. */
export function useSteamEvents(profileId: number, onChange: (kind: "synced" | "state", running?: boolean) => void) {
  useEffect(() => {
    const uns: (() => void)[] = [];
    let cancelled = false;
    listen<{ profileId: number }>("steam-synced", (e) => { if (e.payload.profileId === profileId) onChange("synced"); })
      .then((f) => (cancelled ? f() : uns.push(f)));
    listen<{ profileId: number; running: boolean }>("steam-sync-state", (e) => { if (e.payload.profileId === profileId) onChange("state", e.payload.running); })
      .then((f) => (cancelled ? f() : uns.push(f)));
    return () => { cancelled = true; uns.forEach((f) => f()); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [profileId]);
}
