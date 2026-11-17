/**
 * Central de Controles: cadastro dos controles do usuário e ajustes individuais de cada um.
 *
 * REGRA DE ISOLAMENTO: tudo aqui só muda como o NEXSTAGE interpreta e apresenta o controle (navegação,
 * glifos, vibração de resposta e LED que o próprio app já controlava). Nada é gravado no controle, no
 * driver ou no Windows: jogos e emuladores recebem o controle exatamente como o sistema o entrega.
 * Excluir um cadastro apaga só estes dados; o dispositivo continua conectado e reconhecido.
 *
 * Persistência: `app_settings.controller_registry` (JSON). Os controles físicos são do computador, não
 * de um perfil, então o cadastro é global.
 */
import { useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { NINTENDO_PAD, setPadPrefs } from "./inputDevice";
import { tk } from "./i18n";
import type { Controller } from "../types";
import type { Device, PadPref } from "./inputDevice";

/** Representações visuais disponíveis (ver components/ControllerArt.tsx). */
export type Visual =
  | "xbox-series" | "xbox-360" | "ps1" | "ps2" | "ps3" | "ps4" | "ps5" | "joycon" | "nes" | "snes"
  | "n64" | "switch-pro" | "wii" | "megadrive" | "segacd" | "generic";

export interface VisualPreset { id: Visual; model: string; brand: string; platform: string; glyphs: Device; label: string }

/** Modelo, fabricante e console sugeridos para cada representação (o usuário pode editar tudo). */
export const VISUALS: VisualPreset[] = [
  { id: "xbox-series", label: "Xbox Series", model: "Xbox Series Controller", brand: "Microsoft", platform: "Xbox Series X|S", glyphs: "xbox" },
  { id: "xbox-360", label: "Xbox 360", model: "Xbox 360 Controller", brand: "Microsoft", platform: "Xbox 360", glyphs: "xbox" },
  { id: "ps1", label: "PlayStation 1", model: "PlayStation Controller", brand: "Sony", platform: "PlayStation", glyphs: "playstation" },
  { id: "ps2", label: "PlayStation 2", model: "DualShock 2", brand: "Sony", platform: "PlayStation 2", glyphs: "playstation" },
  { id: "ps3", label: "PlayStation 3", model: "DualShock 3", brand: "Sony", platform: "PlayStation 3", glyphs: "playstation" },
  { id: "ps4", label: "PlayStation 4", model: "DualShock 4", brand: "Sony", platform: "PlayStation 4", glyphs: "playstation" },
  { id: "ps5", label: "PlayStation 5", model: "DualSense", brand: "Sony", platform: "PlayStation 5", glyphs: "playstation" },
  { id: "joycon", label: "Joy-Con", model: "Joy-Con", brand: "Nintendo", platform: "Nintendo Switch", glyphs: "nintendo" },
  { id: "switch-pro", label: "Pro Controller", model: "Nintendo Switch Pro Controller", brand: "Nintendo", platform: "Nintendo Switch", glyphs: "nintendo" },
  { id: "nes", label: "NES", model: "NES Controller", brand: "Nintendo", platform: "Nintendo Entertainment System", glyphs: "nintendo" },
  { id: "snes", label: "Super Nintendo", model: "Super Nintendo Controller", brand: "Nintendo", platform: "Super Nintendo", glyphs: "nintendo" },
  { id: "n64", label: "Nintendo 64", model: "Nintendo 64 Controller", brand: "Nintendo", platform: "Nintendo 64", glyphs: "nintendo" },
  { id: "wii", label: "Nintendo Wii", model: "Wii Remote", brand: "Nintendo", platform: "Nintendo Wii", glyphs: "nintendo" },
  { id: "megadrive", label: "Mega Drive", model: "Mega Drive Control Pad", brand: "Sega", platform: "Mega Drive", glyphs: "generic" },
  { id: "segacd", label: "Sega CD", model: "Sega CD Control Pad", brand: "Sega", platform: "Sega CD", glyphs: "generic" },
  { id: "generic", label: "Genérico", model: "Controle genérico", brand: "", platform: "PC", glyphs: "generic" },
];
export const visualOf = (id: Visual) => VISUALS.find((v) => v.id === id) ?? VISUALS[VISUALS.length - 1];

/** Botões físicos (mesma máscara do backend, `input::btn`). O índice do bit = índice da Gamepad API. */
export type Btn = "A" | "B" | "X" | "Y" | "LB" | "RB" | "LT" | "RT" | "SELECT" | "START" | "L3" | "R3";
export const BTNS: Btn[] = ["A", "B", "X", "Y", "LB", "RB", "LT", "RT", "SELECT", "START", "L3", "R3"];
export const BIT: Record<Btn, number> = Object.fromEntries(BTNS.map((b, i) => [b, 1 << i])) as Record<Btn, number>;
export const DPAD_BITS = { up: 1 << 12, down: 1 << 13, left: 1 << 14, right: 1 << 15 };

/** Ações da interface (mesma ordem de `input::ACTIONS`). */
export type Action = "confirm" | "back" | "x" | "y" | "l1" | "r1" | "l2" | "r2" | "menu";
export const ACTIONS: Action[] = ["confirm", "back", "x", "y", "l1", "r1", "l2", "r2", "menu"];
export const DEFAULT_MAP: Record<Action, Btn> = { confirm: "A", back: "B", x: "X", y: "Y", l1: "LB", r1: "RB", l2: "LT", r2: "RT", menu: "START" };

/** Nome de cada botão no estilo de ícones do controle (posição física: A = baixo, B = direita...). */
export const BTN_LABEL: Record<Exclude<Device, "keyboard">, Record<Btn, string>> = {
  xbox: { A: "A", B: "B", X: "X", Y: "Y", LB: "LB", RB: "RB", LT: "LT", RT: "RT", SELECT: "View", START: "Menu", L3: "LS", R3: "RS" },
  playstation: { A: "✕", B: "○", X: "□", Y: "△", LB: "L1", RB: "R1", LT: "L2", RT: "R2", SELECT: "Create", START: "Options", L3: "L3", R3: "R3" },
  nintendo: { A: "B", B: "A", X: "Y", Y: "X", LB: "L", RB: "R", LT: "ZL", RT: "ZR", SELECT: "−", START: "+", L3: "LS", R3: "RS" },
  generic: { A: "1", B: "2", X: "3", Y: "4", LB: "L1", RB: "R1", LT: "L2", RT: "R2", SELECT: "Select", START: "Start", L3: "L3", R3: "R3" },
};

export type LedMode = "theme" | "custom" | "dark" | "system";
export type GlyphStyle = "auto" | Exclude<Device, "keyboard">;

export interface PadReg {
  id: string;
  name: string;
  model: string;
  brand: string;
  platform: string;
  visual: Visual;
  /** Cor de destaque do cartão (opcional). */
  accent: string | null;
  /**
   * Dispositivo vinculado ou null (cadastro manual): "xbox:<id físico>" (XInput), "ps:<mac>" (Sony por
   * HID), "ns:<mac>" (Nintendo por HID) ou "gp:<id da Gamepad API>[#n]" (genéricos; `#n` = 2º, 3º... igual).
   * Versões anteriores usavam "ps" para qualquer controle Sony e "xbox:<slot>" para o slot do XInput
   * (migrados em `adoptLegacyKeys`).
   */
  deviceKey: string | null;
  createdAt: string;
  led: { mode: LedMode; color: string };
  vibration: { enabled: boolean; strength: number; onNav: boolean };
  deadzone: { left: number; right: number };
  layout: { glyphs: GlyphStyle; map: Record<Action, Btn> };
}

export const DEFAULTS = {
  led: { mode: "theme" as LedMode, color: "#2f6bff" },
  vibration: { enabled: true, strength: 60, onNav: false },
  deadzone: { left: 0.34, right: 0.24 },
};

/** Padrão de confirmação derivado do mapeamento: americano (baixo confirma) ou japonês (direita confirma). */
export type ConfirmPattern = "us" | "jp" | "custom";
export function confirmPattern(map: Record<Action, Btn>): ConfirmPattern {
  if (map.confirm === "A" && map.back === "B") return "us";
  if (map.confirm === "B" && map.back === "A") return "jp";
  return "custom";
}
export function withPattern(map: Record<Action, Btn>, p: "us" | "jp"): Record<Action, Btn> {
  const [c, b]: [Btn, Btn] = p === "us" ? ["A", "B"] : ["B", "A"];
  return assign(assign(map, "confirm", c), "back", b);
}
/** Atribui `btn` à ação; se outra ação já usava esse botão, as duas trocam (1 botão por ação, sempre). */
export function assign(map: Record<Action, Btn>, action: Action, btn: Btn): Record<Action, Btn> {
  const next = { ...map };
  const other = ACTIONS.find((a) => a !== action && next[a] === btn);
  if (other) next[other] = next[action];
  next[action] = btn;
  return next;
}

/** Estilo de ícones efetivo do controle ("auto" = família da representação visual). */
export const glyphsOf = (r: PadReg): Exclude<Device, "keyboard"> =>
  r.layout.glyphs === "auto" ? (visualOf(r.visual).glyphs as Exclude<Device, "keyboard">) : r.layout.glyphs;

// ---------- armazenamento ----------

const KEY = "controller_registry";
let regs: PadReg[] = [];
let loaded = false;
const subs = new Set<() => void>();
const emit = () => { regs = [...regs]; subs.forEach((f) => f()); };

const newId = () => `pad-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;

/** Completa campos ausentes (cadastros de versões anteriores) e corrige valores inválidos. */
function normalize(r: Partial<PadReg>): PadReg | null {
  if (!r || typeof r.id !== "string") return null;
  const map = { ...DEFAULT_MAP, ...(r.layout?.map ?? {}) } as Record<Action, Btn>;
  const used = new Set<string>();
  const valid = ACTIONS.every((a) => BTNS.includes(map[a]) && !used.has(map[a]) && used.add(map[a]));
  const vis = VISUALS.some((v) => v.id === r.visual) ? (r.visual as Visual) : "generic";
  const clamp = (v: unknown, lo: number, hi: number, d: number) => (typeof v === "number" && isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d);
  return {
    id: r.id,
    name: (r.name ?? "").slice(0, 40) || visualOf(vis).model,
    model: (r.model ?? "").slice(0, 60),
    brand: (r.brand ?? "").slice(0, 40),
    platform: (r.platform ?? "").slice(0, 60),
    visual: vis,
    accent: typeof r.accent === "string" && /^#[0-9a-f]{6}$/i.test(r.accent) ? r.accent : null,
    deviceKey: typeof r.deviceKey === "string" && r.deviceKey ? r.deviceKey : null,
    createdAt: r.createdAt ?? new Date().toISOString(),
    led: {
      mode: (["theme", "custom", "dark", "system"] as LedMode[]).includes(r.led?.mode as LedMode) ? (r.led!.mode as LedMode) : DEFAULTS.led.mode,
      color: /^#[0-9a-f]{6}$/i.test(r.led?.color ?? "") ? r.led!.color : DEFAULTS.led.color,
    },
    vibration: {
      enabled: r.vibration?.enabled ?? DEFAULTS.vibration.enabled,
      strength: clamp(r.vibration?.strength, 0, 100, DEFAULTS.vibration.strength),
      onNav: r.vibration?.onNav ?? DEFAULTS.vibration.onNav,
    },
    deadzone: { left: clamp(r.deadzone?.left, 0.05, 0.8, DEFAULTS.deadzone.left), right: clamp(r.deadzone?.right, 0.05, 0.8, DEFAULTS.deadzone.right) },
    layout: {
      glyphs: (["auto", "xbox", "playstation", "nintendo", "generic"] as GlyphStyle[]).includes(r.layout?.glyphs as GlyphStyle) ? (r.layout!.glyphs as GlyphStyle) : "auto",
      map: valid ? map : { ...DEFAULT_MAP },
    },
  };
}

/**
 * Nunca grava antes de o cadastro ter sido lido: com a lista ainda vazia na memória (ex.: módulo
 * recarregado no modo de desenvolvimento), gravar apagaria a coleção salva.
 */
async function persist() {
  clearTimeout(saveTimer);
  if (!loaded) return;
  await api.setAppSetting(KEY, JSON.stringify(regs));
}
/** Ajustes contínuos (barras deslizantes): grava logo depois que o valor para de mudar. */
let saveTimer: ReturnType<typeof setTimeout> | undefined;
function persistSoon() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => { if (loaded) api.setAppSetting(KEY, JSON.stringify(regs)).catch(() => {}); }, 250);
}

/** Carrega o cadastro e aplica os ajustes (chamado na abertura do app). */
let loading: Promise<void> | null = null;
export function loadControllers(): Promise<void> {
  loading ??= readControllers().finally(() => { loading = null; });
  return loading;
}
async function readControllers() {
  try {
    const raw = await api.getAppSetting(KEY);
    const list = raw ? (JSON.parse(raw) as Partial<PadReg>[]) : [];
    regs = (Array.isArray(list) ? list : []).map(normalize).filter((r): r is PadReg => !!r);
  } catch {
    // leitura falhou: continua "não carregado" (nada é gravado por cima) e tenta de novo no próximo uso
    regs = [];
    emit();
    return;
  }
  loaded = true;
  emit();
  applyAll();
  watchNativeDevices();
  loadJoyconPairs();
}

// ---------- Joy-Cons juntos ----------
//
// O backend junta um Joy-Con (L) e um (R) num controle só quando o usuário segura L no esquerdo e R no
// direito ao mesmo tempo (evento `joycon-joined`). O par é gravado aqui (`app_settings.joycon_pairs`,
// global como o cadastro) e enviado ao backend, que volta a formá-lo sempre que os dois reconectam.

const PAIRS_KEY = "joycon_pairs";
let pairs: [string, string][] = [];

const pushPairs = () => api.setJoyconPairs(pairs).catch(() => {});
const savePairs = () => { pushPairs(); api.setAppSetting(PAIRS_KEY, JSON.stringify(pairs)).catch(() => {}); };

let pairsWatching = false;
async function loadJoyconPairs() {
  if (pairsWatching) return; // um único ouvinte de `joycon-joined` por execução
  pairsWatching = true;
  try {
    const raw = await api.getAppSetting(PAIRS_KEY);
    const list = raw ? JSON.parse(raw) : [];
    pairs = Array.isArray(list) ? list.filter((p): p is [string, string] => Array.isArray(p) && p.length === 2 && p.every((k) => typeof k === "string")) : [];
  } catch {
    pairs = [];
  }
  pushPairs();
  listen<{ key: string; left: string; right: string }>("joycon-joined", ({ payload: p }) => {
    // cada Joy-Con em no máximo um par
    pairs = [...pairs.filter(([l, r]) => l !== p.left && r !== p.right), [p.left, p.right]];
    savePairs();
  }).catch(() => {});
}

/** Chave de um par de Joy-Cons ("ns:<esquerdo>+<direito>"). */
export const isJoyconPair = (key: string | null | undefined) => !!key && /^ns:[^+]+\+[^+]+$/.test(key);

/** Separa um par de Joy-Cons: cada um volta a ser um controle independente. */
export function splitJoycons(pairKey: string) {
  const [l, r] = pairKey.slice(3).split("+").map((k) => `ns:${k}`);
  pairs = pairs.filter(([a, b]) => !(a === l && b === r));
  savePairs();
}

export const controllersLoaded = () => loaded;
export const listControllers = () => regs;
/** Quem usa o cadastro antes de ele ter sido lido (ex.: módulo recarregado) dispara a leitura. */
export const useControllers = (): PadReg[] =>
  useSyncExternalStore((cb) => { subs.add(cb); if (!loaded) loadControllers().catch(() => {}); return () => subs.delete(cb); }, () => regs);

/** Cadastro vinculado a um dispositivo (no máximo um por dispositivo). */
export const regForDevice = (key: string | null | undefined) => (key ? regs.find((r) => r.deviceKey === key) ?? null : null);

export interface NewPad { name: string; model: string; brand: string; platform: string; visual: Visual; deviceKey: string | null; accent?: string | null }

/**
 * Cadastra um controle. Idempotente por dispositivo: se o dispositivo já pertence à coleção (ex.: o
 * mesmo comando "confirmar" chegou duas vezes, ou dois cliques rápidos), devolve o cadastro existente
 * em vez de criar outro. `created` diz se um cadastro novo foi criado.
 */
export async function addController(p: NewPad): Promise<PadReg & { created?: boolean }> {
  const existing = regForDevice(p.deviceKey);
  if (existing) return { ...existing, created: false };
  if (p.deviceKey && pendingAdds.has(p.deviceKey)) return pendingAdds.get(p.deviceKey)!;
  const job = createController(p);
  if (p.deviceKey) {
    pendingAdds.set(p.deviceKey, job);
    job.finally(() => pendingAdds.delete(p.deviceKey!)).catch(() => {});
  }
  return job;
}
const pendingAdds = new Map<string, Promise<PadReg & { created?: boolean }>>();

async function createController(p: NewPad): Promise<PadReg & { created?: boolean }> {
  const visual = visualOf(p.visual);
  const r = normalize({
    ...p,
    id: newId(),
    createdAt: new Date().toISOString(),
    layout: { glyphs: "auto", map: { ...DEFAULT_MAP } },
    led: { ...DEFAULTS.led },
    vibration: { ...DEFAULTS.vibration, enabled: visual.id !== "wii" && visual.id !== "nes" && visual.id !== "snes" && visual.id !== "megadrive" && visual.id !== "segacd" && visual.id !== "ps1" },
    deadzone: { ...DEFAULTS.deadzone },
  })!;
  regs = [...regs, r];
  emit();
  applyAll();
  await persist();
  return { ...r, created: true };
}

export async function updateController(id: string, patch: Partial<PadReg> | ((r: PadReg) => Partial<PadReg>)) {
  regs = regs.map((r) => {
    if (r.id !== id) return r;
    const p = typeof patch === "function" ? patch(r) : patch;
    return normalize({ ...r, ...p }) ?? r;
  });
  const me = regs.find((r) => r.id === id);
  if (me?.deviceKey) regs = regs.map((x) => (x.id !== id && x.deviceKey === me.deviceKey ? { ...x, deviceKey: null } : x));
  emit();
  applyAll();
  persistSoon();
}

/** Remove SÓ o cadastro interno: o controle continua conectado, reconhecido e funcionando (com os padrões). */
export async function removeController(id: string) {
  regs = regs.filter((r) => r.id !== id);
  emit();
  applyAll();
  await persist();
}

/** Volta os ajustes de um controle ao padrão, mantendo nome, modelo, imagem e vínculo. */
export async function resetControllerSettings(id: string) {
  await updateController(id, { led: { ...DEFAULTS.led }, vibration: { ...DEFAULTS.vibration }, deadzone: { ...DEFAULTS.deadzone }, layout: { glyphs: "auto", map: { ...DEFAULT_MAP } } });
}

// ---------- aplicação dos ajustes (somente dentro do NEXSTAGE) ----------

/** Ajustes no formato do backend / da leitura da Gamepad API. */
export interface PadCfg { dzLeft: number; dzRight: number; map: number[]; rumble: number; rumbleNav: boolean; led: number }

/** Barra de luz no formato do backend (`input::LED_*`). */
const LED_GLOBAL = 0, LED_KEEP = 1 << 25, LED_THEME = 1 << 26;
function ledCode(led: PadReg["led"]): number {
  if (led.mode === "theme") return LED_THEME;
  if (led.mode === "system") return LED_KEEP;
  const hex = led.mode === "dark" ? "#000000" : led.color;
  return (1 << 24) + parseInt(hex.slice(1), 16);
}
export const DEFAULT_CFG: PadCfg = { dzLeft: 0.34, dzRight: 0.24, map: ACTIONS.map((a) => BIT[DEFAULT_MAP[a]]), rumble: 0, rumbleNav: false, led: LED_GLOBAL };

export function cfgOf(r: PadReg): PadCfg {
  return {
    dzLeft: r.deadzone.left,
    dzRight: r.deadzone.right,
    map: ACTIONS.map((a) => BIT[r.layout.map[a]]),
    rumble: r.vibration.enabled ? Math.round(r.vibration.strength) : 0,
    rumbleNav: r.vibration.enabled && r.vibration.onNav,
    led: ledCode(r.led),
  };
}

/** Ajustes de um dispositivo (controles genéricos lidos pela Gamepad API, em lib/nav.ts). */
export function cfgForDevice(key: string): PadCfg {
  const r = regForDevice(key);
  return r ? cfgOf(r) : DEFAULT_CFG;
}

function applyAll() {
  // 1. controles nativos: zona morta, mapeamento, vibração de resposta e barra de luz (cada Sony a sua)
  const configs: Record<string, PadCfg> = {};
  for (const r of regs) if (r.deviceKey && !r.deviceKey.startsWith("gp:")) configs[r.deviceKey] = cfgOf(r);
  api.setPadConfigs(configs).catch(() => {});
  // nomes personalizados: os avisos de "conectado"/"bateria baixa" usam o nome do cadastro
  const names: Record<string, string> = {};
  for (const r of regs) if (r.deviceKey) names[r.deviceKey] = r.name;
  api.setPadNames(names).catch(() => {});
  // 2. glifos: estilo de ícones e padrão de confirmação de cada controle quando ele está em uso
  const prefs: Record<string, PadPref> = {};
  for (const r of regs) {
    if (!r.deviceKey) continue;
    const custom = ACTIONS.some((a) => r.layout.map[a] !== DEFAULT_MAP[a]);
    prefs[r.deviceKey] = { glyphs: r.layout.glyphs === "auto" ? null : r.layout.glyphs, map: custom ? { ...r.layout.map } : null };
  }
  setPadPrefs(prefs);
}

// ---------- vibração de teste ----------

/** Vibra o controle vinculado (força 0–100). Controles genéricos usam a vibração da Gamepad API. */
export async function testRumble(key: string, strength: number, ms = 450): Promise<boolean> {
  if (key.startsWith("gp:")) {
    const pad = gamepadByKey(key) as (Gamepad & { vibrationActuator?: { playEffect?: (t: string, p: object) => Promise<unknown> } }) | null;
    const act = pad?.vibrationActuator;
    if (!act?.playEffect) return false;
    await act.playEffect("dual-rumble", { duration: ms, strongMagnitude: strength / 100, weakMagnitude: strength / 200 }).catch(() => {});
    return true;
  }
  if (key.startsWith("ns:")) return false; // Nintendo: o NEXSTAGE não aciona a vibração (HD Rumble)
  await api.padRumble(key, Math.round(strength), ms);
  return true;
}

// ---------- dispositivos detectados ----------

/**
 * Controles que o backend lê nativamente (XInput / HID): a Gamepad API ignora esses para não duplicar a
 * entrada. Os Nintendo só são ignorados enquanto o backend estiver de fato lendo algum (se ele não
 * conseguir abrir o controle, a Gamepad API continua valendo como reserva).
 */
const NATIVE = /xinput|xbox|045e|054c|dualsense|dualshock|sony|playstation/i;

let nativeKeys: string[] = [];
const nativeSubs = new Set<() => void>();
export const nativeNintendo = () => nativeKeys.some((k) => k.startsWith("ns:"));
export const onNativeDevices = (fn: () => void) => { nativeSubs.add(fn); return () => { nativeSubs.delete(fn); }; };
function setNativeKeys(keys: string[]) {
  // Só avisa quando a lista MUDA: quem escuta costuma reconsultar os controles (getControllers), e essa
  // consulta passa por aqui de novo — avisar sempre criava um laço sem fim que travava o app.
  const same = keys.length === nativeKeys.length && keys.every((k) => nativeKeys.includes(k));
  nativeKeys = keys;
  adoptLegacyKeys(keys);
  if (same) return;
  nativeSubs.forEach((f) => f());
}
let watching = false;
/** Acompanha os controles nativos conectados (evento `pad-devices` do backend + consulta inicial). */
function watchNativeDevices() {
  if (watching) return;
  watching = true;
  const full = () => api.getControllers().then(noteNativeControllers).catch(() => {});
  // a lista completa traz a chave antiga de cada Xbox: migra cadastros mesmo fora da Central de Controles
  listen<string[]>("pad-devices", (e) => { setNativeKeys(e.payload); full(); }).catch(() => {});
  full();
}
/** Lista nova de controles nativos (ex.: consulta da Central de Controles). */
export const noteNativeControllers = (list: Controller[]) => {
  adoptLegacyXbox(list);
  setNativeKeys(list.map((c) => c.id));
};

/**
 * Cadastros de versões anteriores vinculados ao SLOT do XInput ("xbox:0"…"xbox:3") passam para a
 * identidade física do controle que está nesse slot agora ("xbox:<mac/serial>"), uma única vez.
 */
function adoptLegacyXbox(list: Controller[]) {
  let changed = false;
  for (const c of list) {
    if (!c.legacyKey || c.legacyKey === c.id || c.virtualSource) continue;
    const legacy = regs.find((r) => r.deviceKey === c.legacyKey);
    if (!legacy || regs.some((r) => r.deviceKey === c.id)) continue;
    regs = regs.map((r) => (r.id === legacy.id ? { ...r, deviceKey: c.id } : r));
    changed = true;
  }
  if (!changed) return;
  emit();
  applyAll();
  persist().catch(() => {});
}

/** Gamepad lido pela Gamepad API (não duplicado com a leitura nativa)? */
export const isGenericPad = (g: Gamepad) => !NATIVE.test(g.id) && !(NINTENDO_PAD.test(g.id) && nativeNintendo());

/**
 * Chave de cada controle genérico conectado: `gp:<id>` para o primeiro de cada modelo e `gp:<id>#2`,
 * `#3`... para os seguintes (dois controles iguais têm o mesmo id na Gamepad API; a ordem segue o
 * índice de conexão). Assim cada um tem o próprio cadastro e não aparecem "os dois conectados".
 */
export function gamepadKeys(pads: readonly (Gamepad | null)[] = navigator.getGamepads?.() ?? []): Map<Gamepad, string> {
  const seen = new Map<string, number>();
  const out = new Map<Gamepad, string>();
  for (const g of [...pads].filter((p): p is Gamepad => !!p && isGenericPad(p)).sort((a, b) => a.index - b.index)) {
    const n = (seen.get(g.id) ?? 0) + 1;
    seen.set(g.id, n);
    out.set(g, n === 1 ? `gp:${g.id}` : `gp:${g.id}#${n}`);
  }
  return out;
}
export function gamepadByKey(key: string): Gamepad | null {
  for (const [g, k] of gamepadKeys()) if (k === key) return g;
  return null;
}

export function gamepadControllers(): Controller[] {
  return [...gamepadKeys()].map(([g, key]) => {
    const s = g.id.toLowerCase();
    let name = g.id.split("(")[0].trim() || "Controle genérico";
    let kind = "generic";
    if (NINTENDO_PAD.test(s)) { kind = "nintendo"; name = /joy-?con/.test(s) ? "Joy-Con" : "Pro Controller"; }
    return { id: key, name, kind, batteryPercent: null, batteryLabel: null, charging: false, wired: false, connection: null };
  });
}

/**
 * Cadastros de versões anteriores vinculados a "ps" (qualquer Sony): passam para o primeiro controle
 * Sony conectado que ainda não tem cadastro. Com vários Sony, o usuário vincula os outros em "Editar dados".
 */
function adoptLegacyKeys(keys: string[]) {
  const legacy = regs.find((r) => r.deviceKey === "ps");
  if (!legacy) return;
  const free = keys.find((k) => k.startsWith("ps:") && !regs.some((r) => r.deviceKey === k));
  if (!free) return;
  regs = regs.map((r) => (r.id === legacy.id ? { ...r, deviceKey: free } : r));
  emit();
  applyAll();
  persist().catch(() => {});
}

/** Nome exibido de cada dispositivo, numerando os repetidos ("DualSense", "DualSense (2)"...). */
export function deviceLabels(list: Controller[], tr: (s: string) => string): Map<string, string> {
  const total = new Map<string, number>();
  for (const d of list) total.set(d.name, (total.get(d.name) ?? 0) + 1);
  const seen = new Map<string, number>();
  const out = new Map<string, string>();
  for (const d of list) {
    const n = (seen.get(d.name) ?? 0) + 1;
    seen.set(d.name, n);
    out.set(d.id, (total.get(d.name) ?? 0) > 1 ? `${tr(d.name)} (${n})` : tr(d.name));
  }
  return out;
}

/** Dispositivo criado por software (DS4Windows/ViGEm, G HUB, Steam...) para espelhar outro controle. */
export const isVirtualDevice = (c: Controller) => !!c.virtualSource;

/**
 * Representação sugerida para um dispositivo detectado — pelo MODELO que o backend identificou pelo
 * VID/PID, nunca pelo nome (DualSense e DualShock 4 podem se apresentar como "Wireless Controller").
 * Sony sem modelo seguro: genérico, para o usuário escolher.
 */
export function guessVisual(c: Controller): Visual {
  switch (c.model) {
    case "dualsense": case "dualsense-edge": return "ps5";
    case "dualshock4": return "ps4";
    case "xbox-360": return "xbox-360";
    case "xbox-series": case "xbox-one": case "xbox-elite": case "xbox-adaptive": case "xbox": case "xinput": return "xbox-series";
    case "switch-pro": return "switch-pro";
    case "joycon-l": case "joycon-r": case "joycon-pair": return "joycon";
    case "sony-unknown": case "virtual": return "generic";
  }
  // Gamepad API (genéricos): só a família é conhecida
  if (c.kind === "xbox") return "xbox-series";
  if (c.kind === "nintendo") return /joy-?con/i.test(c.name) ? "joycon" : "switch-pro";
  return "generic";
}

/** Nome comercial do modelo identificado (campo "Modelo" do cadastro). */
const MODEL_NAME: Record<string, string> = {
  "dualsense": "DualSense", "dualsense-edge": "DualSense Edge", "dualshock4": "DualShock 4",
  "xbox-series": "Xbox Series Controller", "xbox-one": "Xbox One Controller", "xbox-elite": "Xbox Elite Controller",
  "xbox-adaptive": "Xbox Adaptive Controller", "xbox-360": "Xbox 360 Controller",
  "switch-pro": "Nintendo Switch Pro Controller", "joycon-l": "Joy-Con (L)", "joycon-r": "Joy-Con (R)",
  "joycon-pair": "Joy-Con (L+R)",
};
const PLATFORM: Record<string, string> = { "xbox-one": "Xbox One", "xbox-elite": "Xbox One", "xbox-adaptive": "Xbox One" };

/** Dados sugeridos para cadastrar um dispositivo detectado (fabricante, modelo e plataforma). */
export function presetFor(c: Controller): { visual: Visual; model: string; brand: string; platform: string } {
  const v = visualOf(guessVisual(c));
  const known = c.model ? MODEL_NAME[c.model] : undefined;
  if (c.model === "sony-unknown") return { visual: v.id, model: "", brand: "Sony", platform: "" };
  if (c.model === "xinput") return { visual: v.id, model: "", brand: c.brand ?? "", platform: "PC" };
  if (c.model === "virtual") return { visual: v.id, model: "", brand: "", platform: "PC" };
  return { visual: v.id, model: known ?? v.model, brand: c.brand ?? v.brand, platform: (c.model && PLATFORM[c.model]) ?? v.platform };
}

export const SOURCE_LABEL = (key: string | null) =>
  !key ? null : key.startsWith("xbox:") ? "XInput" : key.startsWith("ps") ? "HID (Sony)" : key.startsWith("ns:") ? "HID (Nintendo)" : "Gamepad API";

/** Como o controle foi reconhecido como único (diagnóstico). */
export const ID_SOURCE_LABEL: Record<string, string> = {
  mac: tk("Endereço Bluetooth do controle"),
  serial: tk("Número de série do controle"),
  container: tk("Identificador do aparelho no Windows"),
  path: tk("Caminho do dispositivo (válido enquanto conectado)"),
  slot: tk("Posição no XInput (aparelho não identificado)"),
  virtual: tk("Dispositivo virtual"),
};
