import { t } from "../../lib/i18n";

/** Créditos das fontes dos dados/imagens (IGDB, RAWG, SteamGridDB): só texto, sem links nem foco. */
export default function SourceCredits({ igdb, rawgId, rawg, sgdbId, sgdb }: {
  igdb?: boolean; rawgId?: number | null; rawg?: boolean; sgdbId?: number | null; sgdb?: boolean;
}) {
  const names: string[] = [];
  if (igdb) names.push("IGDB");
  if (rawgId || rawg) names.push("RAWG");
  if (sgdbId || sgdb) names.push("SteamGridDB");
  if (!names.length) return null;
  return <p className="zr-credits muted">{t("Dados e imagens:")} {names.join(" · ")}</p>;
}
