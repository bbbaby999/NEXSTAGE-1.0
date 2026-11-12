import { CSSProperties, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import { t } from "../lib/i18n";

interface ToastEvent { thumb: string; title: string | null; secs: number; sound: boolean; show: boolean }
/** Conquista desbloqueada (RetroAchievements ou Steam), confirmada pela API como nova. */
interface AchItem { source: "ra" | "steam"; icon: string | null; title: string; game: string; points: number; hardcore: boolean; rare: boolean; globalPct: number | null; more: number }
interface AchToastEvent { item: AchItem; secs: number; sound: boolean }
/** Controle conectado / desconectado / bateria baixa. */
interface PadItem { event: "connected" | "disconnected" | "battery"; device: "xbox" | "playstation" | "generic"; name: string; percent: number | null; label: string | null }
interface PadToastEvent { item: PadItem; secs: number; sound: boolean }

/**
 * Conteúdo da janela "shot-toast" (notificação de captura por cima do jogo). O backend posiciona e
 * exibe a janela; aqui entram o cartão com a miniatura, a barra de tempo e o som discreto.
 */
export default function ShotToast() {
  const [ev, setEv] = useState<(ToastEvent & { k: number }) | null>(null);
  const [ra, setRa] = useState<(AchToastEvent & { k: number }) | null>(null);
  const [pad, setPad] = useState<(PadToastEvent & { k: number }) | null>(null);
  const [leaving, setLeaving] = useState(false);
  const timer = useRef(0);

  useEffect(() => {
    document.documentElement.classList.add("toast-window");
    let un: (() => void) | undefined;
    let cancelled = false;
    listen<ToastEvent>("shot-toast", async (e) => {
      const p = e.payload;
      if (p.sound) {
        const vol = Number((await api.getAppSetting("sound_volume").catch(() => null)) ?? 50);
        shutter(Math.max(0, Math.min(1, vol / 100)));
      }
      if (!p.show) return;
      clearTimeout(timer.current);
      setLeaving(false);
      setRa(null);
      setPad(null);
      setEv({ ...p, k: Date.now() });
      timer.current = window.setTimeout(() => setLeaving(true), Math.max(1, p.secs * 1000 - 380));
    }).then((f) => (cancelled ? f() : (un = f)));
    let un2: (() => void) | undefined;
    listen<AchToastEvent>("ach-toast", async (e) => {
      const p = e.payload;
      clearTimeout(timer.current);
      setLeaving(false);
      setEv(null);
      setPad(null);
      setRa({ ...p, k: Date.now() });
      // o som acompanha o círculo surgindo
      const soundsOn = (await api.getAppSetting("sound_enabled").catch(() => null)) !== "0";
      if (p.sound && soundsOn) {
        const vol = Number((await api.getAppSetting("sound_volume").catch(() => null)) ?? 50);
        (p.item.rare ? rareFanfare : unlockChime)(Math.max(0, Math.min(1, vol / 100)));
      }
      // recolhe ~0,6 s antes do fim (a janela é escondida pelo backend logo depois)
      timer.current = window.setTimeout(() => setLeaving(true), Math.max(1, p.secs * 1000 - 600));
    }).then((f) => (cancelled ? f() : (un2 = f)));
    let un3: (() => void) | undefined;
    listen<PadToastEvent>("pad-toast", async (e) => {
      const p = e.payload;
      clearTimeout(timer.current);
      setLeaving(false);
      setEv(null);
      setRa(null);
      setPad({ ...p, k: Date.now() });
      const soundsOn = (await api.getAppSetting("sound_enabled").catch(() => null)) !== "0";
      if (p.sound && soundsOn) {
        const vol = Number((await api.getAppSetting("sound_volume").catch(() => null)) ?? 50);
        padSound(p.item.event, Math.max(0, Math.min(1, vol / 100)));
      }
      timer.current = window.setTimeout(() => setLeaving(true), Math.max(1, p.secs * 1000 - 600));
    }).then((f) => (cancelled ? f() : (un3 = f)));
    return () => { cancelled = true; un?.(); un2?.(); un3?.(); clearTimeout(timer.current); };
  }, []);

  if (pad) {
    const d = pad.item;
    const brand = d.device === "xbox" ? "Xbox" : d.device === "playstation" ? "PlayStation" : t("Controle");
    const level = d.percent !== null ? `${d.percent}%` : d.label ? t(d.label) : null;
    const kicker = d.event === "connected" ? t("Controle conectado") : d.event === "disconnected" ? t("Controle desconectado") : t("Bateria fraca");
    const detail = d.event === "battery"
      ? (level ? t("{n} restante · conecte o cabo para carregar", { n: level }) : t("Conecte o cabo para carregar"))
      : d.event === "disconnected" && level ? t("Última bateria: {n}", { n: level }) : brand;
    return (
      <div key={pad.k} className={`ach-toast pad ev-${d.event} ${leaving ? "leaving" : ""}`} style={{ "--secs": `${pad.secs}s` } as CSSProperties} role="status" aria-live="polite">
        <div className="ach-badge">
          <div className="ach-icon">{d.event === "battery" ? <BatteryGlyph pct={d.percent} /> : <PadGlyph off={d.event === "disconnected"} />}</div>
        </div>
        <div className="ach-body">
          <span className="ach-kicker">{kicker}</span>
          <strong>{d.name}</strong>
          <small>{detail}</small>
        </div>
      </div>
    );
  }
  if (ra) {
    const a = ra.item;
    const detail = [
      a.game,
      a.source === "ra" && a.points > 0 ? t("+{n} pontos", { n: a.points }) : null,
      a.globalPct !== null ? t("{p}% dos jogadores", { p: a.globalPct < 10 ? a.globalPct.toFixed(1) : Math.round(a.globalPct) }) : null,
      a.more > 0 ? t("e mais {n} conquista(s)", { n: a.more }) : null,
    ].filter(Boolean).join(" · ");
    return (
      <div key={ra.k} className={`ach-toast ${a.rare ? "rare" : "common"} ${leaving ? "leaving" : ""}`} style={{ "--secs": `${ra.secs}s` } as CSSProperties} role="status" aria-live="polite">
        <div className="ach-badge">
          {a.rare && <><i className="ach-spark s1" /><i className="ach-spark s2" /><i className="ach-spark s3" /><i className="ach-spark s4" /></>}
          <div className="ach-icon">
            {a.icon ? <img src={a.icon} alt="" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : <TrophyGlyph />}
          </div>
        </div>
        <div className="ach-body">
          <span className="ach-kicker">
            <TrophyGlyph size={13} />
            {a.rare ? t("Conquista rara desbloqueada") : t("Conquista desbloqueada")}
            {a.hardcore && <b>HC</b>}
          </span>
          <strong>{a.title}</strong>
          <small>{detail}</small>
        </div>
        <span className="ach-src">{a.source === "steam" ? "Steam" : "RA"}</span>
      </div>
    );
  }
  if (!ev) return null;
  return (
    <div key={ev.k} className={`shot-toast ${leaving ? "leaving" : ""}`} style={{ "--secs": `${ev.secs}s` } as CSSProperties} role="status">
      <img src={ev.thumb} alt="" />
      <div className="st-text">
        <span className="st-kicker">
          <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M4 8h3l2-3h6l2 3h3v11H4z" /><circle cx="12" cy="13" r="3.5" />
          </svg>
          {t("Captura salva")}</span>
        <strong>{ev.title ?? t("NEXSTAGE")}</strong>
        <small>{t("Disponível na Galeria")}</small>
      </div>
      <i className="st-bar" />
    </div>
  );
}

function TrophyGlyph({ size = 30 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M8 4h8v5a4 4 0 0 1-8 0V4ZM8 6H5a3 3 0 0 0 3 4M16 6h3a3 3 0 0 1-3 4M12 13v4M8.5 20h7M10 17h4" />
    </svg>
  );
}

/** Controle (símbolo genérico); `off`: riscado (desconectado). */
function PadGlyph({ off }: { off?: boolean }) {
  return (
    <svg viewBox="0 0 24 24" width={34} height={34} fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M7 8h10a4 4 0 0 1 4 4v1.5a3.5 3.5 0 0 1-6.3 2.1L14 14.5h-4l-.7 1.1A3.5 3.5 0 0 1 3 13.5V12a4 4 0 0 1 4-4Z" />
      <path d="M7.5 10.5v3M6 12h3" />
      <circle cx="15.5" cy="11.5" r=".6" fill="currentColor" /><circle cx="17.5" cy="13" r=".6" fill="currentColor" />
      {off && <path d="M4 4l16 16" strokeWidth="2.2" />}
    </svg>
  );
}

/** Pilha com o nível preenchido. */
function BatteryGlyph({ pct }: { pct: number | null }) {
  const w = Math.max(1.5, Math.min(13, ((pct ?? 8) / 100) * 13));
  return (
    <svg viewBox="0 0 24 24" width={36} height={36} fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="7.5" width="16" height="9" rx="2" />
      <path d="M21 10.5v3" strokeLinecap="round" />
      <rect x="4.5" y="9" width={w} height="6" rx="1" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** Sons dos avisos de controle (sintetizados): conectar sobe, desconectar desce, bateria = dois bipes graves. */
function padSound(event: PadItem["event"], vol: number) {
  if (vol <= 0) return;
  let ctx: AudioContext;
  try { ctx = new AudioContext(); } catch { return; }
  const t0 = ctx.currentTime + 0.03;
  const master = ctx.createGain(); master.gain.value = 0.4 * vol; master.connect(ctx.destination);
  if (event === "battery") {
    for (const at of [0, 0.22]) {
      const o = ctx.createOscillator(); o.type = "square"; o.frequency.value = 440;
      const lp = ctx.createBiquadFilter(); lp.type = "lowpass"; lp.frequency.value = 1800;
      const g = ctx.createGain();
      g.gain.setValueAtTime(0.0001, t0 + at); g.gain.exponentialRampToValueAtTime(0.28, t0 + at + 0.01); g.gain.exponentialRampToValueAtTime(0.0001, t0 + at + 0.16);
      o.connect(lp).connect(g).connect(master); o.start(t0 + at); o.stop(t0 + at + 0.2);
    }
  } else {
    const notes = event === "connected" ? [783.99, 1174.66] : [1174.66, 783.99]; // Sol5 ↔ Ré6
    notes.forEach((f, i) => bell(ctx, master, t0 + i * 0.11, f, 0.4, 0.42));
  }
  setTimeout(() => void ctx.close().catch(() => {}), 1200);
}

/** Nota com ataque rápido e cauda (sino suave). */
function bell(ctx: AudioContext, out: AudioNode, at: number, f: number, vol: number, dur: number, type: OscillatorType = "sine") {
  const o = ctx.createOscillator(); o.type = type; o.frequency.value = f;
  const g = ctx.createGain();
  g.gain.setValueAtTime(0.0001, at);
  g.gain.exponentialRampToValueAtTime(vol, at + 0.012);
  g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
  o.connect(g).connect(out); o.start(at); o.stop(at + dur + 0.05);
  // harmônico (oitava) bem baixo: dá o brilho de "sino"
  const o2 = ctx.createOscillator(); o2.type = "sine"; o2.frequency.value = f * 2;
  const g2 = ctx.createGain();
  g2.gain.setValueAtTime(0.0001, at);
  g2.gain.exponentialRampToValueAtTime(vol * 0.25, at + 0.01);
  g2.gain.exponentialRampToValueAtTime(0.0001, at + dur * 0.6);
  o2.connect(g2).connect(out); o2.start(at); o2.stop(at + dur);
}

/** Conquista comum: "pop" curto + duas notas (Mi6 → Si6), discreto. */
function unlockChime(vol: number) {
  if (vol <= 0) return;
  let ctx: AudioContext;
  try { ctx = new AudioContext(); } catch { return; }
  const t0 = ctx.currentTime + 0.03;
  const master = ctx.createGain(); master.gain.value = 0.42 * vol; master.connect(ctx.destination);
  const pop = ctx.createOscillator(); pop.type = "sine";
  pop.frequency.setValueAtTime(420, t0); pop.frequency.exponentialRampToValueAtTime(880, t0 + 0.07);
  const pg = ctx.createGain();
  pg.gain.setValueAtTime(0.0001, t0); pg.gain.exponentialRampToValueAtTime(0.35, t0 + 0.01); pg.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.12);
  pop.connect(pg).connect(master); pop.start(t0); pop.stop(t0 + 0.15);
  bell(ctx, master, t0 + 0.09, 1318.5, 0.45, 0.5);
  bell(ctx, master, t0 + 0.2, 1975.5, 0.38, 0.7);
  setTimeout(() => void ctx.close().catch(() => {}), 1300);
}

/** Conquista rara: arpejo ascendente (Dó-Mi-Sol-Dó) com brilho e eco curto — mais longo e marcante. */
function rareFanfare(vol: number) {
  if (vol <= 0) return;
  let ctx: AudioContext;
  try { ctx = new AudioContext(); } catch { return; }
  const t0 = ctx.currentTime + 0.03;
  const master = ctx.createGain(); master.gain.value = 0.45 * vol; master.connect(ctx.destination);
  // eco curto (delay com realimentação baixa)
  const dl = ctx.createDelay(); dl.delayTime.value = 0.16;
  const fb = ctx.createGain(); fb.gain.value = 0.28;
  dl.connect(fb).connect(dl); dl.connect(master);
  const bus = ctx.createGain(); bus.connect(master); bus.connect(dl);
  [1046.5, 1318.5, 1568, 2093].forEach((f, i) => bell(ctx, bus, t0 + i * 0.085, f, 0.34, 0.9, "triangle"));
  // acorde final sustentado
  for (const f of [1046.5, 1568, 2093]) bell(ctx, bus, t0 + 0.42, f, 0.16, 1.4);
  // "brilho": ruído agudo filtrado subindo
  const len = Math.floor(ctx.sampleRate * 0.6);
  const buf = ctx.createBuffer(1, len, ctx.sampleRate);
  const d = buf.getChannelData(0);
  for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.sin((Math.PI * i) / len);
  const n = ctx.createBufferSource(); n.buffer = buf;
  const hp = ctx.createBiquadFilter(); hp.type = "highpass"; hp.frequency.setValueAtTime(5000, t0 + 0.3); hp.frequency.exponentialRampToValueAtTime(11000, t0 + 0.9);
  const ng = ctx.createGain(); ng.gain.value = 0.05;
  n.connect(hp).connect(ng).connect(master); n.start(t0 + 0.3);
  setTimeout(() => void ctx.close().catch(() => {}), 2600);
}

/** Som discreto de obturador (sintetizado): dois cliques curtos + um brilho suave. */
function shutter(vol: number) {
  if (vol <= 0) return;
  let ctx: AudioContext;
  try { ctx = new AudioContext(); } catch { return; }
  const t = ctx.currentTime + 0.02;
  const master = ctx.createGain();
  master.gain.value = 0.55 * vol;
  master.connect(ctx.destination);
  const len = Math.floor(ctx.sampleRate * 0.05);
  const buf = ctx.createBuffer(1, len, ctx.sampleRate);
  const d = buf.getChannelData(0);
  for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 3);
  for (const [at, f, v] of [[0, 3200, 0.5], [0.065, 2400, 0.38]] as const) {
    const s = ctx.createBufferSource(); s.buffer = buf;
    const bp = ctx.createBiquadFilter(); bp.type = "bandpass"; bp.frequency.value = f; bp.Q.value = 1.2;
    const g = ctx.createGain(); g.gain.value = v;
    s.connect(bp).connect(g).connect(master); s.start(t + at);
  }
  const o = ctx.createOscillator(); o.type = "sine"; o.frequency.value = 1568; // Sol6
  const og = ctx.createGain();
  og.gain.setValueAtTime(0.0001, t + 0.09);
  og.gain.exponentialRampToValueAtTime(0.09, t + 0.1);
  og.gain.exponentialRampToValueAtTime(0.0001, t + 0.42);
  o.connect(og).connect(master); o.start(t + 0.09); o.stop(t + 0.45);
  setTimeout(() => void ctx.close().catch(() => {}), 700);
}
