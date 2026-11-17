import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { LogoMark } from "./Logo";
import { t } from "../lib/i18n";

/**
 * Inicialização cinematográfica (~4,3 s): escuro → luz → símbolo N1 emerge → nome → slogan → a
 * tela se dissolve revelando o app (que só é montado no início da dissolução). Qualquer tecla,
 * clique ou botão do controle pula. Apenas CSS (transform/opacity/filter): custo mínimo.
 */
export default function Boot({ onReveal, onDone }: { onReveal: () => void; onDone: () => void }) {
  const [out, setOut] = useState(false);
  const ended = useRef(false);

  useEffect(() => {
    const finish = () => {
      if (ended.current) return;
      ended.current = true;
      setOut(true);
      onReveal();
      setTimeout(onDone, 800);
    };
    const t = setTimeout(finish, 4300);
    const skip = () => { clearTimeout(t); finish(); };
    window.addEventListener("keydown", skip);
    window.addEventListener("mousedown", skip);
    let un: (() => void) | undefined;
    let cancelled = false;
    // soltar/segurar o confirmar não conta como "pular" (só apertar um botão)
    listen<{ action: string }>("pad-input", (e) => { if (!e.payload.action.startsWith("confirm_")) skip(); }).then((f) => (cancelled ? f() : (un = f)));
    return () => {
      clearTimeout(t);
      cancelled = true;
      un?.();
      window.removeEventListener("keydown", skip);
      window.removeEventListener("mousedown", skip);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className={`boot ${out ? "out" : ""}`}>
      <div className="boot-glow" />
      <div className="boot-rays" />
      <div className="boot-center">
        <div className="boot-logo"><LogoMark size={150} /></div>
        <div className="boot-name">{t("NEXSTAGE")}</div>
        <div className="boot-slogan">{t("ALL GAMES. ONE STAGE.")}</div>
      </div>
      <div className="boot-skip">{t("Pressione qualquer tecla para pular")}</div>
    </div>
  );
}
