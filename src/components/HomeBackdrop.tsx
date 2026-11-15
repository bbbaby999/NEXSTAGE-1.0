import { CSSProperties, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

interface Layer { k: number; src: string | null; x: number; y: number; soft: boolean }

/** Proporção (largura/altura) de cada imagem já medida — evita medir de novo a cada troca de jogo. */
const ratios = new Map<string, number>();

/**
 * Arte bem mais larga que a janela (ex.: "library hero" da Steam, 3840×1240 ≈ 3:1): preencher a tela
 * inteira ("cover") obrigaria a ampliar até a altura e cortar ~40% das laterais — o zoom exagerado.
 * Nesses casos a arte aparece na largura total, presa ao topo, e desce para uma cópia desfocada dela.
 */
const WIDE = 1.3;

function useImageRatio(src: string | null): number | null {
  const [r, setR] = useState<number | null>(src ? ratios.get(src) ?? null : null);
  useEffect(() => {
    if (!src) { setR(null); return; }
    const known = ratios.get(src);
    if (known) { setR(known); return; }
    setR(null);
    let alive = true;
    const im = new Image();
    im.onload = () => {
      if (!im.naturalWidth || !im.naturalHeight) return;
      const v = im.naturalWidth / im.naturalHeight;
      ratios.set(src, v);
      if (alive) setR(v);
    };
    im.src = src;
    return () => { alive = false; };
  }, [src]);
  return r;
}

function useViewportRatio(): number {
  const [v, setV] = useState(() => innerWidth / Math.max(1, innerHeight));
  useEffect(() => {
    const on = () => setV(innerWidth / Math.max(1, innerHeight));
    window.addEventListener("resize", on);
    return () => window.removeEventListener("resize", on);
  }, []);
  return v;
}

/** Uma camada do fundo: "cover" quando a arte tem o formato da janela; largura total + preenchimento desfocado quando é bem mais larga. */
function BgLayer({ l, view }: { l: Layer; view: number }) {
  const ratio = useImageRatio(l.soft ? null : l.src);
  if (!l.src) return <div className="bg-layer" />;
  // ainda medindo: espera para entrar já no formato certo (sem pular de "cover" para largura total)
  if (!l.soft && ratio === null) return null;
  const wide = !l.soft && ratio !== null && ratio / view > WIDE;
  if (!wide) {
    return <div className={`bg-layer ${l.soft ? "soft" : ""}`} style={{ backgroundImage: `url(${l.src})`, backgroundPosition: `${l.x}% ${l.y}%` }} />;
  }
  // altura da arte na largura total, em % da altura da janela (onde começa o esmaecimento)
  const artH = Math.min(100, (view / ratio) * 100);
  return (
    <div className="bg-layer wide" style={{ "--art-h": `${artH}%` } as CSSProperties}>
      <div className="bg-fill" style={{ backgroundImage: `url(${l.src})` }} />
      <div className="bg-art" style={{ backgroundImage: `url(${l.src})`, backgroundPosition: `${l.x}% 0` }} />
    </div>
  );
}

/** Plano de fundo da Home com transição suave (crossfade) entre imagens. */
export default function HomeBackdrop({ src, x, y, soft }: { src: string | null; x: number; y: number; soft: boolean }) {
  const [layers, setLayers] = useState<Layer[]>([]);
  const counter = useRef(0);
  const view = useViewportRatio();
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
      {layers.map((l) => <BgLayer key={l.k} l={l} view={view} />)}
      <div className="bg-shade" />
    </div>,
    document.body
  );
}
