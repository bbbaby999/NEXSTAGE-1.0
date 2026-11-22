/**
 * Menu do botão direito nos campos de texto, desenhado pelo NEXSTAGE (nada de menu do Windows dentro do
 * app): Copiar, Recortar, Colar, Selecionar tudo e Emojis.
 *
 * O texto passa pela área de transferência do sistema por comandos do backend (o WebView não lê a área de
 * transferência sem pedir permissão). A edição usa `insertText`/`delete`, que disparam o evento `input`
 * normal (campos controlados pelo React atualizam) e entram no desfazer (Ctrl+Z) do campo.
 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { CaptureMenu, type MenuAction } from "./CaptureUi";
import { play } from "../lib/sound";
import { t } from "../lib/i18n";

type Field = HTMLInputElement | HTMLTextAreaElement;
const EDITABLE = "input:not([type=checkbox]):not([type=radio]):not([type=range]):not([type=button]):not([type=submit]):not([type=color]):not([type=file]), textarea";

const ICON = {
  copy: "M8 8h11v11H8zM5 16H4V4h12v1",
  cut: "M6 9a3 3 0 100-6 3 3 0 000 6zM6 21a3 3 0 100-6 3 3 0 000 6zM8.1 7.9L20 20M8.1 16.1L20 4",
  paste: "M9 4h6v3H9zM7 5H5v16h14V5h-2M9 12h6M9 16h4",
  all: "M4 4h4M16 4h4v4M20 16v4h-4M8 20H4v-4M4 8v0M9 9h6v6H9z",
  emoji: "M12 3a9 9 0 100 18 9 9 0 000-18zM8.5 14.5s1.3 2 3.5 2 3.5-2 3.5-2M9 9.5h.01M15 9.5h.01",
};

const EMOJIS = [
  "😀", "😃", "😄", "😁", "😆", "😅", "😂", "🤣", "🙂", "😉", "😊", "😇", "😍", "🤩", "😘", "😋",
  "😎", "🤓", "🤔", "🤨", "😐", "😏", "😴", "😮", "😲", "😳", "🥺", "😢", "😭", "😤", "😡", "🤯",
  "🥳", "🤗", "🙄", "😬", "🤐", "😷", "🤖", "👻", "💀", "👽", "👾", "🎃", "😺", "🙈", "🐶", "🐱",
  "👍", "👎", "👏", "🙌", "🙏", "💪", "✌️", "🤞", "👌", "🤘", "👋", "✋", "👀", "🧠", "🫡", "🤝",
  "❤️", "🧡", "💛", "💚", "💙", "💜", "🖤", "🤍", "💔", "💯", "✨", "⭐", "🌟", "🔥", "💥", "⚡",
  "🎮", "🕹️", "👑", "🏆", "🥇", "🎯", "🎲", "🧩", "⚔️", "🛡️", "🏹", "💣", "🗡️", "🪄", "💎", "🪙",
  "✅", "❌", "⚠️", "❓", "❗", "➕", "➖", "🔒", "🔓", "🔑", "⏱️", "⏳", "📌", "📷", "🎵", "🎉",
];

const isField = (el: Element | null): el is Field => !!el && el.matches(EDITABLE);
const sel = (f: Field) => {
  try { return { start: f.selectionStart ?? 0, end: f.selectionEnd ?? 0 }; } catch { return { start: 0, end: 0 }; }
};
/** Devolve o foco ao campo com a seleção que ele tinha (o menu tirou o foco dele). */
function restore(f: Field, s: { start: number; end: number }) {
  f.focus({ preventScroll: true });
  try { f.setSelectionRange(s.start, s.end); } catch { /* tipos sem seleção (number, email) */ }
}
const readClip = () => invoke<string>("clipboard_read_text").catch(() => "");
const writeClip = (text: string) => invoke<void>("clipboard_write_text", { text }).catch(() => {});
/** Insere no campo como se fosse digitado (evento input, desfazer). */
function insert(f: Field, text: string) {
  if (!document.execCommand("insertText", false, text)) {
    // sem suporte: troca o trecho e avisa o React pelo setter nativo
    const { start, end } = sel(f);
    const proto = f instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")?.set?.call(f, f.value.slice(0, start) + text + f.value.slice(end));
    f.dispatchEvent(new Event("input", { bubbles: true }));
  }
}

interface Req { field: Field; x: number; y: number; s: { start: number; end: number } }

export default function TextMenuHost() {
  const [req, setReq] = useState<Req | null>(null);
  const [emoji, setEmoji] = useState<Req | null>(null);
  useEffect(() => {
    const onCtx = (e: MouseEvent) => {
      const f = (e.target as Element | null)?.closest?.(EDITABLE) ?? null;
      if (!isField(f)) return;
      e.preventDefault(); // nada do menu do Windows
      if (f.disabled) return;
      setEmoji(null);
      setReq({ field: f, x: e.clientX, y: e.clientY, s: sel(f) });
    };
    window.addEventListener("contextmenu", onCtx);
    return () => window.removeEventListener("contextmenu", onCtx);
  }, []);

  if (emoji) return <EmojiPicker req={emoji} onClose={() => setEmoji(null)} />;
  if (!req) return null;
  const { field: f, s } = req;
  const hasSel = s.end > s.start;
  const secret = f instanceof HTMLInputElement && f.type === "password";
  const ro = f.readOnly;
  const actions: MenuAction[] = [
    ...(hasSel && !secret ? [{ label: t("Copiar"), icon: ICON.copy, run: () => { writeClip(f.value.slice(s.start, s.end)); restore(f, s); } }] : []),
    ...(hasSel && !secret && !ro ? [{ label: t("Recortar"), icon: ICON.cut, run: () => {
      writeClip(f.value.slice(s.start, s.end));
      restore(f, s);
      if (!document.execCommand("delete")) insert(f, "");
    } }] : []),
    ...(!ro ? [{ label: t("Colar"), icon: ICON.paste, run: () => { readClip().then((txt) => { restore(f, s); if (txt) insert(f, txt); }); } }] : []),
    { label: t("Selecionar tudo"), icon: ICON.all, run: () => { f.focus({ preventScroll: true }); f.select(); } },
    ...(!ro ? [{ label: t("Emojis"), icon: ICON.emoji, run: () => setTimeout(() => setEmoji(req), 0) }] : []),
  ];
  return <CaptureMenu actions={actions} x={req.x} y={req.y} onClose={() => setReq(null)} />;
}

/** Grade de emojis (mouse, teclado ou controle). Escolher insere no campo e fecha. */
function EmojiPicker({ req, onClose }: { req: Req; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const panel = ref.current?.querySelector<HTMLElement>(".emoji-panel");
    if (panel) {
      const r = panel.getBoundingClientRect();
      panel.style.left = `${Math.max(8, Math.min(req.x, innerWidth - r.width - 8))}px`;
      panel.style.top = `${Math.max(8, Math.min(req.y, innerHeight - r.height - 8))}px`;
    }
    play("open");
    ref.current?.querySelector<HTMLElement>(".emoji-btn")?.focus({ preventScroll: true });
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopImmediatePropagation();
      e.preventDefault();
      play("back");
      done();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const done = () => { onClose(); restore(req.field, req.s); };
  const pick = (em: string) => {
    play("confirm");
    onClose();
    restore(req.field, req.s);
    insert(req.field, em);
  };
  return createPortal(
    <div className="ctx-root" ref={ref}>
      <div className="ctx-backdrop" onClick={done} onContextMenu={(e) => { e.preventDefault(); done(); }} />
      <div className="ctx-panel at emoji-panel" style={{ left: req.x, top: req.y }} role="dialog" aria-label={t("Emojis")}>
        <div className="emoji-grid" data-nav-group>
          {EMOJIS.map((em) => (
            <button key={em} type="button" data-focus className="emoji-btn" onClick={() => pick(em)} aria-label={em}>{em}</button>
          ))}
        </div>
      </div>
    </div>,
    document.body
  );
}
