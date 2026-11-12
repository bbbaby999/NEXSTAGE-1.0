import { ReactNode, useEffect, useRef, useState } from "react";
import { attachPageFocus } from "../lib/pageFocus";

/**
 * Troca de página com saída rápida (160 ms) e entrada suave.
 * Correção: antes, ao voltar para a página atual durante a saída (ex.: ESC duas vezes rápido ou
 * L1/R1 em sequência), o estado "saindo" nunca era desfeito e a página ficava com opacidade 0,
 * invisível porém clicável. Agora o estado é sempre restaurado e o timer não é reiniciado a cada render.
 */
export default function Transition({ id, children }: { id: string; children: ReactNode }) {
  const [cur, setCur] = useState({ id, children });
  const [out, setOut] = useState(false);
  const latest = useRef(children);
  latest.current = children;

  useEffect(() => {
    if (id === cur.id) {
      setOut(false);
      return;
    }
    setOut(true);
    const t = setTimeout(() => {
      setCur({ id, children: latest.current });
      setOut(false);
    }, 160);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  // Foco ao entrar/voltar para a página (memória por página; ver lib/pageFocus.ts).
  const pageRef = useRef<HTMLDivElement>(null);
  useEffect(() => (pageRef.current ? attachPageFocus(cur.id, pageRef.current) : undefined), [cur.id]);

  const shown = id === cur.id ? children : cur.children;
  return <div key={cur.id} ref={pageRef} className={`page ${out ? "out" : ""}`}>{shown}</div>;
}
