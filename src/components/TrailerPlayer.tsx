import { useCallback, useEffect, useRef, useState } from "react";
import { playHls } from "../lib/hlsPlayer";
import { api } from "../lib/api";
import { play as sfx } from "../lib/sound";
import { onPadButton } from "../lib/nav";
import { useInputDevice } from "../lib/inputDevice";
import { t } from "../lib/i18n";
import { VideoFullHud, FULL_SEEK, clock } from "./VideoHud";

/** Trailer a reproduzir: do Xbox (HLS, player próprio) ou do YouTube (player incorporado, sem cookies). */
export type TrailerSource =
  | { kind: "xbox"; key: string; title: string; hls: string; poster: string | null }
  | { kind: "youtube"; key: string; title: string; id: string };

const YT_ORIGIN = "https://www.youtube-nocookie.com";
const ytThumb = (id: string) => `https://i.ytimg.com/vi/${encodeURIComponent(id)}/hqdefault.jpg`;
const SEEK = 10;

const Ico = ({ d }: { d: string }) => (
  <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={d} /></svg>
);
const I = {
  play: "M8 5.5v13l11-6.5z",
  pause: "M8 5v14M16 5v14",
  back: "M11 7l-5 5 5 5M18 7l-5 5 5 5",
  fwd: "M13 7l5 5-5 5M6 7l5 5-5 5",
  vol: "M4 9v6h4l5 4V5L8 9H4zM16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11",
  mute: "M4 9v6h4l5 4V5L8 9H4zM17 9l5 6M22 9l-5 6",
  full: "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5",
  exitFull: "M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5",
  close: "M6 6l12 12M18 6L6 18",
};

/**
 * Bloco de trailers: capa com botão de reproduzir (nada é baixado antes do clique) e, com mais de um
 * trailer, a lista para escolher. Os controles são do app (alcançáveis pelo controle e pelo teclado), sem os
 * controles nativos do vídeo. A tela cheia também é do app — como no visualizador de capturas: a janela vai
 * para tela cheia se ainda não estiver e volta ao modo anterior ao sair (a tela cheia nativa do navegador
 * tirava o app da tela cheia ao sair).
 * Trailers do Xbox tocam no player do app; os do YouTube, no player incorporado do YouTube (domínio sem
 * cookies), comandado pela API de mensagens do próprio player.
 */
export default function TrailerPlayer({ sources }: { sources: TrailerSource[] }) {
  const device = useInputDevice();
  const [sel, setSel] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [failed, setFailed] = useState(false);
  const [paused, setPaused] = useState(true);
  const [muted, setMuted] = useState(false);
  const [time, setTime] = useState(0);
  const [dur, setDur] = useState(0);
  const [full, setFull] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const video = useRef<HTMLVideoElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  const cur = sources[Math.min(sel, sources.length - 1)];

  // ---------- tela cheia do app (preserva o modo da janela) ----------
  const wasFull = useRef<boolean | null>(null);
  const fullOn = useRef(false);
  const setFullscreen = useCallback((on: boolean) => {
    setFull((prev) => {
      if (prev === on) return prev;
      fullOn.current = on;
      sfx(on ? "open" : "close");
      if (!document.documentElement.classList.contains("overlay-mode")) {
        if (on) {
          api.isFullscreen().catch(() => true).then((v) => {
            if (!fullOn.current) return;
            wasFull.current = v;
            if (!v) api.setFullscreen(true).catch(() => {});
          });
        } else {
          if (wasFull.current === false) api.setFullscreen(false).catch(() => {});
          wasFull.current = null;
        }
      }
      // foco: na tela cheia fica na própria camada (sem botões focáveis); ao sair, no botão de tela cheia
      requestAnimationFrame(() => (on ? root.current : root.current?.querySelector<HTMLElement>(".trl-fs"))?.focus({ preventScroll: true }));
      return on;
    });
  }, []);
  // sair da página com a tela cheia aberta também devolve a janela ao modo anterior
  useEffect(() => () => { if (wasFull.current === false) api.setFullscreen(false).catch(() => {}); }, []);

  // ---------- YouTube: comandos pela API de mensagens do player ----------
  const yt = useCallback((func: string, args: unknown[] = []) => {
    frame.current?.contentWindow?.postMessage(JSON.stringify({ event: "command", func, args, id: 1, channel: "widget" }), YT_ORIGIN);
  }, []);
  useEffect(() => {
    if (!playing || cur?.kind !== "youtube") return;
    const onMsg = (e: MessageEvent) => {
      if (e.origin !== YT_ORIGIN || e.source !== frame.current?.contentWindow || typeof e.data !== "string") return;
      try {
        const m = JSON.parse(e.data);
        const info = m?.info;
        if (m?.event === "onStateChange" && typeof info === "number") setPaused(info !== 1 && info !== 3);
        if (m?.event === "infoDelivery" && info) {
          if (typeof info.currentTime === "number") setTime(info.currentTime);
          if (typeof info.duration === "number") setDur(info.duration);
          if (typeof info.muted === "boolean") setMuted(info.muted);
          if (typeof info.playerState === "number") setPaused(info.playerState !== 1 && info.playerState !== 3);
        }
      } catch { /* mensagem de outro formato */ }
    };
    window.addEventListener("message", onMsg);
    return () => window.removeEventListener("message", onMsg);
  }, [playing, cur]);
  const handshake = () => frame.current?.contentWindow?.postMessage(JSON.stringify({ event: "listening", id: 1, channel: "widget" }), YT_ORIGIN);

  // trocar de trailer volta para a capa (e sai da tela cheia)
  useEffect(() => { setPlaying(false); setFailed(false); setTime(0); setDur(0); setPaused(true); }, [cur?.key]);

  // ---------- Xbox: player próprio ----------
  useEffect(() => {
    if (!playing || cur?.kind !== "xbox" || !video.current) return;
    const v = video.current;
    const stop = playHls(v, cur.hls, () => setFailed(true));
    v.muted = muted;
    v.play().catch(() => { /* o usuário aperta reproduzir nos controles */ });
    return stop;
  }, [playing, cur]); // eslint-disable-line react-hooks/exhaustive-deps

  const start = () => {
    setPlaying(true);
    setPaused(false);
    requestAnimationFrame(() => root.current?.querySelector<HTMLElement>(".trl-pp")?.focus({ preventScroll: true }));
  };
  const stopAll = () => {
    setFullscreen(false);
    setPlaying(false);
    requestAnimationFrame(() => root.current?.querySelector<HTMLElement>(".trl-cover")?.focus({ preventScroll: true }));
  };
  const toggle = () => {
    if (cur.kind === "xbox") {
      const v = video.current;
      if (!v) return;
      if (v.paused) v.play().catch(() => {}); else v.pause();
    } else {
      yt(paused ? "playVideo" : "pauseVideo");
      setPaused(!paused);
    }
  };
  const seekTo = (s: number) => {
    const to = Math.max(0, Math.min(dur || s, s));
    if (cur.kind === "xbox") { if (video.current) video.current.currentTime = to; }
    else yt("seekTo", [to, true]);
    setTime(to);
  };
  const toggleMute = () => {
    const m = !muted;
    setMuted(m);
    if (cur.kind === "xbox") { if (video.current) video.current.muted = m; }
    else yt(m ? "mute" : "unMute");
  };

  // referências para os comandos dos botões físicos (sempre com o estado atual)
  const act = useRef({ seekBy: (_d: number) => {}, mute: () => {}, close: () => {}, play: () => {} });
  act.current = { seekBy: (d: number) => seekTo(time + d), mute: toggleMute, close: stopAll, play: toggle };

  // Na página: Y / △ (ou F) abre a tela cheia com o foco no trailer.
  // Na tela cheia, só: ← volta 5 s, → avança 5 s, □ (M) liga/desliga o som, ○ (Esc) fecha o player.
  useEffect(() => {
    if (!playing) return;
    const inside = () => !!root.current && root.current.contains(document.activeElement);
    const onKey = (e: KeyboardEvent) => {
      if (full) {
        if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); sfx("back"); act.current.close(); return; }
        if ((e.key === "m" || e.key === "M") && !e.ctrlKey && !e.altKey && !e.metaKey) { e.preventDefault(); act.current.mute(); }
        // Espaço / K: pausa ou retoma (✕ / A / Enter chegam como clique na camada — ver onClick abaixo)
        if ((e.key === " " || e.key === "k" || e.key === "K") && e.target === root.current) { e.preventDefault(); act.current.play(); }
        return;
      }
      if ((e.key === "f" || e.key === "F") && !e.ctrlKey && !e.altKey && !e.metaKey && inside() && !(e.target instanceof HTMLInputElement)) {
        e.preventDefault(); setFullscreen(true);
      }
    };
    const off = onPadButton((b, e) => {
      if (e.defaultPrevented) return;
      if (full) {
        if (b === "x") { e.preventDefault(); act.current.mute(); }
        return;
      }
      if (b === "y" && inside()) { e.preventDefault(); setFullscreen(true); }
    });
    // ← → do controle/teclado viram -5 s / +5 s; ↑ ↓ não fazem nada
    const onDir = (e: Event) => {
      if (!full) return;
      e.preventDefault();
      const d = (e as CustomEvent<string>).detail;
      if (d === "left") act.current.seekBy(-FULL_SEEK);
      else if (d === "right") act.current.seekBy(FULL_SEEK);
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("nexstage:direction", onDir);
    return () => { off(); window.removeEventListener("keydown", onKey, true); window.removeEventListener("nexstage:direction", onDir); };
  }, [playing, full, setFullscreen]);

  // pílula de comandos da tela cheia: some depois de 4 s sem uso e volta com qualquer botão, tecla ou mouse
  const [awake, setAwake] = useState(true);
  useEffect(() => {
    if (!full) return;
    let h = 0;
    const wake = () => { setAwake(true); clearTimeout(h); h = window.setTimeout(() => setAwake(false), 4000); };
    wake();
    const evs = ["mousemove", "mousedown", "keydown", "nexstage:direction", "nexstage:button"];
    evs.forEach((e) => window.addEventListener(e, wake, true));
    return () => { clearTimeout(h); evs.forEach((e) => window.removeEventListener(e, wake, true)); };
  }, [full]);

  if (!cur) return null;
  const poster = cur.kind === "xbox" ? cur.poster : ytThumb(cur.id);
  const embed = cur.kind === "youtube"
    ? `${YT_ORIGIN}/embed/${encodeURIComponent(cur.id)}?${new URLSearchParams({ autoplay: "1", controls: "0", fs: "0", disablekb: "1", rel: "0", modestbranding: "1", playsinline: "1", iv_load_policy: "3", enablejsapi: "1", origin: location.origin })}`
    : "";
  const fsKey = device === "keyboard" ? "F" : device === "playstation" ? "△" : device === "nintendo" ? "X" : "Y";

  return (
    <div ref={root} className={`trl ${full ? `trl-full ${awake ? "awake" : ""}` : ""}`} tabIndex={full ? -1 : undefined}
      role={full ? "dialog" : undefined} aria-modal={full || undefined} aria-label={full ? cur.title : undefined}
      onClick={(e) => { if (full && e.target === e.currentTarget && playing && !failed) toggle(); }}>
      <div className="trl-stage" onDoubleClick={() => playing && !failed && setFullscreen(!full)}>
        {!playing ? (
          <button type="button" data-focus className="trl-cover" onClick={start}
            aria-label={t("Reproduzir trailer: {title}", { title: cur.title })}
            style={poster ? { backgroundImage: `url("${poster}")` } : undefined}>
            <span className="trl-play" aria-hidden="true">
              <svg viewBox="0 0 24 24" width="34" height="34" fill="currentColor"><path d={I.play} /></svg>
            </span>
            <span className="trl-cap">{cur.title}</span>
          </button>
        ) : cur.kind === "xbox" ? (
          failed
            ? <div className="trl-msg"><p>{t("Não foi possível reproduzir este trailer.")}</p></div>
            : <video ref={video} className="trl-video" playsInline disablePictureInPicture poster={cur.poster ?? undefined} onClick={toggle}
                onPlay={() => setPaused(false)} onPause={() => setPaused(true)} onEnded={() => setPaused(true)}
                onTimeUpdate={(e) => setTime(e.currentTarget.currentTime)} onDurationChange={(e) => setDur(Number.isFinite(e.currentTarget.duration) ? e.currentTarget.duration : 0)} />
        ) : (
          <iframe ref={frame} className="trl-video" src={embed} title={cur.title} onLoad={handshake} tabIndex={-1}
            allow="autoplay; encrypted-media" referrerPolicy="strict-origin-when-cross-origin" />
        )}
        {/* pausado em tela cheia: indicação no centro (✕ / A retoma) */}
        {full && playing && paused && !failed && <div className="igdb-paused" aria-hidden="true">❚❚</div>}
      </div>

      {full && playing && (
        <VideoFullHud device={device} time={time} dur={dur} muted={muted} paused={paused} ready={!failed}
          onPlay={toggle} onSeek={(d) => seekTo(time + d)} onMute={toggleMute} onClose={stopAll} closeLabel={t("Fechar")} />
      )}

      {playing && !failed && !full && (
        <div className="trl-bar" data-nav-group>
          <button type="button" data-focus className="trl-btn trl-pp" onClick={toggle} aria-label={paused ? t("Reproduzir") : t("Pausar")} title={paused ? t("Reproduzir") : t("Pausar")}>
            <Ico d={paused ? I.play : I.pause} />
          </button>
          <button type="button" data-focus className="trl-btn" onClick={() => seekTo(time - SEEK)} aria-label={t("Voltar {n} s", { n: SEEK })} title={t("Voltar {n} s", { n: SEEK })}><Ico d={I.back} /></button>
          <button type="button" data-focus className="trl-btn" onClick={() => seekTo(time + SEEK)} aria-label={t("Avançar {n} s", { n: SEEK })} title={t("Avançar {n} s", { n: SEEK })}><Ico d={I.fwd} /></button>
          {/* linha do tempo: só com o mouse (sem foco no controle/teclado, para ← → passarem pelos botões) */}
          <input type="range" tabIndex={-1} data-no-nav className="trl-seek" onPointerUp={(e) => e.currentTarget.blur()} min={0} max={Math.max(1, Math.floor(dur))} step={5} value={Math.floor(time)}
            onChange={(e) => seekTo(Number(e.currentTarget.value))} aria-label={t("Posição do trailer")}
            aria-valuetext={`${clock(time)} / ${clock(dur)}`} />
          <span className="trl-time">{clock(time)} / {clock(dur)}</span>
          <button type="button" data-focus className="trl-btn" onClick={toggleMute} aria-label={muted ? t("Ativar som") : t("Silenciar")} title={muted ? t("Ativar som") : t("Silenciar")}>
            <Ico d={muted ? I.mute : I.vol} />
          </button>
          <button type="button" data-focus className="trl-btn trl-fs" onClick={() => setFullscreen(!full)}
            aria-label={full ? t("Sair da tela cheia") : t("Tela cheia")} title={`${full ? t("Sair da tela cheia") : t("Tela cheia")} (${fsKey})`}>
            <Ico d={full ? I.exitFull : I.full} />
          </button>
          <button type="button" data-focus className="trl-btn" onClick={stopAll} aria-label={t("Fechar trailer")} title={t("Fechar trailer")}><Ico d={I.close} /></button>
        </div>
      )}

      {!full && sources.length > 1 && (
        <div className="trl-list" data-nav-group>
          {sources.map((s, i) => (
            <button key={s.key} type="button" data-focus className={`trl-item ${i === sel ? "on" : ""}`} aria-pressed={i === sel} onClick={() => setSel(i)}>
              <span className="trl-thumb" style={{ backgroundImage: `url("${s.kind === "xbox" ? s.poster ?? "" : ytThumb(s.id)}")` }} />
              <span className="trl-item-text">
                <strong>{s.title}</strong>
                <small>{s.kind === "xbox" ? t("Trailer do Xbox") : t("YouTube (via IGDB)")}</small>
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
