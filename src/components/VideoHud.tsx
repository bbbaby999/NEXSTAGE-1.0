import { GLYPHS, PS_SHAPES, type GlyphKey } from "./Glyph";
import type { Device } from "../lib/inputDevice";
import { t } from "../lib/i18n";

/** Passo do ← → na tela cheia de vídeo (segundos). */
export const FULL_SEEK = 5;

export const clock = (s: number) => {
  if (!Number.isFinite(s) || s < 0) s = 0;
  const m = Math.floor(s / 60), r = Math.floor(s % 60);
  return `${m}:${String(r).padStart(2, "0")}`;
};

/** Tecla da pílula de comandos: glifo do controle em uso (ícone no PlayStation) ou a tecla do teclado. */
export function HudKey({ device, k, kb }: { device: Device; k: GlyphKey; kb: string }) {
  if (device === "keyboard") return <kbd>{kb}</kbd>;
  const shape = device === "playstation" ? PS_SHAPES[k] : undefined;
  return (
    <kbd className={shape ? "ps" : undefined}>
      {shape ? <svg viewBox="0 0 16 16" aria-hidden="true">{shape}</svg> : GLYPHS[device][k]}
    </kbd>
  );
}

/** Seta do lado (vale para controle — analógico ou direcional — e teclado). */
export function ArrowKey({ dir }: { dir: "left" | "right" }) {
  return (
    <kbd className="ps arrow">
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d={dir === "left" ? "M10 3.5L5.5 8l4.5 4.5" : "M6 3.5L10.5 8 6 12.5"} /></svg>
    </kbd>
  );
}

/**
 * Tela cheia de um vídeo: linha do tempo (só indicação, sem foco) e a pílula com os ÚNICOS comandos:
 * ✕ (A) pausa/retoma, ← volta 5 s, → avança 5 s, □ liga/desliga o som, ○ fecha. Os botões são clicáveis com o mouse e não
 * recebem foco (o controle usa os botões físicos).
 */
export function VideoFullHud({ device, time, dur, muted, paused, ready = true, onPlay, onSeek, onMute, onClose, closeLabel }: {
  device: Device; time: number; dur: number; muted: boolean; paused: boolean; ready?: boolean;
  onPlay: () => void; onSeek: (delta: number) => void; onMute: () => void; onClose: () => void; closeLabel: string;
}) {
  const pct = dur > 0 ? Math.min(100, Math.max(0, (time / dur) * 100)) : 0;
  return (
    <>
      <div className="vhud-line" aria-hidden="true">
        <span className="vhud-time">{clock(time)}</span>
        <span className="vhud-track"><i style={{ width: `${pct}%` }} /></span>
        <span className="vhud-time">{clock(dur)}</span>
      </div>
      <div className="igdb-hud vhud" aria-label={t("Comandos")}>
        <button type="button" tabIndex={-1} onClick={onPlay} disabled={!ready}>
          <HudKey device={device} k="confirm" kb={t("Espaço")} />{paused ? t("Reproduzir") : t("Pausar")}
        </button>
        <i aria-hidden="true" />
        <button type="button" tabIndex={-1} onClick={() => onSeek(-FULL_SEEK)} disabled={!ready} aria-label={t("Voltar {n} s", { n: FULL_SEEK })}>
          <ArrowKey dir="left" />−{FULL_SEEK} s
        </button>
        <button type="button" tabIndex={-1} onClick={() => onSeek(FULL_SEEK)} disabled={!ready} aria-label={t("Avançar {n} s", { n: FULL_SEEK })}>
          +{FULL_SEEK} s<ArrowKey dir="right" />
        </button>
        <i aria-hidden="true" />
        <button type="button" tabIndex={-1} onClick={onMute} disabled={!ready} aria-pressed={!muted}>
          <HudKey device={device} k="x" kb="M" />{muted ? t("Ativar som") : t("Desativar som")}
        </button>
        <button type="button" tabIndex={-1} onClick={onClose}>
          <HudKey device={device} k="back" kb="Esc" />{closeLabel}
        </button>
      </div>
    </>
  );
}
