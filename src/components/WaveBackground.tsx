import { useEffect, useRef } from "react";
import { reducedMotion } from "../lib/motion";

/** Partícula: `ribbon` = presa à onda (segue a curva com deslocamento `off`); senão, solta no ambiente. */
interface Particle { x: number; y: number; off: number; r: number; v: number; tw: number; ph: number; ribbon: boolean; warm: boolean }

/** Uma faixa de onda: feixe de fios finos entre duas bordas que se cruzam (dá a ilusão de torção). */
interface Ribbon {
  y: number;      // altura média (fração da tela)
  amp: number;    // amplitude da curva central (fração da altura)
  thick: number;  // meia-espessura máxima (fração da altura)
  f: number;      // frequência espacial
  s: number;      // velocidade
  twist: number;  // frequência da torção ao longo da tela
  strands: number;
  alpha: number;
  seed: number;
}

const RIBBONS: Ribbon[] = [
  // faixa de trás: mais larga, tênue e lenta
  { y: 0.6, amp: 0.07, thick: 0.13, f: 0.75, s: 0.05, twist: 0.55, strands: 14, alpha: 0.5, seed: 2.1 },
  // faixa principal
  { y: 0.56, amp: 0.085, thick: 0.085, f: 0.95, s: 0.075, twist: 0.8, strands: 22, alpha: 1, seed: 0 },
];

const STEP = 8; // px entre pontos da curva

/**
 * Fundo animado em canvas 2D: faixas de luz sedosas que ondulam e "torcem" devagar no meio da tela
 * (cada faixa é um feixe de fios finos somados com mistura aditiva) + partículas minúsculas que
 * cintilam e escorrem ao longo da onda, com algumas soltas no ambiente.
 * Roda na taxa do monitor; resolução até 1,5x (cai para 1x se o quadro ficar caro), até 150 partículas desenhadas por sprite.
 * Pausa com a janela minimizada, overlay, movimento reduzido ou menu/modal aberto. Cores do tema (--ar / --br).
 */
export default function WaveBackground() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const cv = ref.current!;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    let raf = 0, idle = 0, w = 0, h = 0, t = 0, dpr = 1;
    // Qualidade adaptativa: custo médio do quadro (ms). Se a GPU/CPU não acompanhar a taxa do monitor,
    // a resolução cai de 1,5x para 1x antes de a animação perder quadros.
    let cost = 0, maxDpr = Math.min(1.5, window.devicePixelRatio || 1);
    let last = performance.now();
    let c = ["47,107,255", "34,228,255"];
    let parts: Particle[] = [];
    let dot: HTMLCanvasElement[] = [];
    // buffers reaproveitados por quadro (centro e meia-espessura de cada faixa em cada x)
    let cy: Float32Array[] = [], th: Float32Array[] = [];

    /** Sprite de brilho: núcleo branco minúsculo + halo curto na cor. Desenhado pequeno (2–7 px). */
    const sprite = (rgb: string) => {
      const s = document.createElement("canvas");
      s.width = s.height = 24;
      const g = s.getContext("2d")!;
      const gr = g.createRadialGradient(12, 12, 0, 12, 12, 12);
      gr.addColorStop(0, "rgba(255,255,255,1)");
      gr.addColorStop(0.12, "rgba(255,255,255,0.9)");
      gr.addColorStop(0.3, `rgba(${rgb},0.35)`);
      gr.addColorStop(1, `rgba(${rgb},0)`);
      g.fillStyle = gr;
      g.fillRect(0, 0, 24, 24);
      return s;
    };
    const readColors = () => {
      const s = getComputedStyle(document.documentElement);
      const next = [s.getPropertyValue("--ar").trim() || c[0], s.getPropertyValue("--br").trim() || c[1]];
      if (next[0] !== c[0] || next[1] !== c[1] || !dot.length) { c = next; dot = [sprite(c[1]), sprite("235,242,255")]; buildGradients(); }
    };
    // Gradientes só mudam com tamanho ou tema: criados uma vez, não a cada quadro.
    let glowGrad: CanvasGradient | null = null;
    let veilGrad: CanvasGradient[] = [];
    const buildGradients = () => {
      if (!w || !h) return;
      const mid = h * RIBBONS[1].y;
      glowGrad = ctx.createLinearGradient(0, mid - h * 0.3, 0, mid + h * 0.3);
      glowGrad.addColorStop(0, `rgba(${c[0]},0)`);
      glowGrad.addColorStop(0.5, `rgba(${c[0]},0.07)`);
      glowGrad.addColorStop(1, `rgba(${c[0]},0)`);
      veilGrad = RIBBONS.map((r) => {
        const g = ctx.createLinearGradient(0, 0, w, 0);
        g.addColorStop(0, `rgba(${c[0]},0)`);
        g.addColorStop(0.25, `rgba(${c[0]},${0.06 * r.alpha})`);
        g.addColorStop(0.75, `rgba(${c[1]},${0.05 * r.alpha})`);
        g.addColorStop(1, `rgba(${c[1]},0)`);
        return g;
      });
    };

    // Curva central e torção: soma de senos lentos, sem padrão que se repita à vista.
    const center = (r: Ribbon, x: number) => {
      const k = (x / w) * Math.PI * 2;
      return h * r.y + h * r.amp * (
        Math.sin(k * r.f + t * r.s * 6 + r.seed) * 0.62 +
        Math.sin(k * r.f * 1.9 - t * r.s * 4.3 + r.seed * 1.7) * 0.25 +
        Math.sin(k * 0.35 + t * r.s * 2.2) * 0.4);
    };
    const half = (r: Ribbon, x: number) => {
      const k = (x / w) * Math.PI * 2;
      // passa por zero (torção) e troca de lado; o mínimo de 0.08 evita que vire uma linha só
      const tw = Math.sin(k * r.twist - t * r.s * 5 + r.seed * 0.7);
      return h * r.thick * (Math.sign(tw) * (0.08 + 0.92 * Math.abs(tw))) * (0.85 + 0.15 * Math.sin(k * 1.3 + t * 0.2));
    };

    const seed = () => {
      const count = Math.min(150, Math.round((w * h) / 11000));
      parts = Array.from({ length: count }, () => {
        const ribbon = Math.random() < 0.72;
        // perto da onda: distribuição concentrada no centro da faixa (soma de aleatórios ≈ normal)
        const off = ((Math.random() + Math.random() + Math.random()) / 3 - 0.5) * 2 * h * 0.16;
        return {
          x: Math.random() * w, y: Math.random() * h, off, ribbon,
          r: 0.5 + Math.random() ** 2.2 * 1.6,          // raio do núcleo em px (maioria bem pequena)
          v: 6 + Math.random() * 16,                      // px/s, da esquerda para a direita
          tw: 0.6 + Math.random() * 2.2, ph: Math.random() * 6.28, warm: Math.random() < 0.35,
        };
      });
    };
    /** Ajusta o canvas à janela na resolução `dpr` (sem reposicionar as partículas se `keep`). */
    const fit = (keep: boolean) => {
      w = cv.clientWidth; h = cv.clientHeight;
      cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const pts = Math.ceil(w / STEP) + 2;
      if (!keep || cy[0]?.length !== pts) {
        cy = RIBBONS.map(() => new Float32Array(pts));
        th = RIBBONS.map(() => new Float32Array(pts));
      }
      buildGradients();
      if (!keep) seed();
    };
    const resize = () => {
      maxDpr = Math.min(1.5, window.devicePixelRatio || 1);
      dpr = maxDpr;
      cost = 0;
      fit(false);
    };

    /** Brilho difuso largo atrás da onda principal (dá "atmosfera" sem blur por quadro). */
    const drawGlow = () => {
      if (!glowGrad) return;
      const mid = h * RIBBONS[1].y;
      ctx.fillStyle = glowGrad;
      ctx.fillRect(0, mid - h * 0.3, w, h * 0.6);
    };

    const drawRibbon = (ri: number) => {
      const r = RIBBONS[ri];
      const C = cy[ri], T = th[ri], pts = C.length;
      for (let i = 0; i < pts; i++) { const x = i * STEP; C[i] = center(r, x); T[i] = half(r, x); }

      // véu translúcido entre as bordas
      ctx.beginPath();
      ctx.moveTo(0, C[0] - T[0]);
      for (let i = 1; i < pts; i++) ctx.lineTo(i * STEP, C[i] - T[i]);
      for (let i = pts - 1; i >= 0; i--) ctx.lineTo(i * STEP, C[i] + T[i]);
      ctx.closePath();
      ctx.fillStyle = veilGrad[ri];
      ctx.fill();

      // fios: mais fortes nas bordas (como seda vista de lado) e cor do tema → destaque ao atravessar
      for (let s = 0; s < r.strands; s++) {
        const u = (s / (r.strands - 1)) * 2 - 1; // -1..1 através da faixa
        const edge = Math.abs(u);
        ctx.beginPath();
        ctx.moveTo(0, C[0] + T[0] * u);
        for (let i = 1; i < pts; i++) ctx.lineTo(i * STEP, C[i] + T[i] * u);
        const a = r.alpha * (0.05 + 0.2 * edge ** 3);
        ctx.strokeStyle = `rgba(${u < 0 ? c[0] : c[1]},${a.toFixed(3)})`;
        ctx.lineWidth = 0.8;
        ctx.stroke();
        if (edge > 0.97) { // borda com halo e fio branco fino (mesmo caminho)
          ctx.strokeStyle = `rgba(${c[1]},${(0.07 * r.alpha).toFixed(3)})`;
          ctx.lineWidth = 5;
          ctx.stroke();
          ctx.strokeStyle = `rgba(255,255,255,${(0.22 * r.alpha).toFixed(3)})`;
          ctx.lineWidth = 0.7;
          ctx.stroke();
        }
      }
    };

    /** Altura da onda principal em x, interpolada entre os pontos (sem "degraus" nas partículas). */
    const ribbonY = (x: number) => {
      const C = cy[1], f = Math.max(0, Math.min(C.length - 1.001, x / STEP)), i = f | 0;
      return C[i] + (C[i + 1] - C[i]) * (f - i);
    };
    const moveParticles = (dt: number) => {
      for (const p of parts) {
        p.x += p.v * dt;
        if (!p.ribbon) p.y += Math.sin(t * 0.4 + p.ph) * 4 * dt; // ambiente: flutua devagar
        if (p.x > w + 8) {
          p.x = -8;
          if (!p.ribbon) p.y = Math.random() * h;
        }
      }
    };
    const drawParticles = (wavesOn: boolean) => {
      for (const p of parts) {
        const tw = 0.5 + 0.5 * Math.sin(t * p.tw + p.ph);
        // sem ondas, as partículas presas à onda flutuam onde estão
        const y = p.ribbon && wavesOn ? ribbonY(p.x) + p.off + Math.sin(t * 0.7 + p.ph) * 6 : p.y;
        const near = p.ribbon ? 1 - Math.min(1, Math.abs(p.off) / (h * 0.16)) : 0.4;
        ctx.globalAlpha = (0.25 + 0.75 * tw * tw) * (0.35 + 0.65 * near);
        const size = p.r * (5 + 2 * tw); // halo ≈ 5–7x o núcleo; núcleo visível ≈ 1–3 px
        ctx.drawImage(dot[p.warm ? 1 : 0], p.x - size / 2, y - size / 2, size, size);
      }
      ctx.globalAlpha = 1;
    };

    // Cores do tema: relidas só quando o tema muda (getComputedStyle a cada segundo causava engasgos).
    const themeObs = new MutationObserver(() => readColors());
    // "style": o tema personalizado troca as cores direto nas variáveis do <html>
    themeObs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style"] });

    // Taxa nativa do monitor (60/120/144 Hz): o movimento usa o tempo real entre quadros, sem limitador.
    const frame = (now: number) => {
      const root = document.documentElement;
      const reduced = reducedMotion();
      const wavesOn = root.dataset.waves !== "off";
      const partsOn = root.dataset.particles !== "off";
      cv.style.visibility = reduced || (!wavesOn && !partsOn) ? "hidden" : "visible";
      if (document.hidden || reduced || (!wavesOn && !partsOn) || root.classList.contains("overlay-mode") ||
          document.querySelector(".overlay, .sm-root, .pick-root, .kb-root")) {
        last = now;
        idle = window.setTimeout(() => (raf = requestAnimationFrame(frame)), 500);
        return;
      }
      const dt = Math.min(0.05, Math.max(0, (now - last) / 1000)); // pausa longa não vira "salto"
      t += dt;
      last = now;

      const t0 = performance.now();
      ctx.globalCompositeOperation = "source-over";
      ctx.clearRect(0, 0, w, h);
      ctx.globalCompositeOperation = "lighter"; // luz somada: cruzamentos dos fios brilham mais
      if (wavesOn) {
        drawGlow();
        drawRibbon(0);
        drawRibbon(1);
      }
      if (partsOn) { moveParticles(dt); drawParticles(wavesOn); }
      ctx.globalCompositeOperation = "source-over";

      cost = cost * 0.95 + (performance.now() - t0) * 0.05;
      if (cost > 7 && dpr > 1) { dpr = 1; cost = 0; fit(true); } // máquina mais fraca: resolução 1x
      raf = requestAnimationFrame(frame);
    };

    readColors();
    resize();
    window.addEventListener("resize", resize);
    raf = requestAnimationFrame(frame);
    return () => {
      window.removeEventListener("resize", resize);
      themeObs.disconnect();
      cancelAnimationFrame(raf);
      clearTimeout(idle);
    };
  }, []);

  return <canvas ref={ref} className="waves" aria-hidden="true" />;
}
