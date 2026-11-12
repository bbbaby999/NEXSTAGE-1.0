import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

interface Layer { k: number; src: string | null; x: number; y: number; soft: boolean }

/** Plano de fundo da Home com transição suave (crossfade) entre imagens. */
export default function HomeBackdrop({ src, x, y, soft }: { src: string | null; x: number; y: number; soft: boolean }) {
  const [layers, setLayers] = useState<Layer[]>([]);
  const counter = useRef(0);
  useEffect(() => {
    const k = ++counter.current;
    setLayers((l) => [...l.slice(-1), { k, src, x, y, soft }]);
    const t = setTimeout(() => setLayers((l) => l.filter((v) => v.k >= k)), 900);
    return () => clearTimeout(t);
  }, [src, x, y, soft]);
  // Portal no <body>: o fundo não pode viver dentro da Home, senão seu contexto de empilhamento o
  // desenha por cima da barra superior e do rodapé (interface "sumia", mas continuava clicável).
  return createPortal(
    <div className="home-bg" aria-hidden="true" data-parallax>
      {layers.map((l) => (
        <div key={l.k} className={`bg-layer ${l.soft ? "soft" : ""}`}
          style={l.src ? { backgroundImage: `url(${l.src})`, backgroundPosition: `${l.x}% ${l.y}%` } : undefined} />
      ))}
      <div className="bg-shade" />
    </div>,
    document.body
  );
}
