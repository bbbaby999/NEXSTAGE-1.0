import { CSSProperties, useEffect, useRef, useState } from "react";
import { useSession } from "../state/session";
import { api, errMsg } from "../lib/api";
import Button from "../components/Button";
import Seg from "../components/Seg";
import ProfileForm from "../components/ProfileForm";
import StoragePanel from "../components/StoragePanel";
import BackupPanel from "../components/BackupPanel";
import UpdatePanel from "../components/UpdatePanel";
import RaSettings from "../components/RaSettings";
import SteamSettings from "../components/SteamSettings";
import DataSources from "../components/DataSources";
import { applyHomeBlur, applyParticles, applyTheme, applyWaves, clampHomeBlur, CUSTOM_THEME, CustomColors, DEFAULT_CUSTOM, DEFAULT_THEME, parseCustom, resolveCustom, setCustomColors, setLedSync, THEMES } from "../lib/theme";
import ColorPicker from "../components/ColorPicker";
import Select from "../components/Select";
import { setSoundConfig, play } from "../lib/sound";
import { LayoutOverride, setLayoutOverride } from "../lib/inputDevice";
import { MotionMode, setMotion } from "../lib/motion";
import { listen } from "@tauri-apps/api/event";
import type { GalleryItem, PadCombos, ShortcutInfo, StartupInfo } from "../types";
import { comboLabel, combosConflict, popcount, validateCombo } from "../lib/padCombo";
import PatchNotes from "../components/PatchNotes";
import { VERSION_LABEL } from "../lib/version";
import { setGamebootEnabled } from "../lib/gameboot";
import { IdleMode, setIdleConfig, startIdlePreview, useIdleActive, useIdleConfig } from "../lib/idle";
import { IdleShow } from "../components/IdleScreen";
import { HomeBgMode, setHomeBg, useHomeBg } from "../lib/homeBg";
import { fileToDataUrl } from "../lib/image";
import Modal from "../components/Modal";
import { itemSrc } from "../components/Viewer";
import { formatDate } from "../lib/format";
import { currentLang, Lang, LANGS, setLang, t, tk } from "../lib/i18n";

/** Converte KeyboardEvent.code para o nome aceito pelo registro de atalhos globais. */
function codeToKey(code: string): string | null {
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return m[1];
  m = /^Digit(\d)$/.exec(code);
  if (m) return m[1];
  if (/^F([1-9]|1[0-2])$/.test(code)) return code;
  const map: Record<string, string> = {
    Space: "Space", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
    Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown", Insert: "Insert", Delete: "Delete",
  };
  return map[code] ?? null;
}
const pretty = (a: string) => a.replace(/Super/g, "Win").replace(/\+/g, " + ");

/** Atalho global de teclado: captura a combinação, valida no backend (conflitos) e registra no Windows. */
function ShortcutRow({ title, description, api: A }: {
  title: string; description: string;
  api: { get: () => Promise<ShortcutInfo>; set: (a: string) => Promise<ShortcutInfo>; reset: () => Promise<ShortcutInfo> };
}) {
  const { toast } = useSession();
  const [info, setInfo] = useState<ShortcutInfo | null>(null);
  const [capturing, setCapturing] = useState(false);

  useEffect(() => { A.get().then(setInfo).catch(() => {}); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.key === "Escape") { setCapturing(false); return; }
      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
      const key = codeToKey(e.code);
      if (!key) { toast(t("Tecla não suportada. Use letras, números, F1–F12, setas ou Espaço."), "err"); return; }
      const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean) as string[];
      if (!mods.length) { toast(t("Inclua Ctrl, Alt, Shift ou Win na combinação."), "err"); return; }
      setCapturing(false);
      A.set([...mods, key].join("+"))
        .then((i) => { setInfo(i); toast(t("Atalho salvo e ativo.")); })
        .catch((er) => toast(errMsg(er), "err"));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing, toast]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="setting">
      <div>
        <strong>{title}</strong>
        <small>{description} {t("Atual:")}{" "}<b>{info ? pretty(info.accelerator) : "…"}</b>
          {info && !info.active && t(" — não registrado (conflito com outro programa)")}</small>
      </div>
      <div className="row">
        <Button variant={capturing ? "primary" : "default"} onClick={() => setCapturing((c) => !c)}>
          {capturing ? t("Pressione a combinação… (Esc cancela)") : t("Alterar")}
        </Button>
        <Button variant="ghost" disabled={info?.isDefault} onClick={() =>
          A.reset().then((i) => { setInfo(i); toast(t("Atalho padrão restaurado.")); }).catch((e) => toast(errMsg(e), "err"))}>
          {t("Restaurar padrão")}</Button>
      </div>
    </div>
  );
}

/**
 * Combinação de botões do controle. Ao "Alterar", segure os botões desejados e solte: a maior
 * combinação pressionada ao mesmo tempo vira a nova. Regra anti-conflito: 2 a 3 botões, incluindo
 * Select, Start, L3 ou R3; no jogo é preciso SEGURAR a combinação.
 */
function PadComboRow({ slot, title, description, hold }: { slot: "overlay" | "screenshot"; title: string; description: string; hold: string }) {
  const { toast } = useSession();
  const [combos, setCombos] = useState<PadCombos | null>(null);
  const [capturing, setCapturing] = useState(false);
  const [live, setLive] = useState(0);

  useEffect(() => { api.getPadCombos().then(setCombos).catch(() => {}); }, []);

  useEffect(() => {
    if (!capturing) return;
    let peak = 0, un: (() => void) | undefined, cancelled = false;
    const stop = () => { cancelled = true; un?.(); api.setPadCapture(false).catch(() => {}); setCapturing(false); setLive(0); };
    api.setPadCapture(true).catch(() => {});
    listen<number>("pad-mask", (e) => {
      const m = e.payload;
      if (m !== 0) { setLive(m); if (popcount(m) >= popcount(peak)) peak = m; return; }
      if (peak === 0) return;
      const chosen = peak;
      stop();
      const err = validateCombo(chosen);
      const other = slot === "overlay" ? combos?.screenshot ?? 0 : combos?.overlay ?? 0;
      if (err) { toast(err, "err"); return; }
      if (combosConflict(chosen, other)) { toast(t("Essa combinação conflita com a do outro comando (uma contém a outra)."), "err"); return; }
      api.setPadCombo(slot, chosen).then((c) => { setCombos(c); toast(t("Combinação salva e ativa.")); }).catch((er) => toast(errMsg(er), "err"));
    }).then((f) => (cancelled ? f() : (un = f)));
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopImmediatePropagation(); stop(); } };
    window.addEventListener("keydown", onKey, true);
    const timer = setTimeout(stop, 12000); // sem controle? cancela sozinho
    return () => { window.removeEventListener("keydown", onKey, true); clearTimeout(timer); if (!cancelled) stop(); };
  }, [capturing]); // eslint-disable-line react-hooks/exhaustive-deps

  const cur = combos ? (slot === "overlay" ? combos.overlay : combos.screenshot) : 0;
  const def = combos ? (slot === "overlay" ? combos.overlayDefault : combos.screenshotDefault) : 0;
  return (
    <div className="setting">
      <div>
        <strong>{title}</strong>
        <small>{description} {hold} {t("Atual:")}{" "}<b>{capturing ? (live ? comboLabel(live) : t("segure os botões e solte…")) : comboLabel(cur)}</b></small>
      </div>
      <div className="row">
        <Button variant={capturing ? "primary" : "default"} onClick={() => setCapturing((c) => !c)}>{capturing ? t("Cancelar") : t("Alterar")}</Button>
        <Button variant="ghost" disabled={!combos || cur === def} onClick={() =>
          api.resetPadCombo(slot).then((c) => { setCombos(c); toast(t("Combinação padrão restaurada.")); }).catch((e) => toast(errMsg(e), "err"))}>
          {t("Restaurar padrão")}</Button>
      </div>
    </div>
  );
}

const STARTUP_OPTS: [StartupInfo["mode"], string, string][] = [
  ["none", tk("Não iniciar com o Windows"), tk("O NEXSTAGE só abre quando você o iniciar (usa o “Modo de tela” acima).")],
  ["minimized", tk("Iniciar minimizado"), tk("Inicia com o Windows e sempre abre minimizado na barra de tarefas.")],
  ["window", tk("Iniciar em modo janela"), tk("Inicia com o Windows em janela normal, com o tamanho e a posição que você deixou.")],
  ["fullscreen", tk("Iniciar em tela cheia"), tk("Inicia com o Windows ocupando a tela inteira.")],
];

/** Inicialização com o Windows + modo de abertura (opções exclusivas; o estado real vem do Windows). */
function StartupSection() {
  const { toast } = useSession();
  const [info, setInfo] = useState<StartupInfo | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { api.getStartupInfo().then(setInfo).catch(() => {}); }, []);
  async function choose(mode: StartupInfo["mode"]) {
    if (busy || info?.mode === mode) return;
    setBusy(true);
    try {
      setInfo(await api.setStartupMode(mode));
      toast(mode === "none" ? t("Inicialização com o Windows desativada.") : t("Inicialização com o Windows ativada."));
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }
  const mismatch = info && (info.mode !== "none") !== info.autostartEnabled;
  return (
    <div className="setting col-set">
      <div>
        <strong>{t("Inicialização")}</strong>
        <small>
          {t("Estado no Windows:")}{" "}<b>{info ? (info.autostartEnabled ? t("iniciando com o Windows") : t("não inicia com o Windows")) : "…"}</b>
          {mismatch && t(" — a opção escolhida não está aplicada no Windows; selecione-a novamente.")}
          {info?.devBuild && t(" Esta é uma versão de desenvolvimento: a entrada do Windows apontaria para o executável de desenvolvimento. Use a versão instalada.")}
        </small>
      </div>
      <div className="radio-list" role="radiogroup" aria-label={t("Inicialização")}>
        {STARTUP_OPTS.map(([m, label, desc]) => (
          <button key={m} type="button" data-focus role="radio" aria-checked={info?.mode === m} disabled={busy}
            className={`radio ${info?.mode === m ? "on" : ""}`} onClick={() => choose(m)}>
            <i aria-hidden="true" />
            <span><b>{t(label)}</b><small>{t(desc)}</small></span>
          </button>
        ))}
      </div>
    </div>
  );
}

/** Seções da aba Geral (o id é o do <h2> correspondente). */
const SECTIONS: [id: string, label: string, icon: string][] = [
  ["set-aparencia", tk("Aparência"), "M12 3a9 9 0 1 0 0 18c1.1 0 1.6-.8 1.6-1.6 0-.5-.2-.8-.5-1.2-.3-.3-.5-.7-.5-1.2 0-.9.7-1.6 1.6-1.6H16a5 5 0 0 0 5-5c0-4.1-4-7.4-9-7.4ZM7.5 12.5h0M9.5 8h0M14.5 8h0"],
  ["set-janela", tk("Janela e inicialização"), "M3.5 5h17v14h-17V5ZM3.5 9h17M10 13l4 2.5-4 2.5v-5Z"],
  ["set-perfil", tk("Perfil e contas"), "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8ZM4.5 20a7.5 7.5 0 0 1 15 0"],
  ["set-sons", tk("Sons e notificações"), "M4 9v6h4l5 4V5L8 9H4ZM16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12"],
  ["set-controles", tk("Controles e atalhos"), "M7 8h10a4 4 0 0 1 4 4v1.5a3.5 3.5 0 0 1-6.3 2.1L14 14.5h-4l-.7 1.1A3.5 3.5 0 0 1 3 13.5V12a4 4 0 0 1 4-4ZM7.5 10.5v3M6 12h3M15.5 11.5h0M17.5 13h0"],
  ["set-captura", tk("Captura de tela"), "M4 8h3l1.5-2h7L17 8h3v11H4V8ZM12 16.5a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z"],
  ["set-contas", tk("Contas e plataformas"), "M4 6h16v12H4V6ZM4 10h16M8 15h3"],
  ["set-fontes", tk("Fontes de dados"), "M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3ZM4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6"],
  ["set-ocioso", tk("Modo ocioso"), "M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5ZM16 4h3l-3 3h3"],
  ["set-armazenamento", tk("Armazenamento"), "M12 3a9 9 0 1 0 9 9h-9V3ZM15 3.5A9 9 0 0 1 20.5 9H15V3.5Z"],
  ["set-backup", tk("Backup e restauração"), "M4 8h16v12H4V8ZM3 4h18v4H3V4ZM12 11v6M9 14l3 3 3-3"],
  ["set-atualizacoes", tk("Atualizações"), "M20 12a8 8 0 1 1-2.3-5.6M20 4v4.5h-4.5"],
];

/**
 * Menu lateral fixo da aba Geral: pula para a seção (rolagem suave dentro de .main) e destaca a
 * seção visível enquanto a página rola.
 */
/** Texto comparável na busca: sem acentos e em minúsculas ("Animações" → "animacoes"). */
const normText = (s: string) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();

function SectionNav() {
  const [active, setActive] = useState(SECTIONS[0][0]);
  const nav = useRef<HTMLElement>(null);
  const lockUntil = useRef(0); // durante a rolagem suave de um clique, o destaque não "pisca" nas seções do caminho
  const [query, setQuery] = useState("");
  const [counts, setCounts] = useState<Record<string, number> | null>(null);

  // Busca: esconde as opções (e seções inteiras) que não contêm todas as palavras digitadas, sem
  // acento nem maiúsculas. Marca com [data-search-hide] (o React não mexe nesse atributo) e reaplica
  // quando o conteúdo muda (ex.: o painel de armazenamento termina de carregar).
  useEffect(() => {
    const content = nav.current?.parentElement?.querySelector<HTMLElement>(".set-content");
    if (!content) return;
    const words = normText(query.trim()).split(/\s+/).filter(Boolean);
    const apply = () => {
      content.querySelectorAll("[data-search-hide]").forEach((e) => e.removeAttribute("data-search-hide"));
      if (!words.length) { setCounts(null); return; }
      const match = (el: Element) => { const t = normText(el.textContent ?? ""); return words.every((w) => t.includes(w)); };
      const res: Record<string, number> = {};
      for (const [id] of SECTIONS) {
        const h = document.getElementById(id);
        if (!h) continue;
        const parts: Element[] = [], items: Element[] = [];
        for (let n = h.nextElementSibling; n && !n.matches("h2.section"); n = n.nextElementSibling) {
          parts.push(n);
          if (n.classList.contains("settings")) items.push(...Array.from(n.children));
          else if (!n.matches("p.note, h3.set-sub")) items.push(n);
        }
        const titleHit = match(h); // buscar pelo nome da seção mostra a seção inteira
        let hits = 0;
        for (const it of items) { if (titleHit || match(it)) hits++; else it.setAttribute("data-search-hide", ""); }
        if (!hits) [h, ...parts].forEach((e) => e.setAttribute("data-search-hide", ""));
        res[id] = hits;
      }
      setCounts(res);
    };
    apply();
    if (!words.length) return;
    nav.current?.closest<HTMLElement>(".main")?.scrollTo({ top: 0 });
    let raf = 0;
    const mo = new MutationObserver(() => { cancelAnimationFrame(raf); raf = requestAnimationFrame(apply); });
    mo.observe(content, { childList: true, subtree: true });
    return () => { mo.disconnect(); cancelAnimationFrame(raf); };
  }, [query]);
  const found = counts ? Object.values(counts).reduce((a, b) => a + b, 0) : null;

  useEffect(() => {
    const scroller = nav.current?.closest<HTMLElement>(".main");
    if (!scroller) return;
    const update = () => {
      if (performance.now() < lockUntil.current) return;
      const top = scroller.getBoundingClientRect().top + 120;
      let cur = SECTIONS[0][0];
      for (const [id] of SECTIONS) {
        const el = document.getElementById(id);
        if (el && !el.hasAttribute("data-search-hide") && el.getBoundingClientRect().top <= top) cur = id;
      }
      // fim da página: as últimas seções podem não chegar ao topo
      if (scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 4) cur = SECTIONS[SECTIONS.length - 1][0];
      setActive(cur);
    };
    // Altura do menu = área visível de .main (sem o respiro de cima/baixo): fica de ponta a ponta da tela.
    const fit = () => {
      const cs = getComputedStyle(scroller);
      const h = scroller.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
      nav.current?.style.setProperty("--set-nav-h", `${Math.max(320, h)}px`);
      nav.current?.style.setProperty("--set-nav-top", cs.paddingTop);
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(scroller);
    update();
    scroller.addEventListener("scroll", update, { passive: true });
    return () => { scroller.removeEventListener("scroll", update); ro.disconnect(); };
  }, []);

  const go = (id: string) => {
    setActive(id);
    lockUntil.current = performance.now() + 700;
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <nav className="set-nav" ref={nav} data-nav-group aria-label={t("Seções das configurações")}>
      <h1 className="set-nav-title">{t("Configurações")}</h1>
      <div className="set-search">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
          <circle cx="11" cy="11" r="6.5" /><path d="m16 16 4.5 4.5" />
        </svg>
        <input className="search" type="text" value={query} onChange={(e) => setQuery(e.target.value)}
          placeholder={t("Buscar configuração")} aria-label={t("Buscar configuração")} spellCheck={false} />
        {query && <button type="button" data-focus className="set-search-clear" onClick={() => setQuery("")} aria-label={t("Limpar busca")}>✕</button>}
      </div>
      {found !== null && (
        <small className="set-search-info" role="status">
          {found ? (found === 1 ? t("1 opção encontrada") : t("{n} opções encontradas", { n: found })) : t("Nada encontrado para “{v0}”", { v0: query.trim() })}
        </small>
      )}
      {SECTIONS.map(([id, label, icon]) => {
        const n = counts?.[id];
        return (
          <button key={id} type="button" data-focus className={`${active === id ? "on" : ""} ${n === 0 ? "empty" : ""}`}
            aria-current={active === id ? "true" : undefined} data-autofocus={active === id ? "" : undefined} disabled={n === 0} onClick={() => go(id)}>
            <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d={icon} />
            </svg>
            <span>{t(label)}</span>
            {n !== undefined && n > 0 && <em className="set-count">{n}</em>}
          </button>
        );
      })}
      <small className="set-nav-foot">{t("NEXSTAGE")}{" "}{VERSION_LABEL}</small>
    </nav>
  );
}

type StartPage = "profiles" | "home" | "library" | "gallery" | "last";

/**
 * Modo Ocioso: conteúdo, tempo de ativação e pré-visualização ao vivo (a mesma apresentação da tela
 * cheia, num quadro 16:9). Salvo nas preferências do app e restaurado ao abrir.
 */
/**
 * Plano de fundo da Home: arte do jogo em destaque, sem fundo (só as ondas), uma imagem própria ou
 * uma captura da Galeria (cada modo guarda a sua imagem).
 * Salvo por perfil; a imagem vira arquivo na pasta de dados do app (entra no backup).
 */
function HomeBgSetting({ profileId }: { profileId: number }) {
  const { toast } = useSession();
  const bg = useHomeBg();
  const file = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const [fromGallery, setFromGallery] = useState(false);

  const saveMode = (mode: HomeBgMode) => {
    setHomeBg({ mode });
    api.setProfileSetting(profileId, "home_bg_mode", mode).catch((e) => toast(errMsg(e), "err"));
  };
  const setMode = (mode: HomeBgMode) => {
    // Modo com imagem ainda não escolhida: já abre o seletor (o arquivo no mesmo clique, exigência do
    // navegador); o modo só muda depois da escolha.
    if (mode === "custom" && !bg.image) { file.current?.click(); return; }
    if (mode === "gallery" && !bg.capture) { setFromGallery(true); return; }
    saveMode(mode);
  };
  async function pick(f?: File) {
    if (!f) return;
    setBusy(true);
    try {
      const data = await fileToDataUrl(f, 3840, 2160); // até 4K; JPEG
      const url = await api.setHomeBackground(profileId, data);
      setHomeBg({ image: url });
      saveMode("custom");
      toast(t("Plano de fundo da Home atualizado."));
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }
  async function fromCapture(item: GalleryItem) {
    setFromGallery(false);
    setBusy(true);
    try {
      const url = await api.setHomeBackgroundFromCapture(profileId, item.id);
      setHomeBg({ capture: url });
      saveMode("gallery");
      toast(t("Plano de fundo da Home atualizado."));
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }
  // Remove a imagem do modo atual ("Imagem própria" ou "Escolher da Galeria") e volta à arte do jogo.
  async function remove() {
    const gallery = bg.mode === "gallery";
    try {
      await api.setHomeBackground(profileId, null, gallery);
      setHomeBg(gallery ? { capture: null } : { image: null });
      saveMode("game");
    } catch (e) { toast(errMsg(e), "err"); }
  }

  const own = bg.mode === "custom" ? bg.image : bg.mode === "gallery" ? bg.capture : null;

  return (
    <div className="setting col-set">
      <div><strong>{t("Plano de fundo da Home")}</strong><small>{t("Arte do jogo em destaque, nenhum fundo (só as ondas), uma imagem do computador ou uma captura da Galeria. Salvo por perfil.")}</small></div>
      <Seg<HomeBgMode> value={bg.mode} onChange={setMode}
        opts={[[t("Arte do jogo"), "game"], [t("Sem fundo (ondas)"), "waves"], [t("Imagem própria"), "custom"], [t("Escolher da Galeria"), "gallery"]]} />
      {(bg.mode === "custom" || bg.mode === "gallery") && (
        <div className="home-bg-pick">
          <div className="home-bg-thumb" style={own ? { backgroundImage: `url(${own})` } : undefined}>
            {!own && <span>{t("Nenhuma imagem")}</span>}
          </div>
          <div className="row">
            {bg.mode === "custom"
              ? <Button disabled={busy} onClick={() => file.current?.click()}>{own ? t("Trocar imagem") : t("Escolher imagem")}</Button>
              : <Button disabled={busy} onClick={() => setFromGallery(true)}>{own ? t("Trocar captura") : t("Escolher captura")}</Button>}
            {own && <Button variant="ghost" disabled={busy} onClick={remove}>{t("Remover")}</Button>}
          </div>
        </div>
      )}
      {fromGallery && <CapturePicker profileId={profileId} onPick={fromCapture} onClose={() => setFromGallery(false)} />}
      <input ref={file} type="file" accept="image/png,image/jpeg,image/webp,image/bmp,image/gif,image/avif" hidden
        onChange={(e) => { pick(e.target.files?.[0]); e.target.value = ""; }} />
    </div>
  );
}

/** Escolhe uma captura da Galeria do perfil (para o fundo da Home). */
function CapturePicker({ profileId, onPick, onClose }: {
  profileId: number; onPick: (i: GalleryItem) => void; onClose: () => void;
}) {
  const [items, setItems] = useState<GalleryItem[] | null>(null);
  useEffect(() => {
    let dead = false;
    api.listGallery(profileId)
      .then((l) => { if (!dead) setItems(l.filter((i) => i.exists)); })
      .catch(() => { if (!dead) setItems([]); });
    return () => { dead = true; };
  }, [profileId]);

  return (
    <Modal title={t("Escolher da Galeria")} onClose={onClose} xl>
      {items === null ? (
        <p className="muted">{t("Carregando…")}</p>
      ) : items.length === 0 ? (
        <div className="empty"><p>{t("Nenhuma captura salva neste perfil.")}</p></div>
      ) : (
        <div className="thumbs cap-pick">
          {items.map((i, idx) => (
            <button key={i.id} type="button" data-focus data-capture-id={i.id} data-autofocus={idx === 0 ? "" : undefined}
              className="thumb" onClick={() => onPick(i)} aria-label={`${i.gameTitle ?? t("Sem jogo")} · ${formatDate(i.takenAt ?? i.addedAt)}`}>
              <img src={itemSrc(i)} alt="" loading="lazy" decoding="async" />
              <small>{i.gameTitle ?? t("Sem jogo")} · {formatDate(i.takenAt ?? i.addedAt)}</small>
            </button>
          ))}
        </div>
      )}
      <div className="row end">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
      </div>
    </Modal>
  );
}

function IdleSection({ profileId, save }: { profileId: number; save: (k: string, v: string) => void }) {
  const cfg = useIdleConfig();
  const showing = useIdleActive();
  const setMode = (mode: IdleMode) => { setIdleConfig({ ...cfg, mode }); save("idle_mode", mode); };
  const setMinutes = (minutes: number) => { setIdleConfig({ ...cfg, minutes }); save("idle_minutes", String(minutes)); };
  const off = cfg.mode === "off";
  return (
    <div className="settings">
      <div className="setting">
        <div><strong>{t("Conteúdo")}</strong><small>{t("O que aparece quando o NEXSTAGE fica sem uso: artes dos jogos da biblioteca em ordem aleatória ou as capturas do perfil ativo.")}</small></div>
        <Seg<IdleMode> value={cfg.mode} onChange={setMode}
          opts={[[t("Biblioteca"), "library"], [t("Capturas"), "captures"], [t("Desligado"), "off"]]} />
      </div>
      <div className="setting">
        <div><strong>{t("Tempo para ativação")}</strong><small>{t("Tempo sem mouse, teclado ou controle. Não ativa durante jogos, importações, sincronizações ou janelas abertas; qualquer comando volta para onde você estava.")}</small></div>
        <Select<number> value={cfg.minutes} width={200} title={t("Tempo para ativação")} disabled={off}
          onChange={setMinutes}
          options={[[t("Desligado"), 0], [t("2 minutos"), 2], [t("10 minutos"), 10], [t("20 minutos"), 20], [t("30 minutos"), 30], [t("45 minutos"), 45], [t("1 hora"), 60]]} />
      </div>
      <div className="setting col-set">
        <div><strong>{t("Pré-visualização")}</strong><small>{off ? t("Modo Ocioso desligado: nenhuma apresentação será iniciada.") : cfg.minutes === 0 ? t("Tempo de ativação desligado: a apresentação não inicia sozinha.") : t("Inicia após {v0} sem interação.", { v0: cfg.minutes === 60 ? t("1 hora") : t("{n} minutos", { n: cfg.minutes }) })}</small></div>
        <div className="idle-preview-row">
          <div className="idle-preview">
            {!off && !showing ? <IdleShow key={cfg.mode} mode={cfg.mode as Exclude<IdleMode, "off">} profileId={profileId} /> : <div className="idle-preview-off">{t("Desligado")}</div>}
          </div>
          <Button disabled={off} onClick={() => startIdlePreview()}>{t("Visualizar em tela cheia")}</Button>
        </div>
      </div>
    </div>
  );
}

export default function Settings() {
  const { profile, setProfile, replaceProfile, toast, setView } = useSession();
  const p = profile!;
  const [full, setFull] = useState(false);
  const [editing, setEditing] = useState(false);
  const [theme, setTheme] = useState<string>(DEFAULT_THEME);
  const [custom, setCustom] = useState<CustomColors>(DEFAULT_CUSTOM);
  const [picking, setPicking] = useState(false);
  const [waves, setWaves] = useState(true);
  const [particles, setParticles] = useState(true);
  const [homeBlur, setHomeBlur] = useState(0);
  const blurSave = useRef<ReturnType<typeof setTimeout>>();
  const [notes, setNotes] = useState(false);
  const [notesAuto, setNotesAuto] = useState(true);
  const [folder, setFolder] = useState("");
  const [motion, setMotionState] = useState<MotionMode>("auto");
  const [boot, setBoot] = useState(true);
  const [gameboot, setGameboot] = useState(true);
  const [shotSound, setShotSound] = useState(true);
  const [shotNotify, setShotNotify] = useState(true);
  const [achToast, setAchToast] = useState(true);
  const [padToast, setPadToast] = useState(true);
  const [shotSecs, setShotSecs] = useState(4);
  const [ledSync, setLedSyncState] = useState(true);
  const [sgdbKey, setSgdbKey] = useState("");
  const [rawgKey, setRawgKey] = useState("");
  const [start, setStart] = useState<StartPage>("home");
  const [sound, setSound] = useState(true);
  const [volume, setVolume] = useState(50);
  const [layout, setLayout] = useState<LayoutOverride>("auto");

  useEffect(() => {
    (async () => {
      const g = (k: string) => api.getAppSetting(k).catch(() => null);
      setFull((await g("fullscreen")) === "1");
      setBoot((await g("boot_animation")) !== "0");
      setGameboot((await g("gameboot")) !== "0");
      setShotSound((await g("shot_sound")) !== "0");
      setShotNotify((await g("shot_notify")) !== "0");
      setAchToast((await g("ach_toast")) !== "0");
      setPadToast((await g("pad_toast")) !== "0");
      setShotSecs(Number((await g("shot_notify_secs")) ?? 4) || 4);
      setLedSyncState((await g("pad_led_sync")) !== "0");
      setSgdbKey((await g("sgdb_key")) ?? "");
      setRawgKey((await g("rawg_key")) ?? "");
      setStart(((await g("start_page")) as StartPage) ?? "home");
      setSound((await g("sound_enabled")) !== "0");
      setVolume(Number((await g("sound_volume")) ?? 50));
      setLayout(((await g("input_layout")) as LayoutOverride) ?? "auto");
      setMotionState(((await g("motion")) as MotionMode) ?? "auto");
      setWaves((await api.getProfileSetting(p.id, "waves").catch(() => null)) !== "0");
      setParticles((await api.getProfileSetting(p.id, "particles").catch(() => null)) !== "0");
      setHomeBlur(clampHomeBlur((await api.getProfileSetting(p.id, "home_blur").catch(() => null)) ?? 0));
      setNotesAuto((await g("patchnotes_auto")) !== "0");
      api.capturesFolder().then(setFolder).catch(() => {});
      setTheme((await api.getProfileSetting(p.id, "theme").catch(() => null)) ?? DEFAULT_THEME);
      setCustom(parseCustom(await api.getProfileSetting(p.id, "theme_custom").catch(() => null)));
    })();
  }, [p.id]);

  const saveApp = async (k: string, v: string) => {
    try { await api.setAppSetting(k, v); } catch (e) { toast(errMsg(e), "err"); }
  };

  async function toggleFull() {
    const next = !full;
    try { await api.setFullscreen(next); await api.setAppSetting("fullscreen", next ? "1" : "0"); setFull(next); }
    catch (e) { toast(errMsg(e), "err"); }
  }
  async function pickTheme(id: string) {
    try { await api.setProfileSetting(p.id, "theme", id); applyTheme(id); setTheme(id); }
    catch (e) { toast(errMsg(e), "err"); }
  }
  // Tema personalizado: pré-visualiza ao vivo (1x por quadro enquanto arrasta); Cancelar volta ao tema
  // e às cores de antes; Aplicar salva as cores e ativa o tema personalizado.
  const previewRaf = useRef(0);
  const previewCustom = (c: CustomColors) => {
    cancelAnimationFrame(previewRaf.current);
    previewRaf.current = requestAnimationFrame(() => { setCustomColors(c); applyTheme(CUSTOM_THEME); });
  };
  const cancelCustom = () => {
    cancelAnimationFrame(previewRaf.current);
    setPicking(false);
    setCustomColors(custom);
    applyTheme(theme);
  };
  async function applyCustom(c: CustomColors) {
    cancelAnimationFrame(previewRaf.current);
    setPicking(false);
    try {
      await api.setProfileSetting(p.id, "theme_custom", JSON.stringify(c));
      await api.setProfileSetting(p.id, "theme", CUSTOM_THEME);
      setCustom(c); setCustomColors(c); applyTheme(CUSTOM_THEME); setTheme(CUSTOM_THEME);
      toast(t("Cor personalizada aplicada."));
    } catch (e) { toast(errMsg(e), "err"); cancelCustom(); }
  }
  async function toggleWaves() {
    const next = !waves;
    try { await api.setProfileSetting(p.id, "waves", next ? "1" : "0"); applyWaves(next); setWaves(next); }
    catch (e) { toast(errMsg(e), "err"); }
  }
  async function toggleParticles() {
    const next = !particles;
    try { await api.setProfileSetting(p.id, "particles", next ? "1" : "0"); applyParticles(next); setParticles(next); }
    catch (e) { toast(errMsg(e), "err"); }
  }
  // Desfoque do fundo da Home: aplica na hora e salva por perfil (gravação adiada enquanto arrasta).
  function changeHomeBlur(v: number) {
    const nv = clampHomeBlur(v);
    setHomeBlur(nv);
    applyHomeBlur(nv);
    clearTimeout(blurSave.current);
    blurSave.current = setTimeout(() => {
      api.setProfileSetting(p.id, "home_blur", String(nv)).catch((e) => toast(errMsg(e), "err"));
    }, 250);
  }
  async function afterProfile(deleted: boolean) {
    setEditing(false);
    if (deleted) return setProfile(null);
    const fresh = (await api.listProfiles()).find((x) => x.id === p.id);
    if (fresh) replaceProfile(fresh);
  }
  const changeVolume = (v: number) => {
    const nv = Math.max(0, Math.min(100, v));
    setVolume(nv);
    setSoundConfig(sound, nv);
    saveApp("sound_volume", String(nv));
    setTimeout(() => play("confirm"), 0);
  };

  return (
    <>
        <div className="set-layout">
          <SectionNav />
          <div className="set-content" data-nav-group>
          {/* ---------- Aparência ---------- */}
          <h2 className="section" id="set-aparencia">{t("Aparência")}</h2>
          <div className="settings">
            <div className="setting">
              {/* "Language" fica sempre visível em inglês: quem caiu num idioma que não lê ainda acha a opção */}
              <div><strong>{t("Idioma")}{currentLang() !== "en" && <span className="muted"> · Language</span>}</strong></div>
              <Select<Lang> value={currentLang()} width={240} title={t("Idioma")}
                onChange={(l) => { setLang(l).then(() => saveApp("language", l)).catch((e) => toast(errMsg(e), "err")); }}
                options={LANGS.map((l): [string, Lang] => [l.name, l.id])} />
            </div>
            <div className="setting col-set">
              <div><strong>{t("Tema")}</strong><small>{t("Cor de destaque da interface (e do LED dos controles PlayStation). Salvo por perfil. Atual:")}{" "}<b>{theme === CUSTOM_THEME ? t("Personalizado ({v0})", { v0: resolveCustom(custom).a.toUpperCase() }) : t(THEMES.find((th) => th.id === theme)?.name ?? "…")}</b></small></div>
              <div className="theme-grid" role="radiogroup" aria-label={t("Tema")}>
                {THEMES.map((th) => (
                  <button key={th.id} type="button" data-focus role="radio" aria-checked={theme === th.id}
                    className={`theme ${theme === th.id ? "on" : ""}`} onClick={() => pickTheme(th.id)} title={t(th.name)}>
                    <i style={{ background: "bg" in th
                      ? `linear-gradient(135deg, ${th.bg} 0 42%, ${th.a} 42% 78%, ${th.b} 100%)` // tema com fundo próprio: fundo + destaque
                      : `linear-gradient(135deg, ${th.a}, ${th.b})` }} />
                    <span>{t(th.name)}</span>
                  </button>
                ))}
                {/* cor livre: abre o seletor; a prévia mostra as cores personalizadas salvas */}
                <button type="button" data-focus role="radio" aria-checked={theme === CUSTOM_THEME}
                  className={`theme theme-custom ${theme === CUSTOM_THEME ? "on" : ""}`} onClick={() => setPicking(true)} title={t("Personalizar")}>
                  <i style={{ background: theme === CUSTOM_THEME || custom !== DEFAULT_CUSTOM
                    ? `linear-gradient(135deg, ${resolveCustom(custom).a}, ${resolveCustom(custom).b})`
                    : "conic-gradient(from 0deg, #f33, #fd3, #3f6, #3cf, #63f, #f3c, #f33)" }} />
                  <span>{t("Personalizar")}</span>
                </button>
              </div>
            </div>
            <div className="setting">
              <div><strong>{t("Fundo animado (ondas)")}</strong><small>{t("Pausa com a janela minimizada e respeita “reduzir movimento” do Windows. Salvo por perfil.")}</small></div>
              <Button variant={waves ? "primary" : "default"} onClick={toggleWaves}>{waves ? t("Ligado") : t("Desligado")}</Button>
            </div>
            <div className="setting">
              <div><strong>{t("Partículas cósmicas")}</strong><small>{t("Partículas luminosas que acompanham a onda do plano de fundo. Salvo por perfil.")}</small></div>
              <Button variant={particles ? "primary" : "default"} onClick={toggleParticles}>{particles ? t("Ligadas") : t("Desligadas")}</Button>
            </div>
            <HomeBgSetting profileId={p.id} />
            <div className="setting">
              <div><strong>{t("Desfoque do fundo da Home")}</strong><small>{t("Deixa a arte do jogo no plano de fundo da Home borrada (0% = nítida). Salvo por perfil.")}</small></div>
              <div className="range">
                <input type="range" data-focus min={0} max={100} step={5} value={homeBlur} aria-label={t("Desfoque do fundo da Home")}
                  style={{ "--fill": `${homeBlur}%` } as CSSProperties}
                  onChange={(e) => changeHomeBlur(Number(e.target.value))} />
                <output>{homeBlur}%</output>
              </div>
            </div>
            <div className="setting">
              <div><strong>{t("Animações")}</strong><small>{t("Automático segue o Windows. “Reduzidas” desliga ondas, inclinação 3D, parallax e brilhos animados (e economiza GPU).")}</small></div>
              <Seg<MotionMode> value={motion} onChange={(m) => { setMotionState(m); setMotion(m); saveApp("motion", m); }}
                opts={[[t("Automático"), "auto"], [t("Completas"), "full"], [t("Reduzidas"), "reduced"]]} />
            </div>
          </div>

          {/* ---------- Janela e inicialização ---------- */}
          <h2 className="section" id="set-janela">{t("Janela e inicialização")}</h2>
          <div className="settings">
            <div className="setting">
              <div><strong>{t("Modo de tela")}</strong><small>{t("Alterna entre janela e tela cheia. A escolha é lembrada.")}</small></div>
              <Button variant={full ? "primary" : "default"} onClick={toggleFull}>{full ? t("Tela cheia") : t("Janela")}</Button>
            </div>
            <div className="setting">
              <div><strong>{t("Página inicial")}</strong><small>{t("Aberta após escolher o perfil (a seleção de perfil sempre vem primeiro). Notícias: em breve.")}</small></div>
              <Select<StartPage> value={start} width={260} title={t("Página inicial")}
                onChange={(v) => { setStart(v); saveApp("start_page", v); }}
                options={[[t("Seleção de perfil → Home"), "profiles"], [t("Home"), "home"], [t("Biblioteca"), "library"], [t("Galeria"), "gallery"], [t("Última página utilizada"), "last"]]} />
            </div>
            <div className="setting">
              <div><strong>{t("Animação de inicialização")}</strong><small>{t("Exibida ao abrir o NEXSTAGE. Qualquer tecla ou botão pula durante a exibição.")}</small></div>
              <Button variant={boot ? "primary" : "default"} onClick={() => { setBoot(!boot); saveApp("boot_animation", boot ? "0" : "1"); }}>{boot ? t("Ligada") : t("Desligada")}</Button>
            </div>
            <div className="setting">
              <div><strong>{t("Gameboot ao jogar")}</strong><small>{t("Transição curta com áudio exibida ao iniciar um jogo pelo botão Jogar (o jogo já abre durante a animação). Qualquer tecla ou botão pula.")}</small></div>
              <Button variant={gameboot ? "primary" : "default"} onClick={() => { const v = !gameboot; setGameboot(v); setGamebootEnabled(v); saveApp("gameboot", v ? "1" : "0"); }}>{gameboot ? t("Ligado") : t("Desligado")}</Button>
            </div>
            <StartupSection />
          </div>

          {/* ---------- Perfil e contas ---------- */}
          <h2 className="section" id="set-perfil">{t("Perfil e contas")}</h2>
          <div className="settings">
            <div className="setting">
              <div><strong>{t("Meu perfil")}</strong><small>{t("Foto, banner, biografia, favoritos e exclusão.")}</small></div>
              <Button onClick={() => setEditing(true)}>{t("Editar perfil")}</Button>
            </div>
          </div>

          {/* ---------- Sons e notificações ---------- */}
          <h2 className="section" id="set-sons">{t("Sons e notificações")}</h2>
          <h3 className="set-sub">{t("Sons")}</h3>
          <div className="settings">
            <div className="setting">
              <div><strong>{t("Sons de navegação")}</strong><small>{t("Movimento, confirmar, voltar e abrir/fechar menus.")}</small></div>
              <div className="row">
                <Button variant={sound ? "primary" : "default"} onClick={() => { const n = !sound; setSound(n); setSoundConfig(n, volume); saveApp("sound_enabled", n ? "1" : "0"); if (n) setTimeout(() => play("confirm"), 0); }}>{sound ? t("Ligados") : t("Desligados")}</Button>
                <Button onClick={() => changeVolume(volume - 10)} disabled={!sound}>−</Button>
                <span style={{ minWidth: 48, textAlign: "center" }}>{volume}%</span>
                <Button onClick={() => changeVolume(volume + 10)} disabled={!sound}>+</Button>
              </div>
            </div>
            <div className="setting">
              <div><strong>{t("Som ao capturar")}</strong><small>{t("Efeito sonoro discreto quando uma captura é salva (segue o volume dos sons do app).")}</small></div>
              <Button variant={shotSound ? "primary" : "default"} onClick={() => { const v = !shotSound; setShotSound(v); saveApp("shot_sound", v ? "1" : "0"); }}>{shotSound ? t("Ligado") : t("Desligado")}</Button>
            </div>
          </div>
          <h3 className="set-sub" id="set-notificacoes">{t("Notificações")}</h3>
          <div className="settings">
            <div className="setting">
              <div><strong>{t("Notificações de conquistas")}</strong><small>{t("Cartão com som quando você desbloqueia uma conquista (Steam e RetroAchievements), por cima do jogo e do app. Cada plataforma também pode ser desligada na própria seção.")}</small></div>
              <div className="row">
                <Button variant={achToast ? "primary" : "default"} onClick={() => { const v = !achToast; setAchToast(v); saveApp("ach_toast", v ? "1" : "0"); }}>{achToast ? t("Ligadas") : t("Desligadas")}</Button>
              </div>
            </div>
            <div className="setting">
              <div><strong>{t("Notificações de controles")}</strong><small>{t("Cartão com som quando um controle é conectado ou desconectado e quando a bateria chega a 15% ou menos, por cima do jogo e do app.")}</small></div>
              <div className="row">
                <Button variant={padToast ? "primary" : "default"} onClick={() => { const v = !padToast; setPadToast(v); saveApp("pad_toast", v ? "1" : "0"); }}>{padToast ? t("Ligadas") : t("Desligadas")}</Button>
              </div>
            </div>
            <div className="setting">
              <div><strong>{t("Notificação de captura")}</strong><small>{t("Mostra a miniatura no canto inferior direito, por cima do jogo, sem tirar o foco dele. Em tela cheia exclusiva (alguns jogos antigos) o Windows não permite exibi-la.")}</small></div>
              <Button variant={shotNotify ? "primary" : "default"} onClick={() => { const v = !shotNotify; setShotNotify(v); saveApp("shot_notify", v ? "1" : "0"); }}>{shotNotify ? t("Ligada") : t("Desligada")}</Button>
            </div>
            <div className="setting">
              <div><strong>{t("Duração da notificação")}</strong><small>{t("Tempo até ela desaparecer sozinha.")}</small></div>
              <Select<number> value={shotSecs} width={180} title={t("Duração da notificação")} disabled={!shotNotify}
                onChange={(v) => { setShotSecs(v); saveApp("shot_notify_secs", String(v)); }}
                options={[[t("2 segundos"), 2], [t("3 segundos"), 3], [t("4 segundos"), 4], [t("5 segundos"), 5], [t("7 segundos"), 7], [t("10 segundos"), 10]]} />
            </div>
          </div>

          {/* ---------- Controles e atalhos ---------- */}
          <h2 className="section" id="set-controles">{t("Controles e atalhos")}</h2>
          <div className="settings">
            <div className="setting">
              <div><strong>{t("Central de Controles")}</strong><small>{t("Coleção dos seus controles com ajustes individuais: LED, vibração, zona morta, mapeamento, padrão de confirmação, testador e diagnóstico.")}</small></div>
              <Button data-return-id="controllers" onClick={() => setView("controllers", { returnTo: { view: "settings", focus: "controllers" } })}>{t("Abrir")}</Button>
            </div>
            <div className="setting">
              <div><strong>{t("Ícones de comandos")}</strong><small>{t("Automático acompanha o último dispositivo usado. Escolha manual se o controle não for reconhecido.")}</small></div>
              <Select<LayoutOverride> value={layout} width={220} title={t("Ícones de comandos")}
                onChange={(v) => { setLayout(v); setLayoutOverride(v); saveApp("input_layout", v); }}
                options={[[t("Automático"), "auto"], [t("Xbox"), "xbox"], [t("PlayStation"), "playstation"], [t("Nintendo"), "nintendo"], [t("Controle genérico"), "generic"], [t("Teclado e mouse"), "keyboard"]]} />
            </div>
            <div className="setting">
              <div><strong>{t("LED dos controles PlayStation")}</strong><small>{t("DualSense e DualShock 4 acompanham a cor de destaque do tema. Desligado, o NEXSTAGE não altera a iluminação do controle (a cor já aplicada volta ao padrão quando o controle é reconectado).")}{" "}{t("Um controle cadastrado na Central de Controles usa o próprio ajuste.")}</small></div>
              <Button variant={ledSync ? "primary" : "default"} onClick={() => { const v = !ledSync; setLedSyncState(v); setLedSync(v); saveApp("pad_led_sync", v ? "1" : "0"); }}>{ledSync ? t("Sincronizado") : t("Desligado")}</Button>
            </div>
            <ShortcutRow title={t("Atalho do overlay (teclado)")} description={t("Abre o NEXSTAGE por cima do jogo (jogos em janela ou janela sem bordas).")}
              api={{ get: api.getOverlayShortcut, set: api.setOverlayShortcut, reset: api.resetOverlayShortcut }} />
            <PadComboRow slot="overlay" title={t("Atalho do overlay (controle)")} description={t("Segure a combinação (0,7 s) em qualquer janela, inclusive no jogo.")}
              hold={t("Precisa incluir Select, Start, L3 ou R3 para não atrapalhar os jogos.")} />
            <ShortcutRow title={t("Atalho de captura (teclado)")} description={t("Funciona durante o jogo, sem interrompê-lo.")}
              api={{ get: api.getScreenshotShortcut, set: api.setScreenshotShortcut, reset: api.resetScreenshotShortcut }} />
            <PadComboRow slot="screenshot" title={t("Atalho de captura (controle)")} description={t("Segure a combinação (0,35 s); a captura é salva sem interromper o jogo.")}
              hold={t("Mesma regra de segurança.")} />
          </div>

          {/* ---------- Captura de tela ---------- */}
          <h2 className="section" id="set-captura">{t("Captura de tela")}</h2>
          <div className="settings">
            <div className="setting">
              <div><strong>{t("Pasta das capturas")}</strong><small className="path">{folder || "…"}<br />{t("Cada captura também é copiada para a Galeria do perfil ativo.")}</small></div>
              <div className="row">
                <Button onClick={() => api.openCapturesFolder().catch((e) => toast(errMsg(e), "err"))}>{t("Abrir pasta")}</Button>
                <Button onClick={() => api.takeScreenshot().then(() => toast(t("Captura salva."))).catch((e) => toast(errMsg(e), "err"))}>{t("Testar captura")}</Button>
              </div>
            </div>
          </div>

          {/* ---------- Contas e plataformas → Steam ---------- */}
          <h2 className="section" id="set-contas">{t("Contas e plataformas")}</h2>
          <h3 className="set-sub">{t("Steam")}</h3>
          <div className="settings">
            <SteamSettings />
            <div className="setting disabled">
              <div><strong>{t("Epic Games e Xbox")}</strong><small>{t("Versões futuras.")}</small></div>
              <span className="soon-pill">{t("em breve")}</span>
            </div>
          </div>

          {/* ---------- Fontes de dados: IGDB, Nexus Mods, RetroAchievements e pesquisa de imagens ---------- */}
          <h2 className="section" id="set-fontes">{t("Fontes de dados")}</h2>
          <div className="settings">
            <DataSources />
          </div>
          {/* RetroAchievements (conta por perfil) */}
          <h3 className="set-sub" id="set-retro">{t("RetroAchievements")}</h3>
          <div className="settings">
            <RaSettings />
          </div>
          {/* Pesquisa de imagens */}
          <h3 className="set-sub" id="set-imagens">{t("Pesquisa de imagens")}</h3>
          <div className="settings">
            <div className="setting">
              <div><strong>{t("Fontes incluídas")}</strong><small>{t("Steam (jogos de PC), libretro-thumbnails (consoles retrô) e Wikipédia. Sempre com escolha manual: nada é adicionado sem a sua confirmação.")}</small></div>
            </div>
            <div className="setting">
              <div><strong>{t("SteamGridDB (opcional)")}</strong><small>{t("Banco de capas e fundos feitos para launchers (filtros de conteúdo adulto ligados). Crie uma chave gratuita em steamgriddb.com → Preferências → API e cole aqui.")}</small></div>
              <div className="row">
                <input className="search" type="password" style={{ width: 240 }} value={sgdbKey} onChange={(e) => setSgdbKey(e.target.value)} placeholder={t("Chave da API")} aria-label={t("Chave da API do SteamGridDB")} />
                <Button onClick={() => { saveApp("sgdb_key", sgdbKey.trim()); toast(sgdbKey.trim() ? t("SteamGridDB ativado na pesquisa.") : t("SteamGridDB desativado.")); }}>{t("Salvar")}</Button>
              </div>
            </div>
            <div className="setting">
              <div><strong>{t("RAWG (opcional)")}</strong><small>{t("Banco de dados de jogos com screenshots de várias plataformas. Chave gratuita em rawg.io/apidocs.")}</small></div>
              <div className="row">
                <input className="search" type="password" style={{ width: 240 }} value={rawgKey} onChange={(e) => setRawgKey(e.target.value)} placeholder={t("Chave da API")} aria-label={t("Chave da API do RAWG")} />
                <Button onClick={() => { saveApp("rawg_key", rawgKey.trim()); toast(rawgKey.trim() ? t("RAWG ativado na pesquisa.") : t("RAWG desativado.")); }}>{t("Salvar")}</Button>
              </div>
            </div>
          </div>

          {/* ---------- Modo ocioso ---------- */}
          <h2 className="section" id="set-ocioso">{t("Modo ocioso")}</h2>
          <IdleSection profileId={p.id} save={saveApp} />

          {/* ---------- Armazenamento ---------- */}
          <h2 className="section" id="set-armazenamento">{t("Armazenamento")}</h2>
          <div className="settings">
            <StoragePanel />
          </div>

          {/* ---------- Backup e restauração ---------- */}
          <h2 className="section" id="set-backup">{t("Backup e restauração")}</h2>
          <div className="settings">
            <BackupPanel />
          </div>

          {/* ---------- Atualizações ---------- */}
          <h2 className="section" id="set-atualizacoes">{t("Atualizações")}</h2>
          <UpdatePanel />
          <div className="settings" style={{ marginTop: 12 }}>
            <div className="setting">
              <div><strong>{t("Patch Notes")}</strong><small>{t("Histórico de novidades, melhorias e correções de cada versão.")}</small></div>
              <Button onClick={() => setNotes(true)}>{t("Abrir Patch Notes")}</Button>
            </div>
            <div className="setting">
              <div><strong>{t("Abrir Patch Notes após atualizar")}</strong><small>{t("Mostra as novidades na primeira abertura depois de uma atualização.")}</small></div>
              <Button variant={notesAuto ? "primary" : "default"} onClick={() => { const n = !notesAuto; setNotesAuto(n); saveApp("patchnotes_auto", n ? "1" : "0"); }}>{notesAuto ? t("Ligado") : t("Desligado")}</Button>
            </div>
          </div>
          <p className="muted note">{t("NEXSTAGE")}{" "}{VERSION_LABEL} {t("· ALL GAMES. ONE STAGE.")}</p>
          </div>
        </div>
      {notes && <PatchNotes onClose={() => setNotes(false)} />}
      {editing && <ProfileForm profile={p} onClose={() => setEditing(false)} onDone={afterProfile} />}
      {picking && <ColorPicker initial={custom} onChange={previewCustom} onCancel={cancelCustom} onApply={applyCustom} />}
    </>
  );
}
