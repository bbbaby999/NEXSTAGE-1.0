import { CSSProperties, useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import { itemSrc } from "./Viewer";
import { formatDate } from "../lib/format";
import { onPadActivity } from "../lib/inputDevice";
import { reducedMotion } from "../lib/motion";
import {
  bumpIdleBusy, getIdleConfig, idleBusyNow, isIdleActive, registerIdleHost, setIdleActive, setIdleBusyFlag,
  useIdleConfig, type IdleMode,
} from "../lib/idle";
import { t } from "../lib/i18n";
import { steamHero } from "../lib/steam";
import { COMPLETION_LABEL, fmtDay, fmtHours } from "../lib/zerei";

/** Selo da legenda: um dado por selo (conclusão, data, tempo de jogo, plataforma), com ícone. */
type TagIcon = "trophy" | "calendar" | "clock" | "platform";
interface Tag { icon: TagIcon; text: string; gold?: boolean }
const TAG_ICON: Record<TagIcon, string> = {
  trophy: "M8 4h8v3a4 4 0 0 1-8 0V4ZM8 5H5v1.5A2.5 2.5 0 0 0 7.5 9H8M16 5h3v1.5A2.5 2.5 0 0 1 16.5 9H16M12 11v4M9 20h6M10 15h4v5h-4z",
  calendar: "M4 6h16v14H4V6ZM4 10h16M8 3v4M16 3v4",
  clock: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18ZM12 7v5l3 2",
  platform: "M7 8h10a4 4 0 0 1 4 4v1.5a3.5 3.5 0 0 1-6.3 2.1L14 14.5h-4l-.7 1.1A3.5 3.5 0 0 1 3 13.5V12a4 4 0 0 1 4-4ZM7.5 10.5v3M6 12h3",
};

/* =====================================================================================
 * Apresentação (usada em tela cheia e na pré-visualização das Configurações)
 * ===================================================================================== */

type Kind = "art" | "cover" | "shot";
/** `logo`: logo do jogo (no lugar do título; nas capturas, o do jogo da captura). */
interface Item { src: string; fallback: string | null; kind: Kind; x: number; y: number; title: string | null; tags: Tag[]; logo: string | null }
/** `logo`: o do item, se carregou (senão o título aparece em texto). */
interface Layer { k: number; src: string; kind: Kind; item: Item; kb: CSSProperties; logo: string | null }

/** Tempo de cada imagem e duração do crossfade (ms). */
const HOLD: Record<Exclude<IdleMode, "off">, number> = { library: 9000, completed: 9000, captures: 7000 };
const FADE = 1800;

/**
 * Sorteio sem repetição ("saco embaralhado"): passa por todas as imagens antes de repetir e, ao
 * embaralhar de novo, nunca começa pela última exibida (sem repetição consecutiva quando há mais de uma).
 */
function makeBag(n: number) {
  let q: number[] = [];
  let last = -1;
  return () => {
    if (!q.length) {
      q = Array.from({ length: n }, (_, i) => i);
      for (let i = q.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [q[i], q[j]] = [q[j], q[i]]; }
      if (n > 1 && q[q.length - 1] === last) [q[0], q[q.length - 1]] = [q[q.length - 1], q[0]];
    }
    last = q.pop()!;
    return last;
  };
}

/** Carrega e decodifica a imagem antes de exibir (sem "pipocar"); o objeto é descartado em seguida. */
async function preload(src: string): Promise<boolean> {
  const im = new Image();
  im.decoding = "async";
  im.src = src;
  try { await im.decode(); return im.naturalWidth > 0; } catch { return false; }
}

/**
 * Movimento lento de câmera: sempre um leve zoom in (a imagem preenche a tela o tempo todo), com um
 * pequeno deslocamento sorteado por imagem — abaixo da folga que a escala inicial dá nas bordas.
 */
function kenBurns(ms: number): CSSProperties {
  if (reducedMotion()) return { animation: "none" };
  const r = (a: number) => (Math.random() * 2 - 1) * a;
  const small = 1.02, big = 1.08, pan = 0.7;
  return {
    "--kb-s0": small, "--kb-s1": big,
    "--kb-x0": `${r(pan)}%`, "--kb-y0": `${r(pan)}%`, "--kb-x1": `${r(pan)}%`, "--kb-y1": `${r(pan)}%`,
    animationDuration: `${ms}ms`,
  } as CSSProperties;
}

export function IdleShow({ mode, profileId, hint = false }: { mode: Exclude<IdleMode, "off">; profileId: number; hint?: boolean }) {
  const [items, setItems] = useState<Item[] | null>(null);
  const [layers, setLayers] = useState<Layer[]>([]);
  const counter = useRef(0);

  // Reaproveita os mesmos serviços da Biblioteca e da Galeria (imagens já salvas; nada é copiado).
  useEffect(() => {
    let dead = false;
    setItems(null);
    setLayers([]);
    const load = mode === "library"
      // Só planos de fundo (nunca capas): o cadastrado no jogo e, sem ele, a arte horizontal da Steam
      // (o mesmo recurso que a Home usa). Jogos sem nenhum dos dois ficam fora da apresentação.
      ? api.listLibrary(profileId).then((games) => games
        .filter((g) => g.background || g.steamId)
        .map((g): Item => {
          const hero = g.steamId ? steamHero(g.steamId) : null;
          return {
            src: (g.background ?? hero)!, fallback: g.background ? hero : null, kind: "art",
            x: g.background ? g.bgX : 50, y: g.background ? g.bgY : 50, title: g.title, tags: [], logo: g.logo,
          };
        }))
      // Zerados (Zerei!): só jogos com plano de fundo (sem ele, o jogo fica fora — capa não entra); legenda "Zerado/Platinado · data · plataforma".
      // Várias jogatinas do mesmo jogo/plataforma entram uma vez só (a mais recente).
      : mode === "completed"
      ? api.zereiList(profileId).then((list) => {
        const seen = new Set<string>();
        return list
          .filter((e) => e.status === "completed" && !!e.background)
          .sort((a, b) => (b.completedAt ?? "").localeCompare(a.completedAt ?? "") || b.run - a.run)
          .filter((e) => { const k = `${e.title.toLowerCase()}|${e.platform}`; if (seen.has(k)) return false; seen.add(k); return true; })
          .map((e): Item => ({
            src: e.background!, fallback: null, kind: "art", x: 50, y: 50, title: e.title, logo: e.logo,
            tags: [
              { icon: "trophy", text: t(COMPLETION_LABEL[e.completionType ?? "campaign"]), gold: e.completionType === "full" },
              ...(e.completedAt ? [{ icon: "calendar", text: fmtDay(e.completedAt) } as Tag] : []),
              ...(e.hours !== null ? [{ icon: "clock", text: fmtHours(e.hours) } as Tag] : []),
              ...(e.platform ? [{ icon: "platform", text: e.platform } as Tag] : []),
            ],
          }));
      })
      // Capturas: o logo é o do jogo da captura (biblioteca do perfil); sem jogo ou sem logo, o nome.
      : Promise.all([api.listGallery(profileId), api.listLibrary(profileId).catch(() => [])]).then(([list, games]) => {
        const byId = new Map(games.map((g) => [g.id, g]));
        return list
          .filter((i) => i.exists)
          .map((i): Item => {
            const g = i.gameId !== null ? byId.get(i.gameId) : undefined;
            return {
              src: itemSrc(i), fallback: null, kind: "shot", x: 50, y: 50,
              title: i.gameTitle ?? g?.title ?? null, tags: [{ icon: "calendar", text: formatDate(i.takenAt ?? i.addedAt) }], logo: g?.logo ?? null,
            };
          });
      });
    load.then((l) => { if (!dead) setItems(l); }).catch(() => { if (!dead) setItems([]); });
    return () => { dead = true; };
  }, [mode, profileId]);

  // Ciclo: só a imagem atual e a anterior (durante o crossfade) ficam no DOM → memória constante.
  useEffect(() => {
    if (!items?.length) return;
    let dead = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const next = makeBag(items.length);
    const hold = HOLD[mode];
    const step = async () => {
      // Pula imagens que não carregam (arquivo apagado/movido); tenta a alternativa antes de pular.
      for (let tries = 0; tries < Math.min(items.length, 8); tries++) {
        const item = items[next()];
        let src = item.src, kind = item.kind;
        if (!(await preload(src))) {
          if (!item.fallback || dead || !(await preload(item.fallback))) { if (dead) return; continue; }
          src = item.fallback; kind = item.kind === "art" ? "art" : "cover";
        }
        // logo que não carrega (link quebrado, sem internet): mostra o título em texto
        const logo = item.logo && (await preload(item.logo)) ? item.logo : null;
        if (dead) return;
        const k = ++counter.current;
        setLayers((l) => [...l.slice(-1), { k, src, kind, item, kb: kenBurns(hold + FADE * 2), logo }]);
        setTimeout(() => { if (!dead) setLayers((l) => l.filter((v) => v.k >= k)); }, FADE + 100);
        break;
      }
      // Uma imagem só: fica nela (sem "trocar" pela mesma).
      if (!dead && items.length > 1) timer = setTimeout(step, hold);
    };
    step();
    return () => { dead = true; clearTimeout(timer); };
  }, [items, mode]);

  const empty = items !== null && items.length === 0;

  return (
    <div className="idle-show">
      {layers.map((l) => (
        <div key={l.k} className={`idle-layer idle-${l.kind}`} style={{ animationDuration: `${FADE}ms` }}>
          <img className="idle-img" src={l.src} alt="" draggable={false}
            style={{ ...l.kb, objectPosition: `${l.item.x}% ${l.item.y}%` }} />
          {(l.logo || l.item.title || (l.item.tags?.length ?? 0) > 0) && (
            <div className="idle-caption">
              {l.logo
                ? <img className="idle-logo" src={l.logo} alt={l.item.title ?? ""} draggable={false} referrerPolicy="no-referrer" />
                : l.item.title && <strong>{l.item.title}</strong>}
              {(l.item.tags?.length ?? 0) > 0 && (
                <div className="idle-tags">
                  {(l.item.tags ?? []).map((g, n) => (
                    <span key={n} className={`idle-tag ${g.gold ? "gold" : ""}`} style={{ animationDelay: `${1.05 + n * 0.12}s` }}>
                      <svg viewBox="0 0 24 24" aria-hidden="true"><path d={TAG_ICON[g.icon]} /></svg>{g.text}
                    </span>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      ))}
      <div className="idle-vignette" />
      {empty && (
        <div className="idle-empty">
          {mode === "captures" ? t("Nenhuma captura salva neste perfil.")
            : mode === "completed" ? t("Nenhum jogo zerado com plano de fundo no Zerei!.")
            : t("Nenhum jogo com imagens na biblioteca.")}
        </div>
      )}
      {hint && <div className="idle-hint">{t("Pressione qualquer botão para voltar")}</div>}
    </div>
  );
}

/* =====================================================================================
 * Host: contagem de inatividade, ativação e encerramento
 * ===================================================================================== */

/** Deslocamento do mouse (px, somado) que encerra a apresentação: ignora tremidas e eventos sintéticos. */
const WAKE_PX = 24;
/** Após abrir pela pré-visualização, ignora o próprio clique/toque que a abriu. */
const PREVIEW_GRACE = 700;
const LEAVE_MS = 380;
/** Camadas que indicam interação/processo em andamento (janelas, teclado virtual, Gameboot, abertura). */
const BLOCKING = ".overlay, .kb-root, .gb-root, .boot";

export default function IdleHost({ profileId, ingame }: { profileId: number; ingame: boolean }) {
  const cfg = useIdleConfig();
  const [shown, setShown] = useState<{ preview: boolean } | null>(null);
  const [leaving, setLeaving] = useState(false);
  const last = useRef(Date.now());
  const grace = useRef(0);
  const leaveTimer = useRef<ReturnType<typeof setTimeout>>();
  const ingameRef = useRef(ingame);
  ingameRef.current = ingame;

  const stop = useCallback(() => {
    if (!isIdleActive() || performance.now() < grace.current) return;
    setIdleActive(false);
    last.current = Date.now();
    // A camada continua por cima durante o fade, absorvendo o clique que a encerrou.
    setLeaving(true);
    clearTimeout(leaveTimer.current);
    leaveTimer.current = setTimeout(() => { setShown(null); setLeaving(false); }, LEAVE_MS);
  }, []);

  const start = useCallback((preview: boolean) => {
    if (isIdleActive() || getIdleConfig().mode === "off") return;
    clearTimeout(leaveTimer.current);
    grace.current = preview ? performance.now() + PREVIEW_GRACE : 0;
    setIdleActive(true);
    setLeaving(false);
    setShown({ preview });
  }, []);

  useEffect(() => registerIdleHost({ start, stop }), [start, stop]);
  // Troca de perfil/saída: nunca deixa a apresentação "presa".
  useEffect(() => () => { clearTimeout(leaveTimer.current); setIdleActive(false); }, []);
  // Jogo iniciado (overlay em jogo) encerra na hora.
  useEffect(() => { if (ingame) stop(); }, [ingame, stop]);

  // Interação: zera a contagem; com a apresentação ativa, encerra-a e descarta o comando.
  useEffect(() => {
    let lx = -1, ly = -1, moved = 0;
    let swallowUntil = 0;
    const consume = (e: Event) => {
      if (isIdleActive()) {
        e.preventDefault(); e.stopImmediatePropagation();
        stop();
        swallowUntil = performance.now() + LEAVE_MS + 120;
        return;
      }
      if (performance.now() < swallowUntil) { e.preventDefault(); e.stopImmediatePropagation(); return; }
      last.current = Date.now();
    };
    const onMove = (e: MouseEvent) => {
      const d = lx < 0 ? 0 : Math.abs(e.clientX - lx) + Math.abs(e.clientY - ly);
      lx = e.clientX; ly = e.clientY;
      if (d === 0) return; // evento sintético (layout mudou sob o cursor parado)
      if (isIdleActive()) { moved += d; if (moved >= WAKE_PX) { moved = 0; stop(); } return; }
      moved = 0;
      last.current = Date.now();
    };
    const offPad = onPadActivity(() => { if (isIdleActive()) stop(); else last.current = Date.now(); });
    const leave = () => stop(); // janela perdeu o foco / minimizou
    const onVis = () => { if (document.visibilityState !== "visible") stop(); };

    const opts = { capture: true, passive: false } as AddEventListenerOptions;
    const evs = ["keydown", "mousedown", "mouseup", "pointerdown", "pointerup", "click", "auxclick", "contextmenu"];
    evs.forEach((ev) => window.addEventListener(ev, consume, opts));
    // Rolagem/toque: só contam como interação (passivos, não atrasam a rolagem da página). Com a
    // apresentação ativa a camada cobre tudo, então nada rola por trás.
    const soft = () => { if (isIdleActive()) stop(); else if (performance.now() >= swallowUntil) last.current = Date.now(); };
    window.addEventListener("wheel", soft, { capture: true, passive: true });
    window.addEventListener("touchstart", soft, { capture: true, passive: true });
    window.addEventListener("mousemove", onMove, true);
    window.addEventListener("blur", leave);
    document.addEventListener("visibilitychange", onVis);
    return () => {
      evs.forEach((ev) => window.removeEventListener(ev, consume, opts));
      window.removeEventListener("wheel", soft, true);
      window.removeEventListener("touchstart", soft, true);
      window.removeEventListener("mousemove", onMove, true);
      window.removeEventListener("blur", leave);
      document.removeEventListener("visibilitychange", onVis);
      offPad();
    };
  }, [stop]);

  // Operações longas sinalizadas pelo backend (varredura de pastas, sincronização da Steam).
  useEffect(() => {
    const uns: (() => void)[] = [];
    let cancelled = false;
    const add = (p: Promise<() => void>) => p.then((f) => (cancelled ? f() : uns.push(f)));
    add(listen("scan-progress", () => bumpIdleBusy()));
    add(listen("scan-done", () => bumpIdleBusy(5000)));
    add(listen<{ running: boolean }>("steam-sync-state", (e) => setIdleBusyFlag("steam-sync", e.payload.running)));
    return () => { cancelled = true; uns.forEach((f) => f()); setIdleBusyFlag("steam-sync", false); };
  }, []);

  // Verificação periódica (barata): sem temporizador reiniciado a cada movimento do mouse.
  useEffect(() => {
    if (cfg.mode === "off" || cfg.minutes <= 0) return;
    last.current = Date.now(); // mudou a configuração: conta a partir de agora
    let checking = false;
    const tick = async () => {
      if (checking || isIdleActive() || Date.now() - last.current < cfg.minutes * 60_000) return;
      const blocked = document.visibilityState !== "visible" || !document.hasFocus() || ingameRef.current
        || document.documentElement.classList.contains("overlay-mode") || !!document.querySelector(BLOCKING) || idleBusyNow();
      if (blocked) { last.current = Date.now(); return; }
      checking = true;
      try {
        // Jogo em execução (iniciado pelo NEXSTAGE): suspende.
        const running = await api.runningGames().catch(() => [] as number[]);
        if (running.length) { last.current = Date.now(); return; }
        if (Date.now() - last.current >= cfg.minutes * 60_000 && document.hasFocus()) start(false);
      } finally { checking = false; }
    };
    const id = setInterval(tick, 5000);
    return () => clearInterval(id);
  }, [cfg.mode, cfg.minutes, start]);

  if (!shown || cfg.mode === "off") return null;
  return createPortal(
    <div className={`idle-root ${leaving ? "leaving" : ""}`} aria-hidden="true">
      <IdleShow mode={cfg.mode} profileId={profileId} hint />
    </div>,
    document.body
  );
}
