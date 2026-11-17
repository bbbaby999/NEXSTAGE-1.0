import { CSSProperties, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { GamebootRequest } from "../lib/gameboot";
import { api } from "../lib/api";
import { GB_T as T, playGamebootScore } from "../lib/gamebootAudio";
import { t } from "../lib/i18n";

/**
 * Gameboot (só ao iniciar um jogo pelo botão Jogar): ≈3,5 s em tela cheia e em primeiro plano,
 * inspirado na estética das inicializações de jogos portáteis — faixa de luz ondulante, uma
 * ondulação que se abre, a capa do jogo surgindo com reflexo, o nome e um reflexo de luz
 * atravessando a capa. Áudio original sincronizado.
 * O jogo só é iniciado DEPOIS da animação (com a tela já escura): ao lançar, o backend minimiza o
 * NEXSTAGE, então lançar antes fazia a animação tocar com o app minimizado. Se o lançamento falhar,
 * a janela volta ao modo anterior e o erro chega a quem chamou. Qualquer tecla, clique ou botão
 * encurta a animação (vai direto ao escurecimento e ao lançamento).
 */
type Phase = "" | "out" | "fast" | "closing";

export default function GameBootHost() {
  // `id` fixo por pedido: re-renderizações do App (ex.: aviso "Iniciando…") não podem reiniciar a
  // animação nem lançar o jogo de novo.
  const [req, setReq] = useState<(GamebootRequest & { id: number }) | null>(null);
  const seq = useRef(0);
  const [phase, setPhase] = useState<Phase>("");
  const busy = useRef(false);

  useEffect(() => {
    const on = (e: Event) => {
      if (busy.current) return; // já em exibição: quem chamou lança direto
      e.preventDefault();
      busy.current = true;
      setPhase("");
      setReq({ ...(e as CustomEvent<GamebootRequest>).detail, id: ++seq.current });
    };
    window.addEventListener("nexstage:gameboot", on);
    return () => window.removeEventListener("nexstage:gameboot", on);
  }, []);

  if (!req) return null;
  return <Run key={req.id} req={req} phase={phase} setPhase={setPhase} onEnd={() => { busy.current = false; setReq(null); }} />;
}

function Run({ req, phase, setPhase, onEnd }: { req: GamebootRequest; phase: Phase; setPhase: (p: Phase) => void; onEnd: () => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const skipRef = useRef<() => void>(() => {});

  useEffect(() => {
    const prevFocus = document.activeElement as HTMLElement | null;
    prevFocus?.blur(); // Enter/✕ repetidos não podem acionar "Jogar" de novo
    // NEXSTAGE na frente, em tela cheia, acima de tudo, enquanto a animação toca
    api.gamebootWindow(true).catch(() => {});
    const score = playGamebootScore();
    const start = performance.now();
    const timers: number[] = [];
    let darkening = false, done = false;

    const close = (restoreFocus: boolean) => {
      setPhase("closing");
      window.setTimeout(() => { onEnd(); if (restoreFocus) prevFocus?.focus?.({ preventScroll: true }); }, 380);
    };
    // fim da animação (tela escura) → agora sim, inicia o jogo
    const launch = () => {
      if (done) return;
      done = true;
      req.launch().then(
        () => { req.resolve(); window.setTimeout(() => close(false), 250); }, // o backend já minimizou o NEXSTAGE
        (e) => {
          score?.stop(0.25);
          api.gamebootWindow(false).catch(() => {});
          req.reject(e);
          close(true);
        },
      );
    };
    const darken = (fast: boolean) => {
      if (darkening) return;
      darkening = true;
      timers.forEach(clearTimeout);
      setPhase(fast ? "fast" : "out");
      if (fast) score?.stop(0.35);
      window.setTimeout(launch, fast ? 320 : (T.end - T.out) * 1000);
    };
    timers.push(window.setTimeout(() => darken(false), T.out * 1000));

    // encurtar: qualquer tecla/clique/botão (nada atrás da animação reage)
    const skip = (e?: Event) => {
      if (e && e.type === "keydown") { e.preventDefault(); e.stopImmediatePropagation(); }
      if (performance.now() - start > 500) darken(true);
    };
    skipRef.current = () => skip();
    window.addEventListener("keydown", skip, true);
    let un: (() => void) | undefined;
    let cancelled = false;
    // soltar o Ⓐ que iniciou o jogo (ou segurá-lo) não pula a animação: só um botão apertado agora
    listen<{ action: string }>("pad-input", (e) => { if (!e.payload.action.startsWith("confirm_")) skip(); }).then((f) => (cancelled ? f() : (un = f)));

    const stopDraw = draw(canvas.current!, start);
    return () => {
      timers.forEach(clearTimeout);
      window.removeEventListener("keydown", skip, true);
      cancelled = true;
      un?.();
      stopDraw();
      if (!done) score?.stop(0.2);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const g = req.game;
  return (
    <div className={`gb-root ${phase}`} role="presentation" aria-live="polite" onMouseDown={() => skipRef.current()}
      style={{ "--t-cover": `${T.cover}s`, "--t-title": `${T.title}s`, "--t-sweep": `${T.sweep}s`, "--t-bloom": `${T.bloom}s` } as CSSProperties}>
      <canvas ref={canvas} className="gb-canvas" />
      <div className="gb-center">
        <div className="gb-cover" style={g.cover ? { backgroundImage: `url(${g.cover})` } : undefined}>
          {!g.cover && <b>{g.title.charAt(0).toUpperCase()}</b>}
          <i className="gb-sweep" />
        </div>
        <div className="gb-title">{g.title}</div>
        <div className="gb-sub">{g.platform} <span>·</span> {t("NEXSTAGE")}</div>
      </div>
      <span className="sr-only">{t("Iniciando")}{" "}{g.title}</span>
    </div>
  );
}

// ---------------------------------------------------------------------------------------------
// Faixa de luz ondulante + ondulação + poeira de luz (canvas 2D, composição aditiva).

const clamp = (v: number, a = 0, b = 1) => Math.max(a, Math.min(b, v));
const smooth = (a: number, b: number, t: number) => { const x = clamp((t - a) / (b - a)); return x * x * (3 - 2 * x); };

function draw(cv: HTMLCanvasElement, start: number): () => void {
  const ctx = cv.getContext("2d")!;
  const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
  let W = 0, H = 0;
  const resize = () => { W = cv.width = Math.round(innerWidth * dpr); H = cv.height = Math.round(innerHeight * dpr); };
  resize();
  window.addEventListener("resize", resize);
  const css = getComputedStyle(document.documentElement);
  const ar = css.getPropertyValue("--ar").trim() || "47,107,255";
  const br = css.getPropertyValue("--br").trim() || "34,228,255";
  const dust = Array.from({ length: 70 }, () => ({ x: Math.random(), y: Math.random(), s: Math.random(), v: 0.01 + Math.random() * 0.03 }));
  let raf = 0;

  const frame = (now: number) => {
    const t = (now - start) / 1000;
    const s = Math.min(W, H), cx = W / 2, cy = H / 2;
    const vis = smooth(0, 0.35, t) * (1 - smooth(T.out, T.end, t));
    ctx.globalCompositeOperation = "source-over";
    ctx.clearRect(0, 0, W, H);
    ctx.globalCompositeOperation = "lighter";

    // faixa de luz ondulante (três camadas translúcidas + crista brilhante)
    const step = Math.max(6, W / 160);
    for (let layer = 0; layer < 3; layer++) {
      const amp = H * (0.07 + layer * 0.025), base = H * (0.6 + layer * 0.035), th = H * (0.05 + layer * 0.03);
      const y = (x: number, off: number) => base + Math.sin(x / W * (4.2 - layer * 0.7) + t * (0.7 + layer * 0.2) + layer) * amp
        + Math.sin(x / W * 9 - t * 0.9 + layer * 2) * amp * 0.25 + off;
      const grad = ctx.createLinearGradient(0, base - amp, 0, base + amp + th);
      grad.addColorStop(0, `rgba(${layer === 1 ? br : ar},${0.12 * vis})`);
      grad.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = grad;
      ctx.beginPath();
      for (let x = -step; x <= W + step; x += step) { const yy = y(x, 0); if (x < 0) ctx.moveTo(x, yy); else ctx.lineTo(x, yy); }
      for (let x = W + step; x >= -step; x -= step) ctx.lineTo(x, y(x, th));
      ctx.closePath();
      ctx.fill();
      ctx.strokeStyle = layer === 0 ? `rgba(235,248,255,${0.5 * vis})` : `rgba(${br},${0.28 * vis})`;
      ctx.lineWidth = (layer === 0 ? 1.4 : 1) * dpr;
      ctx.beginPath();
      for (let x = -step; x <= W + step; x += step) { const yy = y(x, 0); if (x < 0) ctx.moveTo(x, yy); else ctx.lineTo(x, yy); }
      ctx.stroke();
    }

    // ondulação de luz que se abre atrás da capa
    for (const delay of [0, 0.22]) {
      const u = clamp((t - T.bloom - delay) / 1.25);
      if (u > 0 && u < 1) {
        const r = s * (0.08 + 0.6 * (1 - Math.pow(1 - u, 3)));
        ctx.strokeStyle = `rgba(${br},${(1 - u) * 0.55 * vis})`;
        ctx.lineWidth = (2.5 - u * 1.8) * dpr;
        ctx.beginPath();
        ctx.arc(cx, cy - s * 0.04, r, 0, Math.PI * 2);
        ctx.stroke();
      }
    }
    // halo atrás da capa
    const halo = smooth(T.bloom, T.bloom + 0.6, t) * vis;
    if (halo > 0) {
      const g = ctx.createRadialGradient(cx, cy - s * 0.04, 0, cx, cy - s * 0.04, s * 0.55);
      g.addColorStop(0, `rgba(${br},${0.28 * halo})`);
      g.addColorStop(0.4, `rgba(${ar},${0.1 * halo})`);
      g.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, W, H);
    }
    // poeira de luz subindo
    for (const d of dust) {
      const y = ((d.y - t * d.v) % 1 + 1) % 1;
      ctx.fillStyle = `rgba(210,235,255,${(0.15 + d.s * 0.45) * vis * (0.6 + 0.4 * Math.sin(t * 3 + d.x * 20))})`;
      const r = (0.6 + d.s * 1.6) * dpr;
      ctx.fillRect(d.x * W, y * H, r, r);
    }

    if (t < T.end + 0.3) raf = requestAnimationFrame(frame);
  };
  raf = requestAnimationFrame(frame);
  return () => { cancelAnimationFrame(raf); window.removeEventListener("resize", resize); };
}
