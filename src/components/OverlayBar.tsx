import { useEffect, useState } from "react";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import { useLibrary } from "../state/useLibrary";
import { formatPlaytime } from "../lib/format";
import { t } from "../lib/i18n";

/** Hora atual (HH:MM no formato do sistema), atualizada a cada 15 s. */
function useClock() {
  const fmt = () => new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const [now, setNow] = useState(fmt);
  useEffect(() => {
    const id = window.setInterval(() => setNow(fmt()), 15_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

/** Cartão exibido apenas no modo overlay (atalho Ctrl+Shift+O). */
export default function OverlayBar() {
  const { profile, toast } = useSession();
  const { games } = useLibrary(profile!.id);
  const [running, setRunning] = useState<number[]>([]);
  const [confirm, setConfirm] = useState<number | null>(null);
  const clock = useClock();

  useEffect(() => {
    api.runningGames().then(setRunning).catch(() => {});
  }, []);

  async function quit(id: number) {
    try {
      await api.quitGame(id);
      toast(t("Jogo encerrado."));
    } catch (e) {
      toast(errMsg(e), "err");
      setConfirm(null);
    }
  }

  return (
    <div className="ingame-bar">
      <div className="ig-brand">
        <span className={`ig-dot ${running.length ? "live" : ""}`} aria-hidden />
        <span className="ig-label">{t("Overlay")}</span>
        <span className="ig-clock">{clock}</span>
      </div>

      <div className="ig-games">
        {running.length === 0 && <span className="ig-empty">{t("Nenhum jogo em execução")}</span>}
        {running.map((id) => {
          const g = games.find((x) => x.id === id);
          const title = g?.title ?? t("Jogo");
          const meta = [g?.platform, g ? formatPlaytime(g.playtimeSeconds) : null].filter(Boolean).join(" · ");
          return (
            <div className="ig-game" key={id}>
              <div className="ig-cover" style={g?.cover ? { backgroundImage: `url(${g.cover})` } : undefined}>
                {!g?.cover && <b>{title.charAt(0).toUpperCase()}</b>}
              </div>
              <div className="ig-info">
                <small>{t("Em jogo:").replace(/:\s*$/, "")}</small>
                <strong title={title}>{title}</strong>
                {meta && <span>{meta}</span>}
              </div>
              {confirm === id ? (
                <Button variant="danger" className="ig-quit armed" onClick={() => quit(id)} onBlur={() => setConfirm(null)} autoFocus>
                  {t("Confirmar: sair do jogo")}
                </Button>
              ) : (
                <Button variant="ghost" className="ig-quit" onClick={() => setConfirm(id)}>{t("Sair do jogo")}</Button>
              )}
            </div>
          );
        })}
      </div>

      <div className="ig-actions">
        <Button variant="ghost" onClick={() => api.openCapturesFolder().catch((e) => toast(errMsg(e), "err"))}>
          <svg viewBox="0 0 24 24" width="17" height="17" aria-hidden><path fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2h9A1.5 1.5 0 0 1 21 9.5v8a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z" /></svg>
          {t("Pasta de capturas")}
        </Button>
        <Button variant="primary" onClick={() => api.closeOverlay()}>
          <svg viewBox="0 0 24 24" width="17" height="17" aria-hidden><path fill="currentColor" d="M8 5.5v13a1 1 0 0 0 1.5.86l10.5-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5z" /></svg>
          {running.length ? t("Voltar ao jogo") : t("Fechar overlay")}
        </Button>
      </div>
    </div>
  );
}
