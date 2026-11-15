import { openUrl } from "@tauri-apps/plugin-opener";
import { t } from "../../lib/i18n";

/**
 * Créditos com link ativo para as fontes dos dados/imagens. O RAWG exige, nos termos de uso, atribuição
 * com hiperlink ativo em toda página onde os dados dele aparecem; IGDB e SteamGridDB recebem o mesmo crédito.
 */
export default function SourceCredits({ igdb, rawgId, rawg, sgdbId, sgdb }: {
  igdb?: boolean; rawgId?: number | null; rawg?: boolean; sgdbId?: number | null; sgdb?: boolean;
}) {
  const links: [string, string][] = [];
  if (igdb) links.push(["IGDB", "https://www.igdb.com/"]);
  if (rawgId) links.push(["RAWG", `https://rawg.io/games/${rawgId}`]);
  else if (rawg) links.push(["RAWG", "https://rawg.io/"]);
  if (sgdbId) links.push(["SteamGridDB", `https://www.steamgriddb.com/game/${sgdbId}`]);
  else if (sgdb) links.push(["SteamGridDB", "https://www.steamgriddb.com/"]);
  if (!links.length) return null;
  return (
    <p className="zr-credits muted">
      {t("Dados e imagens:")}{" "}
      {links.map(([name, url], i) => (
        <span key={name}>
          {i > 0 && " · "}
          <button type="button" data-focus className="zr-credit-link" onClick={() => openUrl(url).catch(() => {})}>{name}</button>
        </span>
      ))}
    </p>
  );
}
