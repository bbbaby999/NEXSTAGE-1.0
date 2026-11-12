import type { MouseEvent } from "react";
import { reducedMotion } from "./motion";

/** Inclinação máxima das capas (graus): 9° + 15% + 10% (≈ 11,4°). Igual na Biblioteca e na Home. */
export const TILT = 9 * 1.15 * 1.1;
const VARS = ["--rx", "--ry", "--mx", "--my"];

/**
 * Parallax 3D das capas seguindo o mouse (só CSS: grava --rx/--ry/--mx/--my na capa; o transform e o
 * reflexo ficam no CSS). `pick` escolhe o elemento da capa a partir do elemento com o evento
 * (padrão: o próprio). Desligado com movimento reduzido.
 */
export function tiltHandlers<T extends HTMLElement>(pick: (el: T) => HTMLElement | null = (el) => el) {
  return {
    onMouseMove: (e: MouseEvent<T>) => {
      if (reducedMotion()) return;
      const c = pick(e.currentTarget);
      if (!c) return;
      const r = c.getBoundingClientRect();
      const x = (e.clientX - r.left) / r.width, y = (e.clientY - r.top) / r.height;
      c.style.setProperty("--ry", `${((x - 0.5) * TILT).toFixed(2)}deg`);
      c.style.setProperty("--rx", `${((0.5 - y) * TILT).toFixed(2)}deg`);
      c.style.setProperty("--mx", `${(x * 100).toFixed(0)}%`);
      c.style.setProperty("--my", `${(y * 100).toFixed(0)}%`);
    },
    onMouseLeave: (e: MouseEvent<T>) => {
      const c = pick(e.currentTarget);
      VARS.forEach((v) => c?.style.removeProperty(v));
    },
  };
}
