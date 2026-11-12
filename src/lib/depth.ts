import { useEffect } from "react";
import { reducedMotion } from "./motion";

/**
 * Profundidade 3D global: inclinação leve que segue o mouse em todas as superfícies da interface
 * (linhas de configuração, miniaturas, temas, painéis…). Um único ouvinte delegado no documento grava
 * --dx/--dy (graus) e --gx/--gy (posição do reflexo) no elemento sob o mouse; o transform fica em
 * depth.css. Capas de jogos já têm a própria inclinação (lib/tilt) e ficam de fora.
 * Não toca no fundo de ondas. Desligado com movimento reduzido.
 */
export const DEPTH_SEL = [
  ".setting:not(.col-set)", ".stat", ".thumb", ".theme", ".emu", ".folder-list li", ".radio", ".note",
  ".pf-list li", ".opt", ".stage-shot", ".mini-shot", ".sm-user", ".kb-key", ".chip", ".btn", ".stage-more",
  ".select-btn", ".coll-row", ".rate-row", ".hltb", ".legend", ".seg",
].join(",");

/** Inclinação máxima (graus) num elemento de até 260px; elementos maiores inclinam menos (bordas não "saltam"). */
const MAX_DEG = 7;
const REF = 260;

export function useDepthTilt() {
  useEffect(() => {
    let cur: HTMLElement | null = null;
    let raf = 0, ex = 0, ey = 0;

    const clear = (el: HTMLElement | null) => {
      if (!el) return;
      ["--dx", "--dy", "--gx", "--gy"].forEach((v) => el.style.removeProperty(v));
      el.classList.remove("depth-on");
    };
    const apply = () => {
      raf = 0;
      if (!cur) return;
      const r = cur.getBoundingClientRect();
      if (!r.width || !r.height) return;
      const x = Math.min(1, Math.max(0, (ex - r.left) / r.width));
      const y = Math.min(1, Math.max(0, (ey - r.top) / r.height));
      const k = MAX_DEG * Math.min(1, REF / Math.max(r.width, r.height));
      cur.style.setProperty("--dy", `${((x - 0.5) * k).toFixed(2)}deg`);
      cur.style.setProperty("--dx", `${((0.5 - y) * k).toFixed(2)}deg`);
      cur.style.setProperty("--gx", `${(x * 100).toFixed(0)}%`);
      cur.style.setProperty("--gy", `${(y * 100).toFixed(0)}%`);
    };
    const onMove = (e: PointerEvent) => {
      if (e.pointerType !== "mouse") return;
      const t = e.target instanceof Element ? e.target : null;
      // superfícies aninhadas (botão dentro de uma linha): a mais interna inclina
      let el = reducedMotion() ? null : (t?.closest<HTMLElement>(DEPTH_SEL) ?? null);
      // um elemento com transform prende janelas position:fixed dentro dele: nunca inclinar esses
      if (el && el.querySelector(".overlay")) el = null;
      if (el !== cur) { clear(cur); cur = el; cur?.classList.add("depth-on"); }
      ex = e.clientX; ey = e.clientY;
      if (cur && !raf) raf = requestAnimationFrame(apply);
    };
    const onLeave = () => { clear(cur); cur = null; };

    document.addEventListener("pointermove", onMove, { passive: true });
    document.documentElement.addEventListener("pointerleave", onLeave);
    window.addEventListener("blur", onLeave);
    return () => {
      document.removeEventListener("pointermove", onMove);
      document.documentElement.removeEventListener("pointerleave", onLeave);
      window.removeEventListener("blur", onLeave);
      cancelAnimationFrame(raf);
      clear(cur);
    };
  }, []);
}
