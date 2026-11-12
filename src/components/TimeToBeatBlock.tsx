import { useEffect, useState } from "react";
import Block from "./DpBlock";
import Button from "./Button";
import { api } from "../lib/api";
import { formatDate, formatDuration } from "../lib/format";
import { currentLang, t } from "../lib/i18n";
import type { Game, GameInfo } from "../types";

/** Horas e minutos ("12 h 30 min"); sem dado → "—" (nada é estimado). */
const fmt = (s: number | null | undefined) => (s ? formatDuration(s) : "—");

/**
 * "Tempo para zerar". Origens, nesta ordem: tempos digitados pelo usuário → IGDB (API oficial,
 * `game_time_to_beats`) → HowLongToBeat obtido por versões anteriores (mantido e identificado).
 * O HowLongToBeat não tem acesso autorizado: nenhuma consulta nova é feita a ele.
 */
export default function TimeToBeatBlock({ game, info, onEdit }: { game: Game; info: GameInfo | null; onEdit: () => void }) {
  // a dica de ativar o IGDB só aparece quando ele está mesmo desativado
  const [igdbOn, setIgdbOn] = useState<boolean | null>(null);
  useEffect(() => { api.sourcesList().then((l) => setIgdbOn(!!l.find((x) => x.id === "igdb")?.enabled)).catch(() => setIgdbOn(null)); }, []);
  const manual = info?.hltb?.source === "manual" ? info.hltb : null;
  const igdb = info?.igdb?.timeToBeat ?? null;
  const legacy = info?.hltb && info.hltb.source !== "manual" ? info.hltb : null;
  const rows = manual
    ? { main: manual.mainSeconds, extra: manual.extraSeconds, comp: manual.completionistSeconds, count: null, src: t("Preenchido manualmente"), at: manual.updatedAt ?? null }
    : igdb
      ? { main: igdb.hastily, extra: igdb.normally, comp: igdb.completely, count: igdb.count, src: t("Fonte: IGDB"), at: info?.igdb?.fetchedAt ?? null }
      : legacy
        ? { main: legacy.mainSeconds, extra: legacy.extraSeconds, comp: legacy.completionistSeconds, count: null, src: t("Fonte: HowLongToBeat (obtido anteriormente){v0}", { v0: legacy.name ? ` · ${legacy.name}` : "" }), at: legacy.updatedAt ?? null }
        : null;
  return (
    <Block title={t("Tempo para zerar")} actions={<Button variant="ghost" onClick={onEdit}>{t("Editar manualmente")}</Button>}>
      {rows ? (
        <table className="hltb">
          <tbody>
            <tr><td>{t("História principal")}</td><td>{fmt(rows.main)}</td></tr>
            <tr><td>{t("Principal + extras")}</td><td>{fmt(rows.extra)}</td></tr>
            <tr><td>{t("Completista")}</td><td>{fmt(rows.comp)}</td></tr>
            {rows.count ? <tr><td>{t("Registros de jogadores")}</td><td>{rows.count.toLocaleString(currentLang())}</td></tr> : null}
            <tr className="mine"><td>{t("Seu tempo (NEXSTAGE)")}</td><td>{formatDuration(game.playtimeSeconds)}</td></tr>
          </tbody>
        </table>
      ) : (
        <p className="muted">{
          info?.igdb ? t("O IGDB não tem tempos registrados para este jogo. Você pode preencher manualmente.")
          : igdbOn === false ? t("Sem dados de tempo para zerar. Ative o IGDB em Configurações → Fontes de dados ou preencha manualmente.")
          : t("Sem dados de tempo para zerar. Você pode preencher manualmente.")
        }</p>
      )}
      {rows && <p className="muted note">{rows.src}{rows.at ? ` · ${t("atualizado em {d}", { d: formatDate(rows.at) })}` : ""}</p>}
    </Block>
  );
}
