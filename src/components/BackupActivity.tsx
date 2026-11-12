import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import Button from "./Button";
import { api } from "../lib/api";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import { BackupProgressBar } from "./BackupPanel";
import type { BackupProgress } from "../types";

/**
 * Atividade do backup automático em qualquer tela:
 * - "antes de encerrar": tela de progresso por cima de tudo até o backup terminar (com Cancelar);
 * - semanal: aviso ao concluir ou falhar (roda em segundo plano, sem interromper).
 */
export default function BackupActivity() {
  const { toast } = useSession();
  const [exit, setExit] = useState<BackupProgress | null>(null);

  useEffect(() => {
    const uns: (() => void)[] = [];
    let cancelled = false;
    const on = <T,>(ev: string, f: (p: T) => void) =>
      listen<T>(ev, (e) => f(e.payload)).then((u) => (cancelled ? u() : uns.push(u)));
    on<BackupProgress>("backup-progress", (p) => {
      if (p.kind !== "exit") return;
      setExit(p.phase === "end" ? null : p);
    });
    on<{ path: string }>("backup-auto-done", (p) => toast(t("Backup automático concluído: {path}", { path: p.path })));
    on<{ error: string }>("backup-auto-failed", (p) => toast(t("O backup automático falhou: {v0}", { v0: t(p.error) }), "err"));
    on<null>("backup-close-blocked", () => toast(t("Aguarde: há um backup ou uma restauração em andamento.")));
    return () => { cancelled = true; uns.forEach((u) => u()); };
  }, [toast]);

  if (!exit) return null;
  return createPortal(
    <div className="overlay bk-exit" role="dialog" aria-modal="true" aria-label={t("Salvando backup antes de encerrar")}>
      <div className="modal">
        <h2>{t("Salvando backup antes de encerrar")}</h2>
        <BackupProgressBar p={exit} />
        <p className="muted">{t("O NEXSTAGE continua a ação assim que o backup terminar.")}</p>
        <div className="row end">
          <Button variant="ghost" autoFocus disabled={!exit.cancelable} onClick={() => api.backupCancel()}>{t("Cancelar backup")}</Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
