import { useEffect, useRef, useState } from "react";
import { LogoMark } from "./Logo";
import Avatar from "./Avatar";
import Glyph from "./Glyph";
import { api, errMsg } from "../lib/api";
import { play } from "../lib/sound";
import { useSession } from "../state/session";
import type { View } from "../types";
import { t, tk } from "../lib/i18n";

type Page = "main" | "power" | "restart" | "shutdown" | "backupfail";
type PowerAct = "exit" | "minimize" | "restart" | "shutdown";
/** Prefixo do erro do backend quando o backup "antes de encerrar" falha ou é cancelado (ver commands/system.rs). */
const BACKUP_FAILED = "BACKUP_FAILED::";

/**
 * Menu lateral (ESC / Círculo na Home). Abre sobre a Home com fundo escurecido e desfocado.
 * Enquanto aberto é a raiz exclusiva da navegação (veja navRoot em lib/nav.ts) e o conteúdo
 * atrás recebe `inert`, então nenhum comando chega à Home. ESC volta um nível e depois fecha.
 */
export default function SideMenu({ open, onClose, onGo }: {
  open: boolean; onClose: () => void; onGo: (v: View) => void;
}) {
  const { toast, profile } = useSession();
  const [mounted, setMounted] = useState(open);
  const [leaving, setLeaving] = useState(false);
  const [page, setPage] = useState<Page>("main");
  const [backupFail, setBackupFail] = useState<{ action: PowerAct; msg: string } | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const prev = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (open) {
      prev.current = document.activeElement as HTMLElement | null;
      setMounted(true);
      setLeaving(false);
      setPage("main");
      play("open");
    } else if (mounted) {
      setLeaving(true);
      play("close");
      const t = setTimeout(() => {
        setMounted(false);
        setLeaving(false);
        // Só devolve o foco se nada mais o pegou (ex.: a Biblioteca recém-aberta já focou o 1º jogo).
        const a = document.activeElement;
        if (!a || a === document.body || ref.current?.contains(a)) prev.current?.focus?.();
      }, 240);
      return () => clearTimeout(t);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!mounted || !open) return;
    const t = setTimeout(() => ref.current?.querySelector<HTMLElement>("[data-focus]")?.focus(), 40);
    return () => clearTimeout(t);
  }, [mounted, open, page]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      play("back");
      if (page !== "main") setPage(page === "power" ? "main" : "power");
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, page, onClose]);

  if (!mounted) return null;

  const power = async (a: PowerAct, skipBackup = false) => {
    try {
      await api.powerAction(a, skipBackup);
      if (a === "minimize") onClose();
    } catch (e) {
      // Backup automático "antes de encerrar" falhou ou foi cancelado: o usuário decide se continua.
      if (typeof e === "string" && e.startsWith(BACKUP_FAILED)) {
        setBackupFail({ action: a, msg: e.slice(BACKUP_FAILED.length) });
        setPage("backupfail");
        return;
      }
      toast(errMsg(e), "err");
    }
  };
  const ICONS: Record<string, string> = {
    "Início": "M3 11l9-8 9 8M5 10v10h5v-6h4v6h5V10",
    "Biblioteca": "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
    "Mídias": "M3 5h18v14H3zM3 16l5-5 4 4 3-3 6 6M9 9.5h.01",
    "Conquistas": "M8 4h8v5a4 4 0 0 1-8 0V4ZM8 6H5a3 3 0 0 0 3 4M16 6h3a3 3 0 0 1-3 4M12 13v4M8.5 20h7M10 17h4",
    "Ver perfil": "M12 12a4 4 0 100-8 4 4 0 000 8zM4 20c0-4 4-6 8-6s8 2 8 6",
    "Central de Controles": "M7 8h10a4 4 0 0 1 4 4v1.5a3.5 3.5 0 0 1-6.3 2.1L14 14.5h-4l-.7 1.1A3.5 3.5 0 0 1 3 13.5V12a4 4 0 0 1 4-4ZM7.5 10.5v3M6 12h3M15.5 11.5h0M17.5 13h0",
    "Configurações": "M4 7h10M18 7h2M4 17h2M10 17h10M14 5v4M6 15v4",
    "Ligar/Desligar": "M12 3v9M6.5 6.5a8 8 0 1011 0",
  };
  const item = (label: string, fn: () => void, danger = false) => (
    <button data-focus className={`sm-item ${danger ? "danger" : ""}`} onClick={fn}>
      {ICONS[label] && (
        <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={ICONS[label]} /></svg>
      )}
      <span>{t(label)}</span>
    </button>
  );

  return (
    <div className={`sm-root ${leaving ? "leaving" : ""}`} ref={ref}>
      <div className="sm-backdrop" onClick={onClose} />
      <aside className="sm-panel">
        <div className="sm-head"><LogoMark size={28} /><span>{t("NEXSTAGE")}</span></div>
        {page === "main" && profile && (
          <button data-focus className="sm-user" onClick={() => onGo("profile")} aria-label={t("Ver perfil")}>
            <Avatar name={profile.displayName} src={profile.avatar} size={44} />
            <span><b>{profile.displayName}</b><small>@{profile.username}</small></span>
          </button>
        )}
        {page === "main" && (
          <>
            {item(tk("Início"), () => onGo("home"))}
            {item(tk("Biblioteca"), () => onGo("library"))}
            {item(tk("Mídias"), () => onGo("gallery"))}
            {item(tk("Conquistas"), () => onGo("achievements"))}
            {item(tk("Ver perfil"), () => onGo("profile"))}
            {item(tk("Central de Controles"), () => onGo("controllers"))}
            {item(tk("Configurações"), () => onGo("settings"))}
            {item(tk("Ligar/Desligar"), () => setPage("power"))}
          </>
        )}
        {page === "power" && (
          <>
            <h3>{t("Ligar/Desligar")}</h3>
            {item(tk("Sair do aplicativo"), () => power("exit"))}
            {item(tk("Minimizar"), () => power("minimize"))}
            {item(tk("Reiniciar o Windows"), () => setPage("restart"), true)}
            {item(tk("Desligar o Windows"), () => setPage("shutdown"), true)}
            {item(tk("Voltar"), () => setPage("main"))}
          </>
        )}
        {(page === "restart" || page === "shutdown") && (
          <>
            <h3>{page === "restart" ? t("Reiniciar o Windows?") : t("Desligar o Windows?")}</h3>
            <p className="muted">{t("Programas abertos e jogos em execução serão fechados.")}</p>
            {item(tk("Cancelar"), () => setPage("power"))}
            {item(page === "restart" ? "Confirmar: reiniciar" : "Confirmar: desligar", () => power(page), true)}
          </>
        )}
        {page === "backupfail" && backupFail && (
          <>
            <h3>{t("O backup automático não foi concluído")}</h3>
            <p className="muted">{t(backupFail.msg)}</p>
            {item(tk("Cancelar"), () => setPage("power"))}
            {item(backupFail.action === "exit" ? tk("Sair sem backup") : backupFail.action === "restart" ? tk("Reiniciar sem backup") : tk("Desligar sem backup"), () => power(backupFail.action, true), true)}
          </>
        )}
        <div className="sm-foot"><Glyph k="confirm" /> {t("Selecionar")}{" "}<Glyph k="back" /> {t("Fechar")}</div>
      </aside>
    </div>
  );
}
