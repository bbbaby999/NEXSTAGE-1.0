/**
 * Áudio original do Gameboot (≈3,5 s), sintetizado em tempo real (Web Audio, sem arquivos).
 * Sopro de ar → "florescer" grave quando a luz se abre → três sinos de vidro subindo (Ré maior com
 * nona) sobre um pad que abre o filtro → brilho quando a luz varre a capa → cauda longa no reverb.
 * Os tempos seguem `GB_T`, o mesmo roteiro da animação.
 */
import { soundLevel } from "./sound";

/** Roteiro em segundos (compartilhado com a animação). */
export const GB_T = {
  bloom: 0.55,   // a luz se abre / ondulação
  cover: 0.6,    // a capa surge
  title: 0.95,   // nome do jogo
  sweep: 1.7,    // reflexo de luz atravessa a capa
  out: 2.85,     // início da saída
  end: 3.45,     // fim: tela escura — só então o jogo é iniciado
};

const hz = (midi: number) => 440 * Math.pow(2, (midi - 69) / 12);
const D3 = 50, A3 = 57, Cs4 = 61, E4 = 64, Fs4 = 66, D2 = 38, A5 = 81, Cs6 = 85, E6 = 88, Fs6 = 90, A6 = 93;

export interface Score { stop: (fade?: number) => void }

export function playGamebootScore(): Score | null {
  const level = soundLevel();
  if (level <= 0) return null;
  let ctx: AudioContext;
  try { ctx = new AudioContext(); } catch { return null; }
  void ctx.resume();
  const t0 = ctx.currentTime + 0.04;

  const master = ctx.createGain();
  master.gain.value = Math.min(1, level * 1.05);
  const comp = ctx.createDynamicsCompressor();
  comp.threshold.value = -16; comp.ratio.value = 3; comp.attack.value = 0.01; comp.release.value = 0.3;
  master.connect(comp).connect(ctx.destination);
  const verb = ctx.createConvolver();
  verb.buffer = impulse(ctx, 2.8, 2.4);
  const wet = ctx.createGain(); wet.gain.value = 0.5;
  verb.connect(wet).connect(master);
  const out = (node: AudioNode, dry = 1, rev = 0.4) => {
    if (dry > 0) { const g = ctx.createGain(); g.gain.value = dry; node.connect(g).connect(master); }
    if (rev > 0) { const g = ctx.createGain(); g.gain.value = rev; node.connect(g).connect(verb); }
  };
  const noise = noiseBuffer(ctx, 2);

  // sopro de ar (entrada)
  {
    const s = ctx.createBufferSource(); s.buffer = noise;
    const bp = ctx.createBiquadFilter(); bp.type = "bandpass"; bp.Q.value = 0.8;
    env(bp.frequency, [[t0, 400], [t0 + GB_T.bloom, 2600], [t0 + 1.2, 900]], true);
    const g = ctx.createGain(); env(g.gain, [[t0, 0.0001], [t0 + GB_T.bloom - 0.05, 0.16], [t0 + 1.3, 0.0001]], true);
    s.connect(bp).connect(g); out(g, 1, 0.5); s.start(t0); s.stop(t0 + 1.35);
  }
  // florescer grave
  {
    const at = t0 + GB_T.bloom;
    const o = ctx.createOscillator(); o.type = "sine";
    env(o.frequency, [[at, hz(D2) * 1.5], [at + 0.5, hz(D2)]], true);
    const g = ctx.createGain(); env(g.gain, [[at, 0.0001], [at + 0.03, 0.42], [at + 1.6, 0.0001]], true);
    o.connect(g); out(g, 1, 0.15); o.start(at); o.stop(at + 1.7);
  }
  // pad (Dmaj9) abrindo o filtro
  {
    const a = t0 + 0.25, b = t0 + GB_T.end + 0.1;
    const lp = ctx.createBiquadFilter(); lp.type = "lowpass"; lp.Q.value = 1.8;
    env(lp.frequency, [[a, 420], [t0 + GB_T.sweep + 0.4, 2800], [b, 1200]], true);
    const bus = ctx.createGain(); env(bus.gain, [[a, 0.0001], [a + 0.7, 0.1], [t0 + GB_T.out, 0.09], [b, 0.0001]], true);
    lp.connect(bus); out(bus, 1, 0.6);
    for (const n of [D3, A3, Cs4, E4, Fs4]) {
      for (const [type, det] of [["sawtooth", -7], ["triangle", 6]] as const) {
        const o = ctx.createOscillator(); o.type = type; o.frequency.value = hz(n); o.detune.value = det;
        o.connect(lp); o.start(a); o.stop(b + 0.05);
      }
    }
  }
  // sinos subindo + acorde de brilho
  [[GB_T.cover + 0.02, A5, 0.13, -0.3], [GB_T.cover + 0.26, Cs6, 0.12, 0.3], [GB_T.cover + 0.5, E6, 0.12, -0.15]].forEach(([at, n, v, pan]) =>
    bell(ctx, hz(n), t0 + at, v, 2.2, pan, out));
  bell(ctx, hz(Fs6), t0 + GB_T.sweep, 0.09, 2.4, 0.2, out);
  bell(ctx, hz(A6), t0 + GB_T.sweep + 0.04, 0.07, 2.4, -0.2, out);
  // reflexo de luz (brilho de ruído agudo)
  {
    const at = t0 + GB_T.sweep - 0.15;
    const s = ctx.createBufferSource(); s.buffer = noise;
    const hp = ctx.createBiquadFilter(); hp.type = "highpass"; hp.frequency.value = 6500;
    const g = ctx.createGain(); env(g.gain, [[at, 0.0001], [at + 0.25, 0.06], [at + 0.8, 0.0001]], true);
    s.connect(hp).connect(g); out(g, 1, 0.6); s.start(at, 0.3); s.stop(at + 0.85);
  }

  const endAt = t0 + GB_T.end + 2.6;
  const closeT = setTimeout(() => void ctx.close().catch(() => {}), (endAt - ctx.currentTime) * 1000 + 200);
  let stopped = false;
  return {
    stop(fade = 0.3) {
      if (stopped) return;
      stopped = true;
      clearTimeout(closeT);
      const now = ctx.currentTime;
      master.gain.cancelScheduledValues(now);
      master.gain.setValueAtTime(master.gain.value, now);
      master.gain.linearRampToValueAtTime(0.0001, now + fade);
      setTimeout(() => void ctx.close().catch(() => {}), fade * 1000 + 120);
    },
  };
}

// ---------- blocos ----------

type Out = (node: AudioNode, dry?: number, rev?: number) => void;

function env(p: AudioParam, pts: [number, number][], exp = false) {
  p.setValueAtTime(Math.max(pts[0][1], exp ? 0.0001 : 0), pts[0][0]);
  for (const [t, v] of pts.slice(1)) {
    if (exp) p.exponentialRampToValueAtTime(Math.max(v, 0.0001), t);
    else p.linearRampToValueAtTime(v, t);
  }
}

/** Sino de vidro (FM): portadora + moduladora inarmônica, ataque curto e cauda longa. */
function bell(ctx: AudioContext, f: number, at: number, vol: number, dur: number, pan: number, out: Out) {
  const car = ctx.createOscillator(); car.type = "sine"; car.frequency.value = f;
  const mod = ctx.createOscillator(); mod.type = "sine"; mod.frequency.value = f * 3.5;
  const mg = ctx.createGain(); env(mg.gain, [[at, f * 1.4], [at + dur * 0.6, f * 0.02]], true);
  mod.connect(mg).connect(car.frequency);
  const g = ctx.createGain(); env(g.gain, [[at, 0.0001], [at + 0.006, vol], [at + dur, 0.0001]], true);
  const p = ctx.createStereoPanner(); p.pan.value = pan;
  car.connect(g).connect(p); out(p, 0.6, 0.9);
  car.start(at); mod.start(at); car.stop(at + dur + 0.05); mod.stop(at + dur + 0.05);
}

function noiseBuffer(ctx: AudioContext, secs: number) {
  const b = ctx.createBuffer(1, Math.floor(ctx.sampleRate * secs), ctx.sampleRate);
  const d = b.getChannelData(0);
  for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  return b;
}

/** Resposta ao impulso sintética (sala ampla) para o reverb. */
function impulse(ctx: AudioContext, secs: number, decay: number) {
  const len = Math.floor(ctx.sampleRate * secs);
  const b = ctx.createBuffer(2, len, ctx.sampleRate);
  for (let c = 0; c < 2; c++) {
    const d = b.getChannelData(c);
    for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, decay);
  }
  return b;
}
