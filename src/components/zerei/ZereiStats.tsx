import type { ZereiEntry } from "../../types";
import { computeStats, fmtDay, fmtHours, fmtRating } from "../../lib/zerei";
import { currentLang, t } from "../../lib/i18n";

/** Barras horizontais de uma série só (cor do tema), com o valor escrito ao lado (a lista é a própria tabela). */
function Bars({ rows, label }: { rows: [string, number][]; label: string }) {
  const max = Math.max(1, ...rows.map((r) => r[1]));
  return (
    <ul className="zr-bars" aria-label={label}>
      {rows.map(([k, v]) => (
        <li key={k} title={`${k}: ${v}`}>
          <span className="zr-bar-k">{k}</span>
          <span className="zr-bar-track"><i style={{ width: `${(v / max) * 100}%` }} /></span>
          <b className="zr-bar-v">{v}</b>
        </li>
      ))}
    </ul>
  );
}

/** Estatísticas do Zerei!: zerados, horas, média das notas, por plataforma e histórico de conclusões. */
export default function ZereiStats({ list, onOpen }: { list: ZereiEntry[]; onOpen: (id: number) => void }) {
  const s = computeStats(list);
  const num = (n: number) => n.toLocaleString(currentLang(), { maximumFractionDigits: 1 });
  const tiles: [string, string, string?][] = [
    [t("Jogos zerados"), num(s.completed), s.full ? t("{n} platinado(s)", { n: s.full }) : undefined],
    [t("Horas jogadas"), num(s.hours), t("em {n} registro(s)", { n: s.total })],
    [t("Média das avaliações"), s.avgRating === null ? "—" : fmtRating(s.avgRating), s.rated ? t("{n} jogo(s) avaliado(s)", { n: s.rated }) : undefined],
    [t("Jogando agora"), num(s.playing), s.paused ? t("{n} pausado(s)", { n: s.paused }) : undefined],
    [t("Lista de desejos"), num(s.wishlist)],
    [t("Não iniciados"), num(s.notStarted)],
  ];
  if (!list.length) return <div className="empty"><p>{t("As estatísticas aparecem quando houver jogos no Zerei!.")}</p></div>;

  return (
    <div className="zr-stats">
      <div className="zr-tiles">
        {tiles.map(([k, v, sub]) => (
          <div key={k} className="zr-tile">
            <small>{k}</small>
            <b>{v}</b>
            {sub && <span className="muted">{sub}</span>}
          </div>
        ))}
      </div>
      <div className="zr-stats-grid">
        <section className="zr-panel">
          <h3 className="section sub">{t("Zerados por plataforma")}</h3>
          {s.byPlatform.length ? <Bars rows={s.byPlatform} label={t("Zerados por plataforma")} /> : <p className="muted">{t("Nenhum jogo zerado ainda.")}</p>}
        </section>
        <section className="zr-panel">
          <h3 className="section sub">{t("Conclusões por ano")}</h3>
          {s.byYear.length ? <Bars rows={s.byYear.map(([y, n]) => [String(y), n])} label={t("Conclusões por ano")} />
            : <p className="muted">{t("Informe a data em que zerou para ver o histórico.")}</p>}
        </section>
        <section className="zr-panel wide">
          <h3 className="section sub">{t("Histórico de conclusões")}</h3>
          {s.recent.length ? (
            <ol className="zr-timeline" data-nav-group>
              {s.recent.map((e) => (
                <li key={e.id}>
                  <button type="button" data-focus data-zerei-hist={e.id} onClick={() => onOpen(e.id)}>
                    <time>{fmtDay(e.completedAt)}</time>
                    <span className="zr-tl-cover" style={e.cover ? { backgroundImage: `url("${e.cover}")` } : undefined} />
                    <span className="zr-tl-text"><strong>{e.title}</strong><small>{[e.platform, e.hours !== null ? fmtHours(e.hours) : null, e.rating !== null ? `★ ${fmtRating(e.rating)}` : null].filter(Boolean).join(" · ")}</small></span>
                  </button>
                </li>
              ))}
            </ol>
          ) : <p className="muted">{t("Nenhuma conclusão com data ainda.")}</p>}
        </section>
      </div>
    </div>
  );
}
