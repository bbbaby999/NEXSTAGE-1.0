/**
 * Preferência de movimento. "auto" segue o Windows (prefers-reduced-motion); "full" força as
 * animações; "reduced" desliga efeitos decorativos (ondas, inclinação 3D, parallax, brilhos).
 * O CSS usa html[data-motion]; os efeitos em JS consultam reducedMotion().
 */
import { useEffect } from "react";

export type MotionMode = "auto" | "full" | "reduced";
let mode: MotionMode = "auto";

export function setMotion(m: MotionMode) {
  mode = m;
  document.documentElement.dataset.motion = m;
}

export function reducedMotion(): boolean {
  if (mode === "reduced") return true;
  if (mode === "full") return false;
  return matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Parallax leve: atualiza --px/--py (-1..1) só nos elementos [data-parallax], 1x por quadro. */
export function useParallax() {
  useEffect(() => {
    let raf = 0, x = 0, y = 0;
    const apply = () => {
      raf = 0;
      if (reducedMotion()) return;
      document.querySelectorAll<HTMLElement>("[data-parallax]").forEach((el) => {
        el.style.setProperty("--px", x.toFixed(3));
        el.style.setProperty("--py", y.toFixed(3));
      });
    };
    const onMove = (e: MouseEvent) => {
      x = (e.clientX / innerWidth) * 2 - 1;
      y = (e.clientY / innerHeight) * 2 - 1;
      if (!raf) raf = requestAnimationFrame(apply);
    };
    window.addEventListener("mousemove", onMove, { passive: true });
    return () => {
      window.removeEventListener("mousemove", onMove);
      cancelAnimationFrame(raf);
    };
  }, []);
}
