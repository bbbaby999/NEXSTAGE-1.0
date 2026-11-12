import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import Modal from "./Modal";
import Button from "./Button";
import Seg from "./Seg";
import Select from "./Select";
import { api, errMsg } from "../lib/api";
import { formatBytes } from "../lib/format";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { BackupAuto, BackupAutoMode, BackupInfo, BackupInspect, BackupProgress, BackupReport, BackupRestoreReport, BackupVerify } from "../types";

const CANCELED = "Operação cancelada.";
const isCanceled = (e: unknown) => typeof e === "string" && e === CANCELED;

/** Nome da etapa exibido durante o progresso. */
export function phaseLabel(p: BackupProgress | null): string {
  switch (p?.phase) {
    case "snapshot": return t("Copiando o banco de dados com segurança…");
    case "files": return t("Salvando perfis, imagens e capturas…");
    case "verify": return t("Verificando a integridade dos arquivos…");
    case "stage": return t("Lendo e conferindo os dados do backup…");
    case "validate": return t("Validando o banco de dados restaurado…");
    case "safety": return t("Criando backup de segurança dos dados atuais…");
    case "apply": return t("Aplicando os dados restaurados…");
    default: return t("Preparando…");
  }
}

/** Barra de progresso (indeterminada quando a etapa não tem tamanho conhecido). */
export function BackupProgressBar({ p }: { p: BackupProgress | null }) {
  const pct = p && p.total > 0 ? Math.min(100, Math.round((p.done / p.total) * 100)) : null;
  return (
    <div className="bk-progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct ?? undefined} aria-label={phaseLabel(p)}>
      <p>{phaseLabel(p)}</p>
      <div className={`bk-track ${pct === null ? "indet" : ""}`}><i style={pct === null ? undefined : { width: `${pct}%` }} /></div>
      <div className="bk-meta">
        <span>{pct === null ? "…" : `${pct}%`}</span>
        <span>{p && p.total > 0 ? `${formatBytes(p.done)} / ${formatBytes(p.total)}` : ""}</span>
      </div>
    </div>
  );
}

/** Acompanha o evento `backup-progress` enquanto `on` (opcionalmente só de um tipo). */
export function useBackupProgress(on: boolean, kind?: string) {
  const [p, setP] = useState<BackupProgress | null>(null);
  useEffect(() => {
    if (!on) { setP(null); return; }
    let un: (() => void) | undefined, cancelled = false;
    listen<BackupProgress>("backup-progress", (e) => { if (!kind || e.payload.kind === kind) setP(e.payload); })
      .then((f) => (cancelled ? f() : (un = f)));
    return () => { cancelled = true; un?.(); };
  }, [on, kind]);
  return p;
}

const fmtDate = (s: string | null) => {
  if (!s) return "";
  const d = new Date(s.replace(" ", "T"));
  return isNaN(d.getTime()) ? s : d.toLocaleString();
};
const hours = (s: number) => Math.round(s / 360) / 10;

type Dlg =
  | { t: "create" }
  | { t: "replace"; dir: string; path: string; pw: string | null; caps: boolean }
  | { t: "progress"; title: string }
  | { t: "created"; r: BackupReport; auto?: boolean }
  | { t: "password"; path: string; error?: string }
  | { t: "confirm"; info: BackupInspect; verify: BackupVerify; pw: string | null }
  | { t: "restored"; r: BackupRestoreReport }
  | { t: "error"; title: string; msg: string }
  | { t: "auto-password" };

/** Senha (com confirmação) usada na criação e no backup automático. Vazio = sem senha. */
function PasswordFields({ pw, setPw, pw2, setPw2 }: { pw: string; setPw: (v: string) => void; pw2: string; setPw2: (v: string) => void }) {
  return (
    <div className="fields">
      <label>{t("Senha do backup (opcional)")}
        <input type="password" value={pw} maxLength={256} autoComplete="new-password" onChange={(e) => setPw(e.target.value)} />
      </label>
      {pw && (
        <label>{t("Confirme a senha")}
          <input type="password" value={pw2} maxLength={256} autoComplete="new-password" onChange={(e) => setPw2(e.target.value)} />
        </label>
      )}
    </div>
  );
}
const pwProblem = (pw: string, pw2: string) =>
  !pw ? null : pw.length < 8 ? t("A senha do backup deve ter pelo menos 8 caracteres.") : pw !== pw2 ? t("As senhas não coincidem.") : null;

/** Criar backup: aviso, opções (senha e capturas) e escolha do local. */
function CreateDialog({ info, onClose, onStart }: {
  info: BackupInfo | null; onClose: () => void; onStart: (pw: string | null, caps: boolean) => void;
}) {
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [caps, setCaps] = useState(true);
  const problem = pwProblem(pw, pw2);
  return (
    <Modal title={t("Criar backup")} onClose={onClose} wide>
      <div className="bk-warn">
        <p>{t("Este backup salva seus perfis, configurações, biblioteca, capturas, estatísticas e demais dados pessoais do NEXSTAGE. Ele foi desenvolvido para permitir a transferência do conteúdo do aplicativo para outro computador.")}</p>
        <p>{t("Jogos instalados e emuladores não serão incluídos. Os caminhos e associações poderão ser preservados, mas os programas e arquivos originais precisarão existir no computador de destino.")}</p>
      </div>
      <p className="muted">{t("Não são copiados: jogos instalados e seus arquivos, emuladores e executáveis, BIOS e arquivos externos de emulação, instaladores de programas.")}</p>
      <h3 className="set-sub">{t("Proteção")}</h3>
      <p className="muted">{t("Com senha, todo o backup é criptografado (AES-256) e as chaves de API e as contas de integração são incluídas. Sem senha, essas credenciais ficam de fora e precisarão ser informadas de novo no outro computador.")}</p>
      {info && info.secrets > 0 && !pw && (
        <p className="bk-note">{t("Há {n} credencial(is) salvas no NEXSTAGE que não entrarão em um backup sem senha.", { n: info.secrets })}</p>
      )}
      <PasswordFields pw={pw} setPw={setPw} pw2={pw2} setPw2={setPw2} />
      {problem && <p className="bk-error" role="alert">{problem}</p>}
      <div className="setting" style={{ marginTop: 14 }}>
        <div><strong>{t("Incluir a pasta de capturas originais")}</strong><small className="path">{info?.capturesDir}</small></div>
        <Button variant={caps ? "primary" : "default"} onClick={() => setCaps(!caps)}>{caps ? t("Incluir") : t("Não incluir")}</Button>
      </div>
      <p className="muted">{t("A Galeria de cada perfil é sempre incluída.")}</p>
      <div className="row end">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="primary" disabled={!!problem} onClick={() => onStart(pw || null, caps)}>{t("Escolher local e criar")}</Button>
      </div>
    </Modal>
  );
}

function SummaryGrid({ i }: { i: BackupInspect }) {
  const s = i.summary;
  const items: [string, string | number][] = [
    [t("Perfis"), s.profiles],
    [t("Jogos"), s.games],
    [t("Horas jogadas"), hours(s.playtimeSeconds)],
    [t("Sessões"), s.sessions],
    [t("Coleções"), s.collections],
    [t("Capturas na Galeria"), s.gallery],
    [t("Imagens de jogos"), s.images],
    [t("Capturas originais"), s.captures],
    [t("Emuladores"), s.emulators],
    [t("Conquistas"), s.achievements],
    [t("Integrações"), s.accounts],
    [t("Tamanho"), formatBytes(i.totalBytes)],
  ];
  return (
    <div className="bk-grid">
      {items.map(([k, v]) => <div key={k}><b>{v}</b><small>{k}</small></div>)}
    </div>
  );
}

/**
 * Configurações → Backup e restauração: criar backup, carregar backup e backup automático.
 * Todas as operações rodam no backend (fora da interface) com progresso e cancelamento seguro.
 */
export default function BackupPanel() {
  const { toast } = useSession();
  const [dlg, setDlg] = useState<Dlg | null>(null);
  const [info, setInfo] = useState<BackupInfo | null>(null);
  const [auto, setAuto] = useState<BackupAuto | null>(null);
  const [pwInput, setPwInput] = useState("");
  const [apw, setApw] = useState("");
  const [apw2, setApw2] = useState("");
  const progress = useBackupProgress(dlg?.t === "progress");

  const loadAuto = useCallback(() => { api.backupAutoGet().then(setAuto).catch(() => {}); }, []);
  useEffect(() => {
    loadAuto();
    api.backupInfo().then(setInfo).catch(() => {});
    let un: (() => void) | undefined, cancelled = false;
    listen("backup-auto-done", loadAuto).then((f) => (cancelled ? f() : (un = f)));
    return () => { cancelled = true; un?.(); };
  }, [loadAuto]);

  const fail = (title: string, e: unknown) => {
    if (isCanceled(e)) { toast(t("Operação cancelada. Nenhum dado foi alterado.")); setDlg(null); return; }
    setDlg({ t: "error", title, msg: errMsg(e) });
  };
  const pickDir = async (title: string) => {
    const sel = await open({ directory: true, multiple: false, title }).catch(() => null);
    return typeof sel === "string" ? sel : null;
  };

  /* ---------- Criar ---------- */
  async function runCreate(dir: string, pw: string | null, caps: boolean, replace: boolean) {
    setDlg({ t: "progress", title: t("Criando backup") });
    try { setDlg({ t: "created", r: await api.backupCreate(dir, pw, caps, replace) }); }
    catch (e) { fail(t("Não foi possível criar o backup"), e); }
  }
  async function startCreate(pw: string | null, caps: boolean) {
    const dir = await pickDir(t("Escolha onde salvar o backup"));
    if (!dir) return;
    try {
      const st = await api.backupTargetStatus(dir);
      if (st.status === "conflict") return setDlg({ t: "error", title: t("Local indisponível"), msg: t("Já existe uma pasta “NEXSTAGE_Backup” nesse local que não é um backup do NEXSTAGE. Escolha outro local.") });
      if (st.status === "replace") return setDlg({ t: "replace", dir, path: st.path, pw, caps });
      await runCreate(dir, pw, caps, false);
    } catch (e) { fail(t("Não foi possível criar o backup"), e); }
  }

  /* ---------- Carregar ---------- */
  async function inspect(path: string, pw: string | null) {
    setDlg({ t: "progress", title: t("Analisando o backup") });
    try {
      const i = await api.backupInspect(path, pw);
      if (i.needsPassword) { setPwInput(""); return setDlg({ t: "password", path: i.path }); }
      if (i.problem) return setDlg({ t: "error", title: t("Backup incompatível com esta versão"), msg: `${i.problem} ${t("Nenhum dado foi alterado.")}` });
      setDlg({ t: "progress", title: t("Verificando o backup") });
      const v = await api.backupVerify(i.path);
      setDlg({ t: "confirm", info: i, verify: v, pw });
    } catch (e) {
      if (pw && typeof e === "string" && e === "Senha incorreta.") { setPwInput(""); return setDlg({ t: "password", path, error: t("Senha incorreta.") }); }
      fail(t("Não foi possível carregar o backup"), e);
    }
  }
  async function startLoad() {
    const dir = await pickDir(t("Selecione a pasta do backup (NEXSTAGE_Backup)"));
    if (dir) await inspect(dir, null);
  }
  async function runRestore(i: BackupInspect, pw: string | null) {
    setDlg({ t: "progress", title: t("Restaurando backup") });
    try { setDlg({ t: "restored", r: await api.backupRestore(i.path, pw) }); }
    catch (e) { fail(t("A restauração não foi concluída"), e); }
  }

  /* ---------- Automático ---------- */
  async function saveAuto(mode: BackupAutoMode, dir: string | null, keep: number, caps: boolean) {
    try { setAuto(await api.backupAutoSet(mode, dir, keep, caps)); }
    catch (e) { toast(errMsg(e), "err"); }
  }
  async function setMode(mode: BackupAutoMode) {
    if (!auto) return;
    let dir: string | null = null;
    if (mode !== "never" && !auto.dir) {
      dir = await pickDir(t("Escolha a pasta dos backups automáticos"));
      if (!dir) return; // sem pasta, continua desativado
    }
    await saveAuto(mode, dir, auto.keep, auto.includeCaptures);
    if (mode === "weekly") toast(t("Backup semanal ativado. O primeiro será criado em alguns minutos."));
  }
  async function changeAutoDir() {
    if (!auto) return;
    const dir = await pickDir(t("Escolha a pasta dos backups automáticos"));
    if (dir) await saveAuto(auto.mode, dir, auto.keep, auto.includeCaptures);
  }
  async function runAutoNow() {
    setDlg({ t: "progress", title: t("Criando backup automático") });
    try { const r = await api.backupAutoRun(); loadAuto(); setDlg({ t: "created", r, auto: true }); }
    catch (e) { loadAuto(); fail(t("Não foi possível criar o backup"), e); }
  }

  const reveal = (p: string) => api.backupReveal(p).catch((e) => toast(errMsg(e), "err"));
  const apwProblem = pwProblem(apw, apw2);

  return (
    <>
      <div className="setting">
        <div><strong>{t("Criar backup")}</strong><small>{t("Salva perfis, configurações, biblioteca, estatísticas, capturas e integrações em uma pasta “NEXSTAGE_Backup”, para levar a outro computador.")}</small></div>
        <Button variant="primary" onClick={() => setDlg({ t: "create" })}>{t("Criar backup")}</Button>
      </div>
      <div className="setting">
        <div><strong>{t("Carregar backup")}</strong><small>{t("Substitui os dados atuais pelos de um backup criado pelo NEXSTAGE. Antes, um backup de segurança dos dados atuais é criado automaticamente.")}</small></div>
        <Button onClick={startLoad}>{t("Carregar backup")}</Button>
      </div>
      <div className="setting">
        <div><strong>{t("Backup automático")}</strong><small>{t("“Antes de encerrar” roda ao sair do NEXSTAGE e ao reiniciar ou desligar o computador pelo NEXSTAGE (desligar direto pelo Windows com o app aberto não espera o backup).")}</small></div>
        <Seg<BackupAutoMode> value={auto?.mode ?? "never"} onChange={setMode}
          opts={[[t("Nunca"), "never"], [t("Antes de encerrar"), "exit"], [t("Uma vez por semana"), "weekly"]]} />
      </div>
      {auto && auto.mode !== "never" && (
        <>
          <div className="setting">
            <div><strong>{t("Pasta dos backups automáticos")}</strong><small className="path">{auto.dir ?? "—"}<br />{t("Cada backup ganha uma pasta com data e hora (ex.: NEXSTAGE_Backup_2026-11-09_19-30); nenhum é sobrescrito.")}</small></div>
            <div className="row">
              {auto.dir && <Button variant="ghost" onClick={() => reveal(auto.dir!)}>{t("Abrir pasta")}</Button>}
              <Button onClick={changeAutoDir}>{t("Alterar")}</Button>
            </div>
          </div>
          <div className="setting">
            <div><strong>{t("Último backup automático")}</strong>
              <small>{auto.last ? fmtDate(auto.last) : t("Nenhum ainda.")}
                {auto.lastError && <><br /><span className="bk-error">{t("Última tentativa falhou: {v0}", { v0: auto.lastError })}</span></>}</small></div>
            <Button onClick={runAutoNow}>{t("Criar agora")}</Button>
          </div>
          <div className="setting">
            <div><strong>{t("Manter backups automáticos")}</strong><small>{auto.keep === 0
              ? t("Nenhum backup é excluído automaticamente. Apague os antigos manualmente quando quiser.")
              : t("Ao criar um novo, os backups automáticos mais antigos além dos {n} mais recentes são excluídos dessa pasta. Backups manuais nunca são excluídos.", { n: auto.keep })}</small></div>
            <Select<number> value={auto.keep} width={220} title={t("Manter backups automáticos")}
              onChange={(k) => saveAuto(auto.mode, null, k, auto.includeCaptures)}
              options={[[t("Todos (não excluir)"), 0], [t("Os 3 mais recentes"), 3], [t("Os 5 mais recentes"), 5], [t("Os 10 mais recentes"), 10], [t("Os 20 mais recentes"), 20]]} />
          </div>
          <div className="setting">
            <div><strong>{t("Senha dos backups automáticos")}</strong><small>{auto.hasPassword
              ? t("Protegidos por senha (guardada com a proteção do Windows neste computador). Inclui as credenciais.")
              : t("Sem senha: os backups automáticos não incluem chaves de API nem credenciais.")}</small></div>
            <div className="row">
              {auto.hasPassword && <Button variant="ghost" onClick={() => api.backupAutoPassword(null).then((v) => { setAuto(v); toast(t("Senha removida.")); }).catch((e) => toast(errMsg(e), "err"))}>{t("Remover")}</Button>}
              <Button onClick={() => { setApw(""); setApw2(""); setDlg({ t: "auto-password" }); }}>{auto.hasPassword ? t("Alterar") : t("Definir senha")}</Button>
            </div>
          </div>
          <div className="setting">
            <div><strong>{t("Incluir capturas originais")}</strong><small>{t("Também copia a pasta Imagens\\NEXSTAGE\\Capturas em cada backup automático (ocupa mais espaço). A Galeria é sempre incluída.")}</small></div>
            <Button variant={auto.includeCaptures ? "primary" : "default"} onClick={() => saveAuto(auto.mode, null, auto.keep, !auto.includeCaptures)}>
              {auto.includeCaptures ? t("Incluir") : t("Não incluir")}</Button>
          </div>
        </>
      )}

      {dlg?.t === "create" && <CreateDialog info={info} onClose={() => setDlg(null)} onStart={startCreate} />}

      {dlg?.t === "replace" && (
        <Modal title={t("Substituir backup existente?")} onClose={() => setDlg(null)}>
          <p>{t("Já existe um backup do NEXSTAGE em:")}</p>
          <p className="path bk-path">{dlg.path}</p>
          <p className="muted">{t("Ele só será apagado depois que o novo backup estiver completo.")}</p>
          <div className="row end">
            <Button variant="ghost" onClick={() => setDlg(null)}>{t("Cancelar")}</Button>
            <Button variant="danger" onClick={() => runCreate(dlg.dir, dlg.pw, dlg.caps, true)}>{t("Substituir")}</Button>
          </div>
        </Modal>
      )}

      {dlg?.t === "progress" && (
        <Modal title={dlg.title} onClose={() => { if (progress?.cancelable) api.backupCancel(); }}>
          <BackupProgressBar p={progress} />
          <p className="muted">{progress && !progress.cancelable && progress.phase === "apply"
            ? t("Não feche o NEXSTAGE: os dados estão sendo substituídos.")
            : t("Você pode continuar usando o NEXSTAGE enquanto isso.")}</p>
          <div className="row end">
            <Button variant="ghost" disabled={!progress?.cancelable} onClick={() => api.backupCancel()}>{t("Cancelar")}</Button>
          </div>
        </Modal>
      )}

      {dlg?.t === "created" && (
        <Modal title={t("Backup concluído")} onClose={() => setDlg(null)}>
          <p>{t("O backup foi salvo em:")}</p>
          <p className="path bk-path">{dlg.r.path}</p>
          <p className="muted">{t("{n} arquivos · {size}", { n: dlg.r.files, size: formatBytes(dlg.r.bytes) })}{" · "}
            {dlg.r.encrypted ? t("Protegido por senha") : t("Sem senha")}</p>
          {dlg.r.secrets === "excluded" && dlg.r.secretsSkipped > 0 && (
            <p className="bk-note">{t("{n} credencial(is) não foram incluídas (backup sem senha). Informe-as de novo no computador de destino.", { n: dlg.r.secretsSkipped })}</p>
          )}
          {dlg.r.secrets === "included" && dlg.r.secretsSkipped > 0 && (
            <p className="bk-note">{t("{n} credencial(is) não puderam ser lidas neste computador e ficaram de fora.", { n: dlg.r.secretsSkipped })}</p>
          )}
          {dlg.r.missing > 0 && <p className="bk-note">{t("{n} captura(s) da Galeria não existiam mais no disco e não foram incluídas.", { n: dlg.r.missing })}</p>}
          <div className="row end">
            <Button variant="ghost" onClick={() => reveal(dlg.r.path)}>{t("Abrir pasta")}</Button>
            <Button variant="primary" onClick={() => setDlg(null)}>{t("Concluir")}</Button>
          </div>
        </Modal>
      )}

      {dlg?.t === "password" && (
        <Modal title={t("Backup protegido por senha")} onClose={() => setDlg(null)}>
          <p className="muted">{t("Informe a senha definida ao criar este backup.")}</p>
          <label>{t("Senha")}
            <input type="password" value={pwInput} maxLength={256} autoComplete="current-password" onChange={(e) => setPwInput(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && pwInput && inspect(dlg.path, pwInput)} />
          </label>
          {dlg.error && <p className="bk-error" role="alert">{dlg.error}</p>}
          <div className="row end">
            <Button variant="ghost" onClick={() => setDlg(null)}>{t("Cancelar")}</Button>
            <Button variant="primary" disabled={!pwInput} onClick={() => inspect(dlg.path, pwInput)}>{t("Continuar")}</Button>
          </div>
        </Modal>
      )}

      {dlg?.t === "confirm" && (
        <Modal title={t("Carregar backup")} onClose={() => setDlg(null)} wide>
          <p className="muted">
            {t("Criado em {date} · NEXSTAGE {v}", { date: fmtDate(dlg.info.createdAt), v: dlg.info.appVersion })}{" · "}
            {dlg.info.kind === "auto" ? t("Automático") : dlg.info.kind === "safety" ? t("Backup de segurança") : t("Manual")}{" · "}
            {dlg.info.encrypted ? t("Protegido por senha") : t("Sem senha")}
          </p>
          <p className="path bk-path">{dlg.info.path}</p>
          <h3 className="set-sub">{t("Dados que serão restaurados")}</h3>
          <SummaryGrid i={dlg.info} />
          {dlg.info.summary.profileNames.length > 0 && <p className="muted">{t("Perfis:")} {dlg.info.summary.profileNames.join(", ")}</p>}
          <p className="muted">{t("Também: configurações do aplicativo e de cada perfil, temas, atalhos, controles, notificações, modo ocioso, favoritos, coleções, histórico, links de mods e associações com emuladores.")}
            {dlg.info.secrets === "included" && ` ${t("Inclui chaves de API e credenciais das integrações.")}`}</p>
          {dlg.info.warnings.map((w) => <p key={w} className="bk-note">{w}</p>)}
          {dlg.verify.invalid.length > 0 && (
            <div className="bk-warn">
              <p><b>{t("{n} arquivo(s) de mídia estão danificados ou ausentes no backup e não serão restaurados:", { n: dlg.verify.invalid.length })}</b></p>
              <ul className="bk-list">{dlg.verify.invalid.slice(0, 8).map((f) => <li key={f}>{f}</li>)}{dlg.verify.invalid.length > 8 && <li>…</li>}</ul>
              <p>{t("O restante foi verificado e está íntegro.")}</p>
            </div>
          )}
          <div className="bk-warn bk-danger">
            <p><b>{t("Carregar este backup substituirá os dados atuais do NEXSTAGE pelos dados encontrados no backup selecionado. As informações atuais poderão ser perdidas.")}</b></p>
            <p>{t("Deseja continuar?")}</p>
            <p className="muted">{t("Um backup de segurança dos dados atuais será criado antes da substituição. Ao final, o NEXSTAGE será reiniciado.")}</p>
          </div>
          <div className="row end">
            <Button variant="ghost" onClick={() => setDlg(null)}>{t("Cancelar")}</Button>
            <Button variant="danger" onClick={() => runRestore(dlg.info, dlg.pw)}>{t("Confirmar e restaurar")}</Button>
          </div>
        </Modal>
      )}

      {dlg?.t === "restored" && (
        <Modal title={t("Backup restaurado")} onClose={() => {}}>
          <p>{t("Os dados foram restaurados. O NEXSTAGE precisa ser reiniciado para aplicar todas as alterações.")}</p>
          {dlg.r.secrets === "included" && dlg.r.secretsApplied > 0 && <p className="muted">{t("{n} credencial(is) foram protegidas neste computador.", { n: dlg.r.secretsApplied })}</p>}
          {dlg.r.warnings.map((w) => <p key={w} className="bk-note">{w}</p>)}
          <p className="muted">{t("Backup de segurança dos dados anteriores:")}</p>
          <p className="path bk-path">{dlg.r.safetyPath}</p>
          <p className="muted">{t("Jogos e emuladores precisam existir nos mesmos caminhos neste computador; caso contrário, ajuste-os na Biblioteca e em Emuladores.")}</p>
          <div className="row end">
            <Button variant="primary" onClick={() => api.backupRestart().catch((e) => toast(errMsg(e), "err"))}>{t("Reiniciar o NEXSTAGE")}</Button>
          </div>
        </Modal>
      )}

      {dlg?.t === "error" && (
        <Modal title={dlg.title} onClose={() => setDlg(null)}>
          <p role="alert">{dlg.msg}</p>
          <div className="row end"><Button variant="primary" onClick={() => setDlg(null)}>{t("Fechar")}</Button></div>
        </Modal>
      )}

      {dlg?.t === "auto-password" && (
        <Modal title={t("Senha dos backups automáticos")} onClose={() => setDlg(null)}>
          <p className="muted">{t("Usada para criptografar os backups automáticos. Guarde-a: ela será pedida para restaurar e não pode ser recuperada.")}</p>
          <PasswordFields pw={apw} setPw={setApw} pw2={apw2} setPw2={setApw2} />
          {apwProblem && <p className="bk-error" role="alert">{apwProblem}</p>}
          <div className="row end">
            <Button variant="ghost" onClick={() => setDlg(null)}>{t("Cancelar")}</Button>
            <Button variant="primary" disabled={!apw || !!apwProblem} onClick={() => api.backupAutoPassword(apw)
              .then((v) => { setAuto(v); setDlg(null); toast(t("Senha dos backups automáticos salva.")); })
              .catch((e) => toast(errMsg(e), "err"))}>{t("Salvar")}</Button>
          </div>
        </Modal>
      )}
    </>
  );
}
