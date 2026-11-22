/**
 * Navegação por foco (espacial) para teclado e controle (Xbox/PlayStation).
 * Todo elemento navegável carrega `data-focus`. Se houver um modal aberto
 * (`.overlay`), a navegação fica restrita ao modal mais recente.
 *
 * Teclado: setas movem o foco; Enter/Espaço ativam (comportamento nativo de <button>).
 * Controle (mapeamento padrão): D-pad/analógico esquerdo = mover; A / Cruz = confirmar;
 * B / Círculo = voltar (dispara Escape). Analógico direito (vertical) = rolar a área de conteúdo
 * (ver `padScroll.ts`); não move o foco.
 */
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { play } from "./sound";
import { NINTENDO_PAD, setActivePad, setDetected, useInputDetection } from "./inputDevice";
import { cfgForDevice, gamepadKeys, type PadCfg } from "./controllers";
import { applyDeadzone, refocusAfterScroll, setScrollInput, setScrollInputX } from "./padScroll";
import { idleSwallow } from "./idle";

type Dir = "left" | "right" | "up" | "down";
let lastMove = 0;
// [data-no-nav]: campo só para o mouse, fora da navegação por controle/teclado (ex.: linha do tempo dos vídeos)
export const SEL = "[data-focus]:not([disabled]), input:not([type=hidden]):not([disabled]):not([data-no-nav]), select:not([disabled]), textarea:not([disabled])";
/**
 * Camadas que isolam a navegação (a mais recente no DOM vence): modal, menu lateral, lista de seleção,
 * teclado virtual, menu de contexto e Gameboot. Fonte única: padScroll e o foco entre páginas usam a mesma.
 */
export const LAYER_SEL = ".overlay:not(.leaving), .sm-root:not(.leaving), .pick-root, .kb-root, .ctx-root, .gb-root, .trl-full";

/** Troca de página em andamento: o foco inicial é do gerenciador de páginas (lib/pageFocus.ts). */
let pagePending = 0;
export const setPageFocusPending = (on: boolean) => { pagePending = on ? performance.now() : 0; };
const pageFocusPending = () => pagePending > 0 && performance.now() - pagePending < 3500;

export const visible = (el: HTMLElement) => {
  const r = el.getBoundingClientRect();
  if (r.width <= 0 || r.height <= 0 || el.closest("[inert]")) return false;
  return el.checkVisibility ? el.checkVisibility({ visibilityProperty: true } as CheckVisibilityOptions) : true;
};

interface Box { left: number; right: number; top: number; bottom: number; cx: number; cy: number }

/**
 * Retângulo de LAYOUT do elemento: tamanho sem transform (offsetWidth/Height) em volta do centro visual.
 * Assim a escala do foco (capas +7%), o "erguer" e a inclinação 3D não fazem o elemento invadir os
 * vizinhos — antes isso descartava candidatos válidos e o foco pulava para longe.
 */
function boxOf(el: HTMLElement): Box {
  const b = el.getBoundingClientRect();
  const w = el.offsetWidth || b.width, h = el.offsetHeight || b.height;
  const cx = b.left + b.width / 2, cy = b.top + b.height / 2;
  return { left: cx - w / 2, right: cx + w / 2, top: cy - h / 2, bottom: cy + h / 2, cx, cy };
}

const OPPOSITE: Record<Dir, Dir> = { left: "right", right: "left", up: "down", down: "up" };
/** Último movimento: voltar na direção oposta retorna ao elemento de origem (caminho previsível). */
let lastHop: { from: HTMLElement; to: HTMLElement; dir: Dir } | null = null;

/** Raiz de navegação: o menu lateral / modal mais recente isola os comandos do que está atrás. */
export function navRoot(): ParentNode {
  const layers = document.querySelectorAll<HTMLElement>(LAYER_SEL);
  return layers.length ? layers[layers.length - 1] : document;
}

/** Elementos navegáveis e visíveis da camada. */
export const candidates = (root: ParentNode = navRoot()) => Array.from(root.querySelectorAll<HTMLElement>(SEL)).filter(visible);

/* ------------------------------------------------------------------------------------------------
 * Memória de foco por camada: último elemento focado em cada camada (página, modal, menu...). Se o foco some
 *    (elemento removido, clique no vazio), a navegação recomeça perto dele, nunca do 1º item da tela.
 * ---------------------------------------------------------------------------------------------- */
type Spot = { el: HTMLElement; x: number; y: number };
const lastIn = new WeakMap<object, Spot>();
/** Esquece o último foco da camada (ex.: troca de página: a posição antiga não vale para a nova). */
export const forgetLayerFocus = (root: ParentNode = document) => { lastIn.delete(root); };

/** O foco se perdeu porque o elemento focado foi removido da tela? */
export function focusLost(): boolean {
  const a = document.activeElement;
  if (a && a !== document.body && a.isConnected) return false;
  const s = lastIn.get(navRoot());
  return !!s && !s.el.isConnected;
}

/** Item visível mais próximo de onde o foco estava na camada (ou o 1º, sem referência). */
export function recoverFocusTarget(root: ParentNode = navRoot(), els = candidates(root)): HTMLElement | null {
  if (!els.length) return null;
  const s = lastIn.get(root);
  // sem referência: o item principal da tela, senão o 1º do conteúdo (não o da barra superior)
  if (!s) return els.find((el) => el.hasAttribute("data-autofocus")) ?? els.find((el) => el.closest(".main")) ?? els[0];
  if (s.el.isConnected && els.includes(s.el)) return s.el;
  let best = els[0], bd = Infinity;
  for (const el of els) {
    const b = boxOf(el);
    const d = Math.hypot(b.cx - s.x, (b.cy - s.y) * 1.4); // desvio vertical pesa mais (mesma linha primeiro)
    if (d < bd) { bd = d; best = el; }
  }
  return best;
}

/** Liga o rastreamento de foco (memória por camada/grupo e trava na camada ativa). Devolve a limpeza. */
export function trackFocus() {
  document.addEventListener("focusin", onFocusIn, true);
  return () => document.removeEventListener("focusin", onFocusIn, true);
}

function onFocusIn(e: FocusEvent) {
  const el = e.target;
  if (!(el instanceof HTMLElement)) return;
  const root = navRoot();
  // Prende o foco na camada ativa: Tab, clique ou um foco programático atrasado não "vazam" para trás
  // do modal/menu (antes o controle passava a mover um foco invisível na página de fundo).
  if (root !== document && !(root as Element).contains(el)) {
    const back = lastIn.get(root)?.el;
    const t = back && back.isConnected && (root as Element).contains(back) && visible(back) ? back : candidates(root)[0];
    if (t && t !== el) { t.focus({ preventScroll: true }); return; }
  }
  const b = el.getBoundingClientRect();
  lastIn.set(root, { el, x: b.left + b.width / 2, y: b.top + b.height / 2 });
}

/* ------------------------------------------------------------------------------------------------
 * Próximo foco: o item MAIS PRÓXIMO na direção pedida (mesmo critério do Android/TV, FocusFinder).
 *
 *  1. Só valem itens realmente naquela direção (`isCandidate`): a borda de trás não fica atrás da
 *     do item atual e a borda da frente passa dela.
 *  2. Quem está no "feixe" (alinhado: se sobrepõe na mesma linha para ←/→ ou na mesma coluna para
 *     ↑/↓) vence quem não está. Exceção, só em ↑/↓: um item fora do feixe que fica INTEIRO antes do
 *     alinhado (ex.: a linha logo abaixo, levemente deslocada) é o mais próximo e vence.
 *  3. Empate de categoria: menor 13·avanço² + desvio² — avançar pouco pesa mais que estar centralizado,
 *     então o vizinho imediato ganha de um item distante só "mais centralizado".
 *
 * Única restrição por área: ↑/↓ não salta para OUTRO painel ([data-nav-group]) na diagonal — só se
 * estiver alinhado na mesma coluna (descer no fim do conteúdo não cai no menu lateral).
 * ---------------------------------------------------------------------------------------------- */
const PANEL = "[data-nav-group]:not(.seg)";
const EPS = 1;

function isCandidate(s: Box, d: Box, dir: Dir) {
  switch (dir) {
    case "left": return (s.right - EPS > d.right || s.left + EPS >= d.right) && s.left - EPS > d.left;
    case "right": return (s.left + EPS < d.left || s.right - EPS <= d.left) && s.right + EPS < d.right;
    case "up": return (s.bottom - EPS > d.bottom || s.top + EPS >= d.bottom) && s.top - EPS > d.top;
    case "down": return (s.top + EPS < d.top || s.bottom - EPS <= d.top) && s.bottom + EPS < d.bottom;
  }
}
const horizontal = (dir: Dir) => dir === "left" || dir === "right";
/** Sobreposição no eixo perpendicular (mesma linha para ←/→, mesma coluna para ↑/↓). */
function inBeam(s: Box, d: Box, dir: Dir) {
  return horizontal(dir)
    ? Math.min(s.bottom, d.bottom) - Math.max(s.top, d.top) > EPS
    : Math.min(s.right, d.right) - Math.max(s.left, d.left) > EPS;
}
/** `d` está inteiramente adiante de `s` (sem sobreposição no eixo do movimento). */
function fullyAhead(s: Box, d: Box, dir: Dir) {
  switch (dir) {
    case "left": return s.left >= d.right - EPS;
    case "right": return s.right <= d.left + EPS;
    case "up": return s.top >= d.bottom - EPS;
    case "down": return s.bottom <= d.top + EPS;
  }
}
/** Distância da borda de `s` até a borda mais próxima de `d`, no eixo do movimento. */
function major(s: Box, d: Box, dir: Dir) {
  return Math.max(0, dir === "right" ? d.left - s.right : dir === "left" ? s.left - d.right : dir === "down" ? d.top - s.bottom : s.top - d.bottom);
}
/** Distância até a borda MAIS DISTANTE de `d` (mín. 1). */
function majorFar(s: Box, d: Box, dir: Dir) {
  return Math.max(1, dir === "right" ? d.right - s.right : dir === "left" ? s.left - d.left : dir === "down" ? d.bottom - s.bottom : s.top - d.top);
}
const minor = (s: Box, d: Box, dir: Dir) => Math.abs(horizontal(dir) ? d.cy - s.cy : d.cx - s.cx);
const score = (s: Box, d: Box, dir: Dir) => { const m = major(s, d, dir), n = minor(s, d, dir); return 13 * m * m + n * n; };

/** `r1` (no feixe) ganha de `r2` (fora dele)? */
function beamBeats(s: Box, r1: Box, r2: Box, dir: Dir) {
  if (inBeam(s, r2, dir) || !inBeam(s, r1, dir)) return false;
  if (!fullyAhead(s, r2, dir)) return true;
  if (horizontal(dir)) return true;
  return major(s, r1, dir) < majorFar(s, r2, dir);
}
function better(s: Box, r1: Box, r2: Box | null, dir: Dir) {
  if (!r2) return true;
  if (beamBeats(s, r1, r2, dir)) return true;
  if (beamBeats(s, r2, r1, dir)) return false;
  return score(s, r1, dir) < score(s, r2, dir);
}

/** Item mais próximo de `cur` na direção `dir` entre `els` (ou null se não houver nada para lá). */
export function pickNext(cur: HTMLElement, els: HTMLElement[], dir: Dir): HTMLElement | null {
  const s = boxOf(cur);
  const vertical = !horizontal(dir);
  let best: HTMLElement | null = null, bb: Box | null = null;
  for (const el of els) {
    if (el === cur) continue;
    const d = boxOf(el);
    if (!isCandidate(s, d, dir)) continue;
    if (vertical) {
      const p = el.closest(PANEL);
      if (p && !p.contains(cur) && !inBeam(s, d, dir)) continue;
    }
    if (better(s, d, bb, dir)) { best = el; bb = d; }
  }
  return best;
}

/**
 * Move o foco na direção pedida. `source`: "pad" (controle) ignora os elementos com [data-pad-skip]
 * (ex.: as abas da barra superior, trocadas só com L1/R1 ou o mouse); "key" (setas do teclado) alcança todos.
 */
export function moveFocus(dir: Dir, source: "pad" | "key" = "key") {
  // Uma tela pode usar as direções para outra coisa (ex.: mover a imagem no editor de enquadramento):
  // quem tratar chama preventDefault() e o foco não se move.
  const claim = new CustomEvent<Dir>("nexstage:direction", { detail: dir, cancelable: true });
  window.dispatchEvent(claim);
  if (claim.defaultPrevented) return;
  // Visualizador de imagens — só quando ele é a camada ativa (com um menu ou confirmação por cima, ex.:
  // "Excluir captura", as setas navegam nos botões dela):
  // - capturas (data-viewer="vertical", ações no rodapé): cima avança e baixo volta a imagem; esquerda/direita
  //   andam entre os botões do rodapé (movimento normal de foco);
  // - demais (ex.: tela cheia do IGDB, sem botões): esquerda/direita trocam de imagem.
  const root = navRoot();
  if (root instanceof Element && root.matches("[data-viewer]")) {
    const vertical = root.getAttribute("data-viewer") === "vertical";
    if (vertical ? dir === "up" || dir === "down" : dir === "left" || dir === "right") {
      window.dispatchEvent(new CustomEvent("nexstage:viewer", { detail: dir === "up" || dir === "right" ? 1 : -1 }));
      return;
    }
  }
  // Barra deslizante em foco: esquerda/direita (controle) ajustam o valor em vez de mover o foco.
  const act = document.activeElement;
  if ((dir === "left" || dir === "right") && act instanceof HTMLInputElement && act.type === "range" && !act.disabled) {
    const before = act.value;
    if (dir === "left") act.stepDown(); else act.stepUp();
    if (act.value !== before) {
      // setter nativo + evento "input": o React só percebe a mudança assim
      const v = act.value;
      act.value = before;
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(act, v);
      act.dispatchEvent(new Event("input", { bubbles: true }));
    }
    return;
  }
  // Elemento com [data-nav-capture] (ex.: quadro de cor) pode usar a direção: recebe "navcapture" e,
  // se chamar preventDefault(), o foco não se move (nas bordas ele deixa passar e o foco sai normalmente).
  if (act instanceof HTMLElement && act.hasAttribute("data-nav-capture")) {
    const ev = new CustomEvent<Dir>("navcapture", { detail: dir, cancelable: true });
    act.dispatchEvent(ev);
    if (ev.defaultPrevented) return;
  }
  const all = candidates(root);
  // Controle: candidatos sem os [data-pad-skip]. A origem (`cur`) pode ser um deles (ex.: aba clicada
  // com o mouse) — a geometria parte dela, mas o foco nunca vai para outro.
  const els = source === "pad" ? all.filter((el) => !el.hasAttribute("data-pad-skip")) : all;
  if (!els.length) return;
  const cur = document.activeElement as HTMLElement | null;
  if (!cur || !all.includes(cur)) {
    // Sem foco válido (item removido, clique no vazio, camada recém-aberta): recomeça perto de onde estava.
    const t = recoverFocusTarget(root, els) ?? els[0];
    lastHop = null;
    t.focus({ preventScroll: true });
    t.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "auto" });
    play("move");
    return;
  }
  // Depois de rolar com o analógico direito, recomeça pelo que está à vista (não volta ao foco antigo).
  const anchor = refocusAfterScroll(cur, els, dir);
  if (anchor) {
    lastHop = null;
    anchor.focus({ preventScroll: true });
    anchor.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "auto" });
    play("move");
    return;
  }
  // Voltar pelo mesmo caminho: → depois ← retorna exatamente ao elemento de onde saiu.
  let best: HTMLElement | null = null;
  if (lastHop && lastHop.to === cur && lastHop.dir === OPPOSITE[dir] && els.includes(lastHop.from) &&
      isCandidate(boxOf(cur), boxOf(lastHop.from), dir)) best = lastHop.from;
  best ??= pickNext(cur, els, dir);
  if (best) {
    lastHop = { from: cur, to: best, dir };
    best.focus();
    play("move");
    const now = performance.now();
    const fast = now - lastMove < 160;
    lastMove = now;
    best.scrollIntoView({ block: "nearest", inline: inHorizontalScroller(best) ? "center" : "nearest", behavior: fast ? "auto" : "smooth" });
  }
}

/** O elemento está dentro de um carrossel que rola na horizontal (ex.: "Volte a jogar")? */
function inHorizontalScroller(el: HTMLElement): boolean {
  for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
    const ox = getComputedStyle(p).overflowX;
    if ((ox === "auto" || ox === "scroll") && p.scrollWidth > p.clientWidth + 1) return true;
  }
  return false;
}

/**
 * Camadas (lista de filtro, modal) que acabaram de fechar: um "voltar" que chega logo depois (evento
 * repetido do controle) não pode atravessar para a página e levar à Home.
 */
let layerClosedAt = 0;
export const markLayerClosed = () => { layerClosedAt = performance.now(); };
export const layerJustClosed = (ms = 350) => performance.now() - layerClosedAt < ms;

const CONFIRM_FALLBACK = () => {
  const cur = document.activeElement as HTMLElement | null;
  const root = navRoot();
  // Só aciona o que está na camada ativa (ex.: com o Gameboot na tela, nada atrás dele reage).
  if (cur && cur !== document.body && (root === document || root.contains(cur))) cur.click();
  else if (root !== document && root instanceof Element && root.matches(".gb-root")) return;
  else recoverFocusTarget(root)?.focus();
};

export type PadAction = "left" | "right" | "up" | "down" | "confirm" | "back" | "x" | "y" | "l1" | "r1" | "l2" | "r2" | "menu"
  /** Confirmar segurado (0,5 s) e solto: só para itens com [data-hold] (ver `handle`). */
  | "confirm_hold" | "confirm_up"
  /** Voltar segurado (0,6 s): `nexstage:back-hold` na janela (ex.: cancelar o navegador de arquivos). */
  | "back_hold";
/** Botões que uma tela pode tomar para si (teclado virtual, filtros da biblioteca...). */
export type PadButton = "x" | "y" | "l1" | "r1" | "l2" | "r2" | "menu";

/**
 * Atalho de botão do controle (X/□, LB/L1, RB/R1, LT/L2, RT/R2, Start/Options = "menu"). Quem tratar o
 * botão chama `preventDefault()` e o comportamento padrão (ex.: L1/R1 trocam de aba; Start abre o menu de
 * opções da tela) não acontece.
 * O ouvinte mais recente (ex.: teclado virtual aberto) roda primeiro se registrado com `capture`.
 */
export function onPadButton(fn: (b: PadButton, e: CustomEvent<PadButton>) => void, capture = false) {
  const h = (e: Event) => fn((e as CustomEvent<PadButton>).detail, e as CustomEvent<PadButton>);
  window.addEventListener("nexstage:button", h, capture);
  return () => window.removeEventListener("nexstage:button", h, capture);
}


/**
 * Um comando por toque. Programas como o Steam (configuração de controle "Área de trabalho"), DS4Windows
 * ou JoyToKey também transformam o controle em teclado: B/○ vira ESC, A/✕ vira Enter, o direcional vira
 * as setas. Sem isto, um toque em "voltar" chegava duas vezes (controle + ESC) e pulava um nível
 * (ex.: da página do controle direto para a Home). Vale o que chegar primeiro; o equivalente que vier
 * logo depois, pelo outro caminho, é descartado. Teclado sozinho ou controle sozinho não são afetados.
 */
const DUP_MS = 300;
const KEY_ACTION: Record<string, PadAction> = {
  Escape: "back", Enter: "confirm", ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down",
};
const lastPad = new Map<PadAction, number>();
const lastKey = new Map<PadAction, number>();
if (typeof window !== "undefined") {
  // fase de captura, registrado ao carregar o módulo: roda antes de qualquer outro ouvinte de teclado
  window.addEventListener("keydown", (e) => {
    const a = e.isTrusted ? KEY_ACTION[e.key] : undefined;
    if (!a) return;
    const now = performance.now();
    if (now - (lastPad.get(a) ?? -Infinity) < DUP_MS) {
      e.stopImmediatePropagation();
      e.preventDefault();
      return;
    }
    lastKey.set(a, now);
  }, true);
}
/**
 * Navegar com WASD (Configurações → Controles e atalhos, `app_settings.wasd_nav`): W/A/S/D viram as setas.
 * Fora de campos de texto e sem Ctrl/Alt/Win. A tecla é trocada por uma seta sintética no mesmo alvo, então
 * vale tudo o que já responde às setas (foco, carrosséis, listas, navegador de arquivos...).
 * Usa a POSIÇÃO da tecla (e.code): em teclados AZERTY, Z/Q/S/D.
 */
const WASD: Record<string, string> = { KeyW: "ArrowUp", KeyA: "ArrowLeft", KeyS: "ArrowDown", KeyD: "ArrowRight" };
let wasdOn = false;
export function setWasdNav(on: boolean) { wasdOn = on; }
export const loadWasdNav = () => api.getAppSetting("wasd_nav").then((v) => { wasdOn = v === "1"; }).catch(() => {});
const typingIn = (el: EventTarget | null) =>
  el instanceof HTMLElement && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName));
if (typeof window !== "undefined") {
  for (const type of ["keydown", "keyup"] as const) {
    window.addEventListener(type, (e) => {
      const arrow = WASD[e.code];
      if (!wasdOn || !arrow || !e.isTrusted || e.ctrlKey || e.altKey || e.metaKey || typingIn(e.target)) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      (e.target ?? window).dispatchEvent(new KeyboardEvent(type, {
        key: arrow, code: arrow, repeat: e.repeat, shiftKey: e.shiftKey, bubbles: true, cancelable: true,
      }));
    }, true);
  }
}

/** `false` = o mesmo comando acabou de chegar pelo teclado (controle convertido em teclas): ignora. */
function firstOfPair(action: PadAction): boolean {
  const now = performance.now();
  if (now - (lastKey.get(action) ?? -Infinity) < DUP_MS) return false;
  lastPad.set(action, now);
  return true;
}

/**
 * Itens com [data-hold] aceitam "tocar" e "segurar" Ⓐ: ao apertar, o clique espera — soltar antes de
 * 0,5 s clica normalmente; segurar dispara `nexstage:hold` no item (ex.: abrir a seleção de vários no Zerei!).
 */
let holdPending: HTMLElement | null = null;

function handle(action: PadAction) {
  if (action === "confirm_hold" || action === "confirm_up") {
    const el = holdPending;
    holdPending = null;
    if (!el || !el.isConnected || document.activeElement !== el) return;
    if (action === "confirm_hold") el.dispatchEvent(new CustomEvent("nexstage:hold", { bubbles: true }));
    else el.click();
    return;
  }
  if (action === "back_hold") {
    window.dispatchEvent(new CustomEvent("nexstage:back-hold", { cancelable: true }));
    return;
  }
  // Modo Ocioso ativo (ou acabou de ser encerrado por este comando): o comando só o encerra.
  if (idleSwallow()) return;
  if (!firstOfPair(action)) return;
  if (action === "x" || action === "y" || action === "l1" || action === "r1" || action === "l2" || action === "r2" || action === "menu") {
    const ev = new CustomEvent<PadButton>("nexstage:button", { detail: action, cancelable: true });
    window.dispatchEvent(ev);
    if (ev.defaultPrevented) return;
  }
  if (action === "left" || action === "right" || action === "up" || action === "down") moveFocus(action, "pad");
  else if (action === "confirm") {
    const cur = document.activeElement;
    const root = navRoot();
    if (cur instanceof HTMLElement && cur.hasAttribute("data-hold") && (root === document || root.contains(cur))) { holdPending = cur; return; }
    CONFIRM_FALLBACK();
  }
  else if (action === "back") {
    // Igual a um ESC de verdade: nasce no elemento em foco e percorre captura → alvo → borbulha. Disparado
    // direto na janela, os ouvintes de captura das páginas (ex.: página de um controle → volta à coleção)
    // não rodavam antes do "voltar para a Home" do app.
    const target = document.activeElement instanceof HTMLElement && document.activeElement.isConnected ? document.activeElement : document.body;
    target.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  }
  else if (action === "menu") window.dispatchEvent(new CustomEvent("nexstage:context"));
  else if (action === "l1") window.dispatchEvent(new CustomEvent("nexstage:tab", { detail: -1 }));
  else if (action === "r1") window.dispatchEvent(new CustomEvent("nexstage:tab", { detail: 1 }));
}

/**
 * Teclado virtual aberto: segurar confirmar, X, Y, L1 ou R1 repete o comando (digitar, apagar, espaço,
 * mover o cursor) no mesmo ritmo do direcional. Liga a repetição também nos controles nativos (backend).
 * Fora do teclado, os botões disparam uma vez por toque.
 */
let buttonRepeat = 0;
const REPEATABLE = new Set<PadAction>(["confirm", "x", "y", "l1", "r1"]);
export function holdToRepeat(): () => void {
  if (buttonRepeat++ === 0) api.setPadButtonRepeat(true).catch(() => {});
  return () => { if (--buttonRepeat === 0) api.setPadButtonRepeat(false).catch(() => {}); };
}

/** Testador da Central de Controles: pausa a navegação pelos controles genéricos. */
let padMonitorOn = false;
export const setPadMonitorNav = (on: boolean) => { padMonitorOn = on; };


/**
 * Entrada de navegação:
 *  - teclado: setas movem o foco (repetição do próprio sistema); Enter/Espaço ativam o botão focado;
 *  - Xbox/PlayStation: eventos `pad-input` enviados pelo backend (1 movimento por toque, atraso
 *    de 380 ms e repetição a cada 95 ms enquanto segurado);
 *  - outros controles: Gamepad API com a mesma lógica, para controles genéricos (DirectInput).
 */
export function useSpatialNav() {
  useInputDetection();

  useEffect(() => {
    const keys: Record<string, Dir> = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" };
    const onKey = (e: KeyboardEvent) => {
      if (["INPUT", "TEXTAREA", "SELECT"].includes((e.target as HTMLElement).tagName)) return;
      const d = keys[e.key];
      if (d) {
        e.preventDefault();
        moveFocus(d);
      }
    };
    const onClick = (e: MouseEvent) => {
      if ((e.target as HTMLElement).closest?.("[data-focus]")) play("confirm");
    };
    window.addEventListener("keydown", onKey);
    document.addEventListener("click", onClick, true);
    loadWasdNav();
    const untrack = trackFocus();

    // Controles nativos
    let un: (() => void) | undefined;
    let cancelled = false;
    listen<{ action: PadAction; device: "xbox" | "playstation" | "nintendo"; key?: string }>("pad-input", (e) => {
      setActivePad(e.payload.key ?? null);
      setDetected(e.payload.device);
      handle(e.payload.action);
    }).then((f) => (cancelled ? f() : (un = f)));
    // Analógico direito (zona morta já aplicada pelo backend): vertical rola a página, horizontal os carrosséis
    let unScroll: (() => void) | undefined;
    listen<{ value: number; x?: number; device: "xbox" | "playstation" | "nintendo" }>("pad-scroll", (e) => {
      const x = e.payload.x ?? 0;
      if (e.payload.value !== 0 || x !== 0) setDetected(e.payload.device);
      const swallow = (e.payload.value !== 0 || x !== 0) && idleSwallow();
      setScrollInput("native", swallow ? 0 : e.payload.value);
      setScrollInputX("native", swallow ? 0 : x);
    }).then((f) => (cancelled ? f() : (unScroll = f)));

    // Fallback: controles genéricos
    const held: Record<Dir, boolean> = { left: false, right: false, up: false, down: false };
    const nextAt: Record<Dir, number> = { left: 0, right: 0, up: 0, down: 0 };
    const btn: boolean[] = [];
    let holdAt = 0;
    let backAt = 0; // voltar segurado (ver "back_hold")
    const btnNext: number[] = [];
    /**
     * Eixos que já passaram pelo centro, por controle. Um eixo só navega depois de ter sido visto em
     * repouso: alguns dispositivos (volantes, adaptadores, controles virtuais ou sem mapeamento padrão)
     * têm um eixo parado em -1/+1, que antes contava como "analógico empurrado" — o foco subia sozinho.
     */
    const centered = new Map<string, Set<number>>();
    let raf = 0;
    let lostCheck = 0;
    const poll = (t: number) => {
      // Item focado removido da tela (excluído, lista recarregada, seção recolhida): o foco vai para o
      // vizinho mais próximo na hora, em vez de sumir até o próximo comando.
      if (t - lostCheck > 200) {
        lostCheck = t;
        if (focusLost() && !pageFocusPending()) recoverFocusTarget()?.focus({ preventScroll: true });
      }
      // Testador da Central de Controles aberto: os controles genéricos não navegam (ele lê a Gamepad API).
      // Tudo conta como "segurado" até ser solto: sair do teste segurando B não dispara um "voltar".
      if (padMonitorOn) {
        btn.fill(true, 0, 9);
        for (const d of ["left", "right", "up", "down"] as Dir[]) { held[d] = true; nextAt[d] = t + 380; }
        raf = requestAnimationFrame(poll);
        return;
      }
      // Controles genéricos (os lidos nativamente — XInput/HID — ficam de fora para não duplicar), cada um
      // com a própria chave: dois controles iguais não compartilham cadastro (ver `gamepadKeys`).
      const keys = gamepadKeys();
      const pads = [...keys.keys()];
      // Ajustes individuais de cada controle (zona morta e mapeamento; só valem dentro do NEXSTAGE).
      const cfg = new Map<Gamepad, PadCfg>(pads.map((p) => [p, cfgForDevice(keys.get(p)!)]));
      // Tipo do controle genérico ativo (glifos e indicadores L/R da barra de páginas).
      const padKind = pads.some((p) => NINTENDO_PAD.test(p.id)) ? "nintendo" : "generic";
      // Analógico direito (eixos 2 e 3 no mapeamento padrão): vertical rola a página, horizontal os
      // carrosséis; vale o controle mais inclinado.
      let ry = 0, rx = 0;
      if (document.hasFocus()) {
        for (const p of pads) {
          const v = applyDeadzone(p.axes[3] ?? 0, cfg.get(p)!.dzRight);
          if (Math.abs(v) > Math.abs(ry)) ry = v;
          // eixo horizontal só depois de visto em repouso (como os do analógico esquerdo): alguns dispositivos
          // têm um eixo 2 parado em -1/+1, que faria os carrosséis rolarem sozinhos
          const h = centered.get(keys.get(p)!)?.has(2) ? applyDeadzone(p.axes[2] ?? 0, cfg.get(p)!.dzRight) : 0;
          if (Math.abs(h) > Math.abs(rx)) rx = h;
        }
      }
      if (ry !== 0 || rx !== 0) setDetected(padKind);
      const swallowR = (ry !== 0 || rx !== 0) && idleSwallow();
      setScrollInput("generic", swallowR ? 0 : ry);
      setScrollInputX("generic", swallowR ? 0 : rx);
      if (pads.length) {
        const pressed = (i: number) => pads.some((p) => p.buttons[i]?.pressed);
        for (const p of pads) {
          const k = keys.get(p)!;
          const set = centered.get(k) ?? new Set<number>();
          centered.set(k, set);
          for (const a of [0, 1, 2]) if (Math.abs(p.axes[a] ?? 0) < Math.min(0.25, cfg.get(p)!.dzLeft)) set.add(a);
        }
        const axis = (a: number, neg: boolean, was: boolean) =>
          pads.some((p) => {
            if (!centered.get(keys.get(p)!)?.has(a)) return false; // eixo nunca visto em repouso: ignorado
            const v = p.axes[a] ?? 0;
            const dz = cfg.get(p)!.dzLeft;
            const th = dz + (1 - dz) * 0.5; // mesmo limiar dos controles nativos (input::nav_threshold)
            const on = was ? Math.max(dz, th * 0.55) : th;
            return neg ? v < -on : v > on;
          });
        const now: Record<Dir, boolean> = {
          left: pressed(14) || axis(0, true, held.left),
          right: pressed(15) || axis(0, false, held.right),
          up: pressed(12) || axis(1, true, held.up),
          down: pressed(13) || axis(1, false, held.down),
        };
        for (const d of ["left", "right", "up", "down"] as Dir[]) {
          if (now[d] && !held[d]) {
            setDetected(padKind);
            handle(d);
            nextAt[d] = t + 380;
          } else if (now[d] && t >= nextAt[d]) {
            handle(d);
            nextAt[d] += 95;
            if (nextAt[d] < t) nextAt[d] = t + 95;
          }
          held[d] = now[d];
        }
        // Ação → botão de cada controle (bit da máscara = índice do mapeamento padrão da Gamepad API).
        const ACTS: PadAction[] = ["confirm", "back", "x", "y", "l1", "r1", "l2", "r2", "menu"];
        ACTS.forEach((action, ai) => {
          const on = pads.some((p) => p.buttons[Math.log2(cfg.get(p)!.map[ai])]?.pressed);
          // confirmar: segurou 0,5 s → "confirm_hold"; soltou → "confirm_up" (fora do teclado virtual)
          if (ai === 0 && buttonRepeat === 0) {
            if (on && btn[0] && holdAt > 0 && t >= holdAt) { holdAt = 0; handle("confirm_hold"); }
            else if (!on && btn[0]) { holdAt = 0; handle("confirm_up"); }
            else if (on && !btn[0]) holdAt = t + 500;
          }
          // voltar: segurou 0,6 s → "back_hold" (o toque já disparou "back" ao apertar)
          if (ai === 1) {
            if (on && btn[1] && backAt > 0 && t >= backAt) { backAt = 0; handle("back_hold"); }
            else if (!on) backAt = 0;
            else if (on && !btn[1]) backAt = t + 600;
          }
          let fire = on && !btn[ai];
          if (fire) btnNext[ai] = t + 380;
          else if (on && buttonRepeat > 0 && REPEATABLE.has(action) && t >= (btnNext[ai] ?? Infinity)) {
            fire = true; // segurando, com o teclado virtual aberto
            btnNext[ai] = t + 95;
          }
          btn[ai] = on;
          if (!fire) return;
          if (action === "confirm") setDetected(padKind);
          const who = pads.find((p) => p.buttons[Math.log2(cfg.get(p)!.map[ai])]?.pressed);
          if (who) setActivePad(keys.get(who) ?? null);
          handle(action);
        });
      }
      raf = requestAnimationFrame(poll);
    };
    raf = requestAnimationFrame(poll);

    return () => {
      cancelled = true;
      un?.();
      unScroll?.();
      setScrollInput("native", 0);
      setScrollInput("generic", 0);
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("click", onClick, true);
      untrack();
      cancelAnimationFrame(raf);
    };
  }, []);
}
