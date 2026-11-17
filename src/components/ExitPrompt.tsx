import { RefObject, useCallback, useEffect, useRef, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import { onPadButton } from "../lib/nav";
import { t } from "../lib/i18n";

/**
 * Atalhos de edição para janelas com "Salvar" (formulários de jogo, escolha de capa/fundo/logo):
 *  - Start (☰ / Options / +) salva;
 *  - sair (Esc / Ⓑ / Cancelar) com alterações não salvas pergunta "Sair sem salvar" ou "Salvar".
 *    Sem alterações, sai direto.
 *
 * Uso: `const exit = useSaveExit({ root, dirty, canSave, save, close })`; passe `exit.requestClose` ao
 * `onClose` do Modal e ao botão Cancelar, e renderize `exit.prompt` dentro da janela.
 * Tudo é lido por refs: o Modal registra o Esc uma única vez, na abertura.
 */
export function useSaveExit(o: {
  /** Elemento dentro da janela (para saber se ela é a camada de cima). */
  root: RefObject<HTMLElement | null>;
  dirty: () => boolean;
  canSave?: () => boolean;
  save: () => void;
  close: () => void;
  saveLabel?: string;
  message?: string;
}) {
  const [asking, setAsking] = useState(false);
  const ref = useRef(o);
  ref.current = o;
  const askingRef = useRef(false);
  askingRef.current = asking;

  const isTop = () => {
    const el = ref.current.root.current;
    const layers = document.querySelectorAll(".overlay:not(.leaving)");
    return !!el && !!layers.length && layers[layers.length - 1].contains(el) && !document.querySelector(".pick-root, .kb-root, .ctx-root");
  };

  const requestClose = useCallback(() => {
    const c = ref.current;
    if (c.dirty() && (c.canSave?.() ?? true)) setAsking(true);
    else c.close();
  }, []);

  // Start salva (também com a pergunta aberta)
  useEffect(() => onPadButton((b, e) => {
    if (b !== "menu" || (!isTop() && !askingRef.current)) return;
    const c = ref.current;
    if (c.canSave && !c.canSave()) return;
    e.preventDefault();
    setAsking(false);
    c.save();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, true), []);

  const prompt = asking ? (
    // Esc / Ⓑ aqui fecham só a pergunta (volta a editar)
    <Modal title={t("Salvar as alterações?")} onClose={() => setAsking(false)} portal>
      <p className="muted">{o.message ?? t("Há alterações que ainda não foram salvas.")}</p>
      <div className="row end">
        <Button variant="danger" onClick={() => { setAsking(false); ref.current.close(); }}>{t("Sair sem salvar")}</Button>
        <Button variant="primary" data-autofocus onClick={() => { setAsking(false); ref.current.save(); }}>{o.saveLabel ?? t("Salvar")}</Button>
      </div>
    </Modal>
  ) : null;

  return { requestClose, prompt, asking: () => askingRef.current };
}
