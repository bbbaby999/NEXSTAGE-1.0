import { useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";
import Modal from "./Modal";
import Button from "./Button";
import { play } from "../lib/sound";
import { formatDateTime } from "../lib/format";
import type { GalleryItem } from "../types";
import { t } from "../lib/i18n";

export interface MenuAction { label: string; icon: string; danger?: boolean; run: () => void }

/** Ícone SVG simples (traço) usado nos controles da Galeria. */
export function Icon({ d, size = 20 }: { d: string; size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d={d} />
    </svg>
  );
}
export const ICON = {
  prev: "M15 5l-7 7 7 7",
  next: "M9 5l7 7-7 7",
  star: "M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z",
  back: "M10 6L4 12l6 6M4 12h16",
  more: "M5 12h.01M12 12h.01M19 12h.01",
  open: "M4 5h16v14H4zM4 15l5-5 4 4 3-3 4 4",
  select: "M5 12l4 4 10-10",
  info: "M12 8h.01M11 12h1v5h1M12 3a9 9 0 100 18 9 9 0 000-18z",
  folder: "M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z",
  trash: "M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3",
  full: "M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5",
  exitFull: "M9 4v5H4M20 9h-5V4M15 20v-5h5M4 15h5v5",
  edit: "M4 20h4L19 9l-4-4L4 16v4zM14 6l4 4",
};

/** Movimento reduzido (sistema ou Configurações): menus aparecem sem a animação de expansão. */
const reducedMotion = () =>
  document.documentElement.dataset.motion === "reduced"
  || (document.documentElement.dataset.motion !== "full" && matchMedia("(prefers-reduced-motion: reduce)").matches);

/** Jogo em destaque ao lado das opções (botão direito / Options sobre uma capa). */
export interface MenuHero { cover: string | null; title: string; sub?: string }

/**
 * Abertura do menu, por RECORTE (clip-path): o painel já está no tamanho e lugar finais e vai sendo
 * descoberto a partir do botão/ponto que o abriu — o recorte começa com a forma do botão (cantos de
 * pílula) e cresce até o painel inteiro. Nada é esticado, então não há distorção nem "pulo" no fim.
 * Os itens entram em cascata logo depois.
 */
const px = (v: string) => parseFloat(v) || 0;
const EASE = "cubic-bezier(.2,.9,.25,1)";
function clipFrom(box: DOMRect, origin: DOMRect | null, radius: number) {
  const w = box.width, h = box.height;
  const cl = (v: number, max: number) => Math.min(Math.max(v, 0), max);
  if (!origin) {
    // sem origem: abre do centro
    return `inset(${h * .42}px ${w * .42}px ${h * .42}px ${w * .42}px round ${radius}px)`;
  }
  const top = cl(origin.top - box.top, h - 1), left = cl(origin.left - box.left, w - 1);
  const right = cl(box.right - origin.right, w - left - 1), bottom = cl(box.bottom - origin.bottom, h - top - 1);
  const r = Math.min(radius, (h - top - bottom) / 2, (w - left - right) / 2);
  return `inset(${top}px ${right}px ${bottom}px ${left}px round ${Math.max(0, r)}px)`;
}
function reveal(panel: HTMLElement, origin: DOMRect | null, startRadius: number | null, dur: number, delay = 0) {
  if (reducedMotion()) return;
  const box = panel.getBoundingClientRect();
  const R = px(getComputedStyle(panel).borderTopLeftRadius);
  // fim com folga negativa: a sombra do painel não fica cortada no último quadro
  panel.animate([{ clipPath: clipFrom(box, origin, startRadius ?? R) }, { clipPath: `inset(-80px round ${R + 80}px)` }],
    { duration: dur, delay, easing: EASE, fill: "backwards" });
  panel.querySelectorAll<HTMLElement>(".ctx-item").forEach((el, i) =>
    el.animate([{ opacity: 0, transform: "translateY(6px)" }, { opacity: 1, transform: "none" }],
      { duration: 220, delay: delay + dur * .4 + i * 30, easing: "ease-out", fill: "backwards" }));
}

/**
 * Menu com o jogo em destaque: a capa clicada fica NO MESMO LUGAR e só cresce (uma cópia dela, por cima do
 * desfoque), o nome aparece logo abaixo e as opções se abrem ao lado, saindo da borda da capa. Tudo cabe na
 * tela: se faltar espaço, a capa se desloca o mínimo necessário e o menu vai para o lado que couber.
 */
function placeHero(root: HTMLElement, o: DOMRect) {
  const cover = root.querySelector<HTMLElement>(".ctx-hero-cover");
  const name = root.querySelector<HTMLElement>(".ctx-hero-name");
  const panel = root.querySelector<HTMLElement>(".ctx-panel");
  if (!cover || !panel) return;
  const M = 16, GAP = 24, NAME = name ? name.getBoundingClientRect().height + 12 : 0;
  const p = panel.getBoundingClientRect();
  const S = Math.max(1, Math.min(1.35, (innerHeight - 2 * M - NAME) / o.height, (innerWidth - 2 * M - GAP - p.width) / o.width));
  const W = o.width * S, H = o.height * S;
  let left = Math.min(Math.max(o.left + o.width / 2 - W / 2, M), innerWidth - M - W);
  const top = Math.min(Math.max(o.top + o.height / 2 - H / 2, M), innerHeight - M - H - NAME);
  // menu à direita; sem espaço, à esquerda; sem espaço dos dois lados, a capa anda para caber à direita
  let right = true;
  if (left + W + GAP + p.width > innerWidth - M) {
    if (left - GAP - p.width >= M) right = false;
    else left = Math.max(M, innerWidth - M - p.width - GAP - W);
  }
  const mx = right ? left + W + GAP : left - GAP - p.width;
  const my = Math.min(Math.max(top, M), innerHeight - M - p.height);
  Object.assign(cover.style, { left: `${left}px`, top: `${top}px`, width: `${W}px`, height: `${H}px` });
  if (name) Object.assign(name.style, { left: `${left - 20}px`, top: `${top + H + 12}px`, width: `${W + 40}px` });
  panel.style.left = `${mx}px`;
  panel.style.top = `${my}px`;
  if (reducedMotion()) return;
  const dur = 380;
  cover.style.transformOrigin = "0 0";
  const grow = cover.animate(
    [{ transform: `translate(${o.left - left}px, ${o.top - top}px) scale(${o.width / W}, ${o.height / H})` }, { transform: "none" }],
    { duration: dur, easing: EASE },
  );
  grow.onfinish = grow.oncancel = () => { cover.style.transformOrigin = ""; };
  name?.animate([{ opacity: 0, transform: "translateY(-6px)" }, { opacity: 1, transform: "none" }],
    { duration: 240, delay: dur * .5, easing: "ease-out", fill: "backwards" });
  // as opções se abrem a partir da borda da capa
  const edge = new DOMRect(right ? mx : mx + p.width - 1, my, 1, p.height);
  reveal(panel, edge, null, 340, dur * .35);
}

/**
 * Menu de contexto (botão direito, tecla de menu, Shift+F10 ou botão Menu/Options do controle).
 * Segue a mesma camada/navegação das listas: foco no primeiro item, Voltar fecha sem executar nada.
 *
 * Posição/animação:
 * - `anchor`: o botão que abre o menu (⋯, "Mais opções"). O próprio botão "se abre": o painel nasce no
 *   lugar e no tamanho dele e cresce revelando as opções (para baixo/direita, ou para onde couber).
 * - `hero` + `from`: a capa clicada (`from`) cresce no lugar, com o nome abaixo e as opções ao lado; o resto
 *   da tela fica desfocado.
 * - `x/y`: no ponto do clique (Galeria, visualizador, campos de texto).
 * - nenhum: centralizado, saindo do elemento em foco.
 */
export function CaptureMenu({ actions, x, y, from, anchor, hero, onClose }: {
  actions: MenuAction[]; x?: number; y?: number; from?: DOMRect | null; anchor?: DOMRect | null; hero?: MenuHero | null; onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const mode = hero && from ? "hero" : anchor ? "anchor" : x !== undefined && y !== undefined ? "at" : "center";
  useLayoutEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const panel = ref.current?.querySelector<HTMLElement>(".ctx-panel");
    if (mode === "hero") {
      if (ref.current) placeHero(ref.current, from!);
    } else if (panel) {
      // posição final com o tamanho real do painel (cabe na tela)
      if (mode === "anchor" || mode === "at") {
        const r = panel.getBoundingClientRect();
        const ax = mode === "anchor" ? anchor! : new DOMRect(x!, y!, 0, 0);
        const left = ax.left + r.width <= innerWidth - 8 ? ax.left : Math.max(8, ax.right - r.width);
        const top = ax.top + r.height <= innerHeight - 8 ? ax.top : Math.max(8, ax.bottom - r.height);
        panel.style.left = `${left}px`;
        panel.style.top = `${top}px`;
      }
      const origin = mode === "anchor" ? anchor!
        : from !== undefined ? from
        : mode === "at" ? new DOMRect(x!, y!, 1, 1) // clique direito: o menu sai do ponto do clique
        : prev && prev !== document.body ? prev.getBoundingClientRect() : null;
      // botão: o recorte começa com os cantos de pílula dele
      reveal(panel, origin, mode === "anchor" && origin ? Math.min(origin.height / 2, 999) : null, 320);
    }
    play("open");
    ref.current?.querySelector<HTMLElement>(".ctx-item")?.focus({ preventScroll: true });
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopImmediatePropagation();
      e.preventDefault();
      play("back");
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => { window.removeEventListener("keydown", onKey, true); play("close"); prev?.focus?.({ preventScroll: true }); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // antes de medir: nasce no ponto/botão (o efeito acima corrige se não couber)
  const style = mode === "anchor" ? { left: anchor!.left, top: anchor!.top }
    : mode === "at" ? { left: x, top: y } : mode === "hero" ? { left: 0, top: 0 } : undefined;
  return createPortal(
    <div className={`ctx-root ${mode === "hero" ? "ctx-blur" : ""}`} ref={ref}>
      <div className="ctx-backdrop" onClick={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }} />
      {mode === "hero" && hero && (
        <>
          <div className="ctx-hero-cover" aria-hidden="true" onClick={onClose}>
            {hero.cover ? <img src={hero.cover} alt="" draggable={false} /> : <span>{hero.title.charAt(0).toUpperCase()}</span>}
          </div>
          <div className="ctx-hero-name" aria-hidden="true"><strong>{hero.title}</strong>{hero.sub && <small>{hero.sub}</small>}</div>
        </>
      )}
      <div className={`ctx-panel ${mode === "center" ? "center" : "at"}`} style={style} role="menu" aria-label={hero?.title}>
        {actions.map((a) => (
          <button key={a.label} type="button" data-focus role="menuitem" className={`ctx-item ${a.danger ? "danger" : ""}`}
            onClick={() => { onClose(); a.run(); }}>
            <Icon d={a.icon} size={18} /><span>{a.label}</span>
          </button>
        ))}
      </div>
    </div>,
    document.body
  );
}

/** "Sobre esta captura": só mostra o que realmente existe. */
export function AboutCapture({ item, onClose }: { item: GalleryItem; onClose: () => void }) {
  const managedName = item.path.split(/[\\/]/).pop() ?? item.path;
  return (
    <Modal title={t("Sobre esta captura")} onClose={onClose}>
      <dl>
        <dt>{t("Nome do arquivo")}</dt><dd className="path">{item.originalName || managedName}</dd>
        <dt>{t("Jogo")}</dt><dd>{item.gameTitle ?? t("Sem jogo associado")}</dd>
        <dt>{t("Data de criação")}</dt><dd>{item.takenAt ? formatDateTime(item.takenAt) : t("Não disponível")}</dd>
        <dt>{t("Importada em")}</dt><dd>{formatDateTime(item.addedAt)}</dd>
        <dt>{t("Armazenada em")}</dt><dd className="path">{item.path}</dd>
        <dt>{t("Arquivo")}</dt><dd>{item.exists ? t("Presente (cópia gerenciada pelo NEXSTAGE)") : t("Ausente no disco")}</dd>
      </dl>
      <p className="muted note">{t("O arquivo original, no local de onde foi importado, não é alterado nem apagado.")}</p>
      <div className="row end"><Button variant="primary" onClick={onClose}>{t("Fechar")}</Button></div>
    </Modal>
  );
}

/** Confirmação de exclusão com a quantidade. Só as cópias do NEXSTAGE são removidas. */
export function ConfirmDelete({ count, onConfirm, onClose }: { count: number; onConfirm: () => void; onClose: () => void }) {
  return (
    <Modal title={count === 1 ? t("Excluir 1 captura?") : t("Excluir {count} capturas?", { count })} onClose={onClose}>
      <p>{count === 1 ? t("Esta captura será removida da Galeria do NEXSTAGE.") : t("Estas {count} capturas serão removidas da Galeria do NEXSTAGE.", { count })}</p>
      <p className="muted note">{t("Somente as cópias gerenciadas pelo NEXSTAGE são apagadas. Os arquivos originais, nas pastas de onde foram importados, permanecem intactos.")}</p>
      <div className="row end">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="danger" onClick={onConfirm}>{t("Excluir")}{" "}{count === 1 ? "captura" : t("{count} capturas", { count })}</Button>
      </div>
    </Modal>
  );
}
