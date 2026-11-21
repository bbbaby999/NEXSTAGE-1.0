import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import Button from "./Button";
import Modal from "./Modal";
import { TrophyIcon } from "./RaUnlockToasts";
import { api } from "../lib/api";
import { pct, raDate } from "../lib/ra";
import { currentLang, t, tk } from "../lib/i18n";
import { useSession } from "../state/session";
import type { NexAchievement } from "../types";

const RARITY: Record<string, string> = { comum: tk("Comum"), rara: tk("Rara"), epica: tk("Épica"), lendaria: tk("Lendária") };

/** Texto no idioma do NexStage (o catálogo traz pt-BR e en; os outros idiomas usam o inglês). */
export function loc(m: Record<string, string> | null | undefined): string {
  if (!m) return "";
  const l = currentLang();
  return m[l] ?? (l.startsWith("pt") ? m["pt-BR"] : m.en) ?? m.en ?? m["pt-BR"] ?? Object.values(m)[0] ?? "";
}

export function NexAchItem({ a }: { a: NexAchievement }) {
  const done = a.unlockedAt !== null;
  const secret = a.hidden && !done;
  return (
    <div className={`ra-ach ${done ? "" : "locked"}`}>
      <div className="ra-badge"><TrophyIcon size={24} /></div>
      <div className="ra-ach-text">
        <strong>{secret ? t("Conquista oculta") : loc(a.name)}</strong>
        <span>{secret ? t("Continue jogando para descobrir.") : loc(a.description)}</span>
        <small>
          {a.reserved ? t("Em breve (modo de jogo futuro)") : done ? t("Obtida em {d}", { d: raDate(a.unlockedAt!) }) : a.target ? t("{a} de {b}", { a: a.current.toLocaleString(), b: a.target.toLocaleString() }) : t("Pendente")}
          {done && a.origin === "offline" && <> · {t("sincronizada depois")}</>}
          {" · "}{t(RARITY[a.rarity] ?? a.rarity)} · <i className="ns-score">+{a.points}</i>
        </small>
      </div>
    </div>
  );
}

/**
 * Conquistas de um jogo oficialmente integrado, do perfil ativo. O catálogo vem do manifesto assinado da versão
 * publicada; os desbloqueios chegam pelo Game Bridge enquanto o jogo roda (ou depois, da caixa de saída do jogo).
 */
/** Conquistas do perfil ativo num jogo integrado; recarrega quando chega catálogo novo ou o jogo fecha. */
export function useNexAchievements(gameId: string) {
  const { profile } = useSession();
  const p = profile!;
  const [list, setList] = useState<NexAchievement[] | null>(null);
  const load = useCallback(() => { api.integratedAchievements(p.id, gameId).then(setList).catch(() => setList([])); }, [p.id, gameId]);
  useEffect(() => { load(); }, [load]);
  // o catálogo chega junto com a procura de atualização; desbloqueios, ao fechar o jogo
  useEffect(() => {
    const offs = [listen("integrated-status", load), listen("game-session-ended", load)];
    return () => { offs.forEach((o) => o.then((f) => f())); };
  }, [load]);
  return list;
}

/** Lista com filtro Todas/Obtidas/Pendentes (janela do bloco e aba Conquistas da página especial). */
export function NexAchList({ list }: { list: NexAchievement[] }) {
  const [show, setShow] = useState<"all" | "done" | "pending">("all");
  const shown = list.filter((a) => show === "all" || (show === "done") === (a.unlockedAt !== null));
  return (
    <>
      <div className="row">
        <div className="seg">
          {([["all", tk("Todas")], ["done", tk("Obtidas")], ["pending", tk("Pendentes")]] as const).map(([k, l]) => (
            <button key={k} type="button" data-focus className={show === k ? "on" : ""} onClick={() => setShow(k)}>{t(l)}</button>
          ))}
        </div>
      </div>
      <div className="ra-ach-list">
        {shown.length === 0 && <div className="empty"><p>{t("Nenhuma conquista nesta lista.")}</p></div>}
        {shown.map((a) => <NexAchItem key={a.id} a={a} />)}
      </div>
    </>
  );
}

export default function NexStageAchievements({ gameId }: { gameId: string }) {
  const list = useNexAchievements(gameId);
  const [open, setOpen] = useState(false);
  if (list === null) return null;
  const avail = list.filter((a) => !a.reserved);
  const got = avail.filter((a) => a.unlockedAt !== null);
  const points = got.reduce((s, a) => s + a.points, 0);
  const total = avail.reduce((s, a) => s + a.points, 0);

  return (
    <>
      <h3 className="nx-sub">{t("Conquistas")}</h3>
      {list.length === 0 ? (
        <p className="muted">{t("As conquistas aparecem depois que o NexStage consultar a versão publicada do jogo (precisa de internet uma vez).")}</p>
      ) : (
        <>
          <div className="ra-prog compact">
            <div className="ra-bar"><i className="soft" style={{ width: `${pct(got.length, avail.length)}%` }} /></div>
            <div className="ra-prog-meta">
              <span>{t("{a} de {b} conquistas", { a: got.length, b: avail.length })} · <i className="ns-score">{points} / {total}</i></span>
              {avail.length > 0 && got.length === avail.length && <span className="ra-award mastered">{t("100% concluído")}</span>}
            </div>
          </div>
          <div className="row"><Button variant="ghost" onClick={() => setOpen(true)}>{t("Ver conquistas")}</Button></div>
        </>
      )}
      {open && (
        <Modal title={t("Conquistas")} onClose={() => setOpen(false)} wide>
          <NexAchList list={list} />
        </Modal>
      )}
    </>
  );
}
