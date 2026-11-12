/**
 * Último dispositivo de entrada usado. Define quais glifos aparecem (✕ ○ / A B / Enter Esc).
 * Teclado: qualquer tecla REAL. Mouse: clique, rolagem ou movimento relevante (>40 px em 400 ms,
 * para ignorar tremidas). Controle: somente ações reais (eventos `pad-input`).
 *
 * Eventos sintéticos (`isTrusted === false`) nunca contam como teclado/mouse: o "voltar" do controle é
 * entregue como um Escape artificial (ver lib/nav.ts) e não pode trocar os glifos e o layout para teclado.
 */
import { useEffect, useSyncExternalStore } from "react";

export type Device = "xbox" | "playstation" | "nintendo" | "generic" | "keyboard";

/** Controles Nintendo (Pro Controller, Joy-Con...) pelo id da Gamepad API (fabricante 057e). */
export const NINTENDO_PAD = /057e|nintendo|pro controller|joy-?con/i;

/** Rótulos dos botões de ombro na barra de páginas; `null` = teclado/mouse (indicadores ocultos). */
export function shoulderLabels(d: Device): [string, string] | null {
  if (d === "playstation") return ["L1", "R1"];
  if (d === "nintendo") return ["L", "R"];
  if (d === "xbox" || d === "generic") return ["LB", "RB"];
  return null;
}
export type LayoutOverride = "auto" | Device;

let detected: Device = "keyboard";
let override: LayoutOverride = "auto";
const subs = new Set<() => void>();
const notify = () => subs.forEach((f) => f());

// Atividade do controle: avisada a CADA comando (não só na troca de dispositivo), para o cursor
// voltar a se esconder mesmo quando o mouse foi mexido sem trocar o dispositivo detectado.
const padSubs = new Set<() => void>();
export function onPadActivity(fn: () => void) {
  padSubs.add(fn);
  return () => { padSubs.delete(fn); };
}

export function setDetected(d: Device) {
  if (d !== "keyboard") padSubs.forEach((f) => f());
  if (d === detected) return;
  detected = d;
  notify();
}
export function setLayoutOverride(o: LayoutOverride) {
  override = o;
  notify();
}

/**
 * Preferências individuais (Central de Controles) do controle em uso: estilo de ícones e mapeamento dos
 * comandos da interface (ação → botão físico, ex.: padrão japonês = confirmar no botão da direita).
 * `activePad` = último dispositivo que enviou um comando.
 */
export type PadPref = { glyphs: Device | null; map: Record<string, string> | null };
let prefs: Record<string, PadPref> = {};
let activePad: string | null = null;
export function setPadPrefs(p: Record<string, PadPref>) {
  prefs = p;
  notify();
}
export function setActivePad(key: string | null) {
  if (key === activePad) return;
  activePad = key;
  notify();
}
export const activePadKey = () => activePad;
const activePref = (): PadPref | null => (detected !== "keyboard" && activePad ? prefs[activePad] ?? null : null);

export const detectedDevice = (): Device => detected;
export const currentDevice = (): Device => (override === "auto" ? activePref()?.glyphs ?? detected : override);
let snap: { device: Device; map: Record<string, string> | null } = { device: currentDevice(), map: null };
/** Dispositivo efetivo + mapeamento do controle em uso (objeto estável enquanto nada muda). */
export const useGlyphState = (): { device: Device; map: Record<string, string> | null } =>
  useSyncExternalStore((cb) => { subs.add(cb); return () => subs.delete(cb); }, () => {
    const d = currentDevice(), m = d === "keyboard" ? null : activePref()?.map ?? null;
    if (snap.device !== d || snap.map !== m) snap = { device: d, map: m };
    return snap;
  });
export const useInputDevice = (): Device =>
  useSyncExternalStore((cb) => { subs.add(cb); return () => subs.delete(cb); }, currentDevice);

export function useInputDetection() {
  useEffect(() => {
    let dist = 0, t0 = 0, lx = 0, ly = 0;
    const key = (e: KeyboardEvent) => { if (e.isTrusted) setDetected("keyboard"); };
    const mouse = (e: Event) => { if (e.isTrusted) setDetected("keyboard"); };
    const move = (e: MouseEvent) => {
      if (!e.isTrusted) return;
      const now = performance.now();
      if (now - t0 > 400) { dist = 0; t0 = now; }
      dist += Math.abs(e.clientX - lx) + Math.abs(e.clientY - ly);
      lx = e.clientX; ly = e.clientY;
      if (dist > 40) { setDetected("keyboard"); dist = 0; }
    };
    window.addEventListener("keydown", key, true);
    window.addEventListener("mousedown", mouse, true);
    window.addEventListener("wheel", mouse, true);
    window.addEventListener("mousemove", move, true);
    return () => {
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("mousedown", mouse, true);
      window.removeEventListener("wheel", mouse, true);
      window.removeEventListener("mousemove", move, true);
    };
  }, []);
}
