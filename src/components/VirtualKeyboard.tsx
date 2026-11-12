import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { detectedDevice, useInputDevice } from "../lib/inputDevice";
import { holdToRepeat, onPadButton } from "../lib/nav";
import { play } from "../lib/sound";
import { t } from "../lib/i18n";

type Target = HTMLInputElement | HTMLTextAreaElement;
const TEXT_TYPES = new Set(["", "text", "search", "url", "email", "password", "number"]);
const LETTERS = ["1234567890", "qwertyuiop", "asdfghjklç", "zxcvbnm.,-"];
const SYMBOLS = ["!@#$%&*()", "-_=+[]{}/\\", "'\":;<>?~`^", ".,|"];
const NUMERIC = ["789", "456", "123", "0."];

const editable = (el: Target) =>
  !el.readOnly && !el.disabled && (el.tagName === "TEXTAREA" || TEXT_TYPES.has((el as HTMLInputElement).type));

/**
 * Teclado virtual para quem usa controle. Abre ao CONFIRMAR em um campo de texto, somente quando
 * o último dispositivo usado foi um controle; com mouse/teclado físico a edição é a convencional.
 * Concluir grava no campo (disparando o onChange normal do React); Cancelar/Voltar mantém o valor anterior.
 */
export default function VirtualKeyboardHost() {
  const [target, setTarget] = useState<Target | null>(null);
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      const el = (e.target as HTMLElement).closest?.("input,textarea") as Target | null;
      if (!el || !editable(el) || detectedDevice() === "keyboard") return;
      e.preventDefault();
      setTarget(el);
    };
    document.addEventListener("click", onClick, true);
    return () => document.removeEventListener("click", onClick, true);
  }, []);
  return target ? <Keyboard target={target} onClose={() => setTarget(null)} /> : null;
}

function Keyboard({ target, onClose }: { target: Target; onClose: () => void }) {
  const multiline = target.tagName === "TEXTAREA";
  const numeric = (target as HTMLInputElement).type === "number";
  const max = target.maxLength > 0 ? target.maxLength : Infinity;
  const label = (target.closest("label")?.childNodes[0]?.textContent ?? target.placeholder ?? "Texto").trim() || "Texto";
  // Texto + posição do cursor (onde as letras são inseridas).
  const [{ text, pos }, setEd] = useState({ text: target.value, pos: target.value.length });
  const [shift, setShift] = useState(false);
  const [sym, setSym] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const latest = useRef(text);
  latest.current = text;
  const device = useInputDevice();
  const ps = device === "playstation";
  const pad = device !== "keyboard";
  const hint = ps
    ? { del: "□", space: "△", shift: "R2", sym: "L2", left: "L1", right: "R1", done: "OPTIONS" }
    : { del: "X", space: "Y", shift: "RT", sym: "LT", left: "LB", right: "RB", done: "START" };

  const close = () => {
    onClose();
    setTimeout(() => target.focus({ preventScroll: true }), 0);
  };
  const done = () => {
    const proto = multiline ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(target, latest.current);
    target.dispatchEvent(new Event("input", { bubbles: true }));
    close();
  };
  const cancel = () => close();
  // O atalho do controle é registrado uma vez; a ref aponta sempre para o "Concluir" atual.
  const doneRef = useRef(done);
  doneRef.current = done;

  useEffect(() => {
    play("open");
    ref.current?.querySelector<HTMLElement>(".kb-key")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopImmediatePropagation();
      e.preventDefault();
      play("back");
      cancel();
    };
    window.addEventListener("keydown", onKey, true);
    // Atalhos do controle (capture: o teclado tem prioridade sobre a página por baixo).
    const offPad = onPadButton((b, e) => {
      e.preventDefault();
      if (b === "menu") { play("confirm"); doneRef.current(); return; } // Start/Options = Concluir
      if (b === "x") erase();
      else if (b === "y") { if (!numeric) add(" "); } // Y / △ = Espaço
      else if (b === "r2") { if (!numeric) setShift((s) => !s); }
      else if (b === "l2") { if (!numeric) setSym((s) => !s); }
      else if (b === "l1") moveCaret(-1);
      else if (b === "r1") moveCaret(1);
      play("move");
    }, true);
    // segurar um comando do controle repete (apagar, espaço, cursor, digitar a tecla em foco)
    const offRepeat = holdToRepeat();
    return () => { window.removeEventListener("keydown", onKey, true); offPad(); offRepeat(); play("close"); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const add = (ch: string) =>
    setEd((s) => (s.text.length + ch.length <= max ? { text: s.text.slice(0, s.pos) + ch + s.text.slice(s.pos), pos: s.pos + ch.length } : s));
  const erase = () =>
    setEd((s) => (s.pos > 0 ? { text: s.text.slice(0, s.pos - 1) + s.text.slice(s.pos), pos: s.pos - 1 } : s));
  const moveCaret = (d: number) =>
    setEd((s) => ({ ...s, pos: Math.max(0, Math.min(s.text.length, s.pos + d)) }));
  const key = (ch: string) => (
    <button key={ch} type="button" data-focus className="kb-key" onClick={() => add(shift && !sym ? ch.toUpperCase() : ch)}>
      {shift && !sym ? ch.toUpperCase() : ch}
    </button>
  );
  const rows = numeric ? NUMERIC : sym ? SYMBOLS : LETTERS;

  return createPortal(
    <div className="kb-root" ref={ref}>
      <div className="kb-panel" role="dialog" aria-label={t("Editar {label}", { label })}>
        <div className="kb-label">{label}</div>
        <div className="kb-preview">
          {text ? <>{text.slice(0, pos)}<i className="kb-caret" />{text.slice(pos)}</> : <><span className="muted">(vazio)</span><i className="kb-caret" /></>}
        </div>
        {rows.map((r) => <div className="kb-row" key={r}>{[...r].map(key)}</div>)}
        <div className="kb-row">
          <button type="button" data-focus className="kb-key act" onClick={cancel}>{t("Cancelar")}</button>
          {!numeric && <button type="button" data-focus className={`kb-key act ${shift ? "on" : ""}`} onClick={() => setShift((s) => !s)}>{t("⇧ Maiúsc.")}{pad && <kbd>{hint.shift}</kbd>}</button>}
          {!numeric && <button type="button" data-focus className={`kb-key act ${sym ? "on" : ""}`} onClick={() => setSym((s) => !s)}>{sym ? t("ABC") : "?123"}{pad && <kbd>{hint.sym}</kbd>}</button>}
          <button type="button" data-focus className="kb-key act" aria-label={t("Mover cursor para a esquerda")} onClick={() => moveCaret(-1)}>◀{pad && <kbd>{hint.left}</kbd>}</button>
          {!numeric && <button type="button" data-focus className="kb-key wide" onClick={() => add(" ")}>{t("Espaço")}{pad && <kbd>{hint.space}</kbd>}</button>}
          <button type="button" data-focus className="kb-key act" aria-label={t("Mover cursor para a direita")} onClick={() => moveCaret(1)}>▶{pad && <kbd>{hint.right}</kbd>}</button>
          {multiline && <button type="button" data-focus className="kb-key act" onClick={() => add("\n")}>↵</button>}
          <button type="button" data-focus className="kb-key act" onClick={erase}>{t("⌫ Apagar")}{pad && <kbd>{hint.del}</kbd>}</button>
          <button type="button" data-focus className="kb-key act primary" onClick={done}>{t("Concluir")}{pad && <kbd>{hint.done}</kbd>}</button>
        </div>
      </div>
    </div>,
    document.body
  );
}
