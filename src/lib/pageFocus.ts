/**
 * Foco ao entrar e ao voltar para uma página (usado por components/Transition.tsx).
 *
 * Antes, só algumas telas posicionavam o foco ao abrir. Nas outras (Configurações, Galeria, Perfil,
 * Conquistas, Emuladores...) o item que abriu a página sumia junto com a página anterior, o foco ia
 * para o <body> e o primeiro comando do controle caía no 1º item do DOM (barra superior), longe do
 * conteúdo. Agora, ao abrir uma página:
 *  1. se a própria página já focou algo (ex.: "Jogar" na página do jogo), nada muda;
 *  2. senão, volta ao item onde o foco estava na última visita (memória por página, por perfil);
 *  3. senão, o item marcado com [data-autofocus] (o principal da tela);
 *  4. senão, o primeiro item visível do conteúdo.
 * Enquanto um modal/menu estiver aberto por cima, espera; o conteúdo carregado depois (listas, grades)
 * ainda pode receber o foco certo, desde que o usuário não o tenha movido.
 */
import { candidates, forgetLayerFocus, navRoot, SEL, setPageFocusPending, visible } from "./nav";

type Sig = { key: string; index: number };
const mem = new Map<string, Sig>();
let fromMemory = false;

/** Troca de perfil: a posição de cada página começa do zero. */
export const resetPageFocus = () => { mem.clear(); };
/** A página atual teve o foco restaurado da visita anterior (telas com foco próprio podem respeitar). */
export const pageFocusFromMemory = () => fromMemory;
export const hasPageMemory = (id: string) => mem.has(id);

/** Identidade estável de um item entre visitas (o elemento em si é recriado). */
function keyOf(el: HTMLElement): string {
  const d = el.dataset;
  const label = el.getAttribute("aria-label") ?? "";
  const text = label ? "" : (el.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 60);
  // ids de dados (data-game-id, data-capture-id...) e data-focus-key identificam itens de listas e grades
  const ids = Object.keys(d).filter((k) => k === "focusKey" || /Id$/.test(k)).sort().map((k) => `${k}=${d[k]}`).join(",");
  return [el.tagName, el.id, ids, label, text, el.className.split(" ")[0] ?? ""].join("|");
}

const inPage = (page: HTMLElement) => candidates(document).filter((el) => page.contains(el));

function find(page: HTMLElement, sig: Sig): HTMLElement | null {
  for (const el of page.querySelectorAll<HTMLElement>(SEL)) if (keyOf(el) === sig.key && visible(el)) return el;
  return null;
}

const onScreen = (el: HTMLElement) => {
  const r = el.getBoundingClientRect();
  return r.bottom > 0 && r.top < innerHeight;
};

/** Liga o gerenciador a uma página recém-montada. Devolve a limpeza. */
export function attachPageFocus(id: string, page: HTMLElement): () => void {
  forgetLayerFocus(document);
  setPageFocusPending(true);
  fromMemory = false;
  const saved = mem.get(id);
  const start = performance.now();
  let placed: HTMLElement | null = null; // foco provisório (pode ser trocado pelo item certo quando carregar)
  let placedRank = 0; // 3 = memória, 2 = autofocus, 1 = primeiro item
  let timer = 0;
  let done = false;
  const later = () => { timer = window.setTimeout(step, 80); }; // ~12 verificações/s (listas grandes)

  const onIn = (e: FocusEvent) => {
    const el = e.target as HTMLElement;
    if (!page.contains(el)) return;
    const index = inPage(page).indexOf(el);
    mem.set(id, { key: keyOf(el), index });
  };
  page.addEventListener("focusin", onIn);

  const put = (el: HTMLElement, rank: number) => {
    placed = el;
    placedRank = rank;
    fromMemory = rank === 3;
    el.focus({ preventScroll: true });
    el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "auto" });
  };

  const step = () => {
    timer = 0;
    if (done || !page.isConnected) return;
    const elapsed = performance.now() - start;
    const a = document.activeElement as HTMLElement | null;
    const free = !a || a === document.body || !a.isConnected;
    // A página (ou o usuário) já escolheu outro foco: terminou.
    if (!free && a !== placed && page.contains(a)) return finish();
    // Foco em outra área (barra superior clicada com o mouse...) ou camada por cima: não rouba.
    if (!free && a !== placed) {
      if (navRoot() !== document && elapsed < 3000) { later(); return; }
      return finish();
    }
    if (navRoot() === document) {
      const memEl = saved ? find(page, saved) : null;
      if (memEl) { if (placedRank < 3) put(memEl, 3); return finish(); }
      const auto = page.querySelector<HTMLElement>("[data-autofocus]");
      const autoOk = auto && inPage(page).includes(auto);
      // dá um tempo para a memória (conteúdo assíncrono) aparecer antes de usar o padrão da tela
      if (autoOk && placedRank < 2 && (!saved || elapsed > 600)) put(auto, 2);
      if (!placed && elapsed > (saved ? 600 : 250)) {
        const els = inPage(page);
        const first = els.find(onScreen) ?? els[0];
        if (first) put(first, 1);
      }
    }
    if (elapsed < 3000) later();
    else finish();
  };

  const finish = () => {
    done = true;
    setPageFocusPending(false);
    if (timer) clearTimeout(timer);
    timer = 0;
  };

  requestAnimationFrame(() => { if (!timer) step(); });
  return () => {
    finish();
    page.removeEventListener("focusin", onIn);
  };
}
