import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import { ALL_FORMATS, EMULATOR_PRESETS } from "../data/emulatorPresets";
import { PLATFORMS } from "../types";
import Button from "../components/Button";
import Modal from "../components/Modal";
import Select from "../components/Select";
import ImagePicker from "../components/ImagePicker";
import EmulatorCover from "../components/EmulatorCover";
import type { Emulator } from "../types";
import { t } from "../lib/i18n";

type Draft = { id?: number; name: string; exePath: string; platforms: string[]; formats: string[]; args: string; cover: string | null; fullscreen: boolean };
const EMPTY: Draft = { name: "", exePath: "", platforms: [], formats: [], args: "", cover: null, fullscreen: false };

/** Emuladores cadastrados. A importação por pasta e o "Editar jogo" usam esta lista. */
export default function Emulators() {
  const { toast } = useSession();
  const [list, setList] = useState<Emulator[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [note, setNote] = useState<string | undefined>();
  const [confirm, setConfirm] = useState<number | null>(null);
  // O NEXSTAGE reconhece o emulador do formulário (nome/executável) e sabe aplicar a tela cheia?
  const [fsOk, setFsOk] = useState(true);
  const dName = draft?.name ?? "", dExe = draft?.exePath ?? "";
  useEffect(() => {
    if (!dName && !dExe) { setFsOk(true); return; }
    let alive = true;
    api.emulatorFullscreenSupported(dName, dExe).then((v) => { if (alive) setFsOk(v); }).catch(() => {});
    return () => { alive = false; };
  }, [dName, dExe]);

  const load = useCallback(() => api.listEmulators().then(setList).catch((e) => toast(errMsg(e), "err")), [toast]);
  useEffect(() => { load(); }, [load]);

  const toggle = (arr: string[], v: string) => (arr.includes(v) ? arr.filter((x) => x !== v) : [...arr, v]);

  async function pickExe() {
    const sel = await open({ multiple: false, directory: false, filters: [{ name: t("Executável"), extensions: ["exe", "bat", "cmd"] }] }).catch(() => null);
    if (typeof sel === "string" && draft) setDraft({ ...draft, exePath: sel });
  }
  async function save() {
    if (!draft) return;
    try { await api.saveEmulator(draft); toast(t("Emulador salvo.")); setDraft(null); load(); }
    catch (e) { toast(errMsg(e), "err"); }
  }

  return (
    <>
      <div className="page-head">
        <h1>{t("Emuladores")}</h1>
        <Button variant="primary" onClick={() => { setDraft({ ...EMPTY }); setNote(undefined); }}>{t("Adicionar emulador")}</Button>
      </div>
      {list.length === 0 ? (
        <div className="empty">
          <p>{t("Nenhum emulador cadastrado. Cadastre o executável de cada emulador e as plataformas/formatos que ele suporta: o NEXSTAGE associa os jogos importados automaticamente.")}</p>
          <Button variant="primary" onClick={() => { setDraft({ ...EMPTY }); setNote(undefined); }}>{t("Adicionar emulador")}</Button>
        </div>
      ) : (
        <div className="emu-list">
          {list.map((e) => (
            <article key={e.id} className="emu">
              <EmulatorCover name={e.name} platforms={e.platforms} cover={e.cover} />
              <div className="emu-body">
              <header><h3>{e.name}</h3><span className="muted">{e.games} {t("jogo(s) associado(s)")}</span></header>
              <p className="path">{e.exePath}{!e.exeExists && <b className="warn"> {t("· executável não encontrado")}</b>}</p>
              <div className="chips">{e.platforms.map((p) => <span key={p}>{p}</span>)}{e.formats.map((f) => <span key={f} className="fmt">.{f}</span>)}</div>
              {e.args && <p className="muted path">{t("Argumentos:")}{" "}{e.args}</p>}
              {e.fullscreen && <p className="muted">{t("Inicia os jogos em tela cheia")}</p>}
              <div className="row">
                <Button onClick={() => { setDraft({ id: e.id, name: e.name, exePath: e.exePath, platforms: e.platforms, formats: e.formats, args: e.args, cover: e.cover, fullscreen: e.fullscreen }); setNote(undefined); }}>{t("Editar")}</Button>
                {confirm === e.id ? (
                  <Button variant="danger" onClick={async () => {
                    try { await api.deleteEmulator(e.id); setConfirm(null); toast(t("Emulador removido. Os jogos foram mantidos, sem associação.")); load(); }
                    catch (er) { toast(errMsg(er), "err"); }
                  }}>{t("Confirmar remoção")}</Button>
                ) : <Button variant="danger" onClick={() => setConfirm(e.id)}>{t("Remover")}</Button>}
              </div>
              </div>
            </article>
          ))}
        </div>
      )}

      {draft && (
        <Modal title={draft.id ? t("Editar emulador") : t("Adicionar emulador")} onClose={() => setDraft(null)} wide>
          <div className="form-grid">
          <div className="emu-cover-pick">
            <span className="muted">{t("Capa")}</span>
            {draft.cover ? null : <EmulatorCover name={draft.name || "Emulador"} platforms={draft.platforms} cover={null} className="preview" />}
            <ImagePicker label={t("Capa")} value={draft.cover} onChange={(v) => setDraft({ ...draft, cover: v })} maxW={600} maxH={800} shape="tall" />
            {!draft.cover && <p className="muted note">{t("Sem imagem personalizada: usa a arte padrão do console.")}</p>}
          </div>
          <div className="fields">
            {!draft.id && (
              <label>{t("Começar por uma sugestão")}<Select<string> value="" width="100%" title={t("Sugestões de emuladores")} onChange={(v) => {
                  const p = EMULATOR_PRESETS.find((x) => x.name === v);
                  if (p) { setDraft({ ...draft, name: t(p.name), platforms: p.platforms, formats: p.formats, args: p.args, fullscreen: p.fullscreen }); setNote(p.note ? t(p.note) : p.note); }
                }} options={[[t("Personalizado (preencher à mão)"), ""], ...EMULATOR_PRESETS.map((p): [string, string] => [t(p.name), p.name])]} />
              </label>
            )}
            {note && <p className="muted note">{note}</p>}
            <label>{t("Nome")}<input value={draft.name} maxLength={40} onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></label>
            <label>{t("Executável do emulador")}<div className="row"><input value={draft.exePath} readOnly placeholder={t("Nenhum arquivo selecionado")} /><Button onClick={pickExe}>{t("Selecionar")}</Button></div>
            </label>
            <div><span className="muted">{t("Plataformas suportadas")}</span>
              <div className="chips pick">{PLATFORMS.map((p) => (
                <button key={p} type="button" data-focus className={`chip ${draft.platforms.includes(p) ? "on" : ""}`} onClick={() => setDraft({ ...draft, platforms: toggle(draft.platforms, p) })}>{p}</button>
              ))}</div></div>
            <div><span className="muted">{t("Formatos suportados")}</span>
              <div className="chips pick">{ALL_FORMATS.map((f) => (
                <button key={f} type="button" data-focus className={`chip ${draft.formats.includes(f) ? "on" : ""}`} onClick={() => setDraft({ ...draft, formats: toggle(draft.formats, f) })}>.{f}</button>
              ))}</div></div>
            <label>{t("Argumentos de inicialização")}<input value={draft.args} placeholder={t("Use {rom} para o caminho do jogo. Ex.: -fullscreen \"{rom}\"")} onChange={(e) => setDraft({ ...draft, args: e.target.value })} />
            </label>
            <label className="ra-check"><input type="checkbox" checked={draft.fullscreen && fsOk} disabled={!fsOk} data-focus
                onChange={(e) => setDraft({ ...draft, fullscreen: e.target.checked })} /> {t("Iniciar jogos em tela cheia")}</label>
            <p className="muted note">{fsOk
              ? t("Vale para todos os jogos associados a este emulador. O NEXSTAGE usa o método que este emulador entende (argumento ou configuração do próprio emulador); argumentos de tela cheia escritos à mão são substituídos.")
              : t("Emulador não reconhecido: o NEXSTAGE não sabe como abri-lo em tela cheia. Escreva o argumento de tela cheia dele nos argumentos acima.")}</p>
            <p className="muted note">{t("Sugestões de argumentos variam por versão: confira a documentação do emulador. Você pode substituir os argumentos de um jogo específico em “Editar jogo”.")}</p>
          </div>
          </div>
          <div className="row end">
            <Button variant="ghost" onClick={() => setDraft(null)}>{t("Cancelar")}</Button>
            <Button variant="primary" onClick={save}>{t("Salvar")}</Button>
          </div>
        </Modal>
      )}
    </>
  );
}
