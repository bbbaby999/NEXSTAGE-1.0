import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "../components/Button";
import { NexAchItem, NexAchList, useNexAchievements } from "../components/NexStageAchievements";
import { NexStatGroups, statValue, useNexStats } from "../components/NexStageStats";
import { api } from "../lib/api";
import { formatDuration } from "../lib/format";
import { pct, raDate } from "../lib/ra";
import { currentLang, t, tk } from "../lib/i18n";
import { useSession } from "../state/session";
import type { IntegratedStatus, NexIntegratedInfo, NexSlot, NexStat } from "../types";

/**
 * Página especial dos jogos oficialmente integrados ao NexStage (Tomb Survivors é o primeiro): abas próprias
 * no lugar de DLCs/Mods. Tudo vem de dados reais — catálogo assinado, eventos do Game Bridge, manifesto
 * publicado; sem dados, cada aba diz o que falta.
 */
export type NexTab = "overview" | "achievements" | "stats" | "slots" | "news";
export const NEX_TABS: [NexTab, string][] = [
  ["overview", tk("Visão geral")], ["achievements", tk("Conquistas")], ["stats", tk("Estatísticas")], ["slots", tk("Slots")], ["news", tk("Novidades")],
];

function locText(m: Record<string, string> | null | undefined): string {
  if (!m) return "";
  const l = currentLang();
  return m[l] ?? (l.startsWith("pt") ? m["pt-BR"] : m.en) ?? m.en ?? m["pt-BR"] ?? "";
}

export function useNexInfo(gameId: string) {
  const [info, setInfo] = useState<NexIntegratedInfo | null>(null);
  useEffect(() => { api.integratedInfo(gameId).then(setInfo).catch(() => setInfo(null)); }, [gameId]);
  return info;
}

/** Anel de progresso (conquistas do perfil). */
function Ring({ value, label }: { value: number; label: string }) {
  const r = 46;
  const c = 2 * Math.PI * r;
  return (
    <div className="nx-ring" role="img" aria-label={label}>
      <svg viewBox="0 0 108 108" width="108" height="108" aria-hidden="true">
        <circle cx="54" cy="54" r={r} className="nx-ring-bg" />
        <circle cx="54" cy="54" r={r} className="nx-ring-fg" strokeDasharray={`${(c * value) / 100} ${c}`} transform="rotate(-90 54 54)" />
      </svg>
      <b>{value}%</b>
    </div>
  );
}

/** Visão geral: progresso do perfil, conquistas recentes, destaques e descrição oficial. */
export function NexOverview({ gameId, status }: { gameId: string; status: IntegratedStatus | null }) {
  const info = useNexInfo(gameId);
  const list = useNexAchievements(gameId);
  const stats = useNexStats(gameId);
  const avail = (list ?? []).filter((a) => !a.reserved);
  const got = avail.filter((a) => a.unlockedAt !== null);
  const points = got.reduce((s, a) => s + a.points, 0);
  const recent = [...got].sort((a, b) => (b.unlockedAt ?? "").localeCompare(a.unlockedAt ?? "")).slice(0, 3);
  const next = avail.filter((a) => a.unlockedAt === null && a.target).sort((a, b) => b.current / (b.target ?? 1) - a.current / (a.target ?? 1)).slice(0, 2);
  const get = (k: string): NexStat | undefined => stats?.stats.find((s) => s.key === k);
  const highlights = ["runs", "wins", "best_time_s", "best_level", "total_kills", "top_weapon"].map(get).filter((s): s is NexStat => !!s);

  return (
    <div className="nx-overview">
      <section className="nx-card nx-progress">
        <Ring value={pct(got.length, avail.length)} label={t("{a} de {b} conquistas", { a: got.length, b: avail.length })} />
        <div>
          <h3>{t("Seu progresso")}</h3>
          <p>{t("{a} de {b} conquistas", { a: got.length, b: avail.length })} · <i className="ns-score">{t("{n} pts", { n: points })}</i></p>
          {get("progress_percent") && <p className="muted">{t("Progresso geral no jogo: {p}%", { p: Number(get("progress_percent")!.value) })}</p>}
          {status?.version && <p className="muted">{t("Versão instalada: {v}", { v: status.version })}{status.updateAvailable && status.latest ? ` · ${t("Nova versão: {v}", { v: status.latest.version })}` : ""}</p>}
        </div>
      </section>

      {highlights.length > 0 ? (
        <section className="nx-card">
          <h3>{t("Destaques")}</h3>
          <div className="nx-tiles">
            {highlights.map((s) => <div key={s.key} className="nx-tile"><b>{statValue(s)}</b><small>{locText(s.name)}</small></div>)}
          </div>
        </section>
      ) : (
        <section className="nx-card"><p className="muted">{t("Jogue uma partida pelo NexStage para ver suas estatísticas aqui.")}</p></section>
      )}

      {(recent.length > 0 || next.length > 0) && (
        <section className="nx-card">
          {recent.length > 0 && <><h3>{t("Conquistas recentes")}</h3><div className="ra-ach-list">{recent.map((a) => <NexAchItem key={a.id} a={a} />)}</div></>}
          {next.length > 0 && <><h3>{t("Quase lá")}</h3><div className="ra-ach-list">{next.map((a) => <NexAchItem key={a.id} a={a} />)}</div></>}
        </section>
      )}

      {info && (
        <section className="nx-card">
          <h3>{t("Sobre o jogo")}</h3>
          <p className="desc">{locText(info.description)}</p>
        </section>
      )}
    </div>
  );
}

export function NexAchievementsTab({ gameId }: { gameId: string }) {
  const list = useNexAchievements(gameId);
  if (list === null) return null;
  if (list.length === 0) return <p className="muted">{t("As conquistas aparecem depois que o NexStage consultar a versão publicada do jogo (precisa de internet uma vez).")}</p>;
  const avail = list.filter((a) => !a.reserved);
  const got = avail.filter((a) => a.unlockedAt !== null);
  return (
    <>
      <div className="ra-prog">
        <div className="ra-bar"><i className="soft" style={{ width: `${pct(got.length, avail.length)}%` }} /></div>
        <div className="ra-prog-meta"><span><b>{pct(got.length, avail.length)}%</b> · {t("{a} de {b} conquistas", { a: got.length, b: avail.length })}</span></div>
      </div>
      <NexAchList list={list} />
    </>
  );
}

export function NexStatsTab({ gameId }: { gameId: string }) {
  const d = useNexStats(gameId);
  if (!d) return null;
  if (d.stats.length === 0) return <p className="muted">{t("Jogue uma partida pelo NexStage para ver suas estatísticas aqui.")}</p>;
  return <NexStatGroups d={d} />;
}

/** Slots do jogo usados neste perfil (o save é do jogo; aqui só o resumo que chegou pelo Bridge). */
export function NexSlotsTab({ gameId }: { gameId: string }) {
  const { profile } = useSession();
  const [slots, setSlots] = useState<NexSlot[] | null>(null);
  useEffect(() => { api.integratedSlots(profile!.id, gameId).then(setSlots).catch(() => setSlots([])); }, [profile, gameId]);
  if (slots === null) return null;
  if (slots.length === 0) return <p className="muted">{t("Nenhum slot deste perfil ainda. Os slots aparecem depois de jogar pelo NexStage.")}</p>;
  const n = (s: NexSlot, k: string) => Number(s.values[k] ?? 0);
  return (
    <>
      <p className="muted">{t("O jogo tem 5 slots de save. Aqui aparecem os que este perfil usou; o progresso fica no próprio save do jogo.")}</p>
      <div className="nx-slots">
        {slots.map((s) => (
          <div key={s.slotUid} className="nx-card nx-slot">
            <h3>{s.slotNumber ? t("Slot {n}", { n: s.slotNumber }) : t("Slot")}</h3>
            <div className="ra-prog compact">
              <div className="ra-bar"><i className="soft" style={{ width: `${n(s, "progress_percent")}%` }} /></div>
              <div className="ra-prog-meta"><span>{t("Progresso geral no jogo: {p}%", { p: n(s, "progress_percent") })}</span></div>
            </div>
            <dl className="steam-dl">
              <dt>{t("Partidas")}</dt><dd>{n(s, "runs").toLocaleString()}</dd>
              <dt>{t("Vitórias")}</dt><dd>{n(s, "wins").toLocaleString()}</dd>
              <dt>{t("Maior tempo sobrevivido")}</dt><dd>{formatDuration(n(s, "best_time_s"))}</dd>
              <dt>{t("Maior nível")}</dt><dd>{n(s, "best_level")}</dd>
              {s.lastSeen && <><dt>{t("Último uso")}</dt><dd>{raDate(s.lastSeen.replace(" ", "T") + (s.lastSeen.endsWith("Z") ? "" : "Z"))}</dd></>}
            </dl>
          </div>
        ))}
      </div>
    </>
  );
}

/** Novidades: notas da versão publicada mais recente (manifesto assinado) e link para todas as versões. */
export function NexNewsTab({ gameId, status }: { gameId: string; status: IntegratedStatus | null }) {
  const info = useNexInfo(gameId);
  const latest = status?.latest ?? null;
  const notes = latest ? locText(latest.notes ?? undefined) : "";
  return (
    <div className="nx-news">
      {latest ? (
        <section className="nx-card">
          <h3>{t("Versão {v}", { v: latest.version })}</h3>
          {latest.releasedAt && <p className="muted">{t("Publicada em {d}", { d: raDate(latest.releasedAt) })}</p>}
          {notes ? <pre className="nx-notes-full">{notes}</pre> : <p className="muted">{t("Esta versão não trouxe notas.")}</p>}
        </section>
      ) : <p className="muted">{t("As novidades aparecem quando o NexStage consegue consultar a versão publicada (precisa de internet).")}</p>}
      {info && <Button variant="ghost" onClick={() => openUrl(info.releasesUrl).catch(() => {})}>{t("Ver todas as versões")}</Button>}
    </div>
  );
}
