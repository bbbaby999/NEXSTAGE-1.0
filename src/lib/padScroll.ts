/**
 * Rolagem com o analógico direito (eixo vertical).
 *
 * Entradas (já normalizadas em -1..1, negativo = cima):
 *  - "native": eventos `pad-scroll` do backend (Xbox/PlayStation), com a zona morta aplicada lá;
 *  - "generic": Gamepad API (controles DirectInput), zona morta aplicada aqui (`applyDeadzone`).
 *
 * A inclinação vira uma velocidade-alvo (curva suave: pouca inclinação = rolagem fina) e a velocidade
 * real se aproxima dela com amortecimento, então não há arrancos ao inclinar nem paradas secas ao soltar.
 * Rola a área rolável do elemento focado (ou a maior área rolável visível da camada ativa: modal,
 * menu lateral...), sempre respeitando os limites dela. Não mexe no foco: o analógico esquerdo/D-pad
 * continuam navegando normalmente.
 */

import { LAYER_SEL } from "./nav";

type Source = "native" | "generic";

/** Zona morta da Gamepad API (o backend usa o mesmo valor para os controles nativos). */
export const SCROLL_DEADZONE = 0.24;
/** Velocidade máxima (px/s) com a alavanca no fim do curso. */
const MAX_SPEED = 1500;
/** Velocidade mínima (px/s) logo após sair da zona morta. */
const MIN_SPEED = 60;
/** Expoente da curva de resposta (>1 = mais precisão perto do centro). */
const CURVE = 1.7;
/** Constante de tempo do amortecimento (s): quanto maior, mais "macio" acelera e freia. */
const SMOOTH_ACCEL = 0.09;
const SMOOTH_BRAKE = 0.06;

const input: Record<Source, number> = { native: 0, generic: 0 };
let vel = 0; // px/s (positivo = para baixo)
let pos = 0; // posição em ponto flutuante (o navegador arredonda scrollTop)
let target: HTMLElement | null = null;
let raf = 0;
let last = 0;
/** Houve rolagem pelo analógico desde a última navegação por foco? */
let scrolledSinceFocus = false;
let lastScroller: HTMLElement | null = null;

/** Zona morta com reescala (sem degrau ao sair dela). */
export function applyDeadzone(v: number, dz = SCROLL_DEADZONE): number {
  const a = Math.min(1, Math.abs(v));
  if (a <= dz) return 0;
  return Math.sign(v) * ((a - dz) / (1 - dz));
}

/** Atualiza a inclinação de uma fonte. Inicia o laço de animação quando preciso. */
export function setScrollInput(src: Source, value: number) {
  input[src] = Number.isFinite(value) ? Math.max(-1, Math.min(1, value)) : 0;
  if (!raf && strongest() !== 0) {
    last = performance.now();
    raf = requestAnimationFrame(tick);
  }
}

const strongest = () => (Math.abs(input.native) >= Math.abs(input.generic) ? input.native : input.generic);

const SCROLLABLE = /(auto|scroll|overlay)/;
function canScroll(el: Element): el is HTMLElement {
  if (!(el instanceof HTMLElement)) return false;
  if (el.scrollHeight - el.clientHeight <= 1) return false;
  if (el === document.scrollingElement) return true;
  return SCROLLABLE.test(getComputedStyle(el).overflowY);
}

/** Camada ativa (mesma regra da navegação: o modal/menu mais recente isola o que está atrás). */
function layer(): Element {
  const layers = document.querySelectorAll<HTMLElement>(LAYER_SEL);
  return layers.length ? layers[layers.length - 1] : document.documentElement;
}

function visibleArea(el: HTMLElement): number {
  const r = el.getBoundingClientRect();
  const w = Math.min(r.right, innerWidth) - Math.max(r.left, 0);
  const h = Math.min(r.bottom, innerHeight) - Math.max(r.top, 0);
  return w > 0 && h > 0 ? w * h : 0;
}

/** Área a rolar: ancestral rolável do foco dentro da camada ativa; senão, a maior área rolável visível. */
function findScroller(): HTMLElement | null {
  const root = layer();
  const active = document.activeElement;
  if (active && active !== document.body && root.contains(active)) {
    for (let el: Element | null = active; el && root.contains(el); el = el.parentElement) {
      if (canScroll(el) && visibleArea(el) > 0) return el;
    }
  }
  let best: HTMLElement | null = null, bestArea = 0;
  const consider = (el: Element) => {
    if (!canScroll(el)) return;
    const a = visibleArea(el);
    if (a > bestArea) { bestArea = a; best = el; }
  };
  consider(root);
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) consider(n as Element);
  if (!best && root === document.documentElement && document.scrollingElement) consider(document.scrollingElement);
  return best;
}

function tick(t: number) {
  const dt = Math.min(0.05, Math.max(0, (t - last) / 1000)); // evita saltos após travadas
  last = t;
  const v = strongest();
  const want = v === 0 ? 0 : Math.sign(v) * (MIN_SPEED + (MAX_SPEED - MIN_SPEED) * Math.pow(Math.abs(v), CURVE));

  // Começo de um gesto (ou a área sumiu/mudou de camada): escolhe de novo onde rolar.
  if (v !== 0 && (!target || !target.isConnected || !layer().contains(target))) {
    target = findScroller();
    if (target) pos = target.scrollTop;
    vel = 0;
  }

  const accelerating = want !== 0 && Math.abs(want) > Math.abs(vel) && want * vel >= 0;
  const tau = accelerating ? SMOOTH_ACCEL : SMOOTH_BRAKE;
  vel += (want - vel) * (1 - Math.exp(-dt / tau));
  if (Math.abs(vel) < 4 && want === 0) vel = 0;

  if (target && vel !== 0) {
    const max = target.scrollHeight - target.clientHeight;
    // Outra coisa (mouse, foco) moveu a área: segue a partir de onde ela está.
    if (Math.abs(target.scrollTop - pos) > 2) pos = target.scrollTop;
    pos += vel * dt;
    // Chegou ao limite: para ali, sem "quicar" nem acumular velocidade contra a borda.
    if (pos <= 0) { pos = 0; if (vel < 0) vel = 0; }
    else if (pos >= max) { pos = max; if (vel > 0) vel = 0; }
    // "instant" ignora o scroll-behavior:smooth do CSS (que faria cada quadro animar sozinho).
    target.scrollTo({ top: pos, behavior: "instant" as ScrollBehavior });
    scrolledSinceFocus = true;
    lastScroller = target;
  }

  if (v === 0 && vel === 0) {
    raf = 0;
    target = null;
    return;
  }
  raf = requestAnimationFrame(tick);
}

/**
 * Se o analógico direito rolou a tela e o item focado ficou fora da área visível, devolve o item
 * visível por onde a navegação deve recomeçar (em vez de pular de volta para o foco antigo).
 */
export function refocusAfterScroll(cur: HTMLElement, els: HTMLElement[], dir: "left" | "right" | "up" | "down"): HTMLElement | null {
  if (!scrolledSinceFocus) return null;
  scrolledSinceFocus = false;
  const sc = lastScroller;
  if (!sc || !sc.isConnected || !sc.contains(cur)) return null;
  const box = sc === document.scrollingElement ? new DOMRect(0, 0, innerWidth, innerHeight) : sc.getBoundingClientRect();
  const top = Math.max(box.top, 0), bottom = Math.min(box.bottom, innerHeight);
  const r = cur.getBoundingClientRect();
  if (r.bottom > top && r.top < bottom) return null; // o foco ainda está à vista
  const inView = els.filter((el) => {
    if (!sc.contains(el)) return false;
    const b = el.getBoundingClientRect();
    return b.top >= top - 1 && b.bottom <= bottom + 1;
  });
  if (!inView.length) return null;
  if (dir === "down") return inView.reduce((p, c) => (c.getBoundingClientRect().top < p.getBoundingClientRect().top ? c : p));
  if (dir === "up") return inView.reduce((p, c) => (c.getBoundingClientRect().bottom > p.getBoundingClientRect().bottom ? c : p));
  const mid = (top + bottom) / 2;
  const d = (el: HTMLElement) => { const b = el.getBoundingClientRect(); return Math.abs(b.top + b.height / 2 - mid); };
  return inView.reduce((p, c) => (d(c) < d(p) ? c : p));
}
