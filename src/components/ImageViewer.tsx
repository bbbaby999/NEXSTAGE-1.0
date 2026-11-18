import { useEffect, useRef, useState } from "react";
import Button from "./Button";
import { Icon, ICON } from "./CaptureUi";
import { api } from "../lib/api";
import { play } from "../lib/sound";
import { useInputDevice } from "../lib/inputDevice";
import { onPadButton } from "../lib/nav";
import { GLYPHS } from "./Glyph";
import { t } from "../lib/i18n";

/**
 * Visualizador de imagens da internet (ex.: capturas do catálogo do Game Pass) com o MESMO desenho e os
 * mesmos comandos do visualizador da Galeria (components/Viewer.tsx): imagem proporcional no centro, ações
 * em coluna, ← → / L1 R1 trocam de imagem, Y/△ ou F alternam a tela cheia, Esc/Ⓑ volta.
 * Sem as ações de arquivo da Galeria (favoritar, abrir pasta, excluir), que não se aplicam a imagens online.
 */
export default function ImageViewer({ images, index, onIndex, onClose, caption, backLabel }: {
  images: string[]; index: number; onIndex: (i: number) => void; onClose: () => void;
  /** Texto da legenda (ex.: nome do jogo); a posição "N de M" é acrescentada. */
  caption: string;
  backLabel: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [broken, setBroken] = useState(false);
  const n = images.length;
  const src = images[index];
  const atFirst = index <= 0, atLast = index >= n - 1;
  const device = useInputDevice();

  // Tela cheia: como na Galeria, a janela também vai para tela cheia (se já não estiver) e volta ao sair.
  const [full, setFull] = useState(false);
  const wasFull = useRef<boolean | null>(null);
  const fullOn = useRef(false);
  const toggleFull = useRef<(on?: boolean) => void>(() => {});
  toggleFull.current = (on = !full) => {
    if (on === full) return;
    setFull(on);
    fullOn.current = on;
    play(on ? "open" : "close");
    if (!on) requestAnimationFrame(() => ref.current?.querySelector<HTMLElement>(".v-full")?.focus());
    if (document.documentElement.classList.contains("overlay-mode")) return;
    if (on) {
      api.isFullscreen().catch(() => true).then((v) => {
        if (!fullOn.current) return;
        wasFull.current = v;
        if (!wasFull.current) api.setFullscreen(true).catch(() => {});
      });
    } else if (wasFull.current === false) {
      api.setFullscreen(false).catch(() => {});
    }
  };
  useEffect(() => () => { if (wasFull.current === false) api.setFullscreen(false).catch(() => {}); }, []);
  useEffect(() => { if (!full) wasFull.current = null; }, [full]);

  const [awake, setAwake] = useState(true);
  useEffect(() => {
    if (!full) return;
    let tm = 0;
    const wake = () => { setAwake(true); clearTimeout(tm); tm = window.setTimeout(() => setAwake(false), 2200); };
    wake();
    const evs = ["mousemove", "keydown", "nexstage:viewer", "nexstage:tab", "nexstage:button"];
    evs.forEach((e) => window.addEventListener(e, wake));
    return () => { clearTimeout(tm); evs.forEach((e) => window.removeEventListener(e, wake)); };
  }, [full]);
  useEffect(() => { if (full) setAwake(true); }, [index, full]);
  useEffect(() => { setBroken(false); }, [index]);

  useEffect(() => {
    const prevFocus = document.activeElement as HTMLElement | null;
    ref.current?.querySelector<HTMLElement>("[data-focus]:not([disabled])")?.focus();
    play("open");
    return () => { play("close"); prevFocus?.focus?.(); };
  }, []);

  useEffect(() => {
    const go = (d: number) => {
      if (document.querySelector(".ctx-root, .pick-root, .kb-root")) return;
      const ni = index + d;
      if (ni >= 0 && ni < n) { onIndex(ni); play("move"); }
    };
    const onNav = (e: Event) => go((e as CustomEvent<number>).detail);
    const topmost = () => { const all = document.querySelectorAll(".overlay"); return all[all.length - 1] === ref.current; };
    const onKey = (e: KeyboardEvent) => {
      if (!topmost()) return;
      if ((e.key === "f" || e.key === "F") && !e.ctrlKey && !e.altKey && !e.metaKey && !(e.target instanceof HTMLInputElement)) {
        e.preventDefault(); toggleFull.current(); return;
      }
      if (e.key !== "Escape") return;
      play("back");
      if (full) toggleFull.current(false);
      else onClose();
    };
    const offPad = onPadButton((b, e) => {
      if (b !== "y" || e.defaultPrevented || !topmost()) return;
      e.preventDefault(); toggleFull.current();
    });
    window.addEventListener("nexstage:viewer", onNav);
    window.addEventListener("nexstage:tab", onNav); // L1 / R1
    window.addEventListener("keydown", onKey);
    return () => {
      offPad();
      window.removeEventListener("nexstage:viewer", onNav);
      window.removeEventListener("nexstage:tab", onNav);
      window.removeEventListener("keydown", onKey);
    };
  }, [index, n, onIndex, onClose, full]);

  if (!src) return null;

  return (
    <div className="overlay viewer" data-viewer="vertical" ref={ref} role="dialog" aria-modal="true" aria-label={caption}>
      <div className="viewer-main" {...({ inert: full ? "" : undefined } as Record<string, unknown>)}>
        <div className="viewer-stage">
          {!broken ? (
            <img key={src} src={src} alt={caption} referrerPolicy="no-referrer" onError={() => setBroken(true)}
              onDoubleClick={() => toggleFull.current(true)} title={t("Clique duas vezes para tela cheia")} />
          ) : (
            <div className="empty"><p>{t("Não foi possível carregar esta imagem.")}</p></div>
          )}
          <div className="viewer-caption">{caption} · {index + 1} de {n}</div>
        </div>
        <div className="viewer-rail" aria-label={t("Ações da captura")}>
          <Button onClick={() => onIndex(index - 1)} disabled={atFirst}><Icon d={ICON.prev} /> {t("Anterior")}</Button>
          <Button onClick={() => onIndex(index + 1)} disabled={atLast}>{t("Próxima")}{" "}<Icon d={ICON.next} /></Button>
          <Button className="v-full" onClick={() => toggleFull.current(true)} disabled={broken}>
            <Icon d={ICON.full} /> {t("Tela cheia")}{device !== "keyboard" ? <kbd className="v-kbd">{device === "playstation" ? "△" : device === "nintendo" ? t("X") : t("Y")}</kbd> : <kbd className="v-kbd">{t("F")}</kbd>}
          </Button>
          <Button variant="primary" onClick={onClose}><Icon d={ICON.back} /> {backLabel}</Button>
        </div>
      </div>
      {full && (
        <div className={`viewer-full ${awake ? "awake" : ""}`} onDoubleClick={() => toggleFull.current(false)}>
          {!broken ? <img key={src} src={src} alt={caption} referrerPolicy="no-referrer" onError={() => setBroken(true)} />
            : <p className="muted">{t("Não foi possível carregar esta imagem.")}</p>}
          <div className="vf-hints">
            <button type="button" tabIndex={-1} disabled={atFirst} onClick={() => { onIndex(index - 1); play("move"); }} aria-label={t("Anterior")}>
              <Icon d={ICON.prev} /><kbd>{device === "keyboard" ? "↓" : GLYPHS[device].l1}</kbd>
            </button>
            <button type="button" tabIndex={-1} disabled={atLast} onClick={() => { onIndex(index + 1); play("move"); }} aria-label={t("Próxima")}>
              <kbd>{device === "keyboard" ? "↑" : GLYPHS[device].r1}</kbd><Icon d={ICON.next} />
            </button>
            <i aria-hidden="true" />
            <button type="button" tabIndex={-1} onClick={() => { play("back"); toggleFull.current(false); }} aria-label={t("Sair da tela cheia")}>
              <Icon d={ICON.exitFull} /><kbd>{GLYPHS[device].back}</kbd>
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
