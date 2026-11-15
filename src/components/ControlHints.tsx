import { useEffect, useState } from "react";
import { useInputDevice } from "../lib/inputDevice";
import { useSession } from "../state/session";
import Glyph, { GlyphKey } from "./Glyph";
import { t, tk } from "../lib/i18n";

/** Uma dica: um ou mais botões + o que eles fazem. */
type Hint = [GlyphKey[], string];

/**
 * Dicas de comando do canto inferior (estilo console): "Ⓐ Selecionar · Ⓑ Voltar · ☰ Opções".
 * Mudam com a tela, com a camada aberta (janela, lista de seleção, visualizador) e com o elemento em
 * foco (ex.: barra deslizante → "Ajustar"). Só aparecem quando o último dispositivo usado é um controle;
 * os glifos seguem o tipo dele (Xbox, PlayStation, Nintendo ou genérico).
 */
export default function ControlHints() {
  const device = useInputDevice();
  const { profile, view, openGameId } = useSession();
  const [hints, setHints] = useState<Hint[] | null>(null);
  const pad = device !== "keyboard";

  useEffect(() => {
    if (!pad) { setHints(null); return; }
    let raf = 0;
    const compute = (): Hint[] | null => {
      // camadas com dicas próprias (ou sem navegação): nada aqui
      if (document.querySelector(".kb-root, .sm-root:not(.leaving), .gb-root, .viewer-full, .igdb-fs")) return null;
      if (document.querySelector(".pick-root, .ctx-root")) return [[["dpad"], tk("Mover")], [["confirm"], tk("Selecionar")], [["back"], tk("Fechar")]];
      const layer = document.querySelector<HTMLElement>(".overlay:not(.leaving)");
      if (layer) {
        if (layer.matches("[data-viewer]")) return [[["dpad"], tk("Anterior / próxima")], [["back"], tk("Fechar")]];
        // Zerei!: formulário (L2/R2 ajustam o campo em foco ou mudam o status)
        if (layer.querySelector(".zr-edit")) {
          const adj = (document.activeElement as HTMLElement | null)?.dataset?.zrAdjust;
          return [[["dpad"], tk("Mover")], [["confirm"], tk("Selecionar")], [["l2", "r2"], adj ? tk("Ajustar") : tk("Status")], [["back"], tk("Fechar")]];
        }
        if (layer.querySelector(".cp")) return [[["dpad"], tk("Ajustar")], [["confirm"], tk("Selecionar")], [["menu"], tk("Aplicar")], [["back"], tk("Cancelar")]];
        return [[["dpad"], tk("Mover")], [["confirm"], tk("Selecionar")], [["back"], tk("Fechar")]];
      }
      const pages: Hint = [["l1", "r1"], tk("Páginas")];
      const act = document.activeElement;
      if (act instanceof HTMLInputElement && act.type === "range") return [[["dpad"], tk("Ajustar")], pages, [["back"], tk("Voltar")]];
      if (!profile) return [[["dpad"], tk("Mover")], [["confirm"], tk("Selecionar")]];
      if (openGameId !== null) return [[["confirm"], tk("Selecionar")], pages, [["back"], tk("Voltar")]];
      switch (view) {
        case "home": return [[["confirm"], tk("Selecionar")], [["menu"], tk("Opções")], pages, [["back"], tk("Menu")]];
        case "stats": return [[["confirm"], tk("Selecionar")], [["l2", "r2"], tk("Abas")], pages, [["back"], tk("Voltar")]];
        case "achievements": return [[["confirm"], tk("Selecionar")], [["l2", "r2"], tk("RetroAchievements / Steam")], pages, [["back"], tk("Voltar")]];
        case "zerei":
          if (document.querySelector(".zr-detail")) return [[["confirm"], tk("Selecionar")], [["l2", "r2"], tk("Registro anterior / próximo")], pages, [["back"], tk("Voltar")]];
          if (document.querySelector(".zr-stats")) return [[["confirm"], tk("Selecionar")], [["menu"], tk("Coleção")], pages, [["back"], tk("Voltar")]];
          if (!document.querySelector(".zr-grid, .zr-filters")) return [[["confirm"], tk("Selecionar")], [["menu"], tk("Estatísticas")], pages, [["back"], tk("Voltar")]];
          return [[["confirm"], tk("Abrir")], [["x"], tk("Ordenar")], [["l2", "r2"], tk("Status")], [["menu"], tk("Estatísticas")], pages, [["back"], tk("Voltar")]];
        case "library": return [[["confirm"], tk("Abrir")], [["x"], tk("Ordenar")], [["l2"], tk("Gênero")], [["r2"], tk("Plataforma")], pages, [["back"], tk("Voltar")]];
        default: return [[["confirm"], tk("Selecionar")], pages, [["back"], tk("Voltar")]];
      }
    };
    const update = () => {
      if (raf) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        const next = compute();
        // evita re-render quando nada mudou (o observador dispara com qualquer atualização do React)
        setHints((prev) => (JSON.stringify(prev) === JSON.stringify(next) ? prev : next));
      });
    };
    update();
    const mo = new MutationObserver(update);
    mo.observe(document.body, { childList: true, subtree: true });
    document.addEventListener("focusin", update);
    return () => { mo.disconnect(); document.removeEventListener("focusin", update); cancelAnimationFrame(raf); };
  }, [pad, profile, view, openGameId]);

  // Reserva espaço no fim das páginas para as dicas não cobrirem o último item.
  const shown = pad && !!hints?.length;
  useEffect(() => {
    document.documentElement.classList.toggle("has-hints", shown);
    return () => document.documentElement.classList.remove("has-hints");
  }, [shown]);

  if (!shown) return null;
  return (
    <div className="ctl-hints" aria-hidden="true">
      {hints!.map(([keys, label]) => (
        <span key={label}>{keys.map((k) => <Glyph key={k} k={k} />)}{t(label)}</span>
      ))}
    </div>
  );
}
