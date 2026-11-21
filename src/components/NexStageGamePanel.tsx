import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import Button from "./Button";
import NexStageAchievements from "./NexStageAchievements";
import NexStageStats from "./NexStageStats";
import { api, errMsg } from "../lib/api";
import { pickOpen } from "../lib/filePicker";
import { formatBytes } from "../lib/format";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { Game, IntegratedJob, IntegratedStatus } from "../types";

/** Executável oficial de cada jogo integrado (usado no filtro do seletor de arquivos). */
const EXE: Record<string, string> = { "tomb-survivors": "TombSurvivors.exe" };
/** Procura atualização uma vez por sessão do app (a página do jogo pode ser aberta várias vezes). */
const checked = new Set<string>();

/**
 * Estado do jogo integrado (instalação, versão, atualização e download em andamento), compartilhado pela página
 * do jogo e pelo bloco NexStage. Atualiza sozinho pelos eventos do instalador.
 */
export function useIntegrated(game?: Game) {
  const gameId = game?.source === "nexstage" ? game.externalId ?? null : null;
  const [st, setSt] = useState<IntegratedStatus | null>(null);
  const reload = useCallback(() => {
    if (gameId) api.integratedStatus(gameId).then(setSt).catch(() => setSt(null));
  }, [gameId]);
  useEffect(() => { reload(); }, [reload]);
  useEffect(() => {
    if (!gameId) return;
    const offs = [
      listen<IntegratedStatus>("integrated-status", (e) => { if (e.payload.gameId === gameId) setSt(e.payload); }),
      listen<{ gameId: string; phase: string; done: number; total: number }>("integrated-progress", (e) => {
        if (e.payload.gameId !== gameId) return;
        const { phase, done, total } = e.payload;
        setSt((s) => (s ? { ...s, job: { phase, done, total, version: s.job?.version ?? "" } } : s));
      }),
    ];
    return () => { offs.forEach((p) => p.then((f) => f())); };
  }, [gameId]);
  // primeira abertura na sessão: procura a versão publicada (sem internet, só não mostra atualização)
  useEffect(() => {
    if (!gameId || checked.has(gameId)) return;
    checked.add(gameId);
    api.integratedCheckUpdate(gameId).then(setSt).catch(() => {});
  }, [gameId]);
  return { gameId, st, setSt, reload };
}

/** "Localizar instalação": escolhe o executável oficial de uma cópia do jogo já existente no PC. */
export async function locateIntegrated(gameId: string): Promise<IntegratedStatus | null> {
  const exe = EXE[gameId] ?? "";
  const path = await pickOpen({ title: t("Localizar {exe}", { exe }), filters: [{ name: exe, extensions: ["exe"] }], purpose: "games" });
  if (!path) return null;
  return api.integratedLocate(gameId, path);
}

/** Texto curto do download em andamento (botão principal e bloco NexStage). */
export function jobLabel(job: IntegratedJob): string {
  const pct = job.total > 0 ? Math.floor((job.done / job.total) * 100) : 0;
  switch (job.phase) {
    case "manifest": return t("Preparando…");
    case "verifying": return t("Conferindo arquivos…");
    case "installing": return t("Instalando…");
    case "paused": return t("Pausado");
    default: return t("Baixando {p}%", { p: pct });
  }
}

/**
 * Bloco "NexStage" na página de um jogo oficialmente integrado: instalar/atualizar pelo NexStage (download do
 * repositório oficial, conferido por assinatura e SHA-256), pausar/continuar/cancelar, verificar arquivos, voltar
 * para a versão anterior, desinstalar — ou usar uma cópia já existente no PC ("Localizar instalação").
 */
export default function NexStageGamePanel({ game, onChanged }: { game: Game; onChanged: () => void }) {
  const { toast } = useSession();
  const { gameId, st, setSt } = useIntegrated(game);
  const [busy, setBusy] = useState(false);
  const [confirmUninstall, setConfirmUninstall] = useState(false);
  const [badFiles, setBadFiles] = useState<string[] | null>(null);
  useEffect(() => {
    if (!gameId) return;
    // erros e "instalado" aparecem pelo aviso global do app (App.tsx), em qualquer tela
    const done = listen<IntegratedStatus>("integrated-status", (e) => { if (e.payload.gameId === gameId && !e.payload.job) onChanged(); });
    return () => { done.then((f) => f()); };
  }, [gameId, onChanged]);
  if (!gameId) return null;

  const run = async (f: () => Promise<IntegratedStatus | null | void>, ok?: string) => {
    setBusy(true);
    try {
      const r = await f();
      if (r) { setSt(r); onChanged(); }
      if (ok) toast(ok);
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  };
  const job = st?.job ?? null;
  const pct = job && job.total > 0 ? Math.min(100, (job.done / job.total) * 100) : 0;
  const latest = st?.latest ?? null;
  const notes = latest?.notes?.["pt-BR"] ?? latest?.notes?.en ?? "";

  return (
    <>
      <h2 className="section nx-sec">NexStage</h2>
      <div className="ra-panel nx-panel">
        <p className="muted">{t("Jogo oficial integrado ao NexStage: conquistas, estatísticas e progresso ligados ao seu perfil.")}</p>
        <div className="steam-status">
          <span className={`steam-inst ${st?.installed ? "installed" : "not_installed"}`}>{st ? (st.installed ? t("Instalado") : t("Não instalado")) : "…"}</span>
          {st?.version && <small className="muted">{t("Versão {v}", { v: st.version })}</small>}
          {st?.updateAvailable && st.installed && latest && <small className="nx-new">{t("Nova versão: {v}", { v: latest.version })}</small>}
        </div>

        {job && (
          <div className="ra-prog nx-job">
            <div className="ra-bar"><i className="soft" style={{ width: `${pct}%` }} /></div>
            <div className="ra-prog-meta">
              <span><b>{jobLabel(job)}</b>{job.total > 0 && <> · {formatBytes(job.done)} / {formatBytes(job.total)}</>}</span>
            </div>
            <div className="row">
              {job.phase === "paused"
                ? <Button variant="primary" onClick={() => run(() => api.integratedInstall(gameId))} disabled={busy}>{t("Continuar")}</Button>
                : job.phase === "downloading" && <Button onClick={() => run(() => api.integratedPause(gameId))} disabled={busy}>{t("Pausar")}</Button>}
              {(job.phase === "downloading" || job.phase === "paused" || job.phase === "manifest") && (
                <Button variant="ghost" onClick={() => run(() => api.integratedCancel(gameId), t("Download cancelado. Nada foi alterado."))} disabled={busy}>{t("Cancelar")}</Button>
              )}
            </div>
          </div>
        )}

        {!job && (
          <div className="row">
            {(!st?.installed || (st.managed && st.updateAvailable)) && (
              <Button variant="primary" onClick={() => run(() => api.integratedInstall(gameId))} disabled={busy}>
                {st?.installed ? t("Atualizar para {v}", { v: latest?.version ?? "" }) : latest ? t("Instalar ({size})", { size: formatBytes(latest.size) }) : t("Instalar")}
              </Button>
            )}
            {st?.installed && st.managed && (
              <Button onClick={() => run(() => api.integratedCheckUpdate(gameId), t("Procura concluída."))} disabled={busy}>{t("Procurar atualização")}</Button>
            )}
          </div>
        )}

        {latest && notes && (st?.updateAvailable || !st?.installed) && !job && (
          <details className="nx-notes">
            <summary>{t("Novidades da versão {v}", { v: latest.version })}</summary>
            <pre>{notes}</pre>
          </details>
        )}

        {st?.installDir && (
          <dl className="steam-dl">
            <dt>{t("Pasta")}</dt><dd title={st.installDir}>{st.installDir}</dd>
            <dt>{t("Instalação")}</dt><dd>{st.managed ? t("Gerenciada pelo NexStage") : t("Localizada manualmente")}</dd>
          </dl>
        )}

        {badFiles && (
          badFiles.length === 0
            ? <p className="muted">{t("Todos os arquivos conferem.")}</p>
            : <p className="ra-warn">{t("{n} arquivo(s) com problema: {files}. Use Reparar para baixar só esses arquivos de novo.", { n: badFiles.length, files: badFiles.join(", ") })}</p>
        )}

        {!job && st?.installed && st.managed && (
          <div className="row">
            <Button variant="ghost" onClick={() => run(async () => setBadFiles(await api.integratedVerify(gameId)))} disabled={busy}>{t("Verificar arquivos")}</Button>
            {badFiles && badFiles.length > 0 && <Button onClick={() => { setBadFiles(null); run(() => api.integratedInstall(gameId)); }} disabled={busy}>{t("Reparar")}</Button>}
            {st.previousVersion && (
              <Button variant="ghost" onClick={() => run(() => api.integratedRollback(gameId), t("Versão {v} restaurada.", { v: st.previousVersion! }))} disabled={busy}>
                {t("Voltar para {v}", { v: st.previousVersion })}
              </Button>
            )}
            {confirmUninstall ? (
              <>
                <Button variant="danger" onClick={() => { setConfirmUninstall(false); run(() => api.integratedUninstall(gameId), t("Jogo desinstalado. Seus saves e conquistas continuam guardados.")); }} disabled={busy}>{t("Confirmar desinstalação")}</Button>
                <Button variant="ghost" onClick={() => setConfirmUninstall(false)}>{t("Cancelar")}</Button>
              </>
            ) : (
              <Button variant="ghost" onClick={() => setConfirmUninstall(true)} disabled={busy}>{t("Desinstalar")}</Button>
            )}
          </div>
        )}

        {!job && !st?.managed && (
          <div className="row">
            <Button variant="ghost" onClick={() => run(() => locateIntegrated(gameId), t("Instalação localizada."))} disabled={busy}>
              {st?.installDir ? t("Trocar pasta do jogo") : t("Já tenho o jogo: localizar instalação")}
            </Button>
            {st?.installDir && (
              <Button variant="ghost" onClick={() => run(() => api.integratedUnlink(gameId), t("Pasta desvinculada (nenhum arquivo foi apagado)."))} disabled={busy}>
                {t("Desvincular pasta")}
              </Button>
            )}
          </div>
        )}
        {confirmUninstall && <p className="muted">{t("Os arquivos do jogo serão apagados. Saves, conquistas e tempo de jogo continuam guardados.")}</p>}
        <NexStageAchievements gameId={gameId} />
        <NexStageStats gameId={gameId} />
      </div>
    </>
  );
}
