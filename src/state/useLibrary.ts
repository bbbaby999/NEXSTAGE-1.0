import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errMsg } from "../lib/api";
import type { Game } from "../types";
import { useSession } from "./session";

/** Carrega a biblioteca do perfil e recarrega quando uma sessão de jogo termina. */
export function useLibrary(profileId: number) {
  const { toast } = useSession();
  const [games, setGames] = useState<Game[]>([]);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    try {
      setGames(await api.listLibrary(profileId));
    } catch (e) {
      toast(errMsg(e), "err");
    } finally {
      setLoading(false);
    }
  }, [profileId, toast]);

  useEffect(() => {
    reload();
    let un: (() => void) | undefined;
    let cancelled = false;
    listen("game-session-ended", () => reload()).then((f) => (cancelled ? f() : (un = f)));
    return () => {
      cancelled = true;
      un?.();
    };
  }, [reload]);

  return { games, loading, reload };
}
