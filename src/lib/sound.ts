/**
 * Sons de navegação (arquivos em src/assets/sounds). Web Audio com buffers pré-decodificados:
 * resposta imediata, sem criar elementos <audio>. Limita a taxa por tipo para não sobrepor.
 */
import move from "../assets/sounds/move.wav";
import confirm from "../assets/sounds/confirm.wav";
import back from "../assets/sounds/back.wav";
import open from "../assets/sounds/open.wav";
import close from "../assets/sounds/close.wav";

export type Sound = "move" | "confirm" | "back" | "open" | "close";
const URLS: Record<Sound, string> = { move, confirm, back, open, close };
const MIN_GAP: Record<Sound, number> = { move: 45, confirm: 80, back: 80, open: 80, close: 80 };
const GAIN: Record<Sound, number> = { move: 0.5, confirm: 0.8, back: 0.8, open: 0.7, close: 0.7 };

let ctx: AudioContext | null = null;
let loading: Promise<void> | null = null;
const buffers: Partial<Record<Sound, AudioBuffer>> = {};
const last: Partial<Record<Sound, number>> = {};
const ACTION_GAP = 90;
let lastAny: { name: Sound; at: number } | null = null;
let enabled = true;
let volume = 0.5;

export function setSoundConfig(on: boolean, vol: number) {
  enabled = on;
  volume = Math.max(0, Math.min(1, vol / 100));
}

/** Volume efetivo (0..1) dos sons do app; 0 quando os sons estão desligados. */
export const soundLevel = () => (enabled ? volume : 0);

export function preloadSounds(): Promise<void> {
  if (loading) return loading;
  loading = (async () => {
    ctx = new AudioContext();
    await Promise.all(
      (Object.keys(URLS) as Sound[]).map(async (k) => {
        try {
          const res = await fetch(URLS[k]);
          buffers[k] = await ctx!.decodeAudioData(await res.arrayBuffer());
        } catch { /* som indisponível: segue sem ele */ }
      })
    );
  })();
  return loading;
}

export function play(name: Sound) {
  if (!enabled || volume <= 0) return;
  const now = performance.now();
  if (now - (last[name] ?? 0) < MIN_GAP[name]) return;
  // Um som por ação: uma ação costuma disparar dois sons diferentes quase juntos (clique "confirm" +
  // modal montando "open"; "back" no Esc + "close" ao desmontar; e, em desenvolvimento, o StrictMode
  // monta duas vezes: open/close/open). Vale o primeiro; os seguintes dentro da janela são ignorados.
  if (lastAny && lastAny.name !== name && now - lastAny.at < ACTION_GAP) return;
  last[name] = now;
  lastAny = { name, at: now };
  if (!ctx) { void preloadSounds(); return; }
  if (ctx.state === "suspended") void ctx.resume();
  const b = buffers[name];
  if (!b) return;
  const src = ctx.createBufferSource();
  const g = ctx.createGain();
  g.gain.value = volume * GAIN[name];
  src.buffer = b;
  src.connect(g);
  g.connect(ctx.destination);
  src.start();
}
