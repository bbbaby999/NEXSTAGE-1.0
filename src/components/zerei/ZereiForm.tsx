import { useEffect, useMemo, useRef, useState } from "react";
import { onPadButton } from "../../lib/nav";
import { play } from "../../lib/sound";
import Modal from "../Modal";
import Button from "../Button";
import Select from "../Select";
import Seg from "../Seg";
import ImagePicker from "../ImagePicker";
import LogoPicker from "../LogoPicker";
import { useSaveExit } from "../ExitPrompt";
import { api, errMsg } from "../../lib/api";
import { useSession } from "../../state/session";
import { PLATFORMS } from "../../types";
import type { ZereiCompletion, ZereiDifficulty, ZereiEntry, ZereiInput, ZereiStatus } from "../../types";
import {
  COMPLETION_LABEL, DIFFICULTY_LABEL, DIFFICULTY_ORDER, dateExample, STATUS_LABEL, STATUS_ORDER, inputToIso, isoToInput, parseDuplicate, todayIso,
} from "../../lib/zerei";
import { t } from "../../lib/i18n";

const OTHER = "__other__";

/**
 * Cadastro/edição de um registro do Zerei!. Todos os campos podem ser editados depois.
 * Capa e fundo: o mesmo seletor do "Editar jogo" da Biblioteca (arquivo do computador ou "Pesquisar na
 * internet": escolhe a fonte/jogo e depois a imagem). Logo: galeria do SteamGridDB.
 * Duplicado (mesmo jogo na mesma plataforma): oferece abrir o existente ou registrar uma nova conclusão.
 */
export default function ZereiForm({ entry, initial, onSaved, onOpenExisting, onClose }: {
  /** Registro existente (edição); ausente = cadastro novo. */
  entry?: ZereiEntry;
  initial: ZereiInput;
  onSaved: (e: ZereiEntry, created: boolean) => void;
  onOpenExisting: (id: number) => void;
  onClose: () => void;
}) {
  const { profile, toast } = useSession();
  const [f, setF] = useState<ZereiInput>(initial);
  const [dates, setDates] = useState({ startedAt: isoToInput(initial.startedAt), completedAt: isoToInput(initial.completedAt) });
  const [hours, setHours] = useState(initial.hours === null ? "" : String(initial.hours));
  const [rating, setRating] = useState(initial.rating === null ? "" : String(initial.rating));
  const [customPlat, setCustomPlat] = useState(false);
  const [logoOpen, setLogoOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [dup, setDup] = useState<{ id: number; count: number } | null>(null);
  const set = <K extends keyof ZereiInput>(k: K, v: ZereiInput[K]) => setF((x) => ({ ...x, [k]: v }));

  // plataformas: as do jogo (pesquisa) primeiro, depois as oficiais do NEXSTAGE
  const platOpts = useMemo<[string, string][]>(() => {
    const list: string[] = [];
    for (const p of [...f.platforms, ...PLATFORMS]) if (!list.some((x) => x.toLowerCase() === p.toLowerCase())) list.push(p);
    if (f.platform && !list.some((x) => x.toLowerCase() === f.platform.toLowerCase())) list.unshift(f.platform);
    return [...list.map((p): [string, string] => [p, p]), [t("Outra…"), OTHER]];
  }, [f.platforms, f.platform]);

  // cadastro novo: completa o que faltar com a primeira capa/logo/fundo do SteamGridDB (tudo pode ser trocado)
  useEffect(() => {
    if (entry || !f.title.trim()) return;
    api.zereiArtwork(f.title, f.year, f.sgdbId).then((a) => {
      setF((x) => ({
        ...x, sgdbId: x.sgdbId ?? a.sgdbId, cover: x.cover ?? a.covers[0]?.url ?? null,
        logo: x.logo ?? a.logos[0]?.url ?? null, background: x.background ?? a.backgrounds[0]?.url ?? null,
      }));
    }).catch(() => { /* sem SteamGridDB: segue com as imagens da pesquisa */ });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /*
   * Controle: L2/R2 ajustam o campo em foco — datas ±1 dia (vazio começa em hoje), horas ±1, nota ±0,5 —
   * e, fora desses campos, mudam o status. Só quando esta janela é a camada de cima (sem lista,
   * teclado virtual ou pesquisa de imagens abertos por cima).
   */
  const root = useRef<HTMLDivElement>(null);
  const live = useRef({ status: f.status, dates, hours, rating });
  live.current = { status: f.status, dates, hours, rating };
  useEffect(() => onPadButton((b, e) => {
    if (e.defaultPrevented || (b !== "l2" && b !== "r2") || !root.current) return;
    const layers = document.querySelectorAll(".overlay:not(.leaving)");
    if (!layers[layers.length - 1]?.contains(root.current) || document.querySelector(".pick-root, .kb-root")) return;
    e.preventDefault();
    const d = b === "r2" ? 1 : -1;
    const cur = live.current;
    const field = (document.activeElement as HTMLElement | null)?.dataset?.zrAdjust;
    play("move");
    if (field === "startedAt" || field === "completedAt") {
      const base = inputToIso(cur.dates[field]) ?? todayIso();
      const dt = new Date(`${base}T12:00:00`);
      dt.setDate(dt.getDate() + d);
      const iso = `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
      setDates((x) => ({ ...x, [field]: isoToInput(iso) }));
    } else if (field === "hours") {
      const h = Math.max(0, (Number(cur.hours.replace(",", ".")) || 0) + d);
      setHours(String(h));
    } else if (field === "rating") {
      const base = cur.rating.trim() === "" ? (d > 0 ? -0.5 : 10.5) : Number(cur.rating.replace(",", ".")) || 0;
      setRating(String(Math.max(0, Math.min(10, base + d * 0.5))));
    } else {
      const i = STATUS_ORDER.indexOf(cur.status);
      changeStatus(STATUS_ORDER[Math.max(0, Math.min(STATUS_ORDER.length - 1, i + d))]);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }), []);

  const changeStatus = (s: ZereiStatus) => {
    set("status", s);
    // atualização pelo estado atual: também é chamada pelo atalho L2/R2 (registrado uma vez só)
    if (s === "completed") setDates((d) => (d.completedAt ? d : { ...d, completedAt: isoToInput(todayIso()) }));
  };

  async function save(newRun = false) {
    const startedAt = inputToIso(dates.startedAt);
    const completedAt = inputToIso(dates.completedAt);
    if (startedAt === undefined) return toast(t("Data de início inválida. Use o formato {f}.", { f: dateExample() }), "err");
    if (completedAt === undefined) return toast(t("Data em que zerou inválida. Use o formato {f}.", { f: dateExample() }), "err");
    const h = hours.trim() === "" ? null : Number(hours.replace(",", "."));
    const r = rating.trim() === "" ? null : Number(rating.replace(",", "."));
    if (h !== null && (!isFinite(h) || h < 0)) return toast(t("Tempo jogado inválido."), "err");
    if (r !== null && (!isFinite(r) || r < 0 || r > 10)) return toast(t("A nota deve ser de 0 a 10."), "err");
    // tipo de conclusão só existe para jogo zerado (Zerado ou Platinado)
    const completionType = f.status === "completed" ? f.completionType ?? "campaign" : null;
    setSaving(true);
    try {
      const saved = await api.zereiSave(profile!.id, entry?.id ?? null, { ...f, completionType, startedAt, completedAt, hours: h, rating: r }, newRun);
      onSaved(saved, !entry);
    } catch (e) {
      const d = parseDuplicate(e);
      if (d) setDup(d);
      else toast(errMsg(e), "err");
    }
    setSaving(false);
  }

  // Start salva; sair com alterações pergunta "Sair sem salvar / Salvar" (estado inicial = 1º desenho)
  const initialSnap = useRef<string | null>(null);
  const snap = JSON.stringify({ f, dates, hours, rating });
  initialSnap.current ??= snap;
  const exit = useSaveExit({
    root, dirty: () => snap !== initialSnap.current, close: onClose, save: () => { save(); },
    canSave: () => !saving && !!f.title.trim() && !!f.platform.trim(),
  });

  if (dup) {
    return (
      <Modal title={t("Jogo já registrado")} onClose={() => setDup(null)}>
        <p>{t("Você já tem {n} registro(s) de “{title}” em {platform}.", { n: dup.count, title: f.title, platform: f.platform })}</p>
        <p className="muted note">{t("Zerou de novo? Registre uma nova conclusão — cada uma fica com as próprias datas, horas, nota e comentários.")}</p>
        <div className="row end">
          <Button variant="ghost" onClick={() => setDup(null)}>{t("Voltar")}</Button>
          <Button onClick={() => onOpenExisting(dup.id)}>{t("Abrir o registro existente")}</Button>
          <Button variant="primary" data-autofocus disabled={saving} onClick={() => { setDup(null); save(true); }}>{t("Registrar nova conclusão")}</Button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal title={entry ? t("Editar registro") : t("Adicionar ao Zerei!")} onClose={exit.requestClose} xl>
      {/* horizontal (igual ao "Editar jogo"): capa | dados do registro | imagens (fundo e logo) */}
      <div className="gf-grid zr-edit" ref={root}>
        <ImagePicker label={t("Capa")} value={f.cover} onChange={(v) => set("cover", v)} maxW={600} maxH={900} shape="tall"
          search={{ term: f.title, platform: f.platform, kind: "cover" }} />

        <div className="fields" data-nav-group>
          <label>{t("Nome")}<input value={f.title} maxLength={200} onChange={(e) => set("title", e.target.value)} /></label>
          <label>{t("Plataforma")}
            {customPlat ? (
              <div className="row">
                <input value={f.platform} maxLength={60} placeholder={t("Nome da plataforma")} onChange={(e) => set("platform", e.target.value)} style={{ flex: 1, width: "auto" }} />
                <Button variant="ghost" onClick={() => setCustomPlat(false)}>{t("Lista")}</Button>
              </div>
            ) : (
              <Select<string> value={f.platform} width="100%" title={t("Plataforma")} options={platOpts}
                onChange={(v) => { if (v === OTHER) { setCustomPlat(true); set("platform", ""); } else set("platform", v); }} />
            )}
          </label>
          <div className="zr-field">
            <span>{t("Status")}</span>
            <Seg<ZereiStatus> value={f.status} opts={STATUS_ORDER.map((s) => [t(STATUS_LABEL[s]), s])} onChange={changeStatus} />
          </div>
          {f.status === "completed" && (
            <div className="zr-field">
              <span>{t("Tipo de conclusão")}</span>
              <Seg<ZereiCompletion> value={f.completionType ?? "campaign"} onChange={(v) => set("completionType", v)}
                opts={[[t(COMPLETION_LABEL.campaign), "campaign"], [t(COMPLETION_LABEL.full), "full"]]} />
            </div>
          )}
          <div className="zr-two">
            <label>{t("Data de início")}
              <input data-zr-adjust="startedAt" value={dates.startedAt} placeholder={dateExample()} inputMode="numeric" maxLength={10}
                onChange={(e) => setDates((d) => ({ ...d, startedAt: e.target.value }))} />
            </label>
            <label>{t("Data em que zerou")}
              <div className="row zr-date">
                <input data-zr-adjust="completedAt" value={dates.completedAt} placeholder={dateExample()} inputMode="numeric" maxLength={10}
                  onChange={(e) => setDates((d) => ({ ...d, completedAt: e.target.value }))} />
                <Button variant="ghost" onClick={() => setDates((d) => ({ ...d, completedAt: isoToInput(todayIso()) }))}>{t("Hoje")}</Button>
              </div>
            </label>
          </div>
          <div className="zr-two">
            <label>{t("Tempo jogado (horas)")}
              <input data-zr-adjust="hours" type="number" min={0} step={0.5} value={hours} placeholder="0" onChange={(e) => setHours(e.target.value)} />
            </label>
            <label>{t("Nota pessoal (0 a 10)")}
              <input data-zr-adjust="rating" type="number" min={0} max={10} step={0.5} value={rating} placeholder="—" onChange={(e) => setRating(e.target.value)} />
            </label>
          </div>
          <label>{t("Dificuldade pessoal")}
            <Select<ZereiDifficulty | ""> value={f.difficulty ?? ""} width="100%" title={t("Dificuldade pessoal")} onChange={(v) => set("difficulty", v || null)}
              options={[[t("Não informada"), ""], ...DIFFICULTY_ORDER.map((d): [string, ZereiDifficulty] => [t(DIFFICULTY_LABEL[d]), d])]} />
          </label>
          <label>{t("Comentários e observações")}
            <textarea rows={4} maxLength={5000} value={f.notes} onChange={(e) => set("notes", e.target.value)} placeholder={t("O que achou, finais, conquistas, dicas…")} />
          </label>

        </div>
        <div className="fields gf-art" data-nav-group>
          <div className="bg-edit">
            <span className="muted">{t("Arte de fundo (página de detalhes)")}</span>
            <ImagePicker label={t("Sem arte de fundo: usa a capa")} value={f.background} onChange={(v) => set("background", v)}
              maxW={1920} maxH={1080} shape="wide" search={{ term: f.title, platform: f.platform, kind: "background" }} />
          </div>
          <div className="bg-edit">
            <span className="muted">{t("Logo (opcional)")}</span>
            <div className="picker">
              <div className="picker-preview wide zr-logo-slot">
                {f.logo ? <img src={f.logo} alt="" referrerPolicy="no-referrer" /> : <span>{t("Sem logo: mostra o nome do jogo")}</span>}
              </div>
              <div className="row">
                <Button onClick={() => setLogoOpen(true)} disabled={!f.title.trim()}>{t("Logos do SteamGridDB")}</Button>
                {f.logo && <Button variant="danger" onClick={() => set("logo", null)}>{t("Remover")}</Button>}
              </div>
            </div>
          </div>
        </div>
      </div>
      <div className="row end">
        <Button variant="ghost" onClick={exit.requestClose}>{t("Cancelar")}</Button>
        <Button variant="primary" disabled={saving || !f.title.trim() || !f.platform.trim()} onClick={() => save()}>
          {saving ? t("Salvando…") : entry ? t("Salvar") : t("Adicionar")}
        </Button>
      </div>
      {logoOpen && (
        <LogoPicker title={f.title} year={f.year} sgdbId={f.sgdbId} current={f.logo} onClose={() => setLogoOpen(false)}
          onPick={(url, id) => setF((x) => ({ ...x, logo: url, sgdbId: x.sgdbId ?? id }))} />
      )}
      {exit.prompt}
    </Modal>
  );
}
