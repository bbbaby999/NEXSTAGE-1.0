import type { CSSProperties } from "react";
import { PlatformGlyph, hasPlatformMark, platformLabel } from "./PlatformBadge";
import { t } from "../lib/i18n";

/**
 * Cor de cada família de console na capa padrão (gradiente). Valores próximos das cores conhecidas de
 * cada marca, só como tom de fundo; o símbolo é o mesmo do selo de plataforma.
 */
const TONE: Record<string, string> = {
  "PC": "38,132,255",
  "XBOX": "16,124,16", "Xbox 360": "16,124,16", "Xbox Series": "16,124,16",
  "Playstation 1": "0,112,209", "Playstation 2": "0,72,170", "Playstation 3": "40,60,140",
  "Playstation 4": "0,84,180", "Playstation 5": "0,112,209", "PSP": "60,70,110", "Ps Vita": "0,90,170",
  "Nintendo Switch": "230,0,18", "Nintendo Switch 2": "230,0,18",
  "Wii U": "0,154,199", "Wii": "120,130,145", "GameCube": "88,72,160",
  "Nintendo 3DS": "206,20,40", "Nintendo DS": "110,110,120", "Game Boy Advance": "80,60,160",
  "Dreamcast": "238,110,30",
  "Nintendo 64": "40,110,60", "Game Boy": "130,140,60", "Game Boy Color": "120,60,170", "Mega Drive": "30,30,40",
  "Super Nintendo": "104,92,170",
  "Nintendinho": "178,34,40",
};

/**
 * Capa do emulador. Com imagem personalizada, mostra a imagem; sem ela, uma arte padrão do console
 * principal (primeira plataforma): gradiente da cor do console + símbolo grande + nome.
 */
export default function EmulatorCover({ name, platforms, cover, className = "" }: {
  name: string; platforms: string[]; cover: string | null; className?: string;
}) {
  if (cover) {
    return <div className={`emu-cover custom ${className}`} style={{ backgroundImage: `url(${cover})` }} role="img" aria-label={t("Capa de {name}", { name })} />;
  }
  const main = platforms.find(hasPlatformMark) ?? platforms[0] ?? "";
  const extra = platforms.length - 1;
  return (
    <div className={`emu-cover ${className}`} style={{ "--tone": TONE[main] ?? "90,110,170" } as CSSProperties}
      role="img" aria-label={t("Capa padrão de {name}: {v1}", { name, v1: platformLabel(main) || "emulador" })}>
      <span className="emu-cover-mark">
        {hasPlatformMark(main) ? <PlatformGlyph platform={main} height={46} maxWidth={120} /> : <b className="emu-cover-letter">{name.charAt(0).toUpperCase()}</b>}
      </span>
      <span className="emu-cover-name">{platformLabel(main) || name}</span>
      {extra > 0 && <span className="emu-cover-more">+{extra}</span>}
    </div>
  );
}
