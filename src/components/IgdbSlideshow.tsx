import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent as RKeyboardEvent, type MouseEvent as RMouseEvent } from "react";
import { createPortal } from "react-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "./Button";
import { GLYPHS, PS_SHAPES, type GlyphKey } from "./Glyph";
import { useInputDevice, type Device } from "../lib/inputDevice";
import { api } from "../lib/api";
import { markLayerClosed, onPadButton } from "../lib/nav";
import { t } from "../lib/i18n";

export type Slide =
  | { kind: "video"; id: string; name: string }
  | { kind: "artwork" | "shot"; url: string };

const YT_ORIGIN = "https://www.youtube-nocookie.com";
const IMAGE_MS = 5000;
const ytThumb = (id: string, q = "hqdefault") => `https://img.youtube.com/vi/${id}/${q}.jpg`;

/**
 * Apresentação das mídias do IGDB: imagens trocam a cada 5 s e a sequência recomeça do início;
 * vídeos (player oficial do YouTube, modo de privacidade reforçada, sem a interface do YouTube) tocam
 * do início ao fim, sem som, e só então a apresentação segue. O controle do player usa a API de
 * mensagens do próprio YouTube (postMessage) — nenhum script externo é carregado no app.
 *
 * "Tela cheia" é uma camada do próprio app (como no visualizador de capturas), nunca a tela cheia
 * nativa do WebView — esta tirava a janela do app do modo tela cheia ao sair.
 */
/** `auto=false`: sem troca automática (só navegação manual) — ex.: imagens de um mod. */
export default function IgdbSlideshow({ slides, auto = true }: { slides: Slide[]; auto?: boolean }) {
  const n = slides.length;
  const [idx, setIdx] = useState(0);
  const [cycle, setCycle] = useState(0); // reinicia timer/vídeo mesmo quando o índice não muda
  const [paused, setPaused] = useState(!auto);
  const [muted, setMuted] = useState(true);
  const [ready, setReady] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [vidPaused, setVidPaused] = useState(false); // vídeo pausado pelo usuário (✕ / A em tela cheia)
  const [failed, setFailed] = useState(false);
  const [full, setFull] = useState(false);
  const frame = useRef<HTMLIFrameElement>(null);
  const layer = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const time = useRef(0); // posição atual do vídeo (para continuar ao entrar/sair da tela cheia)
  const mutedRef = useRef(true);
  mutedRef.current = muted;
  const readyRef = useRef(false);
  const isVideoRef = useRef(false);
  readyRef.current = ready;
  const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  const i = Math.min(idx, Math.max(0, n - 1));
  const cur = slides[i];
  isVideoRef.current = cur?.kind === "video";

  const go = useCallback((to: number) => {
    if (!n) return;
    setIdx(((to % n) + n) % n);
    setCycle((c) => c + 1);
  }, [n]);
  const next = useCallback(() => go(i + 1), [go, i]);

  // a lista mudou (outro jogo / atualização): volta ao começo
  const sig = slides.map((s) => (s.kind === "video" ? s.id : s.url)).join("|");
  useEffect(() => { setIdx(0); setCycle((c) => c + 1); }, [sig]);

  // cada item novo: vídeo começa do zero e sem som
  useEffect(() => { time.current = 0; setMuted(true); setFailed(false); }, [i, cycle]);
  // o player é recriado (novo item ou troca de tela cheia): aguarda ficar pronto de novo
  useEffect(() => { setReady(false); setPlaying(false); setVidPaused(false); }, [i, cycle, full]);

  // imagens: avança após 5 s (vídeos avançam quando terminam)
  useEffect(() => {
    if (!cur || cur.kind === "video" || paused || n < 2) return;
    const h = setTimeout(next, IMAGE_MS);
    return () => clearTimeout(h);
  }, [cur, paused, n, next, cycle]);

  const send = useCallback((func: string, args: unknown[] = []) => {
    frame.current?.contentWindow?.postMessage(JSON.stringify({ event: "command", func, args, id: 1, channel: "widget" }), YT_ORIGIN);
  }, []);

  // mensagens do player: pronto, tocando, fim do vídeo, erro, posição e estado do som
  useEffect(() => {
    if (cur?.kind !== "video") return;
    const onMsg = (e: MessageEvent) => {
      if (e.origin !== YT_ORIGIN || e.source !== frame.current?.contentWindow) return;
      let m: { event?: string; info?: unknown };
      try { m = typeof e.data === "string" ? JSON.parse(e.data) : e.data; } catch { return; }
      if (!m || typeof m !== "object") return;
      const info = m.info as Record<string, unknown> | number | null | undefined;
      if (m.event === "onReady" || m.event === "initialDelivery") {
        setReady(true);
        send("addEventListener", ["onStateChange"]);
        send("addEventListener", ["onError"]);
      }
      const state = m.event === "onStateChange" ? info : info && typeof info === "object" ? info.playerState : undefined;
      if (state === 1) { setPlaying(true); setVidPaused(false); }
      if (state === 2) setVidPaused(true); // 2 = pausado
      if (state === 0) { time.current = 0; if (!paused) next(); } // 0 = terminou
      if (m.event === "onError") setFailed(true);
      if (info && typeof info === "object") {
        if (typeof info.currentTime === "number") time.current = info.currentTime;
        if (typeof info.muted === "boolean") setMuted(info.muted);
      }
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  }, [cur, next, paused, send]);

  // vídeo que não pode ser incorporado: mostra o aviso e segue a apresentação depois de alguns segundos
  useEffect(() => {
    if (!failed || paused || n < 2) return;
    const h = setTimeout(next, IMAGE_MS);
    return () => clearTimeout(h);
  }, [failed, paused, n, next]);

  // "aperto de mão" com o player até ele responder
  const handshake = () => {
    let tries = 0;
    const tick = () => {
      if (!frame.current?.contentWindow || tries++ > 20) return;
      frame.current.contentWindow.postMessage(JSON.stringify({ event: "listening", id: 1, channel: "widget" }), YT_ORIGIN);
      setTimeout(() => { if (!readyRef.current) tick(); }, 400);
    };
    tick();
  };

  // endereço do player: sem a interface do YouTube; ao trocar de tela cheia continua do mesmo ponto
  const embed = useMemo(() => {
    if (cur?.kind !== "video") return "";
    const q = new URLSearchParams({
      autoplay: "1", mute: mutedRef.current ? "1" : "0", controls: "0", disablekb: "1", fs: "0", rel: "0",
      iv_load_policy: "3", modestbranding: "1", playsinline: "1", enablejsapi: "1", origin: location.origin,
      start: String(Math.floor(time.current)),
    });
    return `${YT_ORIGIN}/embed/${encodeURIComponent(cur.id)}?${q}`;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cur, cycle, full]);

  // tela cheia: a janela do app também vai para tela cheia se ainda não estiver, e volta ao sair
  const wasFull = useRef<boolean | null>(null);
  const pausedBefore = useRef(false); // em tela cheia a apresentação pausa; ao sair volta como estava
  const setFullscreen = (on: boolean) => {
    if (on === full) return;
    setFull(on);
    if (on) { pausedBefore.current = paused; setPaused(true); } else setPaused(pausedBefore.current);
    if (document.documentElement.classList.contains("overlay-mode")) return;
    if (on) {
      // estado real da janela (não a preferência salva): se já estava em tela cheia, continua nela ao sair
      api.isFullscreen().catch(() => true).then((v) => {
        wasFull.current = v;
        if (!wasFull.current) api.setFullscreen(true).catch(() => {});
      });
    } else {
      if (wasFull.current === false) api.setFullscreen(false).catch(() => {});
      wasFull.current = null;
      markLayerClosed();
      requestAnimationFrame(() => box.current?.querySelector<HTMLElement>(".igdb-stage[data-focus]")?.focus());
    }
  };
  const fullRef = useRef(setFullscreen);
  fullRef.current = setFullscreen;
  useEffect(() => () => { if (wasFull.current === false) api.setFullscreen(false).catch(() => {}); }, []);

  // na camada de tela cheia: o foco fica na própria camada (A não aciona nada escondido);
  // "Voltar" (Esc / B / ○) só sai da tela cheia; M (teclado) liga/desliga o som
  useEffect(() => {
    if (!full) return;
    requestAnimationFrame(() => layer.current?.focus());
    const onKey = (e: KeyboardEvent) => {
      const all = document.querySelectorAll(".overlay");
      if (all[all.length - 1] !== layer.current) return;
      if ((e.key === "m" || e.key === "M") && !e.ctrlKey && !e.altKey && !e.metaKey) {
        if (readyRef.current) { e.preventDefault(); muteRef.current(); }
        return;
      }
      // Espaço / Enter / K: pausa ou retoma o vídeo
      if ((e.key === " " || e.key === "Enter" || e.key === "k" || e.key === "K") && e.target === layer.current) {
        e.preventDefault();
        if (readyRef.current && isVideoRef.current) playRef.current();
        return;
      }
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopImmediatePropagation();
      fullRef.current(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [full]);

  // HUD da tela cheia: some depois de 5 s sem uso e volta com qualquer botão, tecla ou movimento do mouse
  const device = useInputDevice();
  const [awake, setAwake] = useState(true);
  useEffect(() => {
    if (!full) return;
    let h = 0;
    const wake = () => { setAwake(true); clearTimeout(h); h = window.setTimeout(() => setAwake(false), 5000); };
    wake();
    const evs = ["mousemove", "mousedown", "keydown", "click", "focusin", "nexstage:viewer", "nexstage:tab", "nexstage:button", "nexstage:context"];
    evs.forEach((e) => window.addEventListener(e, wake, true));
    return () => { clearTimeout(h); evs.forEach((e) => window.removeEventListener(e, wake, true)); };
  }, [full]);

  const toggleMute = () => {
    if (muted) { send("unMute"); send("setVolume", [100]); if (!vidPaused) send("playVideo"); } else send("mute");
    setMuted(!muted);
  };
  const togglePlay = () => { send(vidPaused ? "playVideo" : "pauseVideo"); setVidPaused(!vidPaused); };
  const playRef = useRef(togglePlay);
  playRef.current = togglePlay;
  const muteRef = useRef(toggleMute);
  muteRef.current = toggleMute;
  const stageEl = useRef<HTMLDivElement>(null);
  const isVideo = cur?.kind === "video";

  // tela cheia: ←/→ (analógico, direcional ou setas) trocam de item — a camada é marcada com data-viewer
  useEffect(() => {
    if (!full) return;
    const onNav = (e: Event) => {
      const all = document.querySelectorAll(".overlay");
      if (all[all.length - 1] !== layer.current) return;
      go(i + (e as CustomEvent<number>).detail);
    };
    window.addEventListener("nexstage:viewer", onNav);
    return () => window.removeEventListener("nexstage:viewer", onNav);
  }, [full, go, i]);

  // na página (mídia em foco): ←/→ trocam de item em vez de mover o foco
  useEffect(() => {
    const el = stageEl.current;
    if (full || !el) return;
    const onCap = (e: Event) => {
      const d = (e as CustomEvent<string>).detail;
      if ((d === "left" || d === "right") && n > 1) { e.preventDefault(); go(i + (d === "left" ? -1 : 1)); }
    };
    el.addEventListener("navcapture", onCap);
    return () => el.removeEventListener("navcapture", onCap);
  }, [full, go, i, n]);

  // □ (X no Xbox): liga/desliga o som do vídeo — em tela cheia ou com a mídia em foco na página
  useEffect(() => {
    if (!isVideo) return;
    return onPadButton((b, e) => {
      if (b !== "x" || e.defaultPrevented || !readyRef.current) return;
      const all = document.querySelectorAll(".overlay");
      const top = all[all.length - 1];
      const active = full ? top === layer.current : !top && document.activeElement === stageEl.current;
      if (!active) return;
      e.preventDefault();
      muteRef.current();
    });
  }, [isVideo, full]);

  if (!cur) return null;
  const label = cur.kind === "video" ? (cur.name || t("Vídeo")) : cur.kind === "artwork" ? t("Arte oficial") : t("Captura de tela");
  const pauseLabel = paused ? t("Retomar apresentação") : t("Pausar apresentação");

  /** Mídia atual + controles (na página ou na camada de tela cheia). */
  // Na página, a própria mídia é focável: A / clique abre a tela cheia; ←/→ trocam de item; □ liga o som.
  const stageProps = full ? {} : {
    ref: stageEl,
    "data-focus": true,
    "data-nav-capture": true,
    "data-video": cur.kind === "video" ? "" : undefined,
    tabIndex: 0,
    role: "button",
    "aria-label": `${label} · ${t("Tela cheia")}`,
    onClick: (e: RMouseEvent) => { if (!(e.target as HTMLElement).closest(".igdb-tools, .igdb-hud")) setFullscreen(true); },
    onKeyDown: (e: RKeyboardEvent) => {
      if ((e.key === "Enter" || e.key === " ") && e.target === e.currentTarget) { e.preventDefault(); setFullscreen(true); }
    },
  };
  const stage = (
    <div className={`igdb-stage ${full ? "full" : ""}`} {...stageProps}>
      {cur.kind === "video" ? (
        <>
          <iframe key={`${cur.id}-${cycle}-${full}`} ref={frame} src={embed} title={label} onLoad={handshake} tabIndex={-1}
            allow="autoplay; encrypted-media" referrerPolicy="strict-origin-when-cross-origin" />
          {/* capa do vídeo até ele começar a tocar: esconde o carregamento do player */}
          <img className={`igdb-poster ${playing && !failed ? "gone" : ""}`} src={ytThumb(cur.id)} alt="" />
          {full && vidPaused && !failed && <div className="igdb-paused" aria-hidden="true">❚❚</div>}
          {failed && <div className="igdb-stage-msg"><p>{t("Este vídeo não pode ser reproduzido aqui.")}</p></div>}
        </>
      ) : (
        <img key={`${cur.url}-${cycle}`} className={`igdb-pic ${reduced ? "" : "fade"}`} src={cur.url} alt="" />
      )}
      {full ? (
        // tela cheia: só a pílula com os comandos (glifos do controle em uso; clicáveis com o mouse)
        <div className="igdb-hud" aria-label={t("Comandos")}>
          {n > 1 && (
            <>
              <button type="button" tabIndex={-1} onClick={() => go(i - 1)} aria-label={t("Anterior")}>
                <ArrowKey dir="left" />{t("Anterior")}
              </button>
              <button type="button" tabIndex={-1} onClick={() => go(i + 1)} aria-label={t("Próxima")}>
                {t("Próxima")}<ArrowKey dir="right" />
              </button>
              <i aria-hidden="true" />
            </>
          )}
          {cur.kind === "video" && (
            <button type="button" tabIndex={-1} onClick={togglePlay} disabled={!ready}>
              <HudKey device={device} k="confirm" kb={t("Espaço")} />{vidPaused ? t("Reproduzir") : t("Pausar")}
            </button>
          )}
          {cur.kind === "video" && (
            <button type="button" tabIndex={-1} onClick={toggleMute} disabled={!ready} aria-pressed={!muted}>
              <HudKey device={device} k="x" kb="M" />{muted ? t("Ativar som") : t("Desativar som")}
            </button>
          )}
          <button type="button" tabIndex={-1} onClick={() => setFullscreen(false)}>
            <HudKey device={device} k="back" kb="Esc" />{t("Voltar")}
          </button>
        </div>
      ) : device !== "keyboard" ? (
        // na página, com controle: a mesma pílula da tela cheia, visível quando a mídia está em foco
        <div className="igdb-hud inline" aria-label={t("Comandos")}>
          <button type="button" tabIndex={-1} onClick={() => setFullscreen(true)}>
            <HudKey device={device} k="confirm" kb="Enter" />{t("Tela cheia")}
          </button>
          {n > 1 && (
            <>
              <i aria-hidden="true" />
              <button type="button" tabIndex={-1} onClick={() => go(i - 1)} aria-label={t("Anterior")}>
                <ArrowKey dir="left" />{t("Anterior")}
              </button>
              <button type="button" tabIndex={-1} onClick={() => go(i + 1)} aria-label={t("Próxima")}>
                {t("Próxima")}<ArrowKey dir="right" />
              </button>
            </>
          )}
          {cur.kind === "video" && (
            <>
              <i aria-hidden="true" />
              <button type="button" tabIndex={-1} onClick={toggleMute} disabled={!ready} aria-pressed={!muted}>
                <HudKey device={device} k="x" kb="M" />{muted ? t("Ativar som") : t("Desativar som")}
              </button>
            </>
          )}
        </div>
      ) : (
      <div className="igdb-tools">
        {n > 1 && auto && <Button onClick={() => setPaused(!paused)} title={pauseLabel} aria-label={pauseLabel}>{paused ? "▶" : "❚❚"}</Button>}
        {cur.kind === "video" && (
          <>
            <Button onClick={toggleMute} disabled={!ready} aria-pressed={!muted}>{muted ? t("Ativar som") : t("Desativar som")}</Button>
            <Button onClick={() => openUrl(`https://www.youtube.com/watch?v=${cur.id}`).catch(() => {})}>{t("Abrir no YouTube")}</Button>
          </>
        )}
        <Button className="igdb-fs-open" onClick={() => setFullscreen(true)}>{t("Tela cheia")}</Button>
      </div>
      )}
      {cur.kind !== "video" && !paused && n > 1 && !reduced && <i key={`bar-${i}-${cycle}`} className="igdb-timer" aria-hidden="true" />}
    </div>
  );

  return (
    <div className="igdb-media" ref={box}>
      {full ? (
        <>
          {/* lugar reservado na página enquanto a mídia está em tela cheia */}
          <div className="igdb-stage">
            <img className="igdb-pic" src={cur.kind === "video" ? ytThumb(cur.id) : cur.url} alt="" />
          </div>
          {createPortal(
            <div ref={layer} tabIndex={-1} className={`overlay igdb-fs ${awake ? "awake" : ""}`} data-viewer role="dialog" aria-modal="true" aria-label={label}
              onClick={(e) => { if (isVideoRef.current && readyRef.current && !(e.target as HTMLElement).closest(".igdb-hud")) playRef.current(); }}>{stage}</div>,
            document.body,
          )}
        </>
      ) : stage}
      <div className="row">
        <span className="muted">{label} · {i + 1}/{n}</span>
      </div>
      <div className="shots">
        {slides.map((s, k) => (
          <button key={s.kind === "video" ? `v-${s.id}` : s.url} data-focus type="button" className={`shot ${s.kind === "video" ? "vid" : ""} ${k === i ? "on" : ""}`}
            onClick={() => go(k)} aria-label={s.kind === "video" ? s.name || t("Vídeo") : undefined}>
            <img src={s.kind === "video" ? ytThumb(s.id, "mqdefault") : s.url.replace("/t_1080p/", "/t_screenshot_med/")} alt="" loading="lazy" />
            {s.kind === "video" && <i aria-hidden="true">▶</i>}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Tecla da pílula de comandos: glifo do controle em uso (ícone no PlayStation) ou a tecla do teclado. */
function HudKey({ device, k, kb }: { device: Device; k: GlyphKey; kb: string }) {
  if (device === "keyboard") return <kbd>{kb}</kbd>;
  const shape = device === "playstation" ? PS_SHAPES[k] : undefined;
  return (
    <kbd className={shape ? "ps" : undefined}>
      {shape ? <svg viewBox="0 0 16 16" aria-hidden="true">{shape}</svg> : GLYPHS[device][k]}
    </kbd>
  );
}

/** Anterior / próxima: só a seta do lado (vale para controle — analógico ou direcional — e teclado). */
function ArrowKey({ dir }: { dir: "left" | "right" }) {
  return (
    <kbd className="ps arrow">
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d={dir === "left" ? "M10 3.5L5.5 8l4.5 4.5" : "M6 3.5L10.5 8 6 12.5"} /></svg>
    </kbd>
  );
}
