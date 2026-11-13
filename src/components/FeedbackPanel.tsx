import { useEffect, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import Select from "./Select";
import { api, errMsg, FeedbackAttachment, FeedbackCategory, FeedbackInfo, FeedbackInput } from "../lib/api";
import { formatBytes } from "../lib/format";
import { currentLang, t, tk } from "../lib/i18n";

const CATEGORIES: [string, FeedbackCategory][] = [
  [tk("Sugestão de melhoria"), "improvement"],
  [tk("Nova funcionalidade"), "feature"],
  [tk("Problema ou erro"), "bug"],
  [tk("Comentário geral"), "general"],
];

/** O que cada anexo contém (mostrado antes de o usuário decidir). */
const FILE_INFO: Record<string, string> = {
  "diagnostico.txt": tk("Versão do NEXSTAGE e do Windows, idioma, quantidade de jogos, perfis e emuladores, integrações ativas (só sim/não), modelos de controle conectados e situação do backup automático."),
  "nexstage.log": tk("Registro de erros do NEXSTAGE (últimos 48 KB): abertura do app, erros exibidos na tela e falhas internas."),
};

type Step = "form" | "confirm" | "sending" | "done" | "error";

/** Data e hora locais com o fuso (ex.: 12/11/2026 19:30 (UTC−03:00)). */
function nowLabel(): string {
  const d = new Date();
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? "+" : "−";
  const hh = String(Math.floor(Math.abs(off) / 60)).padStart(2, "0");
  const mm = String(Math.abs(off) % 60).padStart(2, "0");
  return `${d.toLocaleString(currentLang(), { dateStyle: "short", timeStyle: "short" })} (UTC${sign}${hh}:${mm})`;
}

/**
 * Feedback e relatórios: o usuário escreve, escolhe se anexa os arquivos de diagnóstico (revisando o
 * conteúdo exato), confirma e envia. O envio roda no backend; a janela pode ser fechada a qualquer momento.
 */
export default function FeedbackPanel({ onClose }: { onClose: () => void }) {
  const [info, setInfo] = useState<FeedbackInfo | null>(null);
  const [category, setCategory] = useState<FeedbackCategory>("improvement");
  const [title, setTitle] = useState("");
  const [comments, setComments] = useState("");
  const [problem, setProblem] = useState("");
  const [attach, setAttach] = useState(false);
  const [files, setFiles] = useState<FeedbackAttachment[] | null>(null);
  const [loadingFiles, setLoadingFiles] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  const [step, setStep] = useState<Step>("form");
  const [error, setError] = useState("");
  const [issue, setIssue] = useState<number | null>(null);

  useEffect(() => { api.feedbackInfo().then(setInfo).catch(() => {}); }, []);

  // os anexos são gerados quando a opção é ligada (e de novo a cada vez, para refletir o log atual)
  useEffect(() => {
    if (!attach) { setFiles(null); setOpen(null); return; }
    setLoadingFiles(true);
    api.feedbackAttachments().then(setFiles).catch((e) => { setFiles([]); setError(errMsg(e)); }).finally(() => setLoadingFiles(false));
  }, [attach]);

  const titleOk = title.trim().length >= 3;
  const textOk = comments.trim().length > 0 || problem.trim().length > 0;
  const problemMsg = !titleOk ? t("Escreva um título com pelo menos 3 letras.") : !textOk ? t("Escreva um comentário ou a descrição do problema.") : "";
  const canReview = !!info?.configured && !problemMsg && !(attach && (loadingFiles || !files));

  async function send() {
    setStep("sending");
    setError("");
    const input: FeedbackInput = {
      category, title: title.trim(), comments: comments.trim(), problem: problem.trim(),
      sentAt: nowLabel(), language: currentLang(),
      // exatamente o que o usuário revisou
      attachments: attach && files ? files.map((f) => ({ name: f.name, content: f.content })) : [],
    };
    try {
      const r = await api.feedbackSend(input);
      setIssue(r.id);
      setStep("done");
    } catch (e) {
      setError(errMsg(e));
      setStep("error");
    }
  }

  const catLabel = t(CATEGORIES.find((c) => c[1] === category)![0]);

  if (step === "confirm") {
    return (
      <Modal title={t("Confirmar envio")} onClose={() => setStep("form")} wide>
        <p>{t("Revise o que será enviado ao desenvolvedor do NEXSTAGE:")}</p>
        <div className="settings">
          <div className="setting"><div><strong>{t("Categoria")}</strong><small>{catLabel}</small></div></div>
          <div className="setting"><div><strong>{t("Título ou assunto")}</strong><small>{title.trim()}</small></div></div>
          <div className="setting"><div><strong>{t("Versão")}</strong><small>NEXSTAGE {info?.version}</small></div></div>
          <div className="setting">
            <div><strong>{t("Anexos")}</strong><small>{attach && files?.length ? files.map((f) => `${f.name} (${formatBytes(f.bytes)})`).join(" · ") : t("Nenhum anexo")}</small></div>
          </div>
        </div>
        <p className="muted">{t("A data e o horário do envio também são incluídos. Nada mais é coletado.")}</p>
        <div className="row end">
          <Button variant="ghost" onClick={() => setStep("form")}>{t("Voltar")}</Button>
          <Button variant="primary" data-autofocus onClick={send}>{t("Enviar")}</Button>
        </div>
      </Modal>
    );
  }

  if (step === "sending" || step === "done" || step === "error") {
    return (
      <Modal title={t("Feedback e relatórios")} onClose={onClose}>
        {step === "sending" && (
          <div className="bk-progress" role="progressbar" aria-label={t("Enviando…")}>
            <p>{t("Enviando…")}</p>
            <div className="bk-track indet"><i /></div>
            <p className="muted">{t("Você pode fechar esta janela e continuar usando o NEXSTAGE; o envio termina sozinho.")}</p>
          </div>
        )}
        {step === "done" && (
          <>
            <p><strong>{t("Feedback enviado. Obrigado!")}</strong></p>
            {issue !== null && <p className="muted">{t("Número do relato: #{n}", { n: issue })}</p>}
            <div className="row end"><Button variant="primary" data-autofocus onClick={onClose}>{t("Fechar")}</Button></div>
          </>
        )}
        {step === "error" && (
          <>
            <p className="bk-error" role="alert">{t("Não foi possível enviar.")} {error}</p>
            <p className="muted">{t("O que você escreveu foi mantido.")}</p>
            <div className="row end">
              <Button variant="ghost" onClick={() => setStep("form")}>{t("Voltar e editar")}</Button>
              <Button variant="primary" data-autofocus onClick={send}>{t("Tentar novamente")}</Button>
            </div>
          </>
        )}
      </Modal>
    );
  }

  return (
    <Modal title={t("Feedback e relatórios")} onClose={onClose} wide>
      <p className="muted">{t("Conte o que achou, sugira melhorias ou relate um problema. O envio é voluntário e não precisa de conta.")}</p>
      {info && !info.configured && <p className="bk-note">{t("O envio de feedback ainda não foi configurado nesta versão do NEXSTAGE.")}</p>}
      <div className="fields" style={{ marginTop: 12 }}>
        <label>{t("Categoria")}
          <Select<FeedbackCategory> value={category} width="100%" title={t("Categoria")} onChange={setCategory}
            options={CATEGORIES.map(([l, v]): [string, FeedbackCategory] => [t(l), v])} />
        </label>
        <label>{t("Título ou assunto")}
          <input type="text" data-autofocus value={title} maxLength={120} onChange={(e) => setTitle(e.target.value)} placeholder={t("Resuma em uma frase")} />
        </label>
        <label>{t("Comentários, sugestões ou ideias")}
          <textarea rows={4} value={comments} maxLength={8000} onChange={(e) => setComments(e.target.value)} />
        </label>
        <label>{t("Descrição do problema (se houver)")}
          <textarea rows={4} value={problem} maxLength={8000} onChange={(e) => setProblem(e.target.value)}
            placeholder={t("O que aconteceu, o que você esperava e como repetir o problema.")} />
        </label>
      </div>

      <div className="setting" style={{ marginTop: 14 }}>
        <div>
          <strong>{t("Anexar arquivos de log e diagnóstico")}</strong>
          <small>{t("Opcional. Ajuda a identificar erros. Nada é anexado sem esta opção ligada.")}</small>
        </div>
        <Button variant={attach ? "primary" : "default"} role="switch" aria-checked={attach} onClick={() => setAttach(!attach)}>{attach ? t("Ligado") : t("Desligado")}</Button>
      </div>
      {attach && (
        <div className="bk-warn">
          <p>{t("Serão enviados os arquivos abaixo. Nome de usuário do Windows, e-mails, chaves, senhas, IDs da Steam e endereços de rede são removidos automaticamente. Abra cada um para revisar o conteúdo exato.")}</p>
          {loadingFiles && <p className="muted">{t("Preparando os anexos…")}</p>}
          {files?.length === 0 && !loadingFiles && <p className="muted">{t("Nenhum arquivo de diagnóstico disponível.")}</p>}
          {files?.map((f) => (
            <div key={f.name}>
              <div className="setting">
                <div><strong>{f.name}</strong><small>{FILE_INFO[f.name] ? t(FILE_INFO[f.name]) : f.description} · {formatBytes(f.bytes)}</small></div>
                <Button variant="ghost" aria-expanded={open === f.name} onClick={() => setOpen(open === f.name ? null : f.name)}>{open === f.name ? t("Ocultar") : t("Revisar")}</Button>
              </div>
              {open === f.name && (
                <pre tabIndex={0} data-focus style={{ maxHeight: 240, overflow: "auto", whiteSpace: "pre-wrap", wordBreak: "break-word", fontSize: ".78rem", padding: 10, borderRadius: 10, background: "rgba(0,0,0,.3)" }}>{f.content}</pre>
              )}
            </div>
          ))}
        </div>
      )}

      <p className="muted">{t("Não escreva senhas nem dados pessoais nos campos acima.")}</p>
      {problemMsg && (title || comments || problem) ? <p className="bk-error" role="alert">{problemMsg}</p> : null}
      <div className="row end">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="primary" disabled={!canReview} onClick={() => setStep("confirm")}>{t("Revisar e enviar")}</Button>
      </div>
    </Modal>
  );
}
