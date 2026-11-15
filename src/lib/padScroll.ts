/**
 * Rolagem com o analógico direito: vertical rola a página; horizontal rola os carrosséis.
 *
 * Entradas (já normalizadas em -1..1, negativo = cima/esquerda):
 *  - "native": eventos `pad-scroll` do backend (Xbox/PlayStation), com a zona morta aplicada lá;
 *  - "generic": Gamepad API (controles DirectInput), zona morta aplicada aqui (`applyDeadzone`).
 *
 * A inclinação vira uma velocidade-alvo (curva suave: pouca inclinação = rolagem fina) e a velocidade
 * real se aproxima dela com amortecimento, então não há arrancos ao inclinar nem paradas secas ao soltar.
 * Vertical: rola a área rolável do elemento focado (ou a maior área rolável visível da camada ativa).
 * Horizontal: rola o carrossel do item focado (só quando o foco está num carrossel). O encaixe
 * (scroll-snap) do carrossel fica desligado enquanto a alavanca o move — senão cada pequeno avanço
 * por quadro era puxado de volta ao cartão mais próximo e o carrossel não saía do lugar.
 * Sempre respeitando os limites da área. Não mexe no foco: o analógico esquerdo/D-pad continuam
 * navegando normalmente.
 */

import { LAYER_SEL } from "./nav";

type Source = "native" | "generic";
type Axis = "x" | "y";

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

/** Estado de um eixo de rolagem. */
interface AxisState {
  input: Record<Source, number>;
  vel: number; // px/s (positivo = baixo/direita)
  pos: number; // posição em ponto flutuante (o navegador arredonda scrollTop/scrollLeft)
  target: HTMLElement | null;
  /** Carrossel com o encaixe (scroll-snap) desligado durante o gesto. */
  snap: HTMLElement | null;
}
const axes: Record<Axis, AxisState> = {
  y: { input: { native: 0, generic: 0 }, vel: 0, pos: 0, target: null, snap: null },
  x: { input: { native: 0, generic: 0 }, vel: 0, pos: 0, target: null, snap: null },
};
let raf = 0;
let last = 0;
/** Houve rolagem vertical pelo analógico desde a última navegação por foco? */
let scrolledSinceFocus = false;
let lastScroller: HTMLElement | null = null;

/** Zona morta com reescala (sem degrau ao sair dela). */
export function applyDeadzone(v: number, dz = SCROLL_DEADZONE): number {
  const a = Math.min(1, Math.abs(v));
  if (a <= dz) return 0;
  return Math.sign(v) * ((a - dz) / (1 - dz));
}

function setInput(axis: Axis, src: Source, value: number) {
  axes[axis].input[src] = Number.isFinite(value) ? Math.max(-1, Math.min(1, value)) : 0;
  if (!raf && (strongest("y") !== 0 || strongest("x") !== 0)) {
    last = performance.now();
    raf = requestAnimationFrame(tick);
  }
}

/** Atualiza a inclinação vertical de uma fonte. Inicia o laço de animação quando preciso. */
export function setScrollInput(src: Source, value: number) {
  setInput("y", src, value);
}
/** Atualiza a inclinação horizontal de uma fonte (carrosséis). */
export function setScrollInputX(src: Source, value: number) {
  setInput("x", src, value);
}

function strongest(axis: Axis) {
  const i = axes[axis].input;
  return Math.abs(i.native) >= Math.abs(i.generic) ? i.native : i.generic;
}

const SCROLLABLE = /(auto|scroll|overlay)/;
function canScroll(el: Element, axis: Axis): el is HTMLElement {
  if (!(el instanceof HTMLElement)) return false;
  if (axis === "y" ? el.scrollHeight - el.clientHeight <= 1 : el.scrollWidth - el.clientWidth <= 1) return false;
  if (axis === "y" && el === document.scrollingElement) return true;
  if (el === document.scrollingElement) return false; // a página nunca rola para o lado
  const cs = getComputedStyle(el);
  return SCROLLABLE.test(axis === "y" ? cs.overflowY : cs.overflowX);
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

/**
 * Área a rolar no eixo: ancestral rolável do foco dentro da camada ativa. Na vertical, sem um, a maior
 * área rolável visível; na horizontal, nenhuma (o carrossel só rola com o foco dentro dele).
 */
function findScroller(axis: Axis): HTMLElement | null {
  const root = layer();
  const active = document.activeElement;
  if (active && active !== document.body && root.contains(active)) {
    for (let el: Element | null = active; el && root.contains(el); el = el.parentElement) {
      if (canScroll(el, axis) && visibleArea(el) > 0) return el;
    }
  }
  if (axis === "x") return null;
  let best: HTMLElement | null = null, bestScore = -Infinity;
  const consider = (el: Element) => {
    if (!canScroll(el, axis)) return;
    const a = visibleArea(el);
    if (a > bestScore && a > 0) { bestScore = a; best = el; }
  };
  consider(root);
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) consider(n as Element);
  if (!best && axis === "y" && root === document.documentElement && document.scrollingElement) consider(document.scrollingElement);
  return best;
}

/** Avança um eixo um quadro (`dt` em s). Devolve se ele ainda está em movimento. */
function step(axis: Axis, dt: number): boolean {
  const s = axes[axis];
  const v = strongest(axis);
  const want = v === 0 ? 0 : Math.sign(v) * (MIN_SPEED + (MAX_SPEED - MIN_SPEED) * Math.pow(Math.abs(v), CURVE));
  const read = (el: HTMLElement) => (axis === "y" ? el.scrollTop : el.scrollLeft);

  // Começo de um gesto (ou a área sumiu/mudou de camada): escolhe de novo onde rolar.
  if (v !== 0 && (!s.target || !s.target.isConnected || !layer().contains(s.target))) {
    release(s);
    s.target = findScroller(axis);
    if (s.target) {
      s.pos = read(s.target);
      // carrossel com encaixe: desligado enquanto a alavanca rola (religado ao parar)
      if (axis === "x") { s.snap = s.target; s.target.style.scrollSnapType = "none"; }
    }
    s.vel = 0;
  }

  const accelerating = want !== 0 && Math.abs(want) > Math.abs(s.vel) && want * s.vel >= 0;
  const tau = accelerating ? SMOOTH_ACCEL : SMOOTH_BRAKE;
  s.vel += (want - s.vel) * (1 - Math.exp(-dt / tau));
  if (Math.abs(s.vel) < 4 && want === 0) s.vel = 0;

  const t = s.target;
  if (t && s.vel !== 0) {
    const max = axis === "y" ? t.scrollHeight - t.clientHeight : t.scrollWidth - t.clientWidth;
    // Outra coisa (mouse, foco) moveu a área: segue a partir de onde ela está.
    if (Math.abs(read(t) - s.pos) > 2) s.pos = read(t);
    s.pos += s.vel * dt;
    // Chegou ao limite: para ali, sem "quicar" nem acumular velocidade contra a borda.
    if (s.pos <= 0) { s.pos = 0; if (s.vel < 0) s.vel = 0; }
    else if (s.pos >= max) { s.pos = max; if (s.vel > 0) s.vel = 0; }
    // "instant" ignora o scroll-behavior:smooth do CSS (que faria cada quadro animar sozinho).
    t.scrollTo(axis === "y" ? { top: s.pos, behavior: "instant" as ScrollBehavior } : { left: s.pos, behavior: "instant" as ScrollBehavior });
    if (axis === "y") {
      scrolledSinceFocus = true;
      lastScroller = t;
    }
  }

  if (v === 0 && s.vel === 0) {
    release(s);
    s.target = null;
    return false;
  }
  return true;
}

/** Religa o encaixe do carrossel que a alavanca estava rolando. */
function release(s: AxisState) {
  if (s.snap) { s.snap.style.scrollSnapType = ""; s.snap = null; }
}

function tick(t: number) {
  const dt = Math.min(0.05, Math.max(0, (t - last) / 1000)); // evita saltos após travadas
  last = t;
  const movingY = step("y", dt);
  const movingX = step("x", dt);
  raf = movingY || movingX ? requestAnimationFrame(tick) : 0;
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
