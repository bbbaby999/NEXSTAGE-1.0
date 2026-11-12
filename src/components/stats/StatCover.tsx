import { useEffect, useState } from "react";
import { imgUrl } from "../../lib/api";

/**
 * Capa de jogo nas Estatísticas. As capas da biblioteca são arquivos no disco: `imgUrl` converte o
 * caminho em endereço que o WebView abre (como nas demais telas). Se a imagem falhar — ex.: jogos
 * novos da Steam não têm capa no endereço fixo —, tenta o ícone do jogo e, por fim, a inicial do título.
 */
export default function StatCover({ cover, icon, title, className }: { cover: string | null; icon?: string | null; title: string; className: string }) {
  const srcs = [imgUrl(cover), imgUrl(icon ?? null)].filter((s): s is string => !!s);
  const [i, setI] = useState(0);
  useEffect(() => setI(0), [cover, icon]);
  const src = srcs[i];
  const isIcon = !!src && !!icon && src === imgUrl(icon) && src !== imgUrl(cover);
  return (
    <span className={`${className} gs-cover`} aria-hidden="true">
      {src
        ? <img src={src} alt="" loading="lazy" className={isIcon ? "is-icon" : undefined} onError={() => setI((n) => n + 1)} />
        : <b>{title.trim().charAt(0).toUpperCase()}</b>}
    </span>
  );
}
