import { ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { useSession } from "../state/session";
import { api, errMsg, imgUrl } from "../lib/api";
import Button from "../components/Button";
import Select from "../components/Select";
import Modal from "../components/Modal";
import { TrophyIcon } from "../components/RaUnlockToasts";
import { TrophyCounts, TrophyItem, gradeLabel, trophyOrder, useTrophyEvents } from "../components/TrophyGamePanel";
import { pct, raAgo } from "../lib/ra";
import { t, tk } from "../lib/i18n";
import type { Trophy, TrophySetSummary } from "../types";

type Tab = "overview" | "recent" | "earned" | "pending";
type Sort = "activity" | "progress" | "progress-asc" | "name";
const TABS: [Tab, string][] = [["overview", tk("Visão geral")], ["recent", tk("Recentes")], ["earned", tk("Obtidas")], ["pending", tk("Pendentes")]];
const RECENT_SECS = 30 * 24 * 3600;

/** Segundos Unix → texto de data usado pelas funções de "há X tempo". */
const ago = (s: number | null) => raAgo(s ? new Date(s * 1000).toISOString().slice(0, 19).replace("T", " ") : null);

function Progress({ unlocked, total, compact }: { unlocked: number; total: number; compact?: boolean }) {
  const p = pct(unlocked, total);
  return (
    <div className={`ra-prog ${compact ? "compact" : ""}`}>
      <div className="ra-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={p} aria-label={t("{a} de {b} troféus", { a: unlocked, b: total })}>
        <i className="soft" style={{ width: `${p}%` }} />
      </div>
      <div className="ra-prog-meta">
        <span><b>{p}%</b> · {t("{a} de {b} troféus", { a: unlocked, b: total })}</span>
        {total > 0 && unlocked === total && <span className="ra-award mastered">{t("100% concluído")}</span>}
      </div>
    </div>
  );
}

/** Troféus de um conjunto (já carregados com a visão geral). */
function TrophySetModal({ set, onClose, onOpenLibrary }: { set: TrophySetSummary; onClose: () => void; onOpenLibrary: (id: number) => void }) {
  const [show, setShow] = useState<"all" | "done" | "pending">("all");
  const list = [...set.trophies].sort(trophyOrder).filter((a) => show === "all" || (show === "done") === a.unlocked);
  return (
    <Modal title={set.title} onClose={onClose} wide>
      <Progress unlocked={set.unlocked} total={set.total} />
      <TrophyCounts trophies={set.trophies} />
      <div className="row">
        <div className="seg">
          {([["all", tk("Todas")], ["done", tk("Obtidas")], ["pending", tk("Pendentes")]] as const).map(([k, l]) => (
            <button key={k} type="button" data-focus className={show === k ? "on" : ""} onClick={() => setShow(k)}>{t(l)}</button>
          ))}
        </div>
        {set.libraryGameId !== null && <Button variant="ghost" onClick={() => onOpenLibrary(set.libraryGameId!)}>{t("Abrir na biblioteca")}</Button>}
      </div>
      <div className="ra-ach-list">
        {list.length === 0 && <div className="empty"><p>{t("Nenhum troféu nesta lista.")}</p></div>}
        {list.map((a) => <TrophyItem key={a.id} a={a} />)}
      </div>
    </Modal>
  );
}

/**
 * Aba Conquistas → Troféus: PS Vita (Vita3K) e PS3 (RPCS3). Os troféus vêm dos próprios jogos e o
 * progresso é o que os emuladores gravaram; o NEXSTAGE só lê os arquivos (sem conta e sem internet).
 */
export default function TrophyAchievements({ switcher }: { switcher: ReactNode }) {
  const { profile, toast, setView, setOpenGameId } = useSession();
  const p = profile!;
  const [sets, setSets] = useState<TrophySetSummary[] | null>(null);
  const [hasEmu, setHasEmu] = useState(true);
  const [tab, setTab] = useState<Tab>("overview");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("name");
  const [open, setOpen] = useState<TrophySetSummary | null>(null);

  const load = useCallback(() => {
    api.trophyOverview(p.id).then(setSets).catch((e) => toast(errMsg(e), "err"));
    api.listEmulators().then((l) => setHasEmu(l.some((e) => /vita3k|rpcs3/i.test(`${e.name} ${e.exePath}`)))).catch(() => {});
  }, [p.id, toast]);
  useEffect(() => { load(); }, [load]);
  useTrophyEvents(load);

  const q = query.trim().toLowerCase();
  const games = useMemo(() => {
    const g = (sets ?? []).filter((x) => !q || x.title.toLowerCase().includes(q) || x.emulator.toLowerCase().includes(q));
    const by: Record<Sort, (a: TrophySetSummary, b: TrophySetSummary) => number> = {
      activity: (a, b) => (b.lastUnlockAt ?? 0) - (a.lastUnlockAt ?? 0) || b.unlocked - a.unlocked,
      progress: (a, b) => pct(b.unlocked, b.total) - pct(a.unlocked, a.total) || b.unlocked - a.unlocked,
      "progress-asc": (a, b) => pct(a.unlocked, a.total) - pct(b.unlocked, b.total) || a.unlocked - b.unlocked,
      name: (a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true }),
    };
    return [...g].sort(by[sort]);
  }, [sets, q, sort]);

  // todos os troféus com o conjunto de origem (abas Recentes/Obtidas/Pendentes)
  const all = useMemo(() => (sets ?? []).flatMap((s) => s.trophies.map((a) => ({ a, s }))), [sets]);
  const recent = useMemo(() => all.filter((x) => x.a.unlocked).sort((x, y) => (y.a.unlockedAt ?? 0) - (x.a.unlockedAt ?? 0)), [all]);
  const list = useMemo(() => {
    if (tab === "overview") return [];
    const now = Date.now() / 1000;
    const base = tab === "pending" ? all.filter((x) => !x.a.unlocked).sort((x, y) => x.s.title.localeCompare(y.s.title) || x.a.id - y.a.id)
      : tab === "recent" ? recent.filter((x) => (x.a.unlockedAt ?? 0) >= now - RECENT_SECS) : recent;
    return base.filter((x) => !q || x.s.title.toLowerCase().includes(q) || (!(x.a.hidden && !x.a.unlocked) && x.a.name.toLowerCase().includes(q)));
  }, [tab, all, recent, q]);

  const openLibrary = (gameId: number) => { setOpen(null); setOpenGameId(gameId); };
  if (!sets) return null;

  if (!hasEmu && sets.length === 0) {
    return (
      <>
        <div className="page-head"><h1>{t("Conquistas")}</h1>{switcher}</div>
        <div className="empty ra-empty">
          <TrophyIcon size={40} />
          <h2>{t("Cadastre o Vita3K ou o RPCS3")}</h2>
          <p>{t("Os troféus de PS Vita e PS3 são lidos dos arquivos que esses emuladores gravam: cadastre-os na página Emuladores e jogue normalmente.")}</p>
          <Button variant="primary" onClick={() => setView("emulators")}>{t("Ir para Emuladores")}</Button>
        </div>
      </>
    );
  }

  const tot = sets.reduce((a, s) => ({ unlocked: a.unlocked + s.unlocked, total: a.total + s.total }), { unlocked: 0, total: 0 });
  const plat = all.filter((x) => x.a.grade === "P" && x.a.unlocked).length;
  const perfect = sets.filter((s) => s.total > 0 && s.unlocked === s.total).length;
  const byGrade = (g: Trophy["grade"]) => all.filter((x) => x.a.grade === g && x.a.unlocked).length;

  return (
    <div className="ra-page">
      <div className="page-head"><h1>{t("Conquistas")}</h1>{switcher}</div>

      <section className="ra-hero">
        <div className="ra-user trophy-user">
          <span className="ra-user-ph"><TrophyIcon size={28} /></span>
          <span><b>{t("Troféus")}</b><small>Vita3K · RPCS3</small></span>
        </div>
        <div className="ra-stats">
          <div className="ra-stat main"><strong>{tot.unlocked.toLocaleString(undefined)}</strong><span>{t("Troféus obtidos")}</span></div>
          <div className="ra-stat"><strong>{plat}</strong><span>{gradeLabel("P")}</span></div>
          <div className="ra-stat"><strong>{pct(tot.unlocked, tot.total)}%</strong><span>{t("Conclusão média")}</span></div>
          <div className="ra-stat"><strong>{sets.length}</strong><span>{t("Jogos com troféus")}</span></div>
          <div className="ra-stat"><strong>{perfect}</strong><span>{t("Jogos 100%")}</span></div>
          <div className="ra-stat trophy-mix">
            <div className="trophy-mix-row">
              {(["G", "S", "B"] as const).map((g) => <span key={g} className={`trophy-grade g-${g}`} title={gradeLabel(g)}><i /><b>{byGrade(g)}</b></span>)}
            </div>
            <span>{`${gradeLabel("G")} · ${gradeLabel("S")} · ${gradeLabel("B")}`}</span>
          </div>
        </div>
      </section>
      <p className="ra-note">{t("Os troféus aparecem aqui depois que o jogo é aberto no emulador. O aviso de troféu desbloqueado sai na hora, enquanto você joga.")}</p>

      <div className="ra-toolbar">
        <div className="seg">
          {TABS.map(([k, l]) => <button key={k} type="button" data-focus className={tab === k ? "on" : ""} onClick={() => setTab(k)}>{t(l)}</button>)}
        </div>
        <input className="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("Pesquisar jogo…")} aria-label={t("Pesquisar jogo")} />
        {tab === "overview" && (
          <Select<Sort> value={sort} width={230} title={t("Ordenar")} onChange={setSort}
            options={[[t("Nome"), "name"], [t("Atividade recente"), "activity"], [t("Maior progresso"), "progress"], [t("Menor progresso"), "progress-asc"]]} />
        )}
      </div>

      {tab === "overview" ? (
        <>
          {recent.length > 0 && !q && (
            <>
              <h2 className="section">{t("Troféus recentes")}</h2>
              <div className="ra-recent">
                {recent.slice(0, 12).map(({ a, s }) => (
                  <button key={`${s.emulator}-${s.commId}-${a.id}`} data-focus className="ra-recent-item" onClick={() => setOpen(s)} title={`${a.name} — ${a.detail}`}>
                    {a.icon ? <img src={imgUrl(a.icon) ?? undefined} alt="" loading="lazy" /> : <TrophyIcon size={30} />}
                    <b>{a.name || `#${a.id}`}</b>
                    <small>{s.title}</small>
                    <em>{a.unlockedAt ? ago(a.unlockedAt) : t("Obtida")} · {gradeLabel(a.grade)}</em>
                  </button>
                ))}
              </div>
            </>
          )}
          <h2 className="section">{t("Jogos com troféus")}</h2>
          {games.length === 0 ? (
            <div className="empty"><p>{sets.length ? t("Nenhum jogo encontrado para essa pesquisa.") : t("Nenhum troféu ainda. Abra um jogo de PS Vita ou PS3 no emulador para os troféus aparecerem aqui.")}</p></div>
          ) : (
            <div className="ra-games">
              {games.map((g) => (
                <button key={`${g.emulator}-${g.commId}`} data-focus className="ra-game" onClick={() => setOpen(g)}>
                  <div className="ra-game-top">
                    {g.icon ? <img src={imgUrl(g.icon) ?? undefined} alt="" loading="lazy" /> : <span className="ra-icon-ph"><TrophyIcon /></span>}
                    <div>
                      <strong>{g.title}</strong>
                      <small>{g.emulator}{g.libraryGameId ? ` · ${t("Na biblioteca")}` : ""}</small>
                      <small>{t("Último desbloqueio: {ago}", { ago: ago(g.lastUnlockAt) })}</small>
                    </div>
                  </div>
                  <Progress unlocked={g.unlocked} total={g.total} compact />
                </button>
              ))}
            </div>
          )}
        </>
      ) : (
        <div className="ra-ach-list">
          {list.length === 0 && (
            <div className="empty"><p>{
              tab === "pending" ? t("Nenhum troféu pendente.") : tab === "recent" ? t("Nenhum troféu nos últimos 30 dias.") : t("Nenhum troféu obtido ainda.")
            }</p></div>
          )}
          {list.slice(0, 300).map(({ a, s }) => <TrophyItem key={`${s.emulator}-${s.commId}-${a.id}`} a={a} gameName={s.title} onClick={() => setOpen(s)} />)}
        </div>
      )}

      {open && <TrophySetModal set={open} onClose={() => setOpen(null)} onOpenLibrary={openLibrary} />}
    </div>
  );
}
