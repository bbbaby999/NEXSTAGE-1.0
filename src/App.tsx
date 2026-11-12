import { Fragment, lazy, Suspense, useEffect, useRef, useState } from "react";
import { useLang } from "./lib/i18n";
import { check } from "@tauri-apps/plugin-updater";
import { listen } from "@tauri-apps/api/event";
import { SessionProvider, useSession } from "./state/session";
import { layerJustClosed, useSpatialNav } from "./lib/nav";
import { resetPageFocus } from "./lib/pageFocus";
import { api } from "./lib/api";
import { applyHomeBlur, applyParticles, applyTheme, applyWaves, DEFAULT_THEME, loadProfileTheme, setLedSync } from "./lib/theme";
import { play, preloadSounds, setSoundConfig } from "./lib/sound";
import { LayoutOverride, setLayoutOverride } from "./lib/inputDevice";
import { MotionMode, setMotion, useParallax } from "./lib/motion";
import { useDepthTilt } from "./lib/depth";
import { useAutoHideCursor } from "./lib/cursor";
import { setGamebootEnabled } from "./lib/gameboot";
import { parseIdleConfig, setIdleConfig } from "./lib/idle";
import { loadHomeBg, setHomeBg } from "./lib/homeBg";
import IdleHost from "./components/IdleScreen";
import TopBar from "./components/TopBar";
import OverlayBar from "./components/OverlayBar";
import WaveBackground from "./components/WaveBackground";
import Transition from "./components/Transition";
import SideMenu from "./components/SideMenu";
import BackupActivity from "./components/BackupActivity";
import Boot from "./components/Boot";
import GameBootHost from "./components/GameBoot";
import ControlHints from "./components/ControlHints";
import PatchNotes from "./components/PatchNotes";
import { APP_VERSION } from "./lib/version";
import VirtualKeyboardHost from "./components/VirtualKeyboard";
import ProfileSelect from "./features/ProfileSelect";
import Home from "./features/Home";
import Library from "./features/Library";
import GameDetail from "./features/GameDetail";
import { loadControllers } from "./lib/controllers";
import type { View } from "./types";
import { t } from "./lib/i18n";

// Telas menos usadas em arquivos separados: a abertura carrega menos código. Depois da abertura elas
// são pré-carregadas em segundo plano (preloadPages), então a primeira visita não espera nada.
const PAGES = {
  collection: () => import("./features/Collection"),
  gallery: () => import("./features/Gallery"),
  profile: () => import("./features/Profile"),
  emulators: () => import("./features/Emulators"),
  stats: () => import("./features/Stats"),
  settings: () => import("./features/Settings"),
  achievements: () => import("./features/Achievements"),
  controllers: () => import("./features/Controllers"),
};
const Collection = lazy(PAGES.collection);
const Gallery = lazy(PAGES.gallery);
const Profile = lazy(PAGES.profile);
const Emulators = lazy(PAGES.emulators);
const Stats = lazy(PAGES.stats);
const Settings = lazy(PAGES.settings);
const Achievements = lazy(PAGES.achievements);
const Controllers = lazy(PAGES.controllers);
const preloadPages = () => { for (const load of Object.values(PAGES)) load().catch(() => {}); };

type Phase = "loading" | "boot" | "done";

function Shell() {
  const { profile, view, setView, returnTo, openGameId, setOpenGameId, toast } = useSession();
  const checkedUpdate = useRef(false);
  const [ingame, setIngame] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [phase, setPhase] = useState<Phase>("loading");
  const [patchOpen, setPatchOpen] = useState(false);
  const [revealed, setRevealed] = useState(false);
  useSpatialNav();
  const lang = useLang();
  useParallax();
  useDepthTilt();
  useAutoHideCursor();

  // Inicialização real: carrega as preferências antes de decidir se exibe a animação.
  useEffect(() => {
    (async () => {
      const g = (k: string) => api.getAppSetting(k).catch(() => null);
      const [, bootAnim, snd, vol, layout, motion, gameboot, ledSync, idleMode, idleMinutes] = await Promise.all([
        g("fullscreen"), g("boot_animation"), g("sound_enabled"), g("sound_volume"), g("input_layout"), g("motion"), g("gameboot"), g("pad_led_sync"),
        g("idle_mode"), g("idle_minutes"),
      ]);
      setIdleConfig(parseIdleConfig(idleMode, idleMinutes)); // Modo Ocioso
      setSoundConfig(snd !== "0", Number(vol ?? 50));
      setLayoutOverride(((layout as LayoutOverride) ?? "auto"));
      setMotion((motion as MotionMode) ?? "auto");
      preloadSounds().catch(() => {});
      setGamebootEnabled(gameboot !== "0");
      setLedSync(ledSync !== "0"); // LED dos controles PlayStation (padrão: acompanha a cor do tema)
      loadControllers().catch(() => {}); // Central de Controles: ajustes individuais valem desde a abertura
      // A animação de abertura do app é sempre a clássica (o Gameboot só aparece ao iniciar um jogo).
      if (bootAnim === "0") { setRevealed(true); setPhase("done"); }
      else setPhase("boot");
    })();
  }, []);

  // Verificação automática de atualização (silenciosa; só avisa quando há versão nova).
  useEffect(() => {
    if (phase !== "done" || checkedUpdate.current) return;
    checkedUpdate.current = true;
    api.getAppSetting("auto_update")
      .then((v) => (v === "0" ? null : check()))
      .then((u) => { if (u) toast(t("Atualização {version} disponível. Veja em Configurações → Atualizações.", { version: u.version })); })
      .catch(() => { /* sem internet ou atualização não configurada: ignora */ });
  }, [phase, toast]);

  // O backend precisa saber o perfil ativo para guardar as capturas feitas em jogo na Galeria certa.
  useEffect(() => { api.setActiveProfile(profile?.id ?? 0).catch(() => {}); }, [profile?.id]);

  // Aviso quando uma captura é salva (útil ao voltar do jogo / no overlay).
  useEffect(() => {
    let un: (() => void) | undefined;
    let cancelled = false;
    listen<{ path: string }>("screenshot-taken", (e) => toast(t("Captura salva: {path}", { path: e.payload.path }))).then((f) => (cancelled ? f() : (un = f)));
    return () => { cancelled = true; un?.(); };
  }, [toast]);

  // Patch Notes: abre sozinho na primeira abertura após uma atualização (se a opção estiver ligada).
  useEffect(() => {
    if (phase !== "done") return;
    Promise.all([api.getAppSetting("patchnotes_auto"), api.getAppSetting("patchnotes_seen")]).then(([auto, seen]) => {
      if (seen === APP_VERSION) return;
      if (auto !== "0") setPatchOpen(true);
      api.setAppSetting("patchnotes_seen", APP_VERSION).catch(() => {});
    }).catch(() => {});
  }, [phase]);

  // Posição do foco lembrada por página: cada perfil começa do zero.
  useEffect(() => { resetPageFocus(); }, [profile?.id]);
  useEffect(() => {
    if (phase !== "done") return;
    const id = window.setTimeout(preloadPages, 1500);
    return () => window.clearTimeout(id);
  }, [phase]);

  // Tema e fundo animado individuais por perfil.
  useEffect(() => {
    if (!profile) { applyTheme(DEFAULT_THEME); applyWaves(true); applyParticles(true); applyHomeBlur(0); setHomeBg({ mode: "game", image: null, capture: null }); return; }
    loadHomeBg(profile.id).catch(() => {}); // plano de fundo da Home (jogo / ondas / imagem própria)
    loadProfileTheme(profile.id).catch(() => {}); // inclui as cores do tema personalizado
    api.getProfileSetting(profile.id, "waves").then((v) => applyWaves(v !== "0")).catch(() => {});
    api.getProfileSetting(profile.id, "particles").then((v) => applyParticles(v !== "0")).catch(() => {});
    api.getProfileSetting(profile.id, "home_blur").then((v) => applyHomeBlur(Number(v ?? 0))).catch(() => {});
  }, [profile?.id]);

  // Última página usada (por perfil), para a opção "Última página utilizada".
  useEffect(() => {
    if (profile) api.setProfileSetting(profile.id, "last_view", view).catch(() => {});
  }, [profile?.id, view]);

  // Overlay em jogo (atalho global tratado no backend).
  useEffect(() => {
    let un: (() => void) | undefined;
    let cancelled = false;
    listen<boolean>("overlay-changed", (e) => setIngame(e.payload)).then((f) => (cancelled ? f() : (un = f)));
    return () => { cancelled = true; un?.(); };
  }, []);
  useEffect(() => { document.documentElement.classList.toggle("overlay-mode", ingame); }, [ingame]);

  /**
   * Hierarquia única de "voltar" (ESC / Círculo):
   * janela ou visualizador → fecha só ele (tratado pelo próprio componente);
   * menu lateral → fecha/volta nível (tratado por ele); overlay em jogo → fecha o overlay;
   * página de jogo → volta; página interna → Home; Home → abre o menu lateral.
   */
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !profile) return;
      const t = e.target as HTMLElement;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName)) { t.blur(); return; }
      // Camadas por cima (modal, menu, lista de filtro, teclado virtual, menu de contexto) tratam o próprio
      // "voltar"; e um "voltar" repetido logo após uma delas fechar não leva a página para a Home.
      if (document.querySelector(".overlay, .sm-root:not(.leaving), .pick-root, .kb-root, .ctx-root") || layerJustClosed()) return;
      play("back");
      if (ingame) { api.closeOverlay(); return; }
      if (openGameId !== null) { setOpenGameId(null); return; }
      if (returnTo && view !== returnTo.view) {
        // página aberta a partir de outra (ex.: Configurações → Central de Controles): volta para ela,
        // com o foco no item que a abriu
        const { view: to, focus } = returnTo;
        setView(to);
        if (focus) {
          setTimeout(() => {
            const el = document.querySelector<HTMLElement>(`[data-return-id="${CSS.escape(focus)}"]`);
            el?.scrollIntoView({ block: "center", behavior: "auto" });
            el?.focus({ preventScroll: true });
          }, 80);
        }
        return;
      }
      if (view !== "home") { setView("home"); return; }
      setMenuOpen(true);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [profile, ingame, openGameId, view, returnTo, setOpenGameId, setView]);

  const go = (v: View) => { setView(v); setMenuOpen(false); };
  // Home: a área de conteúdo ocupa a altura toda (arte do jogo em tela cheia, destaque embaixo).
  const homeMode = view === "home" && openGameId === null && !ingame;

  const content = openGameId !== null ? (
    <GameDetail gameId={openGameId} />
  ) : (
    <Suspense fallback={null}>
      {view === "home" && <Home />}
      {view === "library" && <Library />}
      {view === "collection" && <Collection />}
      {view === "gallery" && <Gallery />}
      {view === "profile" && <Profile />}
      {view === "stats" && <Stats />}
      {view === "settings" && <Settings />}
      {view === "emulators" && <Emulators />}
      {view === "achievements" && <Achievements />}
      {view === "controllers" && <Controllers />}
    </Suspense>
  );

  const app = !profile ? (
    <ProfileSelect />
  ) : (
    <>
      <div className={`shell ${homeMode ? "home-mode" : ""}`} {...({ inert: menuOpen ? "" : undefined } as Record<string, unknown>)}>
        <TopBar />
        {ingame && <OverlayBar />}
        <main className="main">
          <Transition id={openGameId !== null ? `g${openGameId}` : view}>{content}</Transition>
        </main>
      </div>
      <SideMenu open={menuOpen} onClose={() => setMenuOpen(false)} onGo={go} />
      {/* Modo Ocioso: só com perfil ativo e depois da abertura; trocar de perfil reinicia a contagem */}
      {phase === "done" && <IdleHost key={profile.id} profileId={profile.id} ingame={ingame} />}
    </>
  );

  return (
    <>
      <WaveBackground />
      <VirtualKeyboardHost />
      {patchOpen && <PatchNotes onClose={() => setPatchOpen(false)} />}
      {/* trocar o idioma remonta a interface (o estado da sessão fica no SessionProvider, acima) */}
      {(revealed || phase === "done") && <Fragment key={`app-${lang}`}>{app}</Fragment>}
      {phase === "loading" && <div className="boot" />}
      {phase === "boot" && <Boot onReveal={() => setRevealed(true)} onDone={() => setPhase("done")} />}
      <GameBootHost />
      {/* backup automático: progresso do "antes de encerrar" e avisos do semanal (em qualquer tela) */}
      <BackupActivity />
      {(revealed || phase === "done") && <ControlHints key={`hints-${lang}`} />}
    </>
  );
}

export default function App() {
  return (
    <SessionProvider>
      <Shell />
    </SessionProvider>
  );
}
