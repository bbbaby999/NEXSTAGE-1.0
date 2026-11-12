import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import { play } from "../lib/sound";
import { layerJustClosed, setPadMonitorNav } from "../lib/nav";
import {
  ACTIONS, BIT, BTN_LABEL, BTNS, DEFAULTS, DEFAULT_MAP, ID_SOURCE_LABEL, SOURCE_LABEL, VISUALS,
  addController, assign, confirmPattern, deviceLabels, gamepadByKey, gamepadControllers, glyphsOf, guessVisual,
  isJoyconPair, isVirtualDevice, noteNativeControllers, onNativeDevices, presetFor, removeController,
  resetControllerSettings, splitJoycons, testRumble, updateController, useControllers, visualOf, withPattern,
  type Action, type Btn, type ConfirmPattern, type GlyphStyle, type LedMode, type PadReg, type Visual,
} from "../lib/controllers";
import ControllerArt, { BITS as ART_BITS, HAS_TOUCHPAD } from "../components/ControllerArt";
import ControllerVisual from "../components/ControllerVisual";
import { PHOTO_CREDITS, commonsUrl } from "../data/controllerPhotos";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Battery } from "../components/ControllerStatus";
import Button from "../components/Button";
import Modal from "../components/Modal";
import Select from "../components/Select";
import Seg from "../components/Seg";
import type { Controller } from "../types";
import { t, tk } from "../lib/i18n";

/**
 * Central de Controles.
 *  - Coleção: grade de cartões (um por controle cadastrado) + cartão "Adicionar controle" no fim.
 *  - Página individual: cabeçalho com imagem, identidade, conexão e bateria; abaixo, as categorias de
 *    configuração. "Voltar" (ESC / B / ○ / botão) retorna à coleção com o foco no mesmo cartão.
 * Tudo é individual por controle e persistente (lib/controllers.ts), e só afeta o NEXSTAGE.
 */

// ---------- dispositivos conectados agora ----------

/**
 * Controles conectados agora. Nomes repetidos (dois controles do mesmo modelo) ganham numeração para
 * ficar claro qual é qual; a chave de cada um é única (MAC no HID, slot no XInput, `#n` na Gamepad API).
 */
function useDevices(): Controller[] {
  const [native, setNative] = useState<Controller[]>([]);
  const [pads, setPads] = useState<Controller[]>([]);
  useEffect(() => {
    let alive = true;
    const scan = () => setPads(gamepadControllers());
    const load = () => api.getControllers().then((v) => { if (!alive) return; noteNativeControllers(v); setNative(v); scan(); }).catch(() => {});
    const both = () => { load(); scan(); };
    both();
    const iv = setInterval(both, 4000);
    const offNative = onNativeDevices(both); // controle HID conectado/desconectado: atualiza na hora
    window.addEventListener("gamepadconnected", both);
    window.addEventListener("gamepaddisconnected", both);
    return () => {
      alive = false;
      clearInterval(iv);
      offNative();
      window.removeEventListener("gamepadconnected", both);
      window.removeEventListener("gamepaddisconnected", both);
    };
  }, []);
  const all = mergePhysical([...native, ...pads]);
  const labels = deviceLabels(all, t);
  return all.map((d) => ({ ...d, name: labels.get(d.id) ?? d.name }));
}

/**
 * Uma entrada por controle FÍSICO: a mesma chave, ou interfaces diferentes do mesmo aparelho (mesmo
 * ContainerId do Windows), contam uma vez — vale a primeira (leitura nativa antes da Gamepad API).
 * Virtuais nunca são agrupados com o controle real (têm aparelho próprio no Windows).
 */
function mergePhysical(list: Controller[]): Controller[] {
  const out: Controller[] = [];
  for (const d of list) {
    const dup = out.some((o) => o.id === d.id ||
      (!!d.physicalId && !d.virtualSource && !o.virtualSource && o.physicalId === d.physicalId));
    if (!dup) out.push(d);
  }
  return out;
}

const CONN_LABEL: Record<string, string> = { usb: tk("Cabo USB"), bluetooth: tk("Bluetooth"), wireless: tk("Sem fio") };

// ---------- página ----------

export default function Controllers() {
  const regs = useControllers();
  const devices = useDevices();
  const [openId, setOpenId] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  /** Cartão que recebe o foco ao voltar para a coleção (preserva a posição de navegação). */
  const restore = useRef<string | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const open = regs.find((r) => r.id === openId) ?? null;

  // Foco ao mostrar a coleção: o cartão de onde o usuário saiu ou, na primeira visita, o primeiro.
  useLayoutEffect(() => {
    if (open) return;
    const id = restore.current;
    restore.current = null;
    const grid = gridRef.current;
    if (!grid) return;
    const target = (id && grid.querySelector<HTMLElement>(`[data-pad-id="${CSS.escape(id)}"]`)) ||
      (id === "__add" ? grid.querySelector<HTMLElement>("[data-pad-add]") : null);
    const a = document.activeElement;
    if (target) {
      target.focus({ preventScroll: true });
      target.scrollIntoView({ block: "nearest", behavior: "auto" });
    } else if (!a || a === document.body || !document.contains(a) || a.closest(".topbar")) {
      grid.querySelector<HTMLElement>("[data-focus]")?.focus({ preventScroll: true });
    }
  }, [open]);

  const back = useCallback((focusId: string | null) => { restore.current = focusId; setOpenId(null); }, []);

  if (open) {
    return <PadDetail key={open.id} reg={open} devices={devices} onBack={() => back(open.id)}
      onDeleted={(neighbor) => back(neighbor ?? "__add")} />;
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t("Central de Controles")}</h1>
          <p className="muted ctl-sub">{t("Sua coleção de controles. Os ajustes de cada um valem só para a navegação e a interface do NEXSTAGE.")}</p>
        </div>
      </div>
      <div className="pad-grid" ref={gridRef}>
        {regs.map((r) => (
          <PadCard key={r.id} reg={r} device={devices.find((d) => d.id === r.deviceKey) ?? null} onOpen={() => setOpenId(r.id)} />
        ))}
        <button type="button" data-focus data-pad-add className="pad-card pad-add" onClick={() => setAdding(true)}>
          <span className="pad-add-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" width="44" height="44" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round"><path d="M12 5v14M5 12h14" /></svg>
          </span>
          <strong>{t("Adicionar controle")}</strong>
          <small>{regs.length ? t("Detectado ou manual") : t("Cadastre o primeiro controle da coleção")}</small>
        </button>
      </div>
      {adding && (
        <AddFlow devices={devices} regs={regs} onClose={() => setAdding(false)}
          onCreated={(r) => { setAdding(false); restore.current = r.id; setOpenId(r.id); }}
          onOpen={(id) => { setAdding(false); restore.current = id; setOpenId(id); }} />
      )}
    </>
  );
}

// ---------- cartão ----------

function PadCard({ reg, device, onOpen }: { reg: PadReg; device: Controller | null; onOpen: () => void }) {
  // sem dispositivo vinculado (cadastro manual) também mostra o estado, em vez de nada
  const state = !reg.deviceKey ? "none" : device ? "on" : "off";
  return (
    <button type="button" data-focus data-pad-id={reg.id} className="pad-card" onClick={onOpen}
      style={reg.accent ? ({ "--pad-accent": reg.accent } as React.CSSProperties) : undefined}
      aria-label={`${reg.name} — ${[reg.model, reg.brand, reg.platform].filter(Boolean).join(", ")}`}>
      <span className="pad-art"><ControllerVisual visual={reg.visual} /></span>
      <span className={`pad-conn ${state}`} title={state === "none" ? t("Vincule um dispositivo em “Editar dados” para ver se este controle está conectado.") : undefined}>
        <i />{state === "on" ? t("Conectado") : state === "off" ? t("Desconectado") : t("Sem vínculo")}
      </span>
      <span className="pad-meta">
        <strong>{reg.name}</strong>
        <small>{reg.model || t("Modelo não informado")}</small>
        <span className="pad-tags">
          {reg.brand && <em>{reg.brand}</em>}
          {reg.platform && <em>{reg.platform}</em>}
        </span>
      </span>
    </button>
  );
}

// ---------- cadastro ----------

/** `pattern`: confirmar/voltar (ocidental, oriental ou personalizado em "Layout e mapeamento"). */
type Draft = { name: string; model: string; brand: string; platform: string; visual: Visual; deviceKey: string | null; pattern: ConfirmPattern };

/** Linha de detalhes de um dispositivo detectado: fabricante, conexão e origem da leitura. */
const deviceMeta = (d: Controller) =>
  [d.brand, d.connection ? t(CONN_LABEL[d.connection] ?? d.connection) : null, SOURCE_LABEL(d.id)].filter(Boolean).join(" · ");

function AddFlow({ devices, regs, onClose, onCreated, onOpen }: {
  devices: Controller[]; regs: PadReg[]; onClose: () => void; onCreated: (r: PadReg) => void; onOpen: (regId: string) => void;
}) {
  const { toast } = useSession();
  const [draft, setDraft] = useState<Draft | null>(null);
  /** Dispositivo escolhido (para avisar se ele foi cadastrado enquanto o formulário estava aberto). */
  const [picked, setPicked] = useState<Controller | null>(null);
  const [busy, setBusy] = useState(false);
  const owner = (d: Controller) => regs.find((r) => r.deviceKey === d.id) ?? null;
  const physical = devices.filter((d) => !isVirtualDevice(d));
  const free = physical.filter((d) => !owner(d));
  const inCollection = physical.filter((d) => owner(d));
  const virtuals = devices.filter((d) => isVirtualDevice(d) && !owner(d));
  const halves = physical.filter((d) => d.model === "joycon-l" || d.model === "joycon-r");
  const canJoin = halves.some((d) => d.model === "joycon-l") && halves.some((d) => d.model === "joycon-r");

  const fromDevice = (d: Controller) => {
    const p = presetFor(d);
    setPicked(d);
    setDraft({ name: d.model === "sony-unknown" || d.model === "virtual" ? "" : d.name, model: p.model, brand: p.brand, platform: p.platform, visual: p.visual, deviceKey: d.id, pattern: "us" });
  };
  const manual = () => {
    const v = visualOf("xbox-series");
    setPicked(null);
    setDraft({ name: "", model: v.model, brand: v.brand, platform: v.platform, visual: v.id, deviceKey: null, pattern: "us" });
  };

  if (draft) {
    const already = draft.deviceKey ? regs.find((r) => r.deviceKey === draft.deviceKey) ?? null : null;
    return (
      <Modal title={t("Adicionar controle")} onClose={onClose} wide>
        {picked?.model === "sony-unknown" && (
          <div className="pad-callout">{t("Não foi possível identificar com segurança qual controle Sony é este. Escolha abaixo a representação e o modelo corretos.")}</div>
        )}
        {picked?.virtualSource && (
          <div className="pad-callout">{t("Este é um controle virtual criado por {src}. Ele normalmente espelha outro controle físico, que pode ser cadastrado separadamente.", { src: picked.virtualSource })}</div>
        )}
        {already && (
          <div className="pad-callout">{t("Este controle já pertence à coleção como “{name}”.", { name: already.name })}</div>
        )}
        <PadForm draft={draft} onChange={setDraft} devices={devices} regs={regs} selfId={null} />
        <div className="row end">
          <Button variant="ghost" onClick={() => { setDraft(null); setPicked(null); }}>{t("Voltar")}</Button>
          {already ? (
            <Button variant="primary" onClick={() => onOpen(already.id)}>{t("Abrir configurações")}</Button>
          ) : (
            <Button variant="primary" disabled={!draft.name.trim() || busy} onClick={async () => {
              if (busy) return; // um único cadastro mesmo com o comando "confirmar" repetido
              setBusy(true);
              try {
                const { pattern, ...data } = draft;
                const r = await addController({ ...data, name: data.name.trim() });
                if (r.created !== false && pattern === "jp") {
                  await updateController(r.id, (x) => ({ layout: { ...x.layout, map: withPattern(x.layout.map, "jp") } }));
                }
                toast(r.created === false ? t("Este controle já estava na coleção.") : t("Controle adicionado à coleção."));
                onCreated(r);
              } catch (e) { toast(errMsg(e), "err"); setBusy(false); }
            }}>{t("Adicionar à coleção")}</Button>
          )}
        </div>
      </Modal>
    );
  }

  return (
    <Modal title={t("Adicionar controle")} onClose={onClose} wide>
      <h3 className="section sub">{t("Dispositivos detectados")}</h3>
      {halves.length > 0 && (
        <div className="pad-callout">{canJoin
          ? t("Para usar dois Joy-Cons como um controle só, segure L no Joy-Con esquerdo e R no direito ao mesmo tempo. O par aparece aqui como “Joy-Con (L+R)” e volta junto quando reconectar.")
          : t("Conecte o outro Joy-Con para juntar os dois num controle só (segurando L no esquerdo e R no direito ao mesmo tempo).")}</div>
      )}
      {free.length === 0 ? (
        <p className="muted note">{physical.length
          ? t("Todos os controles conectados já estão na coleção.")
          : t("Nenhum controle conectado agora. Conecte por cabo ou Bluetooth, ou cadastre manualmente.")}</p>
      ) : (
        <div className="pad-detect">
          {free.map((d) => (
            <button key={d.id} type="button" data-focus className="pad-detect-item" onClick={() => fromDevice(d)}>
              <ControllerVisual visual={guessVisual(d)} />
              <span><strong>{t(d.name)}</strong><small>{deviceMeta(d)}</small></span>
            </button>
          ))}
        </div>
      )}
      {inCollection.length > 0 && (
        <>
          <h3 className="section sub">{t("Conectados e já na coleção")}</h3>
          <div className="pad-detect">
            {inCollection.map((d) => {
              const r = owner(d)!;
              return (
                <button key={d.id} type="button" data-focus className="pad-detect-item" onClick={() => onOpen(r.id)}>
                  <ControllerVisual visual={r.visual} />
                  <span><strong>{r.name}</strong><small>{t("Já pertence à coleção · abrir configurações")}</small></span>
                </button>
              );
            })}
          </div>
        </>
      )}
      {virtuals.length > 0 && (
        <>
          <h3 className="section sub">{t("Controles virtuais")}</h3>
          <p className="muted note">{t("Criados por programas como DS4Windows, Steam ou Logitech G HUB para espelhar outro controle. Não são controles físicos adicionais e normalmente não precisam ser cadastrados.")}</p>
          <div className="pad-detect">
            {virtuals.map((d) => (
              <button key={d.id} type="button" data-focus className="pad-detect-item virtual" onClick={() => fromDevice(d)}>
                <ControllerVisual visual="generic" />
                <span><strong>{t(d.name)}</strong><small>{t("Virtual · {src}", { src: d.virtualSource ?? "" })}</small></span>
              </button>
            ))}
          </div>
        </>
      )}
      <h3 className="section sub">{t("Ou cadastre manualmente")}</h3>
      <p className="muted note">{t("Para controles que não estão conectados agora ou que o NEXSTAGE não reconhece. Você pode vincular o dispositivo depois.")}</p>
      <div className="row end">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button onClick={manual}>{t("Cadastrar manualmente")}</Button>
      </div>
    </Modal>
  );
}

/** Formulário dos dados do controle (cadastro e "Editar dados"). */
function PadForm({ draft, onChange, devices, regs, selfId }: {
  draft: Draft; onChange: (d: Draft) => void; devices: Controller[]; regs: PadReg[]; selfId: string | null;
}) {
  const set = (p: Partial<Draft>) => onChange({ ...draft, ...p });
  // Trocar a imagem sugere modelo/marca/console, sem apagar o que o usuário já personalizou.
  const pickVisual = (id: Visual) => {
    const prev = visualOf(draft.visual), next = visualOf(id);
    set({
      visual: id,
      model: !draft.model || draft.model === prev.model ? next.model : draft.model,
      brand: !draft.brand || draft.brand === prev.brand ? next.brand : draft.brand,
      platform: !draft.platform || draft.platform === prev.platform ? next.platform : draft.platform,
    });
  };
  const linkOpts: [string, string][] = [[t("Nenhum (cadastro manual)"), ""]];
  for (const d of devices) {
    const owner = regs.find((r) => r.deviceKey === d.id && r.id !== selfId);
    linkOpts.push([owner ? t("{name} (vinculado a {owner})", { name: t(d.name), owner: owner.name }) : t(d.name), d.id]);
  }
  if (draft.deviceKey && !devices.some((d) => d.id === draft.deviceKey)) linkOpts.push([t("{key} (desconectado)", { key: draft.deviceKey }), draft.deviceKey]);

  return (
    <div className="pad-form">
      <div className="pad-form-preview"><ControllerVisual visual={draft.visual} /></div>
      <div className="fields">
        <label>{t("Nome personalizado")}<input value={draft.name} maxLength={40} placeholder={t("Ex.: Controle da sala")} onChange={(e) => set({ name: e.target.value })} /></label>
        <div className="pad-form-row">
          <label>{t("Modelo")}<input value={draft.model} maxLength={60} onChange={(e) => set({ model: e.target.value })} /></label>
          <label>{t("Marca / fabricante")}<input value={draft.brand} maxLength={40} onChange={(e) => set({ brand: e.target.value })} /></label>
        </div>
        <label>{t("Console ou plataforma de origem")}<input value={draft.platform} maxLength={60} onChange={(e) => set({ platform: e.target.value })} /></label>
        <label>{t("Dispositivo vinculado")}<Select<string> value={draft.deviceKey ?? ""} width="100%" title={t("Dispositivo vinculado")}
          options={linkOpts} onChange={(v) => set({ deviceKey: v || null })} /></label>
        <p className="muted note">{t("O vínculo faz os ajustes deste cadastro valerem quando esse dispositivo estiver em uso no NEXSTAGE.")}</p>
        <div className="pad-form-pattern" role="radiogroup" aria-label={t("Confirmar e voltar")}>
          <span className="muted">{t("Confirmar e voltar")}</span>
          <PatternOptions value={draft.pattern} style={styleOf(draft.visual)} onPick={(pattern) => set({ pattern })} />
          {draft.pattern === "custom" && <p className="muted note">{t("Mapeamento personalizado em “Layout e mapeamento”. Escolher um padrão acima redefine só confirmar e voltar.")}</p>}
        </div>
      </div>
      <div className="pad-form-visuals">
        <span className="muted">{t("Representação visual")}</span>
        <VisualPicker value={draft.visual} onPick={pickVisual} />
      </div>
    </div>
  );
}

function VisualPicker({ value, onPick }: { value: Visual; onPick: (v: Visual) => void }) {
  return (
    <div className="pad-visuals" role="radiogroup" aria-label={t("Representação visual")}>
      {VISUALS.map((v) => (
        <button key={v.id} type="button" data-focus role="radio" aria-checked={value === v.id}
          className={`pad-visual ${value === v.id ? "on" : ""}`} onClick={() => onPick(v.id)}>
          <ControllerVisual visual={v.id} />
          <small>{t(v.label)}</small>
        </button>
      ))}
    </div>
  );
}

// ---------- página individual ----------

type Cat = "identity" | "led" | "vibration" | "deadzone" | "layout" | "confirm" | "tester" | "diagnostics";
const CATS: { id: Cat; label: string; icon: string }[] = [
  { id: "identity", label: tk("Identidade e aparência"), icon: "M12 12a4 4 0 100-8 4 4 0 000 8zM4 20c0-4 4-6 8-6s8 2 8 6" },
  { id: "led", label: tk("Iluminação LED"), icon: "M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2.1h5c0-.9.4-1.6 1-2.1A6 6 0 0 0 12 3Z" },
  { id: "vibration", label: tk("Vibração"), icon: "M8 5h8v14H8zM4 8v8M20 8v8M2 10v4M22 10v4" },
  { id: "deadzone", label: tk("Zona morta dos analógicos"), icon: "M12 21a9 9 0 100-18 9 9 0 000 18zM12 15a3 3 0 100-6 3 3 0 000 6z" },
  { id: "layout", label: tk("Layout e mapeamento"), icon: "M4 6h7M4 12h7M4 18h7M15 6h5M15 12h5M15 18h5M11 6l4 6M11 12l4-6M11 18h4" },
  { id: "confirm", label: tk("Padrão de confirmação"), icon: "M5 12l4 4L19 6" },
  { id: "tester", label: tk("Testador de controles"), icon: "M6 8h12a4 4 0 014 4v3a2 2 0 01-3.5 1.3L16 14H8l-2.5 2.3A2 2 0 012 15v-3a4 4 0 014-4zM8 10v3M6.5 11.5h3M16 11h.01M18 12.5h.01" },
  { id: "diagnostics", label: tk("Diagnóstico do dispositivo"), icon: "M3 12h4l2-6 4 12 2-6h6" },
];

const Icon = ({ d, size = 18 }: { d: string; size?: number }) => (
  <svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={d} /></svg>
);

/** Testador / pré-visualização ao vivo em uso: o "voltar" da página não deve reagir. */
let monitorBusy = false;

function PadDetail({ reg, devices, onBack, onDeleted }: {
  reg: PadReg; devices: Controller[]; onBack: () => void; onDeleted: (neighbor: string | null) => void;
}) {
  const { toast } = useSession();
  const regs = useControllers();
  const [cat, setCat] = useState<Cat>("identity");
  const [editing, setEditing] = useState<Draft | null>(null);
  const [confirmDel, setConfirmDel] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const device = devices.find((d) => d.id === reg.deviceKey) ?? null;
  const style = glyphsOf(reg);

  // Foco inicial no botão "Voltar à coleção".
  useEffect(() => { ref.current?.querySelector<HTMLElement>("[data-focus]")?.focus({ preventScroll: true }); }, []);

  // ESC / B / ○: volta à coleção (antes do "voltar para a Home" do app), se nada estiver por cima.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || monitorBusy) return;
      if (document.querySelector(".overlay, .sm-root:not(.leaving), .pick-root, .kb-root, .ctx-root") || layerJustClosed()) return;
      const tg = e.target as HTMLElement;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(tg.tagName)) return;
      e.stopImmediatePropagation();
      play("back");
      onBack();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onBack]);

  const conn = !reg.deviceKey ? t("Sem dispositivo vinculado") : device ? t("Conectado") : t("Desconectado");

  return (
    <div className="pad-detail" ref={ref} style={reg.accent ? ({ "--pad-accent": reg.accent } as React.CSSProperties) : undefined}>
      <div className="pad-back-row">
        <Button variant="ghost" className="pad-back" onClick={() => { play("back"); onBack(); }}>
          <Icon d="M15 18l-6-6 6-6" /> {t("Voltar à coleção")}
        </Button>
      </div>

      <header className="pad-head">
        <div className="pad-head-art"><ControllerVisual visual={reg.visual} title={reg.model} /></div>
        <div className="pad-head-info">
          <span className="pad-eyebrow">{t("Central de Controles")}</span>
          <h1>{reg.name}</h1>
          <dl className="pad-facts">
            <div><dt>{t("Marca")}</dt><dd>{reg.brand || "—"}</dd></div>
            <div><dt>{t("Modelo")}</dt><dd>{reg.model || "—"}</dd></div>
            <div><dt>{t("Console de origem")}</dt><dd>{reg.platform || "—"}</dd></div>
          </dl>
          <div className="pad-status">
            <span className={`pad-conn ${!reg.deviceKey ? "none" : device ? "on" : "off"}`}><i />{conn}</span>
            {device?.connection && <span className="pad-chip">{t(CONN_LABEL[device.connection] ?? device.connection)}</span>}
            {device && (device.wired || device.batteryPercent !== null || device.batteryLabel) && <Battery c={device} />}
          </div>
          <div className="row pad-head-actions">
            <Button onClick={() => setEditing(draftOf(reg))}>
              {t("Editar dados")}
            </Button>
            <Button variant="danger" onClick={() => setConfirmDel(true)}>{t("Excluir da coleção")}</Button>
          </div>
        </div>
      </header>

      <nav className="pad-cats" data-nav-group aria-label={t("Categorias de configuração")}>
        {CATS.map((c, i) => (
          <button key={c.id} type="button" data-focus className={`pad-cat ${cat === c.id ? "on" : ""}`} aria-current={cat === c.id ? "true" : undefined} onClick={() => setCat(c.id)}>
            <span className="pad-cat-n">{i + 1}</span><Icon d={c.icon} /><span>{t(c.label)}</span>
          </button>
        ))}
      </nav>

      <section className="pad-panel" aria-label={t(CATS.find((c) => c.id === cat)!.label)}>
        {cat === "identity" && <IdentityPanel reg={reg} onEdit={() => setEditing(draftOf(reg))} />}
        {cat === "led" && <LedPanel reg={reg} device={device} />}
        {cat === "vibration" && <VibrationPanel reg={reg} device={device} />}
        {cat === "deadzone" && <DeadzonePanel reg={reg} device={device} />}
        {cat === "layout" && <LayoutPanel reg={reg} style={style} />}
        {cat === "confirm" && <ConfirmPanel reg={reg} style={style} />}
        {cat === "tester" && <TesterPanel reg={reg} device={device} style={style} />}
        {cat === "diagnostics" && <DiagnosticsPanel reg={reg} device={device} />}
        <p className="muted note pad-isolation">
          {t("Estes ajustes valem só para a navegação e a interface do NEXSTAGE. Jogos, emuladores e o Windows continuam recebendo o controle exatamente como antes.")}
        </p>
      </section>

      {editing && (
        <Modal title={t("Editar dados do controle")} onClose={() => setEditing(null)} wide>
          <PadForm draft={editing} onChange={setEditing} devices={devices} regs={regs} selfId={reg.id} />
          <div className="row end">
            <Button variant="ghost" onClick={() => setEditing(null)}>{t("Cancelar")}</Button>
            <Button variant="primary" disabled={!editing.name.trim()} onClick={async () => {
              try {
                const { pattern, ...data } = editing;
                await updateController(reg.id, (r) => ({
                  ...data,
                  name: data.name.trim(),
                  // personalizado: mantém o mapeamento feito em "Layout e mapeamento"
                  ...(pattern !== "custom" && pattern !== confirmPattern(r.layout.map) ? { layout: { ...r.layout, map: withPattern(r.layout.map, pattern) } } : {}),
                }));
                toast(t("Dados do controle salvos."));
                setEditing(null);
              }
              catch (e) { toast(errMsg(e), "err"); }
            }}>{t("Salvar")}</Button>
          </div>
        </Modal>
      )}
      {confirmDel && (
        <Modal title={t("Excluir “{name}” da coleção?", { name: reg.name })} onClose={() => setConfirmDel(false)}>
          <p className="muted">{t("Remove só o cadastro e os ajustes deste controle no NEXSTAGE.")}</p>
          <p className="muted note">{t("O dispositivo não é desinstalado nem desconectado e continua sendo reconhecido normalmente, pelo NEXSTAGE (com os ajustes padrão), pelos jogos e pelo Windows.")}</p>
          <div className="row end">
            <Button variant="ghost" onClick={() => setConfirmDel(false)}>{t("Cancelar")}</Button>
            <Button variant="danger" onClick={async () => {
              const i = regs.findIndex((r) => r.id === reg.id);
              const neighbor = regs[i + 1]?.id ?? regs[i - 1]?.id ?? null;
              try { await removeController(reg.id); toast(t("Controle removido da coleção.")); setConfirmDel(false); onDeleted(neighbor); }
              catch (e) { toast(errMsg(e), "err"); }
            }}>{t("Excluir")}</Button>
          </div>
        </Modal>
      )}
    </div>
  );
}

// ---------- 1. Identidade e aparência ----------

const SWATCHES = ["#2f6bff", "#22e4ff", "#10b981", "#4ade80", "#facc15", "#f97316", "#ef4444", "#ff4fa3", "#a855f7", "#e6ebff", "#7d86a3", "#111827"];

function IdentityPanel({ reg, onEdit }: { reg: PadReg; onEdit: () => void }) {
  return (
    <div className="settings">
      <div className="setting">
        <div><strong>{t("Dados do controle")}</strong>
          <small>{[reg.name, reg.model, reg.brand, reg.platform].filter(Boolean).join(" · ")}</small></div>
        <Button onClick={onEdit}>{t("Editar dados")}</Button>
      </div>
      <div className="setting col-set">
        <div><strong>{t("Representação visual")}</strong><small>{t("Imagem usada no cartão da coleção e no cabeçalho.")}</small></div>
        <VisualPicker value={reg.visual} onPick={(v) => updateController(reg.id, { visual: v })} />
        <PhotoCredit visual={reg.visual} />
      </div>
      <div className="setting col-set">
        <div><strong>{t("Cor do cartão")}</strong><small>{t("Brilho de destaque do cartão na coleção. Automática acompanha o tema.")}</small></div>
        <div className="pad-swatches">
          <button type="button" data-focus className={`pad-swatch auto ${!reg.accent ? "on" : ""}`} onClick={() => updateController(reg.id, { accent: null })}>{t("Automática")}</button>
          {SWATCHES.map((c) => (
            <button key={c} type="button" data-focus className={`pad-swatch ${reg.accent === c ? "on" : ""}`} style={{ background: c }} aria-label={c}
              onClick={() => updateController(reg.id, { accent: c })} />
          ))}
        </div>
      </div>
    </div>
  );
}

/** Crédito da foto real (licenças livres do Wikimedia Commons). */
function PhotoCredit({ visual }: { visual: Visual }) {
  const c = PHOTO_CREDITS[visual];
  if (!c) return <p className="muted pad-credit">{t("Ilustração do NEXSTAGE.")}</p>;
  return (
    <p className="muted pad-credit">
      {t("Foto: {author} · {license} · Wikimedia Commons (fundo removido pelo NEXSTAGE).", { author: c.author, license: t(c.license) })}{" "}
      <button type="button" data-focus className="pad-credit-link" onClick={() => openUrl(commonsUrl(c.file)).catch(() => {})}>{t("Ver original")}</button>
    </p>
  );
}

// ---------- 2. Iluminação LED ----------

function LedPanel({ reg, device }: { reg: PadReg; device: Controller | null }) {
  const sonyVisual = ["ps4", "ps5"].includes(reg.visual);
  const supported = !!reg.deviceKey?.startsWith("ps:");
  const modes: { id: LedMode; label: string; desc: string }[] = [
    { id: "theme", label: tk("Cor do tema"), desc: tk("Acompanha a cor de destaque do perfil ativo.") },
    { id: "custom", label: tk("Cor personalizada"), desc: tk("Uma cor fixa só para este controle.") },
    { id: "dark", label: tk("Apagado"), desc: tk("Desliga a barra de luz enquanto o NEXSTAGE controla a iluminação.") },
    { id: "system", label: tk("Não alterar"), desc: tk("O NEXSTAGE não mexe na iluminação deste controle.") },
  ];
  return (
    <div className="settings">
      {!supported && (
        <div className="pad-callout">
          {sonyVisual
            ? t("A barra de luz é controlada quando este cadastro está vinculado a um DualSense ou DualShock 4 conectado. Vincule o dispositivo em “Editar dados”.")
            : t("Este modelo não tem iluminação que o NEXSTAGE consiga controlar (disponível para DualSense e DualShock 4). A preferência fica salva caso você vincule outro dispositivo.")}
        </div>
      )}
      <div className="pad-options">
        {modes.map((m) => (
          <button key={m.id} type="button" data-focus className={`radio ${reg.led.mode === m.id ? "on" : ""}`} role="radio" aria-checked={reg.led.mode === m.id}
            onClick={() => updateController(reg.id, (r) => ({ led: { ...r.led, mode: m.id } }))}>
            <i aria-hidden="true" />
            <span><strong>{t(m.label)}</strong><small>{t(m.desc)}</small></span>
          </button>
        ))}
      </div>
      {reg.led.mode === "custom" && (
        <div className="setting col-set">
          <div><strong>{t("Cor da barra de luz")}</strong><small>{t("Atual:")} <b>{reg.led.color.toUpperCase()}</b>{device && supported ? "" : ` · ${t("aplicada quando o controle estiver conectado")}`}</small></div>
          <div className="pad-swatches">
            {SWATCHES.filter((c) => c !== "#111827").map((c) => (
              <button key={c} type="button" data-focus className={`pad-swatch ${reg.led.color === c ? "on" : ""}`} style={{ background: c }} aria-label={c}
                onClick={() => updateController(reg.id, (r) => ({ led: { ...r.led, color: c } }))} />
            ))}
            <HexInput value={reg.led.color} onChange={(c) => updateController(reg.id, (r) => ({ led: { ...r.led, color: c } }))} />
          </div>
        </div>
      )}
    </div>
  );
}

function HexInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  return (
    <input className="pad-hex" value={v} maxLength={7} spellCheck={false} aria-label={t("Cor em hexadecimal")}
      onChange={(e) => { const n = e.target.value.startsWith("#") ? e.target.value : `#${e.target.value}`; setV(n); if (/^#[0-9a-f]{6}$/i.test(n)) onChange(n.toLowerCase()); }} />
  );
}

// ---------- 3. Vibração ----------

function VibrationPanel({ reg, device }: { reg: PadReg; device: Controller | null }) {
  const { toast } = useSession();
  const v = reg.vibration;
  const set = (p: Partial<PadReg["vibration"]>) => updateController(reg.id, (r) => ({ vibration: { ...r.vibration, ...p } }));
  const canTest = !!device && !!reg.deviceKey;
  return (
    <div className="settings">
      <div className="setting">
        <div><strong>{t("Vibração no NEXSTAGE")}</strong><small>{t("Usada só pelas respostas da interface do NEXSTAGE. A vibração dos jogos não é alterada.")}</small></div>
        <Button variant={v.enabled ? "primary" : "default"} onClick={() => set({ enabled: !v.enabled })}>{v.enabled ? t("Ligada") : t("Desligada")}</Button>
      </div>
      <div className={`setting ${v.enabled ? "" : "disabled"}`}>
        <div><strong>{t("Intensidade")}</strong><small>{t("{v}%", { v: Math.round(v.strength) })}</small></div>
        <input type="range" min={10} max={100} step={5} value={v.strength} disabled={!v.enabled} aria-label={t("Intensidade")}
          onChange={(e) => set({ strength: Number(e.target.value) })} className="pad-range" />
      </div>
      <div className={`setting ${v.enabled ? "" : "disabled"}`}>
        <div><strong>{t("Resposta ao confirmar e voltar")}</strong><small>{t("Um toque curto de vibração ao confirmar ou voltar na interface.")}</small></div>
        <Button variant={v.onNav ? "primary" : "default"} disabled={!v.enabled} onClick={() => set({ onNav: !v.onNav })}>{v.onNav ? t("Ligada") : t("Desligada")}</Button>
      </div>
      <div className="setting">
        <div><strong>{t("Testar vibração")}</strong><small>{canTest ? t("Vibra o controle vinculado com a intensidade escolhida.") : t("Conecte e vincule o controle para testar.")}</small></div>
        <Button disabled={!canTest || !v.enabled} onClick={async () => {
          const ok = await testRumble(reg.deviceKey!, v.strength).catch(() => false);
          if (!ok) toast(t("Este controle não informa suporte a vibração."), "err");
        }}>{t("Testar")}</Button>
      </div>
    </div>
  );
}

// ---------- monitor (testador e pré-visualizações ao vivo) ----------

interface PadState { key: string; mask: number; lx: number; ly: number; rx: number; ry: number; lt: number; rt: number; hz: number }
const HOLD_EXIT_MS = 1500;

/** Estado bruto do controle `key` enquanto `on`; segurar B/○ (ou ESC) chama `onExit`. Pausa a navegação. */
function useMonitor(key: string | null, on: boolean, onExit: () => void): { st: PadState | null; hold: number } {
  const [st, setSt] = useState<PadState | null>(null);
  const [hold, setHold] = useState(0);
  const exitRef = useRef(onExit);
  exitRef.current = onExit;

  useEffect(() => {
    if (!on || !key) return;
    monitorBusy = true;
    setPadMonitorNav(true);
    let since = 0, done = false, raf = 0, un: (() => void) | undefined, cancelled = false;
    const exit = () => { if (!done) { done = true; exitRef.current(); } };
    const feed = (s: PadState) => {
      setSt(s);
      if (s.mask & BIT.B) {
        since ||= performance.now();
        const p = Math.min(1, (performance.now() - since) / HOLD_EXIT_MS);
        setHold(p);
        if (p >= 1) exit();
      } else { since = 0; setHold(0); }
    };
    if (key.startsWith("gp:")) {
      let stamps = 0, last = 0, t0 = performance.now(), hz = 0;
      const loop = () => {
        const g = gamepadByKey(key); // o controle certo mesmo com dois do mesmo modelo
        if (g) {
          if (g.timestamp !== last) { stamps++; last = g.timestamp; }
          const now = performance.now();
          if (now - t0 >= 1000) { hz = Math.round(stamps / ((now - t0) / 1000)); stamps = 0; t0 = now; }
          let mask = 0;
          g.buttons.forEach((b, i) => { if (i < 18 && b.pressed) mask |= 1 << i; /* 16 = Home, 17 = touchpad */ });
          feed({ key, mask, lx: g.axes[0] ?? 0, ly: g.axes[1] ?? 0, rx: g.axes[2] ?? 0, ry: g.axes[3] ?? 0, lt: g.buttons[6]?.value ?? 0, rt: g.buttons[7]?.value ?? 0, hz });
        }
        raf = requestAnimationFrame(loop);
      };
      raf = requestAnimationFrame(loop);
    } else {
      api.setPadMonitor(true).catch(() => {});
      listen<PadState[]>("pad-state", (e) => {
        const s = e.payload.find((x) => x.key === key);
        if (s) feed(s);
      }).then((f) => (cancelled ? f() : (un = f)));
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(".overlay")) return;
      e.stopImmediatePropagation();
      exit();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      cancelled = true;
      un?.();
      cancelAnimationFrame(raf);
      window.removeEventListener("keydown", onKey, true);
      if (!key.startsWith("gp:")) api.setPadMonitor(false).catch(() => {});
      setPadMonitorNav(false);
      monitorBusy = false;
      setSt(null);
      setHold(0);
    };
  }, [on, key]);

  // Segurança: ao sair da página com o monitor ligado, a limpeza acima desliga tudo.
  return { st, hold };
}

function MonitorBar({ hold, onStop }: { hold: number; onStop: () => void }) {
  return (
    <div className="pad-monitor-bar" role="status">
      <span className="pad-live-dot" aria-hidden="true" />
      <span>{t("Navegação pelos controles pausada. Segure B / ○ ou pressione ESC para sair.")}</span>
      <span className="pad-hold" aria-hidden="true"><b style={{ transform: `scaleX(${hold})` }} /></span>
      <Button onClick={onStop}>{t("Encerrar")}</Button>
    </div>
  );
}

// ---------- 4. Zona morta ----------

function DeadzonePanel({ reg, device }: { reg: PadReg; device: Controller | null }) {
  const [live, setLive] = useState(false);
  const stop = useCallback(() => setLive(false), []);
  const { st, hold } = useMonitor(reg.deviceKey, live, stop);
  // A navegação fica pausada durante a visualização: os círculos vêm para a tela antes.
  const dzRef = useRef<HTMLDivElement>(null);
  useEffect(() => { if (live) dzRef.current?.scrollIntoView({ block: "center", behavior: "smooth" }); }, [live]);
  const dz = reg.deadzone;
  const set = (p: Partial<PadReg["deadzone"]>) => updateController(reg.id, (r) => ({ deadzone: { ...r.deadzone, ...p } }));
  const pct = (v: number) => `${Math.round(v * 100)}%`;
  return (
    <div className="settings">
      {live && <MonitorBar hold={hold} onStop={stop} />}
      <div className="pad-dz" ref={dzRef}>
        <DzStick label={t("Analógico esquerdo")} hint={t("Mover o foco")} dz={dz.left} th={dz.left + (1 - dz.left) * 0.5} x={st?.lx ?? 0} y={st?.ly ?? 0} live={live} />
        <DzStick label={t("Analógico direito")} hint={t("Rolar a página")} dz={dz.right} x={st?.rx ?? 0} y={st?.ry ?? 0} live={live} />
      </div>
      <div className="setting">
        <div><strong>{t("Analógico esquerdo")}</strong><small>{t("Zona morta: {dz} · move o foco a partir de {th} do curso", { dz: pct(dz.left), th: pct(dz.left + (1 - dz.left) * 0.5) })}</small></div>
        <input type="range" className="pad-range" min={5} max={80} step={1} value={Math.round(dz.left * 100)} aria-label={t("Zona morta do analógico esquerdo")}
          onChange={(e) => set({ left: Number(e.target.value) / 100 })} />
      </div>
      <div className="setting">
        <div><strong>{t("Analógico direito")}</strong><small>{t("Zona morta: {dz} · abaixo disso a página não rola", { dz: pct(dz.right) })}</small></div>
        <input type="range" className="pad-range" min={5} max={80} step={1} value={Math.round(dz.right * 100)} aria-label={t("Zona morta do analógico direito")}
          onChange={(e) => set({ right: Number(e.target.value) / 100 })} />
      </div>
      <div className="setting">
        <div><strong>{t("Visualizar ao vivo")}</strong><small>{device ? t("Mostra a posição real dos analógicos sobre as zonas.") : t("Conecte e vincule o controle para visualizar.")}</small></div>
        <div className="row">
          <Button variant="ghost" disabled={dz.left === DEFAULTS.deadzone.left && dz.right === DEFAULTS.deadzone.right} onClick={() => set({ ...DEFAULTS.deadzone })}>{t("Restaurar padrão")}</Button>
          <Button variant={live ? "primary" : "default"} disabled={!device} onClick={() => setLive(!live)}>{live ? t("Encerrar") : t("Visualizar")}</Button>
        </div>
      </div>
    </div>
  );
}

function DzStick({ label, hint, dz, th, x, y, live }: { label: string; hint: string; dz: number; th?: number; x: number; y: number; live: boolean }) {
  const mag = Math.min(1, Math.hypot(x, y));
  const inside = mag <= dz;
  return (
    <figure className="pad-dz-stick">
      <svg viewBox="-50 -50 100 100" aria-hidden="true">
        <circle r="46" className="dz-range" />
        {th !== undefined && <circle r={46 * th} className="dz-th" />}
        <circle r={46 * dz} className="dz-dead" />
        <path d="M-46 0H46M0 -46V46" className="dz-axis" />
        {live && <circle cx={x * 46} cy={y * 46} r="5" className={`dz-dot ${inside ? "in" : ""}`} />}
      </svg>
      <figcaption><strong>{label}</strong><small>{hint}</small></figcaption>
    </figure>
  );
}

// ---------- 5. Layout e mapeamento ----------

const ACTION_LABEL: Record<Action, string> = {
  confirm: tk("Confirmar"), back: tk("Voltar"), x: tk("Ação secundária"), y: tk("Ação extra"),
  l1: tk("Página anterior"), r1: tk("Próxima página"), l2: tk("Atalho esquerdo (filtros)"), r2: tk("Atalho direito (filtros)"), menu: tk("Menu de opções"),
};

function LayoutPanel({ reg, style }: { reg: PadReg; style: Exclude<GlyphStyle, "auto"> }) {
  const map = reg.layout.map;
  const labels = BTN_LABEL[style];
  const changed = ACTIONS.some((a) => map[a] !== DEFAULT_MAP[a]);
  const setMap = (m: Record<Action, Btn>) => updateController(reg.id, (r) => ({ layout: { ...r.layout, map: m } }));
  return (
    <div className="settings">
      <div className="setting">
        <div><strong>{t("Estilo dos ícones")}</strong><small>{t("Ícones de botões exibidos pela interface quando este controle está em uso. Automático segue o modelo.")}</small></div>
        <Seg<GlyphStyle> value={reg.layout.glyphs} onChange={(g) => updateController(reg.id, (r) => ({ layout: { ...r.layout, glyphs: g } }))}
          opts={[[t("Automático"), "auto"], ["Xbox", "xbox"], ["PlayStation", "playstation"], ["Nintendo", "nintendo"], [t("Genérico"), "generic"]]} />
      </div>
      <h3 className="section sub">{t("Mapeamento interno")}</h3>
      <p className="muted note">{t("Escolha qual botão faz cada comando da interface. Ao escolher um botão já usado, os dois comandos trocam de botão. O direcional e o analógico esquerdo sempre movem o foco.")}</p>
      <div className="pad-map">
        {ACTIONS.map((a) => (
          <div key={a} className="pad-map-row">
            <span>{t(ACTION_LABEL[a])}</span>
            <Select<Btn> value={map[a]} width={190} title={t(ACTION_LABEL[a])} onChange={(b) => setMap(assign(map, a, b))}
              options={BTNS.map((b): [string, Btn] => [labels[b], b])} />
          </div>
        ))}
      </div>
      <div className="row end">
        <Button variant="ghost" disabled={!changed} onClick={() => setMap({ ...DEFAULT_MAP })}>{t("Restaurar mapeamento padrão")}</Button>
      </div>
    </div>
  );
}

// ---------- 6. Padrão de confirmação ----------

/** Dados de um cadastro no formulário "Editar dados". */
const draftOf = (r: PadReg): Draft => ({
  name: r.name, model: r.model, brand: r.brand, platform: r.platform, visual: r.visual, deviceKey: r.deviceKey, pattern: confirmPattern(r.layout.map),
});

/** Estilo de ícones sugerido por uma representação visual (formulário de cadastro/edição). */
const styleOf = (v: Visual): Exclude<GlyphStyle, "auto"> => {
  const g = visualOf(v).glyphs;
  return g === "keyboard" ? "xbox" : g;
};

/** Ocidental (botão de baixo confirma) ou oriental (botão da direita confirma). */
function PatternOptions({ value, style, onPick }: { value: ConfirmPattern; style: Exclude<GlyphStyle, "auto">; onPick: (p: "us" | "jp") => void }) {
  const L = BTN_LABEL[style];
  const opt = (id: "us" | "jp", title: string, desc: string, c: Btn, b: Btn) => (
    <button type="button" data-focus role="radio" aria-checked={value === id} className={`radio pad-pattern ${value === id ? "on" : ""}`}
      onClick={() => onPick(id)}>
      <i aria-hidden="true" />
      <span className="pad-pattern-body">
        <strong>{t(title)}</strong>
        <small>{t(desc)}</small>
        <span className="pad-pattern-demo">
          {/* os quatro botões de ação na posição física: confirmar em destaque, voltar contornado */}
          <span className="pad-diamond" aria-hidden="true">
            {(["Y", "X", "B", "A"] as Btn[]).map((k) => (
              <kbd key={k} className={`d-${k}${k === c ? " ok" : k === b ? " back" : ""}`}>{L[k]}</kbd>
            ))}
          </span>
          <span className="pad-pattern-legend">
            <span><kbd className="ok">{L[c]}</kbd>{t("confirma")}</span>
            <span><kbd className="back">{L[b]}</kbd>{t("volta")}</span>
          </span>
        </span>
      </span>
    </button>
  );
  return (
    <div className="pad-options two">
      {opt("us", tk("Ocidental"), tk("Botão de baixo confirma e o da direita volta."), "A", "B")}
      {opt("jp", tk("Oriental"), tk("Botão da direita confirma e o de baixo volta, como nos consoles japoneses."), "B", "A")}
    </div>
  );
}

function ConfirmPanel({ reg, style }: { reg: PadReg; style: Exclude<GlyphStyle, "auto"> }) {
  const p = confirmPattern(reg.layout.map);
  return (
    <div className="settings">
      <PatternOptions value={p} style={style}
        onPick={(id) => updateController(reg.id, (r) => ({ layout: { ...r.layout, map: withPattern(r.layout.map, id) } }))} />
      {p === "custom" && <div className="pad-callout">{t("Mapeamento personalizado em “Layout e mapeamento”. Escolher um padrão acima redefine só confirmar e voltar.")}</div>}
      <p className="muted note">{t("Os ícones de comando da interface acompanham o padrão quando este controle está em uso.")}</p>
    </div>
  );
}

// ---------- 7. Testador ----------

function TesterPanel({ reg, device, style }: { reg: PadReg; device: Controller | null; style: Exclude<GlyphStyle, "auto"> }) {
  const [on, setOn] = useState(false);
  const stop = useCallback(() => setOn(false), []);
  const { st, hold } = useMonitor(reg.deviceKey, on, stop);
  const startRef = useRef<HTMLButtonElement>(null);
  // Ao encerrar, o foco volta ao botão "Iniciar teste".
  const wasOn = useRef(false);
  useEffect(() => { if (wasOn.current && !on) startRef.current?.focus({ preventScroll: true }); wasOn.current = on; }, [on]);
  if (!reg.deviceKey || !device) {
    return <div className="pad-callout">{t("Conecte e vincule este controle (em “Editar dados”) para usar o testador.")}</div>;
  }
  return (
    <div className="settings">
      <div className="setting">
        <div><strong>{t("Testar botões, analógicos e gatilhos")}</strong>
          <small>{t("Abre o testador em tela cheia. A navegação pelos controles fica pausada; para sair, segure B / ○ por 1,5 s ou pressione ESC.")}</small></div>
        <button ref={startRef} type="button" data-focus className="btn primary" onClick={() => setOn(true)}>{t("Iniciar teste")}</button>
      </div>
      {on && <TesterLayer reg={reg} st={st} hold={hold} style={style} onStop={stop} />}
    </div>
  );
}

/**
 * Testador em tela cheia: o MESMO desenho do modelo cadastrado (touchpad, botão PS/Home...) acende
 * ao vivo. Fica por cima de tudo e cabe na tela, então nada precisa ser rolado durante o teste.
 */
function TesterLayer({ reg, st, hold, style, onStop }: {
  reg: PadReg; st: PadState | null; hold: number; style: Exclude<GlyphStyle, "auto">; onStop: () => void;
}) {
  const L = BTN_LABEL[style];
  const m = st?.mask ?? 0;
  const lit = (bit: number) => (m & bit ? "on" : "");
  const trig = (v: number, b: "LT" | "RT", sh: "LB" | "RB") => (
    <div className="pad-trig">
      <span className={`pad-key ${lit(BIT[sh])}`}>{L[sh]}</span>
      <span className={`pad-key ${lit(BIT[b])}`}>{L[b]}</span>
      <span className="pad-trig-bar"><b style={{ transform: `scaleX(${Math.min(1, Math.max(0, v))})` }} /></span>
      <span className="pad-trig-val">{Math.round(v * 100)}%</span>
    </div>
  );
  const extras: [string, number][] = [
    [L.SELECT, BIT.SELECT], [L.START, BIT.START], [L.L3, BIT.L3], [L.R3, BIT.R3],
    [style === "playstation" ? "PS" : style === "xbox" ? "Guide" : "Home", ART_BITS.HOME],
    ...(HAS_TOUCHPAD(reg.visual) ? [[t("Touchpad"), ART_BITS.TOUCH] as [string, number]] : []),
  ];
  return (
    <div className="pad-test-layer" role="dialog" aria-modal="true" aria-label={t("Testador de controles")}>
      <div className="pad-test-card">
        <header className="pad-test-head">
          <div><span className="pad-eyebrow">{t("Testador de controles")}</span><h2>{reg.name}</h2></div>
          <button type="button" className="btn" onClick={onStop}>{t("Encerrar")}</button>
        </header>
        <div className="pad-test-trigs">{trig(st?.lt ?? 0, "LT", "LB")}{trig(st?.rt ?? 0, "RT", "RB")}</div>
        <div className="pad-test-art"><ControllerArt visual={reg.visual} live={st} title={reg.model} /></div>
        <div className="pad-test-keys">
          {extras.map(([label, bit]) => <span key={label} className={`pad-key ${lit(bit)}`}>{label}</span>)}
        </div>
        <dl className="pad-readout four">
          <div><dt>{t("Analógico esquerdo")}</dt><dd>{fmt(st?.lx)} · {fmt(st?.ly)}</dd></div>
          <div><dt>{t("Analógico direito")}</dt><dd>{fmt(st?.rx)} · {fmt(st?.ry)}</dd></div>
          <div><dt>{t("Gatilhos")}</dt><dd>{Math.round((st?.lt ?? 0) * 100)}% · {Math.round((st?.rt ?? 0) * 100)}%</dd></div>
          <div><dt>{t("Taxa de leitura")}</dt><dd>{st?.hz ? `${st.hz} Hz` : "—"}</dd></div>
        </dl>
        <footer className="pad-test-foot">
          <span className="pad-live-dot" aria-hidden="true" />
          <span>{st ? t("Recebendo dados do controle.") : t("Aguardando o controle… pressione qualquer botão.")}</span>
          <span className="grow" />
          <span>{t("Segure B / ○ para sair")}</span>
          <span className="pad-hold" aria-hidden="true"><b style={{ transform: `scaleX(${hold})` }} /></span>
        </footer>
      </div>
    </div>
  );
}
const fmt = (v?: number) => (v ?? 0).toFixed(2).replace("-0.00", "0.00");

// ---------- 8. Diagnóstico ----------

interface DiagResult { hz: number; drift: [number, number]; trig: number; stuck: number; samples: number }

function DiagnosticsPanel({ reg, device }: { reg: PadReg; device: Controller | null }) {
  const { toast } = useSession();
  const [running, setRunning] = useState(false);
  const [res, setRes] = useState<DiagResult | null>(null);
  const acc = useRef<{ hz: number[]; l: number; r: number; trig: number; stuck: number; n: number }>({ hz: [], l: 0, r: 0, trig: 0, stuck: 0xffff, n: 0 });
  const stop = useCallback(() => setRunning(false), []);
  const { st } = useMonitor(reg.deviceKey, running, stop);

  // Coleta durante a medição (3 s com os analógicos soltos).
  useEffect(() => {
    if (!running || !st) return;
    const a = acc.current;
    a.n++;
    if (st.hz) a.hz.push(st.hz);
    a.l = Math.max(a.l, Math.hypot(st.lx, st.ly));
    a.r = Math.max(a.r, Math.hypot(st.rx, st.ry));
    a.trig = Math.max(a.trig, st.lt, st.rt);
    a.stuck &= st.mask; // botão "preso" = pressionado em todas as amostras
  }, [st, running]);
  useEffect(() => {
    if (!running) return;
    acc.current = { hz: [], l: 0, r: 0, trig: 0, stuck: 0xffff, n: 0 };
    const tm = setTimeout(() => {
      const a = acc.current;
      setRes({ hz: a.hz.length ? Math.round(a.hz.reduce((x, y) => x + y, 0) / a.hz.length) : 0, drift: [a.l, a.r], trig: a.trig, stuck: a.n ? a.stuck : 0, samples: a.n });
      setRunning(false);
    }, 3000);
    return () => clearTimeout(tm);
  }, [running]);

  const dz = reg.deadzone;
  const tips: string[] = [];
  if (res) {
    if (res.samples === 0) tips.push(t("Nenhuma leitura recebida do controle. Verifique a conexão e o vínculo do dispositivo."));
    if (res.drift[0] >= dz.left) tips.push(t("O analógico esquerdo saiu da zona morta em repouso ({v}%). Aumente a zona morta do analógico esquerdo.", { v: Math.round(res.drift[0] * 100) }));
    if (res.drift[1] >= dz.right) tips.push(t("O analógico direito saiu da zona morta em repouso ({v}%). Aumente a zona morta do analógico direito.", { v: Math.round(res.drift[1] * 100) }));
    if (res.trig > 0.15) tips.push(t("Um gatilho não voltou totalmente ao repouso ({v}%).", { v: Math.round(res.trig * 100) }));
    if (res.stuck) tips.push(t("Botão pressionado durante toda a medição: {b}.", { b: BTNS.filter((b) => res.stuck & BIT[b]).join(", ") || t("direcional") }));
    if (res.samples > 0 && !tips.length) tips.push(t("Nenhum problema encontrado."));
  }
  const rows: [string, string][] = [
    [tk("Dispositivo vinculado"), reg.deviceKey ?? t("Nenhum (cadastro manual)")],
    [tk("Leitura pelo NEXSTAGE"), SOURCE_LABEL(reg.deviceKey) ?? "—"],
    [tk("Estado"), !reg.deviceKey ? "—" : device ? t("Conectado") : t("Desconectado")],
    [tk("Conexão"), device?.connection ? t(CONN_LABEL[device.connection] ?? device.connection) : "—"],
    [tk("Bateria"), !device ? "—" : device.wired ? t("Com cabo (sem leitura de bateria)") : device.batteryPercent !== null ? `${device.batteryPercent}%${device.charging ? ` · ${t("carregando")}` : ""}` : device.batteryLabel ? t(device.batteryLabel) : t("Não informada pelo controle")],
    [tk("Nome informado pelo sistema"), device ? t(device.name) : "—"],
    [tk("Fabricante e produto (VID:PID)"), device?.vid != null && device.pid != null
      ? `${device.vid.toString(16).padStart(4, "0").toUpperCase()}:${device.pid.toString(16).padStart(4, "0").toUpperCase()}${device.brand ? ` · ${device.brand}` : ""}` : "—"],
    [tk("Identificação única"), device?.idSource ? t(ID_SOURCE_LABEL[device.idSource] ?? device.idSource) : "—"],
    ...(device?.virtualSource ? [[tk("Dispositivo virtual"), t("Criado por {src}", { src: device.virtualSource })] as [string, string]] : []),
    [tk("Cadastrado em"), new Date(reg.createdAt).toLocaleDateString()],
  ];
  return (
    <div className="settings">
      <dl className="pad-diag">
        {rows.map(([k, v]) => <div key={k}><dt>{t(k)}</dt><dd>{v}</dd></div>)}
      </dl>
      {isJoyconPair(reg.deviceKey) && (
        <div className="setting">
          <div><strong>{t("Separar Joy-Cons")}</strong>
            <small>{t("Cada Joy-Con volta a ser um controle independente. Para juntar de novo, segure L no esquerdo e R no direito ao mesmo tempo.")}</small></div>
          <Button onClick={() => { splitJoycons(reg.deviceKey!); toast(t("Joy-Cons separados.")); }}>{t("Separar")}</Button>
        </div>
      )}
      <div className="setting">
        <div><strong>{t("Medição em repouso")}</strong>
          <small>{running ? t("Medindo… solte os analógicos, gatilhos e botões.") : t("Mede por 3 s a taxa de leitura, o desvio dos analógicos soltos e botões presos.")}</small></div>
        <Button variant="primary" disabled={!device || running} onClick={() => { setRes(null); setRunning(true); }}>{running ? t("Medindo…") : t("Executar diagnóstico")}</Button>
      </div>
      {res && (
        <div className="pad-diag-res">
          <dl className="pad-readout">
            <div><dt>{t("Taxa de leitura")}</dt><dd>{res.hz ? `${res.hz} Hz` : "—"}</dd></div>
            <div><dt>{t("Desvio em repouso (esq.)")}</dt><dd>{Math.round(res.drift[0] * 100)}%</dd></div>
            <div><dt>{t("Desvio em repouso (dir.)")}</dt><dd>{Math.round(res.drift[1] * 100)}%</dd></div>
            <div><dt>{t("Gatilhos em repouso")}</dt><dd>{Math.round(res.trig * 100)}%</dd></div>
          </dl>
          <ul className="pad-tips">{tips.map((x) => <li key={x}>{x}</li>)}</ul>
        </div>
      )}
      <div className="setting">
        <div><strong>{t("Restaurar ajustes padrão")}</strong><small>{t("Volta LED, vibração, zonas mortas, ícones e mapeamento ao padrão. Nome, imagem e vínculo são mantidos.")}</small></div>
        <Button variant="ghost" onClick={async () => { await resetControllerSettings(reg.id); toast(t("Ajustes do controle restaurados.")); }}>{t("Restaurar")}</Button>
      </div>
    </div>
  );
}
