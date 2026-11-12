import { useEffect, useState } from "react";

/**
 * Identifica a lista de candidatas sem copiar o conteúdo inteiro (data: URLs têm centenas de KB).
 * Usa começo + fim + tamanho: o começo sozinho é o mesmo cabeçalho JPEG em toda imagem salva, e duas
 * imagens diferentes com o mesmo tamanho pareciam "iguais" (o fundo não trocava).
 */
const sig = (c: string | null | undefined) => (c ? `${c.length}:${c.slice(0, 48)}:${c.slice(-48)}` : "");

async function firstLoading(cands: (string | null | undefined)[], dead: () => boolean): Promise<string | null | undefined> {
  for (const c of cands) {
    if (dead()) return undefined;
    if (!c) continue;
    if (c.startsWith("data:")) return c;
    const ok = await new Promise<boolean>((r) => {
      const i = new Image();
      i.onload = () => r(true);
      i.onerror = () => r(false);
      i.src = c;
    });
    if (ok) return c;
  }
  return null;
}

/** Resolve a primeira imagem que realmente carrega; mantém a anterior até a nova estar pronta (sem flash). */
export function useFirstImage(cands: (string | null | undefined)[]): string | null {
  return useTaggedImage(cands, null).src;
}

/**
 * Como `useFirstImage`, mas devolve também a `tag` (ex.: o jogo) DONA da imagem exibida. Enquanto a
 * imagem nova carrega, `src` continua sendo a anterior — e `tag` continua sendo a dona dela, para quem
 * desenha usar a posição/ajustes certos (antes a imagem antiga aparecia com os ajustes do jogo novo).
 */
export function useTaggedImage<T>(cands: (string | null | undefined)[], tag: T): { src: string | null; tag: T | null } {
  const key = `${tagKey(tag)}|${cands.map(sig).join("|")}`;
  const [state, setState] = useState<{ src: string | null; tag: T | null }>({ src: null, tag: null });
  useEffect(() => {
    let dead = false;
    firstLoading(cands, () => dead).then((src) => {
      if (!dead && src !== undefined) setState({ src, tag });
    });
    return () => { dead = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return state;
}

/** Chave estável da tag: o id quando houver (objetos são recriados a cada recarga da biblioteca). */
function tagKey(tag: unknown): string {
  if (tag && typeof tag === "object" && "id" in tag) return String((tag as { id: unknown }).id);
  return tag == null ? "" : String(tag);
}
