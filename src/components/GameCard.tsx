import type { CSSProperties } from "react";
import { tiltHandlers } from "../lib/tilt";
import PlatformBadge from "./PlatformBadge";
import { formatPlaytime } from "../lib/format";
import type { Game } from "../types";
import { siSteam } from "simple-icons";
import { t } from "../lib/i18n";

// Inclinação 3D sutil seguindo o mouse (CSS 3D; só transform, sem layout). Mesma da Home.
const tilt = tiltHandlers<HTMLButtonElement>((el) => el.querySelector<HTMLElement>(".cover"));

export default function GameCard({ game, onOpen, onSelect, active, index = 0, meta, entry }: {
  game: Game; onOpen: (g: Game) => void; onSelect?: (g: Game) => void; active?: boolean; index?: number; meta?: string;
  /** Item principal da tela: recebe o foco ao abrir a página (lib/pageFocus.ts). */
  entry?: boolean;
}) {
  return (
    <button
      data-focus
      data-game-id={game.id}
      data-autofocus={entry ? "" : undefined}
      className={`card ${active ? "active" : ""}`}
      style={{ "--i": Math.min(index, 12) } as CSSProperties}
      onClick={() => onOpen(game)}
      onFocus={() => onSelect?.(game)}
      onMouseEnter={() => onSelect?.(game)}
      onMouseMove={tilt.onMouseMove}
      onMouseLeave={tilt.onMouseLeave}
    >
      <div className="cover" style={game.cover ? { backgroundImage: `url(${game.cover})` } : undefined}>
        {!game.cover && <span>{game.title.charAt(0).toUpperCase()}</span>}
        {game.favorite && <i className="fav" aria-label={t("Favorito")}>★</i>}
        <PlatformBadge platform={game.platform} />
        {game.steamAppId !== null && (
          <i className={`steam-cover-badge ${game.installed ? "" : "off"}`} title={game.installed ? t("Steam · instalado") : t("Steam · não instalado")}
            aria-label={game.installed ? t("Steam · instalado") : t("Steam · não instalado")}>
            <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true"><path d={siSteam.path} fill="currentColor" /></svg>
          </i>
        )}
        {game.raGameId !== null && <i className="ra-cover-badge" title={t("Compatível com RetroAchievements")} aria-label={t("Compatível com RetroAchievements")}>RA</i>}
      </div>
      <div className="card-meta">
        <strong>{game.title}</strong>
        <small>{meta ?? [game.platform, formatPlaytime(game.playtimeSeconds)].filter(Boolean).join(" · ")}</small>
      </div>
    </button>
  );
}
