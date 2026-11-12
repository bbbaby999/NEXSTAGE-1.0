import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import Modal from "./Modal";
import Button from "./Button";
import Select from "./Select";
import { api, errMsg } from "../lib/api";
import { compatiblePaths, counts, selectable, STATUS_LABEL, summarize } from "../lib/scanSelect";
import { formatBytes } from "../lib/format";
import { useSession } from "../state/session";
import type { Emulator, ScanDone, ScanItem, ScanProgress } from "../types";
import { currentLang, t } from "../lib/i18n";

/**
 * Importação por pasta: 1) varredura em segundo plano com progresso; 2) pré-visualização com
 * seleção; 3) confirmação; 4) registro no banco. Os arquivos só são LIDOS (cabeçalhos): nada é
 * copiado, movido, modificado nem executado.
 */
export default function FolderImport({ profileId, folder, folderId, onClose, onImported }: {
  profileId: number; folder: string; folderId: number | null; onClose: () => void; onImported: () => void;
}) {
  const { toast } = useSession();
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [done, setDone] = useState<ScanDone | null>(null);
  const [emus, setEmus] = useState<Emulator[]>([]);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [choice, setChoice] = useState<Map<string, number | null>>(new Map());
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const started = useRef(false);

  useEffect(() => {
    // Os ouvintes são registrados a cada montagem (o StrictMode monta/desmonta/monta de novo);
    // a varredura em si só é disparada uma vez, pela montagem que continua viva.
    const uns: (() => void)[] = [];
    let dead = false;
    const keep = (u: () => void) => { if (dead) { u(); return false; } uns.push(u); return true; };
    (async () => {
      if (!keep(await listen<ScanProgress>("scan-progress", (e) => setProgress(e.payload)))) return;
      if (!keep(await listen<ScanDone>("scan-done", (e) => {
        const d = e.payload;
        if (d.cancelled) { onClose(); return; }
        setDone(d);
        setChoice(new Map(d.items.map((i) => [i.path, i.assigned])));
        setSel(new Set(compatiblePaths(d.items))); // já vem marcado: tudo que é compatível
      }))) return;
      if (started.current) return;
      started.current = true;
      api.listEmulators().then(setEmus).catch(() => {});
      api.scanFolder(profileId, folder, folderId).catch((e) => { toast(errMsg(e), "err"); onClose(); });
    })();
    return () => { dead = true; uns.forEach((u) => u()); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const items = done?.items ?? [];
  const c = useMemo(() => counts(items), [items]);
  const emuName = (id: number | null | undefined) => emus.find((e) => e.id === id)?.name ?? "—";
  const sum = useMemo(() => summarize(items, sel, choice), [items, sel, choice]);
  const toggle = (p: string) => setSel((s) => { const n = new Set(s); n.has(p) ? n.delete(p) : n.add(p); return n; });

  async function runImport() {
    setBusy(true);
    try {
      const chosen = items.filter((i) => sel.has(i.path) && selectable(i) && i.platform);
      const r = await api.importScanItems(profileId, folder, chosen.map((i) => ({
        path: i.path, ext: i.ext, platform: i.platform!, title: i.name, emulatorId: i.kind === "windows" ? null : choice.get(i.path) ?? null,
      })));
      toast(t("{imported} jogo(s) importado(s){v1}{v2}.", { imported: r.imported, v1: r.duplicates ? t(", {n} já existiam", { n: r.duplicates }) : "", v2: r.failed.length ? t(", {n} com erro", { n: r.failed.length }) : "" }), r.failed.length && !r.imported ? "err" : "ok");
      if (r.failed.length) toast(r.failed[0], "err");
      onImported();
      onClose();
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); setConfirm(false); }
  }

  // ----- 1) varredura -----
  if (!done) {
    return (
      <Modal title={t("Procurando jogos…")} onClose={() => { api.cancelScan().catch(() => {}); onClose(); }} wide>
        <p className="muted path">{folder}</p>
        <div className="scan-bar"><i className={progress?.phase === "analyzing" && progress.found ? "" : "indet"}
          style={progress?.phase === "analyzing" && progress.found ? { width: `${Math.min(100, (progress.scanned / progress.found) * 100)}%` } : undefined} /></div>
        <p>{progress ? (progress.phase === "walking" ? t("Percorrendo as pastas…") : t("Identificando as plataformas…")) : t("Iniciando…")}</p>
        <div className="stats">
          <div className="stat"><strong>{progress?.phase === "walking" ? progress.scanned.toLocaleString(currentLang()) : "—"}</strong><span>{t("Arquivos verificados")}</span></div>
          <div className="stat"><strong>{progress?.found ?? 0}</strong><span>{t("Arquivos de jogo encontrados")}</span></div>
        </div>
        <p className="muted note path">{progress?.current}</p>
        <div className="row end"><Button variant="ghost" onClick={() => { api.cancelScan().catch(() => {}); onClose(); }}>{t("Cancelar varredura")}</Button></div>
      </Modal>
    );
  }

  // ----- 2) pré-visualização -----
  return (
    <>
      <Modal title={t("Pré-visualização da importação")} onClose={onClose} wide>
        <p className="muted path">{done.folder}</p>
        <p>
          {t("{n} arquivos verificados", { n: done.scanned.toLocaleString() })} · <b>{t("{n} jogos encontrados", { n: c.total })}</b> · <b>{t("{n} novos", { n: c.new + c.secondary })}</b> · <b>{t("{n} já na biblioteca", { n: c.duplicate })}</b> · {t("{n} sem identificação", { n: c.invalid + c.unrecognized })} · {t("{n} ignorados (instaladores, partes de .cue)", { n: done.ignored })}</p>
        {done.errors.length > 0 && <p className="muted note" title={done.errors.join("\n")}>{done.errors.length} {t("local(is) inacessível(is) foram ignorados (passe o mouse para ver).")}</p>}
        {done.missing.length > 0 && (
          <p className="muted note" title={done.missing.map((m) => m.path).join("\n")}>
            {done.missing.length} {t("jogo(s) desta pasta já cadastrados não foram encontrados no disco:")}{" "}{done.missing.slice(0, 3).map((m) => m.title).join(", ")}{done.missing.length > 3 ? "…" : ""}.
          </p>
        )}
        {c.total === 0 ? (
          <div className="empty"><p>{t("Nenhum arquivo .iso, .bin, .exe, .nsp, .nup, .pkg ou .vpk foi encontrado nessa pasta.")}</p></div>
        ) : (
          <div className="scan-table" role="table">
            <div className="scan-row head" role="row"><span /><span>{t("Nome")}</span><span>{t("Formato")}</span><span>{t("Plataforma")}</span><span>{t("Emulador")}</span><span>{t("Status")}</span></div>
            {items.map((i) => (
              <ScanRow key={i.path} item={i} on={sel.has(i.path)} onToggle={() => toggle(i.path)} emus={emus}
                chosen={choice.get(i.path) ?? null} onChoose={(v) => setChoice(new Map(choice).set(i.path, v))} emuName={emuName} />
            ))}
          </div>
        )}
        {items.some((i) => i.kind !== "windows" && i.status === "new" && i.compatible.length === 0) && (
          <p className="muted note">{t("Alguns jogos não têm emulador compatível cadastrado. Cadastre o emulador na página Emuladores; você pode importá-los assim mesmo e associar depois.")}</p>
        )}
        <div className="row end" style={{ marginTop: 16 }}>
          <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
          <Button onClick={() => setSel(new Set())} disabled={!sel.size}>{t("Limpar seleção")}</Button>
          <Button onClick={() => setSel(new Set(compatiblePaths(items)))} disabled={!c.compatible}>{t("Selecionar todos os compatíveis ({n})", { n: c.compatible })}</Button>
          <Button variant="primary" disabled={!sum.count} onClick={() => setConfirm(true)}>{t("Importar")}{" "}{sum.count} selecionado(s)</Button>
        </div>
      </Modal>

      {/* ----- 3) confirmação ----- */}
      {confirm && (
        <Modal title={t("Importar {count} jogo(s)?", { count: sum.count })} onClose={() => setConfirm(false)}>
          <ul className="plain">
            {Object.entries(sum.byPlatform).map(([p, n]) => <li key={p}><b>{n}</b> · {p}</li>)}
          </ul>
          {sum.withoutEmulator > 0 && <p className="muted note">{sum.withoutEmulator} {t("jogo(s) ficarão sem emulador associado; você poderá associá-lo depois em “Editar jogo”.")}</p>}
          <p className="muted note">{t("Os arquivos originais não serão copiados, movidos nem modificados: o NEXSTAGE só registra o caminho de cada um. A pasta ficará na lista de pastas de jogos para novas varreduras.")}</p>
          <div className="row end">
            <Button variant="ghost" onClick={() => setConfirm(false)}>{t("Voltar")}</Button>
            <Button variant="primary" disabled={busy} onClick={runImport}>{busy ? t("Importando…") : t("Confirmar importação")}</Button>
          </div>
        </Modal>
      )}
    </>
  );
}

function ScanRow({ item: i, on, onToggle, emus, chosen, onChoose, emuName }: {
  item: ScanItem; on: boolean; onToggle: () => void; emus: Emulator[]; chosen: number | null; onChoose: (v: number | null) => void; emuName: (id: number | null) => string;
}) {
  const can = selectable(i);
  return (
    <div className={`scan-row ${i.status}`} role="row">
      <button type="button" data-focus role="checkbox" aria-checked={on} aria-label={t("Selecionar {name}", { name: i.name })} disabled={!can} className={`chk ${on ? "on" : ""}`} onClick={onToggle}>{on ? "✓" : ""}</button>
      <span className="nm" title={`${i.path}\n${formatBytes(i.size)}`}><b>{i.name}</b><small>{i.note && t(i.note)}</small></span>
      <span>.{i.ext.toUpperCase()}</span>
      <span>{i.platform ?? <em className="muted">{t("Não identificada")}</em>}</span>
      <span>
        {i.kind === "windows" ? <em className="muted">{t("Não precisa")}</em>
          : !can || !i.platform ? "—"
          : i.compatible.length === 0 ? <em className="warn">{t("Nenhum compatível")}</em>
          : i.compatible.length === 1 ? emuName(i.compatible[0])
          : <Select<number | ""> value={chosen ?? ""} width={170} title={t("Escolher emulador")} onChange={(v) => onChoose(v === "" ? null : v)}
              options={[[t("Escolher…"), ""], ...i.compatible.map((id): [string, number] => [emuName(id), id])]} />}
      </span>
      <span><b className={`st ${i.status}`}>{t(STATUS_LABEL[i.status])}</b></span>
    </div>
  );
}
