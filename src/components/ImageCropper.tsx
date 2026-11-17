import { useEffect, useRef, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import Glyph from "./Glyph";
import { Crop, centerCrop, clampCrop, sourceRect, zoomCrop } from "../lib/crop";
import { onPadButton } from "../lib/nav";
import { useInputDevice } from "../lib/inputDevice";
import { t } from "../lib/i18n";

/** Passo do direcional/setas (px da prévia) e do zoom pelos gatilhos. */
const STEP = 18;
const ZOOM_STEP = 0.1;

/** Tamanho da prévia conforme a janela: grande, mas sempre cabendo junto com os controles (sem rolar). */
function viewSize(aspectW: number, aspectH: number) {
  const maxW = Math.min(820, innerWidth - 120);
  const maxH = Math.max(160, Math.min(520, innerHeight * 0.48));
  let w = maxW, h = Math.round((w * aspectH) / aspectW);
  if (h > maxH) { h = Math.round(maxH); w = Math.round((h * aspectW) / aspectH); }
  return { vw: Math.max(160, w), vh: Math.max(90, h) };
}

/**
 * Editor de enquadramento: zoom, reposicionamento e prévia em tempo real. Funciona com mouse (arrastar,
 * rolar), teclado e controle:
 *  - com a prévia selecionada (seleção inicial), direcional/setas movem o enquadramento; quando não há
 *    mais para onde mover naquela direção, o mesmo toque leva o foco aos botões;
 *  - L1/L2 diminuem e R1/R2 aumentam o zoom; □/X centraliza; △/Y redefine;
 *  - ✕/A (ou Enter) na prévia aplica; ○/B cancela.
 * Renderizado direto no <body>: abre de dentro de outra janela (perfil) sem ficar preso à rolagem dela.
 * O resultado salvo é exatamente o que aparece no quadro (já recortado).
 */
export default function ImageCropper({ src, aspectW, aspectH, outW, outH, round, title, onApply, onClose }: {
  src: string; aspectW: number; aspectH: number; outW: number; outH: number; round?: boolean;
  title: string; onApply: (dataUrl: string) => void; onClose: () => void;
}) {
  const [{ vw, vh }] = useState(() => viewSize(aspectW, aspectH));
  const device = useInputDevice();
  const canvas = useRef<HTMLCanvasElement>(null);
  const view = useRef<HTMLDivElement>(null);
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const [crop, setCrop] = useState<Crop>({ zoom: 1, ox: 0, oy: 0 });
  const [error, setError] = useState(false);
  const drag = useRef<{ x: number; y: number } | null>(null);
  /** Pergunta ao sair (Esc / Ⓑ / Cancelar): "Sair sem salvar" ou "Salvar". */
  const [askExit, setAskExit] = useState(false);
  // ref: o Modal registra o Esc uma vez só (na abertura) e guardaria a 1ª versão desta função
  const loadedRef = useRef(false);
  const requestClose = () => { if (loadedRef.current) setAskExit(true); else onClose(); };

  useEffect(() => {
    const i = new Image();
    i.onload = () => { loadedRef.current = true; setImg(i); setCrop(centerCrop(i.naturalWidth, i.naturalHeight, vw, vh)); };
    i.onerror = () => setError(true);
    i.src = src;
  }, [src, vw, vh]);

  // Prévia ao vivo (2x para ficar nítida)
  useEffect(() => {
    if (!img || !canvas.current) return;
    const ctx = canvas.current.getContext("2d")!;
    const r = sourceRect(img.naturalWidth, img.naturalHeight, vw, vh, crop);
    ctx.clearRect(0, 0, vw * 2, vh * 2);
    ctx.drawImage(img, r.sx, r.sy, r.sw, r.sh, 0, 0, vw * 2, vh * 2);
  }, [img, crop, vw, vh]);

  const iw = img?.naturalWidth ?? 1, ih = img?.naturalHeight ?? 1;
  const set = (c: Crop) => setCrop(clampCrop(iw, ih, vw, vh, c));
  const nudge = (dx: number, dy: number) => set({ ...crop, ox: crop.ox + dx, oy: crop.oy + dy });
  const zoom = (z: number) => img && setCrop(zoomCrop(iw, ih, vw, vh, crop, z));
  const center = () => img && setCrop(centerCrop(iw, ih, vw, vh, crop.zoom));
  const reset = () => img && setCrop(centerCrop(iw, ih, vw, vh));

  // Rolagem do mouse = zoom (precisa de listener não passivo para impedir a rolagem da página).
  useEffect(() => {
    const el = canvas.current;
    if (!el || !img) return;
    const onWheel = (e: WheelEvent) => { e.preventDefault(); zoom(crop.zoom + (e.deltaY < 0 ? ZOOM_STEP : -ZOOM_STEP)); };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  });

  /** Esta janela é a camada de cima (nada aberto por cima dela, ex.: teclado virtual)? */
  const isTop = () => {
    const mine = view.current?.closest(".overlay");
    const all = document.querySelectorAll(".overlay");
    return !!mine && all[all.length - 1] === mine && !document.querySelector(".kb-root");
  };

  // Direcional/setas com a prévia selecionada: move o enquadramento. Sem espaço para mover naquela
  // direção, o toque segue para a navegação normal (o foco vai para os botões).
  useEffect(() => {
    const onDir = (e: Event) => {
      if (!img || document.activeElement !== view.current || !isTop()) return;
      const d = (e as CustomEvent<"left" | "right" | "up" | "down">).detail;
      // mesmo sentido dos botões de posição: ← mostra mais da esquerda da imagem
      const [dx, dy] = d === "left" ? [STEP, 0] : d === "right" ? [-STEP, 0] : d === "up" ? [0, STEP] : [0, -STEP];
      const next = clampCrop(iw, ih, vw, vh, { ...crop, ox: crop.ox + dx, oy: crop.oy + dy });
      if (next.ox === crop.ox && next.oy === crop.oy) return;
      e.preventDefault();
      setCrop(next);
    };
    window.addEventListener("nexstage:direction", onDir);
    return () => window.removeEventListener("nexstage:direction", onDir);
  });

  // Gatilhos/ombros = zoom; □/X centraliza; △/Y redefine; Start salva (antes do comportamento padrão, ex.: trocar de aba)
  useEffect(() => onPadButton((b, e) => {
    if (!img) return;
    // Start salva — também com a pergunta de saída aberta
    if (b === "menu" && (isTop() || askExit)) { e.preventDefault(); apply(); return; }
    if (!isTop()) return;
    if (b === "l1" || b === "l2") zoom(crop.zoom - ZOOM_STEP);
    else if (b === "r1" || b === "r2") zoom(crop.zoom + ZOOM_STEP);
    else if (b === "x") center();
    else if (b === "y") reset();
    else return;
    e.preventDefault();
  }, true));

  function apply() {
    if (!img) return;
    const out = document.createElement("canvas");
    out.width = outW; out.height = outH;
    const r = sourceRect(iw, ih, vw, vh, crop);
    out.getContext("2d")!.drawImage(img, r.sx, r.sy, r.sw, r.sh, 0, 0, outW, outH);
    onApply(out.toDataURL("image/jpeg", 0.92));
  }

  const pad = device !== "keyboard"; // controle (o mouse conta como teclado)

  return (
    <Modal title={title} onClose={requestClose} wide portal className="crop-modal">
      {error ? <div className="empty"><p>{t("Não foi possível abrir essa imagem.")}</p></div> : (
        <div className="crop">
          <div ref={view} className={`crop-view ${round ? "round" : ""}`} style={{ width: vw, height: vh }}
            tabIndex={0} data-focus data-autofocus role="application" aria-label={t("Prévia do enquadramento")}
            // ✕/A do controle (clique sintético, detail 0) ou Enter na prévia: aplica; clique do mouse não
            onClick={(e) => { if (e.detail === 0) apply(); }}
            onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); apply(); } }}>
            <canvas ref={canvas} width={vw * 2} height={vh * 2} style={{ width: vw, height: vh }}
              onPointerDown={(e) => { view.current?.focus({ preventScroll: true }); drag.current = { x: e.clientX, y: e.clientY }; e.currentTarget.setPointerCapture(e.pointerId); }}
              onPointerMove={(e) => { if (!drag.current) return; nudge(e.clientX - drag.current.x, e.clientY - drag.current.y); drag.current = { x: e.clientX, y: e.clientY }; }}
              onPointerUp={() => { drag.current = null; }} />
          </div>
          <div className="crop-tools">
            {pad ? (
              <p className="crop-hint">
                <span><Glyph k="dpad" /> {t("Mover")}</span>
                <span><Glyph k="l1" /><Glyph k="r1" /> {t("Zoom")}</span>
                <span><Glyph k="x" /> {t("Centralizar")}</span>
                <span><Glyph k="confirm" /> {t("Aplicar")}</span>
                <span><Glyph k="menu" /> {t("Salvar")}</span>
              </p>
            ) : (
              <p className="muted">{t("Arraste a imagem, role o mouse ou use os botões. A prévia mostra o resultado final.")}</p>
            )}
            <div className="row">
              <span>{t("Zoom")}</span>
              <Button onClick={() => zoom(crop.zoom - 0.2)} disabled={crop.zoom <= 1}>−</Button>
              <input type="range" min={1} max={4} step={0.05} value={crop.zoom} onChange={(e) => zoom(Number(e.target.value))} style={{ width: 180 }} />
              <Button onClick={() => zoom(crop.zoom + 0.2)} disabled={crop.zoom >= 4}>+</Button>
              <span className="muted">{Math.round(crop.zoom * 100)}%</span>
            </div>
            <div className="row">
              <span>{t("Posição")}</span>
              <Button onClick={() => nudge(24, 0)} aria-label={t("Mover para a esquerda")}>←</Button>
              <Button onClick={() => nudge(-24, 0)} aria-label={t("Mover para a direita")}>→</Button>
              <Button onClick={() => nudge(0, 24)} aria-label={t("Mover para cima")}>↑</Button>
              <Button onClick={() => nudge(0, -24)} aria-label={t("Mover para baixo")}>↓</Button>
              <Button variant="ghost" onClick={center}>{t("Centralizar")}</Button>
              <Button variant="ghost" onClick={reset}>{t("Redefinir")}</Button>
            </div>
          </div>
        </div>
      )}
      <div className="row end">
        <Button variant="ghost" onClick={requestClose}>{t("Cancelar")}</Button>
        <Button variant="primary" onClick={apply} disabled={!img}>{t("Aplicar")}</Button>
      </div>
      {askExit && (
        // Esc / Ⓑ aqui fecham só a pergunta (volta a editar)
        <Modal title={t("Salvar as alterações?")} onClose={() => setAskExit(false)} portal>
          <p className="muted">{t("Você está saindo do editor de imagem. Deseja salvar o enquadramento antes de sair?")}</p>
          <div className="row end">
            <Button variant="danger" onClick={() => { setAskExit(false); onClose(); }}>{t("Sair sem salvar")}</Button>
            <Button variant="primary" data-autofocus onClick={() => { setAskExit(false); apply(); }}>{t("Salvar")}</Button>
          </div>
        </Modal>
      )}
    </Modal>
  );
}
