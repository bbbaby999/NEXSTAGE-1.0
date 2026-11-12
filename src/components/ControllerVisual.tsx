import { useState } from "react";
import type { Visual } from "../lib/controllers";
import ControllerArt from "./ControllerArt";

/**
 * Representação do controle com FOTO real quando disponível (src/assets/controllers/<visual>.webp,
 * fotos livres do Wikimedia Commons — créditos em src/data/controllerPhotos.ts), com o fundo de
 * estúdio removido. Sem foto (ou se ela falhar ao carregar), usa a ilustração vetorial.
 * O testador continua usando a ilustração, que acende os botões ao vivo.
 */
const PHOTOS = import.meta.glob("../assets/controllers/*.webp", { eager: true, query: "?url", import: "default" }) as Record<string, string>;
const photoOf = (v: Visual): string | null => PHOTOS[`../assets/controllers/${v}.webp`] ?? null;
export const hasPhoto = (v: Visual) => !!photoOf(v);

export default function ControllerVisual({ visual, className = "", title }: { visual: Visual; className?: string; title?: string }) {
  const src = photoOf(visual);
  const [failed, setFailed] = useState<string | null>(null);
  if (!src || failed === src) return <ControllerArt visual={visual} className={className} title={title} />;
  return (
    <img className={`ctl-art ctl-photo ${className}`} src={src} alt={title ?? ""} aria-hidden={title ? undefined : true}
      draggable={false} decoding="async" loading="lazy" onError={() => setFailed(src)} />
  );
}
