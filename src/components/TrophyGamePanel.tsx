import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import Button from "./Button";
import { api, errMsg, imgUrl } from "../lib/api";
import { raDate } from "../lib/ra";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import { trophyValue } from "../lib/score";
import type { Game, GameTrophies, Trophy, TrophyGrade } from "../types";

export const GRADES: TrophyGrade[] = ["P", "G", "S", "B"];
/** Data do troféu (segundos Unix) no formato das outras conquistas. */
export const trophyDate = (s: number) => raDate(new Date(s * 1000).toISOString().slice(0, 19));
export const gradeLabel = (g: string) =>
  g === "P" ? t("Platina") : g === "G" ? t("Ouro") : g === "S" ? t("Prata") : t("Bronze");

/** Recarrega quando um troféu novo é lido (ou um conjunto é aprendido). */
export function useTrophyEvents(onChange: () => void) {
  useEffect(() => {
    let off: (() => void) | undefined, dead = false;
    listen("trophies-changed", () => onChange()).then((f) => (dead ? f() : (off = f)));
    return () => { dead = true; off?.(); };
  }, [onChange]);
}

/** Contagem obtidos/total por tipo (platina, ouro, prata, bronze). */
export function TrophyCounts({ trophies }: { trophies: Trophy[] }) {
  return (
    <div className="trophy-counts">
      {GRADES.map((g) => {
        const of = trophies.filter((x) => x.grade === g);
        if (!of.length) return null;
        return <span key={g} className={`trophy-grade g-${g}`} title={gradeLabel(g)}><i />{of.filter((x) => x.unlocked).length}/{of.length}</span>;
      })}
    </div>
  );
}

/** Linha de troféu: ícone (cinza se pendente), nome, descrição, data e tipo. Oculto e pendente: sem spoiler. */
export function TrophyItem({ a, gameName, onClick }: { a: Trophy; gameName?: string; onClick?: () => void }) {
  const secret = a.hidden && !a.unlocked;
  const body = (
    <>
      <div className="ra-badge">{a.icon && !secret ? <img src={imgUrl(a.icon) ?? undefined} alt="" loading="lazy" /> : null}</div>
      <div className="ra-ach-text">
        <strong>{secret ? t("Troféu oculto") : a.name || `#${a.id}`}</strong>
        <span>{secret ? t("Continue jogando para descobrir.") : a.detail || t("Sem descrição.")}</span>
        <small>
          {gameName && <>{gameName} · </>}
          {a.unlocked ? (a.unlockedAt ? t("Obtida em {d}", { d: trophyDate(a.unlockedAt) }) : t("Obtida")) : t("Pendente")}
          {" · "}<i className="ns-score" title={t("Pontuação de conquistas")}>+{trophyValue(a.grade)}</i>
        </small>
      </div>
      <div className="ra-ach-side"><span className={`trophy-grade g-${a.grade}`} title={gradeLabel(a.grade)}><i /></span><small>{gradeLabel(a.grade)}</small></div>
    </>
  );
  const cls = `ra-ach ${a.unlocked ? "done" : "locked"}`;
  return onClick ? <button data-focus className={cls} onClick={onClick}>{body}</button> : <div className={cls}>{body}</div>;
}

/** Ordem das listas: obtidos (mais recentes primeiro), depois pendentes pela ordem do jogo. */
export const trophyOrder = (a: Trophy, b: Trophy) =>
  Number(b.unlocked) - Number(a.unlocked) || (b.unlockedAt ?? 0) - (a.unlockedAt ?? 0) || a.id - b.id;

/**
 * Bloco "Troféus" na página de um jogo de PS Vita (Vita3K) ou PS3 (RPCS3). Os troféus vêm do próprio
 * jogo e o progresso é o que o emulador gravou: o NEXSTAGE só lê (nada vai para a internet).
 */
export default function TrophyGamePanel({ game }: { game: Game }) {
  const { profile, toast } = useSession();
  const p = profile!;
  const [d, setD] = useState<GameTrophies | null>(null);
  const [all, setAll] = useState(false);

  const load = useCallback(() => {
    api.gameTrophies(p.id, game.id).then(setD).catch((e) => toast(errMsg(e), "err"));
  }, [p.id, game.id, toast]);
  useEffect(() => { load(); }, [load]);
  // troféu novo (ou conjunto aprendido) enquanto a página está aberta
  useTrophyEvents(load);

  if (!d) return null;
  const list = [...d.trophies].sort(trophyOrder);
  const shown = all ? list : list.slice(0, 6);
  const pctDone = d.total ? Math.round((d.unlocked / d.total) * 100) : 0;

  return (
    <>
      <h2 className="section">{t("Troféus")}</h2>
      <div className="ra-panel trophy-panel">
        {d.status === "notFound" && <p className="muted">{t("Nenhum conjunto de troféus encontrado para este jogo. Se ele tiver troféus, eles aparecem aqui depois que você abrir o jogo pelo NEXSTAGE.")}</p>}
        {d.status === "notInstalled" && <p className="muted">{t("Os troféus deste jogo aparecem aqui depois que ele for aberto pela primeira vez no {emu}.", { emu: d.emulator })}</p>}
        {d.status === "ok" && (
          <>
            <div className="trophy-head">
              {d.icon && <img className="trophy-set-icon" src={imgUrl(d.icon) ?? undefined} alt="" />}
              <div className="ra-prog">
                <div className="ra-bar"><i className="soft" style={{ width: `${pctDone}%` }} /></div>
                <div className="ra-prog-meta">
                  <span><b>{pctDone}%</b> · {t("{a} de {b} troféus", { a: d.unlocked, b: d.total })}</span>
                  <span>{d.emulator}</span>
                </div>
                <TrophyCounts trophies={d.trophies} />
              </div>
            </div>
            <div className="ra-ach-list mini">
              {shown.map((a) => <TrophyItem key={a.id} a={a} />)}
            </div>
            {list.length > 6 && <Button variant="ghost" onClick={() => setAll(!all)}>{all ? t("Mostrar menos") : t("Ver todas ({n})", { n: list.length })}</Button>}
          </>
        )}
      </div>
    </>
  );
}
