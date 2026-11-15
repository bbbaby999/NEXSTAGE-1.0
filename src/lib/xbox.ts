import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

/** Chama `onSynced` quando o Xbox do perfil é sincronizado ou uma conquista nova chega (backend emite "xbox-synced"). */
export function useXboxSynced(profileId: number, onSynced: () => void) {
  const cb = useRef(onSynced);
  cb.current = onSynced;
  useEffect(() => {
    let off: (() => void) | undefined, dead = false;
    listen<{ profileId: number }>("xbox-synced", (e) => { if (e.payload.profileId === profileId) cb.current(); })
      .then((f) => (dead ? f() : (off = f)));
    return () => { dead = true; off?.(); };
  }, [profileId]);
}
