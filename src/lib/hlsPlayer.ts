/**
 * Player HLS mínimo para os trailers do Xbox (cdn.trailers.xboxservices.com), sem biblioteca externa.
 *
 * Esses trailers são HLS com fragmentos MP4 (CMAF): uma lista principal com as resoluções de vídeo (cada
 * uma com a sua lista de fragmentos + `#EXT-X-MAP` de inicialização) e uma faixa de áudio separada.
 * O motor do app (WebView2/Chromium) toca isso pelas Media Source Extensions: um SourceBuffer de vídeo e
 * outro de áudio, alimentados em ordem, mantendo ~30 s à frente do ponto atual (memória contida).
 *
 * Qualidade automática entre 480p e 1080p: começa numa resolução média (abre rápido) e, a cada fragmento
 * baixado, mede a velocidade da conexão e escolhe a maior resolução que ela sustenta com folga — sobe ou
 * desce sozinha, nunca abaixo de 480p (se o trailer não tiver 480p ou mais, usa as que existirem). (O modo HLS nativo do motor começava em 320×180 e não subia a tempo nos trailers curtos.)
 */

interface Track { init: string; segments: { url: string; start: number; dur: number }[] }
export interface Variant { url: string; bw: number; h: number }

const AHEAD = 30; // segundos de vídeo baixados à frente
const BEHIND = 60; // segundos mantidos para trás (o resto é liberado)
const START_BW = 1_600_000; // primeira resolução: a maior até ~1,6 Mbps (540p nos trailers do Xbox)
const MIN_HEIGHT = 480; // resolução mínima
const SAFETY = 0.7; // usa só 70% da velocidade medida
const VIDEO_MIME = 'video/mp4; codecs="avc1.4D402A"';
const AUDIO_MIME = 'audio/mp4; codecs="mp4a.40.2"';

const attr = (line: string, key: string): string | null => {
  const m = new RegExp(`(?:^|[,:])${key}=("([^"]*)"|[^,]*)`).exec(line);
  return m ? (m[2] ?? m[1]) : null;
};

async function text(url: string, signal: AbortSignal): Promise<string> {
  const r = await fetch(url, { signal });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.text();
}

/** Lista principal → resoluções de vídeo até 1080p (da menor para a maior) e a faixa de áudio. */
export function parseMaster(master: string, base: string): { variants: Variant[]; audio: string | null } {
  const lines = master.split(/\r?\n/);
  let audio: string | null = null;
  const variants: Variant[] = [];
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i].trim();
    if (l.startsWith("#EXT-X-MEDIA:") && attr(l, "TYPE") === "AUDIO" && !audio) {
      const uri = attr(l, "URI");
      if (uri) audio = new URL(uri, base).href;
    } else if (l.startsWith("#EXT-X-STREAM-INF:")) {
      const bw = Number(attr(l, "BANDWIDTH") ?? 0);
      const h = Number((attr(l, "RESOLUTION") ?? "0x0").split("x")[1] ?? 0);
      const uri = lines.slice(i + 1).find((x) => x.trim() && !x.startsWith("#"))?.trim();
      if (uri && h <= 1080 && bw > 0) variants.push({ url: new URL(uri, base).href, bw, h });
    }
  }
  variants.sort((a, b) => a.bw - b.bw);
  // mínimo de 480p (trailer sem nenhuma resolução de 480p ou mais: fica com as que tiver)
  const hd = variants.filter((v) => v.h >= MIN_HEIGHT);
  return { variants: hd.length ? hd : variants, audio };
}

/** Maior resolução que cabe na taxa (bits/s); nenhuma cabe: a menor. */
export function chooseLevel(variants: Variant[], bps: number): number {
  let pick = 0;
  variants.forEach((v, i) => { if (v.bw <= bps) pick = i; });
  return pick;
}

/** Lista de fragmentos (`#EXT-X-MAP` + `#EXTINF`) com o início de cada um. */
export function parseMedia(playlist: string, base: string): Track | null {
  let init = "";
  let t = 0;
  let dur = 0;
  const segments: Track["segments"] = [];
  for (const raw of playlist.split(/\r?\n/)) {
    const l = raw.trim();
    if (l.startsWith("#EXT-X-MAP:")) { const u = attr(l, "URI"); if (u) init = new URL(u, base).href; }
    else if (l.startsWith("#EXTINF:")) dur = parseFloat(l.slice(8)) || 0;
    else if (l && !l.startsWith("#")) { segments.push({ url: new URL(l, base).href, start: t, dur }); t += dur; dur = 0; }
  }
  return init && segments.length ? { init, segments } : null;
}

const waitUpdate = (sb: SourceBuffer) =>
  sb.updating ? new Promise<void>((ok) => sb.addEventListener("updateend", () => ok(), { once: true })) : Promise.resolve();

const bufferedEnd = (sb: SourceBuffer, at: number): number => {
  for (let i = 0; i < sb.buffered.length; i++) {
    if (sb.buffered.start(i) <= at + 0.25 && sb.buffered.end(i) > at) return sb.buffered.end(i);
  }
  return at;
};

/**
 * Liga o trailer ao <video>. Devolve a função que encerra tudo (downloads e MediaSource).
 * `onError` recebe falhas que impedem a reprodução.
 */
export function playHls(video: HTMLVideoElement, masterUrl: string, onError: (e: unknown) => void): () => void {
  const ctl = new AbortController();
  const signal = ctl.signal;
  if (typeof MediaSource === "undefined" || !MediaSource.isTypeSupported(VIDEO_MIME)) {
    onError(new Error("MSE indisponível"));
    return () => ctl.abort();
  }
  const ms = new MediaSource();
  const objUrl = URL.createObjectURL(ms);
  video.src = objUrl;
  const seekers: (() => void)[] = [];
  const onSeeking = () => seekers.forEach((f) => f());
  video.addEventListener("seeking", onSeeking);
  const sleep = (t: number) => new Promise((ok) => setTimeout(ok, t));

  // fim dos dois fluxos baixado: avisa o fim (o vídeo termina); pular para trás reabre o MediaSource
  const done = new Map<SourceBuffer, boolean>();
  const maybeEnd = async () => {
    if (signal.aborted || ms.readyState !== "open" || ![...done.values()].every(Boolean)) return;
    await Promise.all([...done.keys()].map(waitUpdate));
    if (ms.readyState === "open" && [...done.values()].every(Boolean)) { try { ms.endOfStream(); } catch { /* já encerrado */ } }
  };

  /**
   * Alimenta um SourceBuffer. `track()` devolve a lista a usar agora (no vídeo, a da resolução escolhida);
   * ao trocar de lista, o segmento de inicialização dela é enviado antes do próximo fragmento.
   * `measured` recebe bytes e milissegundos de cada fragmento (velocidade da conexão).
   */
  const feed = async (sb: SourceBuffer, track: () => Promise<Track>, measured?: (bytes: number, ms: number) => void) => {
    done.set(sb, false);
    let next = 0; // início do próximo trecho a baixar (s)
    let jumped = true;
    let inited: Track | null = null;
    seekers.push(() => { jumped = true; });
    while (!signal.aborted) {
      const now = video.currentTime;
      if (jumped) { jumped = false; next = bufferedEnd(sb, now); }
      const tr = await track();
      if (signal.aborted) return;
      const idx = tr.segments.findIndex((s) => s.start + s.dur > next + 0.05);
      if (idx < 0) {
        if (!done.get(sb)) { done.set(sb, true); await maybeEnd(); }
        await sleep(500);
        continue;
      }
      done.set(sb, false);
      if (bufferedEnd(sb, now) - now > AHEAD) { await sleep(500); continue; }
      if (inited !== tr) {
        const init = await (await fetch(tr.init, { signal })).arrayBuffer();
        await waitUpdate(sb);
        sb.appendBuffer(init);
        await waitUpdate(sb);
        inited = tr;
      }
      const seg = tr.segments[idx];
      const t0 = performance.now();
      const data = await (await fetch(seg.url, { signal })).arrayBuffer();
      if (signal.aborted) return;
      measured?.(data.byteLength, performance.now() - t0);
      if (jumped) continue; // o usuário pulou enquanto baixava: recalcula
      await waitUpdate(sb);
      // libera o que ficou muito para trás (limite de memória do MediaSource)
      if (now > BEHIND && sb.buffered.length && sb.buffered.start(0) < now - BEHIND) {
        sb.remove(0, now - BEHIND);
        await waitUpdate(sb);
      }
      try {
        sb.appendBuffer(data);
      } catch (e) {
        if ((e as DOMException).name === "QuotaExceededError") { await sleep(1000); continue; }
        throw e;
      }
      await waitUpdate(sb);
      next = seg.start + seg.dur;
    }
  };

  ms.addEventListener("sourceopen", async () => {
    try {
      const master = await text(masterUrl, signal);
      const { variants, audio } = parseMaster(master, masterUrl);
      if (!variants.length) throw new Error("trailer sem resolução compatível");
      // listas de fragmentos de cada resolução, buscadas só quando usadas
      const lists = new Map<number, Promise<Track>>();
      const listOf = (lv: number) => {
        let p = lists.get(lv);
        if (!p) {
          p = text(variants[lv].url, signal).then((s) => {
            const tr = parseMedia(s, variants[lv].url);
            if (!tr) throw new Error("lista de vídeo inválida");
            return tr;
          });
          lists.set(lv, p);
        }
        return p;
      };
      let level = chooseLevel(variants, START_BW);
      let bps = 0; // velocidade estimada (média móvel)
      const first = await listOf(level);
      const aTrack = audio ? parseMedia(await text(audio, signal), audio) : null;
      const total = first.segments.reduce((s, x) => s + x.dur, 0);
      if (total > 0) ms.duration = total;
      const vsb = ms.addSourceBuffer(VIDEO_MIME);
      const asb = aTrack && MediaSource.isTypeSupported(AUDIO_MIME) ? ms.addSourceBuffer(AUDIO_MIME) : null;
      const onMeasure = (bytes: number, took: number) => {
        if (took < 1 || bytes < 20_000) return; // medição curta demais (cache, fragmento minúsculo)
        const now = (bytes * 8 * 1000) / took;
        bps = bps ? bps * 0.6 + now * 0.4 : now;
        level = chooseLevel(variants, bps * SAFETY);
      };
      await Promise.all([
        feed(vsb, () => listOf(level), onMeasure),
        asb && aTrack ? feed(asb, () => Promise.resolve(aTrack)) : Promise.resolve(),
      ]);
    } catch (e) {
      if (!signal.aborted) onError(e);
    }
  }, { once: true });

  return () => {
    ctl.abort();
    video.removeEventListener("seeking", onSeeking);
    try { if (ms.readyState === "open") ms.endOfStream(); } catch { /* já encerrado */ }
    video.removeAttribute("src");
    video.load();
    URL.revokeObjectURL(objUrl);
  };
}
