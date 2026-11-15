import { useEffect } from "react";
import { useSession } from "../state/session";
import { shoulderLabels, useInputDevice } from "../lib/inputDevice";
import { LogoMark } from "./Logo";
import Avatar from "./Avatar";
import Clock from "./Clock";
import ControllerStatus from "./ControllerStatus";
import type { View } from "../types";
import { t, tk } from "../lib/i18n";

const ICONS: Record<View, string> = {
  home: "M3 11l9-8 9 8M5 10v10h5v-6h4v6h5V10",
  library: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
  emulators: "M6 8h12a4 4 0 014 4v3a2 2 0 01-3.5 1.3L16 14H8l-2.5 2.3A2 2 0 012 15v-3a4 4 0 014-4zM8 10v3M6.5 11.5h3M16 11h.01M18 12.5h.01",
  collection: "M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z",
  stats: "M5 20V10M12 20V4M19 20v-7",
  profile: "M12 12a4 4 0 100-8 4 4 0 000 8zM4 20c0-4 4-6 8-6s8 2 8 6",
  gallery: "M3 5h18v14H3zM3 16l5-5 4 4 3-3 6 6M9 9.5h.01",
  settings: "M4 7h10M18 7h2M4 17h2M10 17h10M14 5v4M6 15v4",
  achievements: "M8 4h8v5a4 4 0 0 1-8 0V4ZM8 6H5a3 3 0 0 0 3 4M16 6h3a3 3 0 0 1-3 4M12 13v4M8.5 20h7M10 17h4",
  zerei: "M7 4h10v4a5 5 0 0 1-10 0V4ZM4 5h3M17 5h3M12 13v3M8 20h8M9.5 16.5h5M9.5 8.2l1.8 1.8 3.2-3.4",
  controllers: "M7 8h10a4 4 0 0 1 4 4v1.5a3.5 3.5 0 0 1-6.3 2.1L14 14.5h-4l-.7 1.1A3.5 3.5 0 0 1 3 13.5V12a4 4 0 0 1 4-4ZM7.5 10.5v3M6 12h3M15.5 11.5h0M17.5 13h0",
};

const NAV: { id: View; label: string; soon?: boolean }[] = [
  { id: "home", label: tk("Home") },
  { id: "library", label: tk("Biblioteca") },
  { id: "emulators", label: tk("Emuladores"), soon: true },
  { id: "collection", label: tk("Coleção") },
  { id: "gallery", label: tk("Galeria") },
  { id: "stats", label: tk("Estatísticas") },
  { id: "achievements", label: tk("Conquistas") },
  { id: "zerei", label: tk("Zerei!") },
  { id: "controllers", label: tk("Central de Controles") },
  { id: "settings", label: tk("Configurações") },
];

const Svg = ({ d, size = 24 }: { d: string; size?: number }) => (
  <svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d={d} />
  </svg>
);

/**
 * Barra de páginas (no topo em todas as telas, inclusive na Home). Os indicadores de ombro seguem o
 * controle em uso (L1/R1, LB/RB, L/R) e somem com teclado e mouse.
 */
export default function TopBar() {
  const { profile, view, setView } = useSession();
  const device = useInputDevice();
  const shoulders = shoulderLabels(device);

  // L1/R1 (controle) ou Q/E (teclado) trocam de aba. O direcional do controle não alcança as abas
  // ([data-pad-skip] em lib/nav.ts); as setas do teclado e o mouse continuam alcançando.
  useEffect(() => {
    const go = (d: number) => {
      if (document.querySelector(".overlay")) return;
      const i = NAV.findIndex((n) => n.id === view);
      setView(NAV[(i + d + NAV.length) % NAV.length].id);
    };
    const onTab = (e: Event) => go((e as CustomEvent<number>).detail);
    const onKey = (e: KeyboardEvent) => {
      if (["INPUT", "TEXTAREA", "SELECT"].includes((e.target as HTMLElement).tagName)) return;
      if (e.key === "q" || e.key === "Q") go(-1);
      if (e.key === "e" || e.key === "E") go(1);
    };
    window.addEventListener("nexstage:tab", onTab);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("nexstage:tab", onTab);
      window.removeEventListener("keydown", onKey);
    };
  }, [view, setView]);

  if (!profile) return null;
  const pill = (i: 0 | 1) =>
    shoulders && <span key={`${device}-${i}`} className={`pill shoulder ${device}`} aria-hidden="true">{shoulders[i]}</span>;

  return (
    <header className="topbar">
      <div className="top-left">
        <LogoMark size={30} />
        <ControllerStatus />
      </div>
      <nav aria-label={t("Páginas")}>
        {pill(0)}
        {NAV.map((n) => (
          <button
            key={n.id}
            data-focus
            data-pad-skip /* abas: no controle só L1/R1 trocam (o direcional não para nelas) */
            className={`tab ${view === n.id ? "active" : ""}`}
            onClick={() => setView(n.id)}
            title={n.soon ? t("{label} (em breve)", { label: t(n.label) }) : t(n.label)}
            aria-label={t(n.label)}
            aria-current={view === n.id ? "page" : undefined}
          >
            <Svg d={ICONS[n.id]} />
            {view === n.id && <span className="tab-label">{t(n.label)}</span>}
          </button>
        ))}
        {pill(1)}
      </nav>
      <div className="who-wrap">
        {/* Relógio fixo na barra, à esquerda do perfil. Clique no nome/avatar: página do jogador
            (a troca de usuário fica lá, em "Gerenciar perfis"). */}
        <Clock />
        <button data-focus className="who" onClick={() => setView("profile")} title={t("Ver perfil")} aria-label={t("Ver perfil")}>
          <span>{profile.displayName}</span>
          <Avatar name={profile.displayName} src={profile.avatar} size={38} />
        </button>
      </div>
    </header>
  );
}
