import { useCallback, useEffect, useState } from "react";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { steamTime } from "../lib/steam";
import { useXboxSynced } from "../lib/xbox";
import { pct } from "../lib/ra";
import { trophyDate } from "./TrophyGamePanel";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { Game, XboxGameData } from "../types";

/**
 * Bloco "Xbox" na página de um jogo encontrado no histórico da conta Xbox (pela família do pacote do
 * atalho do app Xbox ou pelo nome). O tempo de jogo do Xbox é dado externo: não altera o histórico local.
 * Sem conta conectada ou sem jogo correspondente, o bloco não aparece.
 */
export default function XboxGamePanel({ game }: { game: Game }) {
  const { profile, toast } = useSession();
  const p = profile!;
  const [d, setD] = useState<XboxGameData | null>(null);
  const [busy, setBusy] = useState(false);
  const [all, setAll] = useState(false);

  const load = useCallback(async (refresh: boolean) => {
    setBusy(refresh);
    try {
      const data = await api.xboxGame(p.id, game.id, refresh);
      setD(data);
      if (data?.error && refresh) toast(data.error, "err");
    } catch (e) { if (refresh) toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }, [p.id, game.id, toast]);
  useEffect(() => { load(false); }, [load]);
  // sincronização ou conquista nova enquanto a página está aberta: recarrega do cache
  useXboxSynced(p.id, () => load(false));

  if (!d) return null;
  // obtidas (mais recentes primeiro), depois pendentes na ordem do jogo
  const achs = [...d.achievements].sort((a, b) => Number(b.unlocked) - Number(a.unlocked) || (b.unlockedAt ?? 0) - (a.unlockedAt ?? 0));
  const shown = all ? achs : achs.slice(0, 6);
  const got = d.achCurrent ?? 0, total = d.achTotal ?? 0;

  return (
    <>
      <h2 className="section steam-sec">{t("Xbox")}</h2>
      <div className="ra-panel">
        <dl className="steam-dl">
          <dt>{t("Tempo no Xbox")}</dt><dd>{d.minutes !== null ? steamTime(d.minutes) : t("Não informado")}</dd>
          <dt>{t("Última vez (Xbox)")}</dt><dd>{d.lastPlayed ? trophyDate(d.lastPlayed) : t("Não informado")}</dd>
          {d.gsTotal ? <><dt>{t("Gamerscore")}</dt><dd>{d.gsCurrent ?? 0} / {d.gsTotal}</dd></> : null}
        </dl>

        <h3 className="steam-h3">{t("Conquistas")}</h3>
        {d.error && <p className="ra-warn">{d.error}</p>}
        {total > 0 && (
          <div className="ra-prog">
            <div className="ra-bar"><i className="soft" style={{ width: `${pct(got, total)}%` }} /></div>
            <div className="ra-prog-meta"><span><b>{pct(got, total)}%</b> · {t("{a} de {b} conquistas", { a: got, b: total })}</span></div>
          </div>
        )}
        {achs.length > 0 && (
          <>
            <div className="ra-ach-list mini">
              {shown.map((a) => {
                const secret = a.secret && !a.unlocked;
                return (
                  <div key={a.id} className={`ra-ach ${a.unlocked ? "done" : "locked"}`}>
                    <div className="ra-badge">{a.icon && !secret ? <img src={a.icon} alt="" loading="lazy" referrerPolicy="no-referrer" /> : null}</div>
                    <div className="ra-ach-text">
                      <strong>{secret ? t("Conquista secreta") : a.name}</strong>
                      <span>{secret ? t("Continue jogando para descobrir.") : a.description || t("Sem descrição.")}</span>
                      <small>
                        {a.unlocked ? (a.unlockedAt ? t("Obtida em {d}", { d: trophyDate(a.unlockedAt) }) : t("Obtida")) : t("Pendente")}
                        {a.rarity !== null && <> · {t("{p}% dos jogadores", { p: a.rarity.toFixed(1) })}</>}
                        {a.gamerscore > 0 && <> · <b>{a.gamerscore}G</b></>}
                      </small>
                    </div>
                  </div>
                );
              })}
            </div>
            {achs.length > 6 && <Button variant="ghost" onClick={() => setAll(!all)}>{all ? t("Mostrar menos") : t("Ver todas ({n})", { n: achs.length })}</Button>}
          </>
        )}
        {achs.length === 0 && !d.error && <p className="muted">{d.fetchedAt ? t("Nenhuma conquista encontrada.") : t("Conquistas ainda não carregadas.")}</p>}

        <div className="row">
          <Button onClick={() => load(true)} disabled={busy}>{busy ? t("Atualizando…") : t("Atualizar dados do Xbox")}</Button>
        </div>
      </div>
    </>
  );
}
