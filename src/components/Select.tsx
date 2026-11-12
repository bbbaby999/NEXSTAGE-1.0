import { MutableRefObject, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { play } from "../lib/sound";
import { markLayerClosed } from "../lib/nav";

/**
 * Lista de seleção própria (substitui <select> nativo, que não abre por controle).
 * Confirmar abre/seleciona; direcional/analógico navega; Voltar (Esc/B) fecha sem alterar.
 * Valores antigos fora da lista aparecem como "(valor anterior)" e nunca são perdidos.
 */
export default function Select<T extends string | number>({ value, options, onChange, title, width, disabled, openRef }: {
  value: T; options: [string, T][]; onChange: (v: T) => void; title?: string; width?: number | string; disabled?: boolean;
  /** Recebe uma função que abre a lista (atalhos do controle, ex.: L2/R2 na Biblioteca). */
  openRef?: MutableRefObject<(() => void) | null>;
}) {
  const [open, setOpen] = useState(false);
  if (openRef) openRef.current = disabled ? null : () => setOpen(true);
  const opts: [string, T][] = options.some(([, v]) => v === value)
    ? options
    : [[`${String(value)} (valor anterior)`, value], ...options];
  const current = opts.find(([, v]) => v === value)?.[0] ?? "—";
  return (
    <>
      <button type="button" data-focus className="select-btn" style={{ width }} disabled={disabled}
        aria-haspopup="listbox" onClick={() => setOpen(true)}>
        <span>{current}</span><i>▾</i>
      </button>
      {open && <List title={title} opts={opts} value={value}
        onPick={(v) => { onChange(v); setOpen(false); }} onClose={() => setOpen(false)} />}
    </>
  );
}

function List<T extends string | number>({ title, opts, value, onPick, onClose }: {
  title?: string; opts: [string, T][]; value: T; onPick: (v: T) => void; onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    play("open");
    const first = ref.current?.querySelector<HTMLElement>(".opt.on") ?? ref.current?.querySelector<HTMLElement>(".opt");
    first?.focus();
    first?.scrollIntoView({ block: "center" });
    // Captura: Esc/Círculo fecha só esta lista (não chega ao modal/menu por baixo).
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopImmediatePropagation();
      e.preventDefault();
      play("back");
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      markLayerClosed();
      play("close");
      prev?.focus?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return createPortal(
    <div className="pick-root" ref={ref}>
      <div className="pick-backdrop" onClick={onClose} />
      <div className="pick-panel" role="listbox">
        {title && <h3>{title}</h3>}
        <div className="pick-list">
          {opts.map(([label, v]) => (
            <button key={String(v)} type="button" data-focus role="option" aria-selected={v === value}
              className={`opt ${v === value ? "on" : ""}`} onClick={() => onPick(v)}>
              <span>{label}</span>{v === value && <i>✓</i>}
            </button>
          ))}
        </div>
      </div>
    </div>,
    document.body
  );
}
