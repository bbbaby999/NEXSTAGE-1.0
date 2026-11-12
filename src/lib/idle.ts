/**
 * Modo Ocioso (protetor de tela no estilo dos consoles): estado compartilhado, sem dependências de UI.
 *
 * - Preferências (app): `idle_mode` = "library" | "captures" | "off" e `idle_minutes` = 0 (desligado) | 2 | 10 | …
 * - "Ocupado": operações importantes (importação, varredura, sincronização, instalação de atualização)
 *   seguram o Modo Ocioso enquanto rodam (`idleBusy`, `setIdleBusyFlag`, `bumpIdleBusy`).
 * - Entrada: quando a apresentação está ativa, o primeiro comando só a encerra; `idleSwallow()` diz à
 *   navegação (nav.ts) para descartar esse comando — ele nunca chega à tela de trás nem ao jogo.
 */
import { useSyncExternalStore } from "react";

export type IdleMode = "library" | "captures" | "off";
export const IDLE_MINUTES = [0, 2, 10, 20, 30, 45, 60] as const;
export interface IdleConfig { mode: IdleMode; minutes: number }

/** Padrão: conteúdo "Biblioteca", mas tempo desligado (nada muda para quem não ativar a opção). */
let config: IdleConfig = { mode: "library", minutes: 0 };
let active = false;
/** Comandos recebidos até este instante são descartados (o que encerrou a apresentação + repetição). */
let swallowUntil = 0;
const SWALLOW_MS = 450;

const subs = new Set<() => void>();
const notify = () => subs.forEach((f) => f());
const subscribe = (cb: () => void) => { subs.add(cb); return () => { subs.delete(cb); }; };

export function parseIdleConfig(mode: string | null, minutes: string | null): IdleConfig {
  const m: IdleMode = mode === "captures" || mode === "off" ? mode : "library";
  const n = Number(minutes ?? 0);
  return { mode: m, minutes: (IDLE_MINUTES as readonly number[]).includes(n) ? n : 0 };
}
export const getIdleConfig = () => config;
export function setIdleConfig(c: IdleConfig) {
  if (c.mode === config.mode && c.minutes === config.minutes) return;
  config = c;
  notify();
}
export const useIdleConfig = () => useSyncExternalStore(subscribe, getIdleConfig);

export const isIdleActive = () => active;
export const useIdleActive = () => useSyncExternalStore(subscribe, isIdleActive);

/* ---------- Ativação / encerramento (o host em IdleScreen.tsx registra as ações) ---------- */
let host: { start: (preview: boolean) => void; stop: () => void } | null = null;
export function registerIdleHost(h: typeof host) { host = h; return () => { if (host === h) host = null; }; }

export function setIdleActive(on: boolean) {
  if (on === active) return;
  active = on;
  if (!on) swallowUntil = performance.now() + SWALLOW_MS;
  notify();
}
/** Mostra a apresentação agora (botão "Visualizar em tela cheia" das Configurações). */
export const startIdlePreview = () => host?.start(true);

/**
 * Chamado pela navegação antes de executar um comando de controle. Com a apresentação ativa, encerra-a
 * e devolve `true` (o comando é descartado); logo depois de encerrar, continua descartando por um
 * instante para o mesmo toque/repetição não navegar a tela que reaparece.
 */
export function idleSwallow(): boolean {
  if (active) { host?.stop(); return true; }
  return performance.now() < swallowUntil;
}

/* ---------- Operações importantes em andamento ---------- */
let busyCount = 0;
const busyFlags = new Set<string>();
let busyUntil = 0;

/** Segura o Modo Ocioso até a promessa terminar (e devolve o mesmo resultado). */
export function idleBusy<T>(p: Promise<T>): Promise<T> {
  busyCount++;
  const done = () => { busyCount = Math.max(0, busyCount - 1); bumpIdleBusy(); };
  p.then(done, done);
  return p;
}
/** Operação longa sinalizada por eventos (ex.: "running": true/false). */
export function setIdleBusyFlag(key: string, on: boolean) {
  if (on) busyFlags.add(key); else { busyFlags.delete(key); bumpIdleBusy(); }
}
/** Sinal de progresso: segura o Modo Ocioso por mais um minuto. */
export function bumpIdleBusy(ms = 60_000) { busyUntil = Math.max(busyUntil, Date.now() + ms); }
export const idleBusyNow = () => busyCount > 0 || busyFlags.size > 0 || Date.now() < busyUntil;
