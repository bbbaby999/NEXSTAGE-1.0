import { Fragment, useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import Button from "./Button";
import Modal from "./Modal";
import { api } from "../lib/api";
import { formatDuration } from "../lib/format";
import { currentLang, t, tk } from "../lib/i18n";
import { useSession } from "../state/session";
import type { NexProfileStats, NexStat } from "../types";

export const GROUPS: [string, string][] = [
  ["geral", tk("Geral")], ["recordes", tk("Recordes")], ["combate", tk("Combate")], ["armas", tk("Armas")], ["colecao", tk("Coleção")], ["exploracao", tk("Exploração")],
];
/** Destaques do bloco (o resto fica em "Ver estatísticas"). */
const HIGHLIGHTS = ["runs", "wins", "best_time_s", "best_level", "total_kills", "top_weapon"];

export function loc(m: Record<string, string> | null | undefined): string {
  if (!m) return "";
  const l = currentLang();
  return m[l] ?? (l.startsWith("pt") ? m["pt-BR"] : m.en) ?? m.en ?? m["pt-BR"] ?? Object.values(m)[0] ?? "";
}

export function statValue(s: NexStat): string {
  if (s.kind === "ref") return s.label ? loc(s.label) : String(s.value);
  const n = Number(s.value) || 0;
  if (s.kind === "duration_s" || s.kind === "max_duration_s") return formatDuration(n);
  return s.of ? t("{a} de {b}", { a: n.toLocaleString(), b: s.of.toLocaleString() }) : n.toLocaleString();
}

/**
 * Estatísticas de um jogo integrado no perfil ativo: soma dos slots do perfil (contadores somados, recordes pelo
 * maior). Chegam pelo Game Bridge no fim de cada partida; definições vêm do manifesto assinado.
 */
/** Estatísticas do perfil ativo num jogo integrado; recarrega quando o jogo fecha. */
export function useNexStats(gameId: string) {
  const { profile } = useSession();
  const p = profile!;
  const [d, setD] = useState<NexProfileStats | null>(null);
  const load = useCallback(() => { api.integratedStats(p.id, gameId).then(setD).catch(() => setD(null)); }, [p.id, gameId]);
  useEffect(() => { load(); }, [load]);
  useEffect(() => {
    const offs = [listen("integrated-status", load), listen("game-session-ended", load)];
    return () => { offs.forEach((o) => o.then((f) => f())); };
  }, [load]);
  return d;
}

/** Todas as estatísticas, por grupo (janela do bloco e aba Estatísticas da página especial). */
export function NexStatGroups({ d }: { d: NexProfileStats }) {
  return (
    <>
      <p className="muted">{t("Somando os slots de save deste perfil: contadores são somados e recordes mostram o melhor.")}</p>
      {GROUPS.map(([g, label]) => {
        const list = d.stats.filter((s) => s.group === g);
        if (list.length === 0) return null;
        return (
          <div key={g} className="nx-stat-group">
            <h4>{t(label)}</h4>
            <dl className="steam-dl">
              {list.map((s) => <Fragment key={s.key}><dt>{loc(s.name)}</dt><dd>{statValue(s)}</dd></Fragment>)}
            </dl>
          </div>
        );
      })}
    </>
  );
}

export default function NexStageStats({ gameId }: { gameId: string }) {
  const d = useNexStats(gameId);
  const [open, setOpen] = useState(false);
  if (!d || d.stats.length === 0) return null;
  const top = HIGHLIGHTS.map((k) => d.stats.find((s) => s.key === k)).filter((s): s is NexStat => !!s);

  return (
    <>
      <h3 className="nx-sub">{t("Estatísticas")}</h3>
      <dl className="steam-dl">
        {top.map((s) => <Fragment key={s.key}><dt>{loc(s.name)}</dt><dd>{statValue(s)}</dd></Fragment>)}
      </dl>
      <div className="row">
        <Button variant="ghost" onClick={() => setOpen(true)}>{t("Ver estatísticas")}</Button>
        <small className="muted">{d.slots === 1 ? t("1 slot de save neste perfil") : t("{n} slots de save neste perfil", { n: d.slots })}</small>
      </div>
      {open && (
        <Modal title={t("Estatísticas")} onClose={() => setOpen(false)} wide>
          <NexStatGroups d={d} />
        </Modal>
      )}
    </>
  );
}
