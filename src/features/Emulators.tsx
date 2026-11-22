import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { pickOpen as open, browseFolder } from "../lib/filePicker";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import { ALL_FORMATS, EMULATOR_PRESETS, type EmulatorPreset } from "../data/emulatorPresets";
import { PLATFORMS } from "../types";
import Button from "../components/Button";
import Modal from "../components/Modal";
import Select from "../components/Select";
import Seg from "../components/Seg";
import ImagePicker from "../components/ImagePicker";
import EmulatorCover from "../components/EmulatorCover";
import type { Emulator, EmuUpdateStatus, EmuCatalogInfo, EmuFolderInfo } from "../types";
import { t } from "../lib/i18n";

type Draft = { id?: number; name: string; exePath: string; platforms: string[]; formats: string[]; args: string; cover: string | null; fullscreen: boolean; updateOn: boolean; updateSource: string };
const EMPTY: Draft = { name: "", exePath: "", platforms: [], formats: [], args: "", cover: null, fullscreen: false, updateOn: true, updateSource: "" };
type Progress = { phase: "check" | "download" | "install" | "done"; pct: number | null };
type Filter = "all" | "installed" | "missing";

const day = (iso: string | null) => (iso ? new Date(iso).toLocaleDateString() : "");

/** Linha de situação da atualização no cartão do emulador. */
function updateLine(u: EmuUpdateStatus | undefined, p: Progress | undefined): string {
  if (p && p.phase !== "done") {
    if (p.phase === "download") return t("Baixando atualização…{v0}", { v0: p.pct !== null ? ` ${p.pct}%` : "" });
    if (p.phase === "install") return t("Instalando atualização…");
    return t("Verificando…");
  }
  if (!u) return "";
  switch (u.state) {
    case "off": return t("Atualizações desligadas para este emulador.");
    case "unsupported": return t("Sem fonte de atualização conhecida: informe o repositório do emulador em Editar.");
    case "upToDate": return u.latest ? t("Em dia (última versão: {v}, {d}).", { v: u.latest, d: day(u.latestDate) }) : t("Em dia.");
    case "available": return t("Nova versão disponível: {v} ({d}).", { v: u.latest ?? "", d: day(u.latestDate) });
    case "noPackage": return t("A última versão ({v}) não tem pacote portátil para Windows.", { v: u.latest ?? "" });
    case "error": return t("Não foi possível verificar atualizações: {msg}", { msg: u.message ? t(u.message) : "" });
  }
}

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

  // Atualizações dos emuladores: situação de cada um, progresso da instalação e as opções automáticas.
  const [upd, setUpd] = useState<Record<number, EmuUpdateStatus>>({});
  const [prog, setProg] = useState<Record<number, Progress>>({});
  const [checking, setChecking] = useState(false);
  const [checkOn, setCheckOn] = useState(true);
  const [autoOn, setAutoOn] = useState(false);
  const checkUpdates = useCallback(async (force: boolean) => {
    setChecking(true);
    try { const l = await api.emulatorUpdates(force); setUpd(Object.fromEntries(l.map((u) => [u.emulatorId, u]))); }
    catch (e) { if (force) toast(errMsg(e), "err"); }
    finally { setChecking(false); }
  }, [toast]);
  useEffect(() => {
    checkUpdates(false);
    api.getAppSetting("emu_update_check").then((v) => setCheckOn(v !== "0")).catch(() => {});
    api.getAppSetting("emu_update_auto").then((v) => setAutoOn(v === "1")).catch(() => {});
    let cancelled = false;
    const uns: (() => void)[] = [];
    const keep = (u: () => void) => (cancelled ? u() : uns.push(u));
    listen<{ id: number } & Progress>("emulator-update-progress", (e) => setProg((m) => ({ ...m, [e.payload.id]: { phase: e.payload.phase, pct: e.payload.pct } }))).then(keep);
    // instalada sozinha (atualização automática) com a tela aberta
    listen("emulator-update-installed", () => { checkUpdates(false); }).then(keep);
    return () => { cancelled = true; uns.forEach((u) => u()); };
  }, [checkUpdates]);
  async function installUpdate(e: Emulator) {
    try {
      const u = await api.emulatorUpdateInstall(e.id);
      setUpd((m) => ({ ...m, [e.id]: u }));
      toast(t("{name} atualizado para {v}.", { name: e.name, v: u.installed ?? u.latest ?? "" }));
    } catch (er) { toast(errMsg(er), "err"); }
    finally { setProg((m) => { const n = { ...m }; delete n[e.id]; return n; }); }
  }
  const setFlag = (key: string, on: boolean, set: (v: boolean) => void) => { set(on); api.setAppSetting(key, on ? "1" : "0").catch(() => {}); };

  // Fonte automática do emulador do formulário (sugestão no campo do repositório).
  const [presetSrc, setPresetSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api.emulatorUpdatePreset(dName, dExe).then((v) => { if (alive) setPresetSrc(v); }).catch(() => {});
    return () => { alive = false; };
  }, [dName, dExe]);

  // Catálogo: emuladores que o NEXSTAGE sabe baixar e localizar (os ainda não cadastrados aparecem como cartões prontos).
  const [catalog, setCatalog] = useState<EmuCatalogInfo | null>(null);
  const [catProg, setCatProg] = useState<Record<string, Progress>>({});
  const [found, setFound] = useState<Record<string, string>>({});
  const [searching, setSearching] = useState(false);
  const [filter, setFilter] = useState<Filter>("all");
  const loadCatalog = useCallback(() => api.emulatorCatalog().then(setCatalog).catch(() => {}), []);
  useEffect(() => {
    loadCatalog();
    let cancelled = false, un: (() => void) | undefined;
    listen<{ id: string } & Progress>("emulator-catalog-progress", (e) => setCatProg((m) => ({ ...m, [e.payload.id]: { phase: e.payload.phase, pct: e.payload.pct } })))
      .then((u) => (cancelled ? u() : (un = u)));
    return () => { cancelled = true; un?.(); };
  }, [loadCatalog]);
  const presetOf = (id: string) => EMULATOR_PRESETS.find((p) => p.catalog === id)!;
  const presetInput = (p: EmulatorPreset, exePath: string) => ({ name: p.name, exePath, platforms: p.platforms, formats: p.formats, args: p.args, cover: null, fullscreen: p.fullscreen });
  const refreshAll = () => { load(); loadCatalog(); checkUpdates(false); };
  // "Baixar e instalar": antes, pergunta onde criar a pasta do emulador.
  const [ask, setAsk] = useState<{ id: string; base: string; remember: boolean; exists: boolean } | null>(null);
  const folderOf = (id: string) => catalog?.items.find((i) => i.id === id)?.folder ?? "";
  const sep = (base: string) => (base.endsWith("\\") || base.endsWith("/") ? "" : "\\");
  // já existe uma pasta com o nome do emulador no destino? (a caixa pergunta se substitui)
  const existsAt = (base: string, name: string) => api.fsPathInfo(`${base}${sep(base)}${name}`).then((i) => i.exists).catch(() => false);
  async function installCatalog(id: string) {
    const base = catalog?.installDir ?? "";
    setAsk({ id, base, remember: false, exists: await existsAt(base, folderOf(id)) });
  }
  async function pickInstallBase() {
    if (!ask) return;
    const dir = await open({ purpose: "emulator-install-dir", directory: true, multiple: false, defaultPath: ask.base || undefined }).catch(() => null);
    if (typeof dir === "string") setAsk({ ...ask, base: dir, exists: await existsAt(dir, folderOf(ask.id)) });
  }
  async function confirmInstall(replace = false) {
    if (!ask) return;
    const { id, base, remember } = ask;
    const p = presetOf(id);
    setAsk(null);
    try {
      await api.emulatorCatalogInstall(id, presetInput(p, ""), base, remember, replace);
      toast(t("{name} instalado e cadastrado.", { name: p.name }));
      refreshAll();
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setCatProg((m) => { const n = { ...m }; delete n[id]; return n; }); }
  }

  // "Mover pasta de instalação" (no Editar): mostra o que será movido, escolhe o destino e confirma.
  const [move, setMove] = useState<{ id: number; info: EmuFolderInfo; dest: string; pct: number | null; busy: boolean; exists: boolean } | null>(null);
  useEffect(() => {
    let cancelled = false, un: (() => void) | undefined;
    listen<{ id: number; pct: number }>("emulator-move-progress", (e) => setMove((m) => (m && m.id === e.payload.id ? { ...m, pct: e.payload.pct } : m)))
      .then((u) => (cancelled ? u() : (un = u)));
    return () => { cancelled = true; un?.(); };
  }, []);
  async function startMove(id: number) {
    try {
      const info = await api.emulatorFolderInfo(id);
      if (info.blocked) { toast(t(info.blocked), "err"); return; }
      const dest = await open({ purpose: "emulator-move", directory: true, multiple: false, title: t("Para onde mover a pasta do emulador?") }).catch(() => null);
      if (typeof dest !== "string") return;
      setMove({ id, info, dest, pct: null, busy: false, exists: await existsAt(dest, info.name) });
    } catch (e) { toast(errMsg(e), "err"); }
  }
  async function confirmMove(replace = false) {
    if (!move) return;
    setMove({ ...move, busy: true });
    try {
      const e = await api.emulatorMoveFolder(move.id, move.dest, replace);
      toast(t("Pasta do emulador movida para {dir}.", { dir: e.exePath }));
      if (draft && draft.id === e.id) setDraft({ ...draft, exePath: e.exePath });
      setMove(null);
      refreshAll();
    } catch (er) { toast(errMsg(er), "err"); setMove((m) => (m ? { ...m, busy: false, pct: null } : m)); }
  }
  const size = (b: number) => (b >= 1 << 30 ? `${(b / (1 << 30)).toFixed(1)} GB` : `${Math.max(1, Math.round(b / (1 << 20)))} MB`);
  async function linkCatalog(id: string, exePath?: string) {
    const p = presetOf(id);
    const path = exePath ?? await open({ purpose: "emulator-link", multiple: false, directory: false, filters: [{ name: t("Executável"), extensions: ["exe"] }] }).catch(() => null);
    if (typeof path !== "string") return;
    try { await api.saveEmulator(presetInput(p, path)); toast(t("{name} vinculado.", { name: p.name })); refreshAll(); }
    catch (e) { toast(errMsg(e), "err"); }
  }
  async function searchPc() {
    setSearching(true);
    try {
      const l = await api.emulatorCatalogDetect();
      setFound(Object.fromEntries(l.map((f) => [f.id, f.exePath])));
      const fresh = l.filter((f) => catalog?.items.find((i) => i.id === f.id)?.registered == null).length;
      toast(fresh ? t("{n} emulador(es) encontrado(s) no PC. Confira e clique em Vincular.", { n: fresh }) : t("Nenhum emulador novo encontrado no PC."));
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setSearching(false); }
  }
  async function changeDir(reset: boolean) {
    const dir = reset ? "" : await open({ purpose: "emulator-install-dir", directory: true, multiple: false, defaultPath: catalog?.installDir }).catch(() => null);
    if (typeof dir !== "string") return;
    try { await api.emulatorCatalogSetDir(dir); loadCatalog(); } catch (e) { toast(errMsg(e), "err"); }
  }
  const pending = (catalog?.items ?? []).filter((i) => i.registered == null && EMULATOR_PRESETS.some((p) => p.catalog === i.id));
  const showInstalled = filter !== "missing", showMissing = filter !== "installed";

  const toggle = (arr: string[], v: string) => (arr.includes(v) ? arr.filter((x) => x !== v) : [...arr, v]);

  async function pickExe() {
    const sel = await open({ purpose: "emulator", multiple: false, directory: false, defaultPath: draft?.exePath || undefined, filters: [{ name: t("Executável"), extensions: ["exe", "bat", "cmd"] }] }).catch(() => null);
    if (typeof sel === "string" && draft) setDraft({ ...draft, exePath: sel });
  }
  async function save() {
    if (!draft) return;
    const { updateOn, updateSource, ...rest } = draft;
    try {
      await api.saveEmulator({ ...rest, updateSource: updateOn ? (updateSource.trim() || null) : "-" });
      toast(t("Emulador salvo.")); setDraft(null); load(); checkUpdates(false);
    }
    catch (e) { toast(errMsg(e), "err"); }
  }

  return (
    <>
      <div className="page-head">
        <h1>{t("Emuladores")}</h1>
        {list.length > 0 && <Button onClick={() => checkUpdates(true)} disabled={checking}>{checking ? t("Verificando…") : t("Verificar atualizações")}</Button>}
        <Button variant="primary" onClick={() => { setDraft({ ...EMPTY }); setNote(undefined); }}>{t("Adicionar emulador")}</Button>
      </div>
      <div className="emu-toolbar">
        <Seg<Filter> value={filter} onChange={setFilter} opts={[[t("Todos"), "all"], [t("Instalados ({n})", { n: list.length }), "installed"], [t("Não instalados ({n})", { n: pending.length }), "missing"]]} />
        <Button onClick={searchPc} disabled={searching}>{searching ? t("Procurando…") : t("Procurar no PC")}</Button>
      </div>
      {showInstalled && list.length === 0 && (
        <div className="empty">
          <p>{t("Nenhum emulador cadastrado. Baixe um dos emuladores abaixo, vincule um que você já tem ou cadastre o executável à mão: o NEXSTAGE associa os jogos importados automaticamente.")}</p>
        </div>
      )}
      {showInstalled && list.length > 0 && (
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
              {(upd[e.id] || prog[e.id]) && (
                <p className={`emu-update ${upd[e.id]?.state === "available" ? "new" : ""}`}>
                  {updateLine(upd[e.id], prog[e.id])}
                  {upd[e.id]?.state === "available" && upd[e.id]?.message && <small className="muted"> {t(upd[e.id]!.message!)}</small>}
                </p>
              )}
              <div className="row">
                {upd[e.id]?.state === "available" && upd[e.id]!.installable && (
                  <Button variant="primary" disabled={!!prog[e.id] || upd[e.id]!.busy} onClick={() => installUpdate(e)}>{t("Atualizar")}</Button>
                )}
                {upd[e.id]?.page && (upd[e.id]!.state === "noPackage" || (upd[e.id]!.state === "available" && !upd[e.id]!.installable)) && (
                  <Button onClick={() => openUrl(upd[e.id]!.page!).catch(() => {})}>{t("Ver versões")}</Button>
                )}
                <Button disabled={!e.exeExists} title={t("Abre o emulador, sem jogo (para configurar, instalar firmware etc.)")}
                  onClick={() => api.launchEmulator(e.id).then(() => toast(t("Abrindo {name}…", { name: e.name }))).catch((er) => toast(errMsg(er), "err"))}>{t("Abrir")}</Button>
                <Button onClick={() => { setDraft({ id: e.id, name: e.name, exePath: e.exePath, platforms: e.platforms, formats: e.formats, args: e.args, cover: e.cover, fullscreen: e.fullscreen, updateOn: e.updateSource !== "-", updateSource: e.updateSource && e.updateSource !== "-" ? e.updateSource : "" }); setNote(undefined); }}>{t("Editar")}</Button>
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
      {showMissing && pending.length > 0 && (
        <>
          <h2 className="emu-section">{t("Disponíveis para baixar")}</h2>
          <p className="muted emu-dir">
            {t("Instalados pelo NEXSTAGE em modo portátil (configurações e dados ficam na pasta do emulador): {dir}", { dir: catalog?.installDir ?? "" })}{" "}
            <Button variant="ghost" onClick={() => changeDir(false)}>{t("Alterar pasta")}</Button>
            <Button variant="ghost" onClick={() => changeDir(true)}>{t("Pasta padrão")}</Button>
          </p>
          <div className="emu-list">
            {pending.map((it) => {
              const p = presetOf(it.id), cp = catProg[it.id], here = found[it.id];
              return (
                <article key={it.id} className="emu missing">
                  <EmulatorCover name={p.name} platforms={p.platforms} cover={null} />
                  <div className="emu-body">
                    <header><h3>{p.name}</h3><span className="muted">{t("Não instalado")}</span></header>
                    <div className="chips">{p.platforms.map((x) => <span key={x}>{x}</span>)}{p.formats.map((f) => <span key={f} className="fmt">.{f}</span>)}</div>
                    {p.note && <p className="muted note">{t(p.note)}</p>}
                    {here && <p className="emu-update new">{t("Encontrado no PC:")} <span className="path">{here}</span></p>}
                    {cp && cp.phase !== "done" && <p className="emu-update">{updateLine(undefined, cp)}</p>}
                    <div className="row">
                      {here && <Button variant="primary" onClick={() => linkCatalog(it.id, here)}>{t("Vincular")}</Button>}
                      <Button variant={here ? "default" : "primary"} disabled={!!cp || it.busy} onClick={() => installCatalog(it.id)}>{t("Baixar e instalar")}</Button>
                      <Button onClick={() => linkCatalog(it.id)}>{t("Localizar instalação")}</Button>
                      {it.page && <Button variant="ghost" onClick={() => openUrl(it.page).catch(() => {})}>{t("Ver versões")}</Button>}
                    </div>
                  </div>
                </article>
              );
            })}
          </div>
        </>
      )}
      {list.length > 0 && (
        <div className="settings emu-update-settings">
          <div className="setting">
            <div><strong>{t("Verificar atualizações dos emuladores")}</strong><small>{t("Ao abrir o NEXSTAGE e a cada 6 horas: avisa quando sair versão nova de um emulador cadastrado.")}</small></div>
            <Button variant={checkOn ? "primary" : "default"} onClick={() => setFlag("emu_update_check", !checkOn, setCheckOn)}>{checkOn ? t("Ligado") : t("Desligado")}</Button>
          </div>
          <div className="setting">
            <div><strong>{t("Atualizar emuladores automaticamente")}</strong><small>{t("Baixa e instala a versão nova sozinho, com o emulador fechado e nenhum jogo aberto. Só vale para emuladores portáteis (pasta com permissão de escrita); configurações, BIOS e saves são mantidos.")}</small></div>
            <Button variant={autoOn ? "primary" : "default"} onClick={() => setFlag("emu_update_auto", !autoOn, setAutoOn)}>{autoOn ? t("Ligado") : t("Desligado")}</Button>
          </div>
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
            {draft.id && (
              <div className="row">
                <Button variant="ghost" disabled={!draft.exePath} onClick={() => browseFolder({ title: t("Pasta de {name}", { name: draft.name }), defaultPath: draft.exePath, purpose: "emulator-browse" })}>{t("Abrir pasta de instalação")}</Button>
                <Button variant="ghost" onClick={() => api.emulatorReveal(draft.id!).catch((e) => toast(errMsg(e), "err"))}>{t("Abrir no Explorador do Windows")}</Button>
                <Button variant="ghost" onClick={() => startMove(draft.id!)}>{t("Mover pasta de instalação")}</Button>
              </div>
            )}
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
            <label className="ra-check"><input type="checkbox" checked={draft.updateOn} data-focus
                onChange={(e) => setDraft({ ...draft, updateOn: e.target.checked })} /> {t("Procurar atualizações deste emulador")}</label>
            {draft.updateOn && (
              <label>{t("Repositório de atualização")}<input value={draft.updateSource} placeholder={presetSrc ?? t("dono/repositório no GitHub")} onChange={(e) => setDraft({ ...draft, updateSource: e.target.value })} />
              </label>
            )}
            {draft.updateOn && <p className="muted note">{presetSrc && !draft.updateSource.trim()
              ? t("Emulador reconhecido: as versões vêm de {src}. Deixe em branco para usar essa fonte.", { src: presetSrc })
              : t("Informe dono/repositório do GitHub (ex.: PCSX2/pcsx2) ou o endereço https do repositório. O NEXSTAGE baixa o pacote portátil (.zip/.7z) para Windows da última versão publicada.")}</p>}
          </div>
          </div>
          <div className="row end">
            <Button variant="ghost" onClick={() => setDraft(null)}>{t("Cancelar")}</Button>
            <Button variant="primary" onClick={save}>{t("Salvar")}</Button>
          </div>
        </Modal>
      )}

      {ask && (
        <Modal portal title={t("Instalar {name}", { name: presetOf(ask.id).name })} onClose={() => setAsk(null)}>
          <p>{t("Onde você quer instalar o emulador? O NEXSTAGE cria a pasta do emulador dentro da pasta escolhida.")}</p>
          <p className="path emu-dest">{ask.base}{sep(ask.base)}{catalog?.items.find((i) => i.id === ask.id)?.folder}</p>
          <div className="row">
            <Button onClick={pickInstallBase}>{t("Escolher pasta…")}</Button>
            {catalog && ask.base !== catalog.installDir && <Button variant="ghost" onClick={async () => setAsk({ ...ask, base: catalog.installDir, exists: await existsAt(catalog.installDir, folderOf(ask.id)) })}>{t("Pasta padrão")}</Button>}
          </div>
          {catalog && ask.base !== catalog.installDir && (
            <label className="ra-check"><input type="checkbox" checked={ask.remember} data-focus onChange={(e) => setAsk({ ...ask, remember: e.target.checked })} /> {t("Usar esta pasta para os próximos emuladores")}</label>
          )}
          {ask.exists && (
            <p className="emu-exists">{t("Já existe uma pasta “{name}” nesse local. Quer substituí-la? Ao substituir, a pasta atual vai para a Lixeira e a instalação começa do zero; sem substituir, o emulador é instalado por cima, mantendo configurações, BIOS e saves.", { name: folderOf(ask.id) })}</p>
          )}
          <div className="row end">
            <Button variant="ghost" onClick={() => setAsk(null)}>{t("Cancelar")}</Button>
            {ask.exists ? (
              <>
                <Button variant="danger" onClick={() => confirmInstall(true)}>{t("Substituir")}</Button>
                <Button variant="primary" data-autofocus onClick={() => confirmInstall(false)}>{t("Não substituir")}</Button>
              </>
            ) : <Button variant="primary" data-autofocus onClick={() => confirmInstall(false)}>{t("Baixar e instalar")}</Button>}
          </div>
        </Modal>
      )}

      {move && (
        <Modal portal title={t("Mover pasta de instalação")} onClose={() => { if (!move.busy) setMove(null); }}>
          <p>{t("A pasta inteira do emulador ({n} arquivos, {size}) será movida. Configurações e saves que estiverem nela vão junto.", { n: move.info.files, size: size(move.info.bytes) })}</p>
          <p className="muted">{t("De:")}</p>
          <p className="path emu-dest">{move.info.dir}</p>
          <p className="muted">{t("Para:")}</p>
          <p className="path emu-dest">{move.dest}{sep(move.dest)}{move.info.name}</p>
          {move.exists && !move.busy && (
            <p className="emu-exists">{t("Já existe uma pasta “{name}” no destino. Quer substituí-la? A pasta que está lá vai para a Lixeira (dá para recuperar).", { name: move.info.name })}</p>
          )}
          {move.busy && <p className="emu-update">{t("Movendo…{v0}", { v0: move.pct !== null ? ` ${move.pct}%` : "" })}</p>}
          <div className="row end">
            <Button variant="ghost" disabled={move.busy} onClick={() => setMove(null)}>{t("Cancelar")}</Button>
            {move.exists ? (
              <>
                <Button disabled={move.busy} onClick={() => { const id = move.id; setMove(null); startMove(id); }}>{t("Não substituir (escolher outro destino)")}</Button>
                <Button variant="danger" data-autofocus disabled={move.busy} onClick={() => confirmMove(true)}>{t("Substituir")}</Button>
              </>
            ) : <Button variant="primary" data-autofocus disabled={move.busy} onClick={() => confirmMove(false)}>{t("Mover")}</Button>}
          </div>
        </Modal>
      )}
    </>
  );
}
