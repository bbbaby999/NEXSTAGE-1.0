import { useEffect, useRef, useState } from "react";
import { currentLang, t } from "../../lib/i18n";

/** Série de um gráfico: cor de identidade (validada para o fundo escuro) + rótulo para legenda/tabela. */
export interface Series { key: string; label: string; color: string; values: number[] }

/**
 * Gráfico SVG próprio (sem dependências): barras (uma série ou duas empilhadas) ou linha.
 * Regras: um único eixo, grade discreta, barras finas com topo arredondado de 4px ancorado na base,
 * 2px de espaço entre segmentos empilhados, dica ao passar o mouse/foco, legenda com 2+ séries e
 * alternativa em tabela.
 */
export default function StatChart({ kind, labels, series, format, height = 180, title }: {
  kind: "bar" | "line"; labels: string[]; series: Series[]; format: (v: number) => string; height?: number; title: string;
}) {
  const wrap = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(600);
  const [hover, setHover] = useState<number | null>(null);
  const [table, setTable] = useState(false);
  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setW(Math.max(260, el.clientWidth)));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const n = labels.length;
  const totals = labels.map((_, i) => series.reduce((s, x) => s + (x.values[i] ?? 0), 0));
  const max = Math.max(1, ...(kind === "bar" ? totals : series.flatMap((s) => s.values)));
  const pad = { l: 8, r: 8, t: 10, b: 22 };
  const ih = height - pad.t - pad.b;
  const step = (w - pad.l - pad.r) / Math.max(1, n);
  const bw = Math.min(28, Math.max(6, step * 0.55));
  const y = (v: number) => pad.t + ih - (v / max) * ih;
  const x = (i: number) => pad.l + step * i + step / 2;
  const every = Math.ceil(n / Math.max(1, Math.floor((w - 16) / 56))); // rótulos do eixo sem colisão

  const tip = hover !== null ? (
    <div className="sc-tip" style={{ left: Math.min(Math.max(x(hover), 70), w - 70), top: 0 }} role="status">
      <b>{labels[hover]}</b>
      {series.map((s) => <span key={s.key}><i style={{ background: s.color }} />{s.label}: {format(s.values[hover] ?? 0)}</span>)}
      {series.length > 1 && <span className="sc-sum">{t("Total")}: {format(totals[hover])}</span>}
    </div>
  ) : null;

  return (
    <figure className="sc" aria-label={title}>
      <div className="sc-top">
        {series.length > 1 && (
          <div className="sc-legend">{series.map((s) => <span key={s.key}><i style={{ background: s.color }} />{s.label}</span>)}</div>
        )}
        <button data-focus type="button" className="sc-toggle" onClick={() => setTable(!table)}>{table ? t("Ver gráfico") : t("Ver tabela")}</button>
      </div>
      {table ? (
        <table className="sc-table">
          <thead><tr><th>{t("Período")}</th>{series.map((s) => <th key={s.key}>{s.label}</th>)}</tr></thead>
          <tbody>{labels.map((l, i) => <tr key={l}><td>{l}</td>{series.map((s) => <td key={s.key}>{format(s.values[i] ?? 0)}</td>)}</tr>)}</tbody>
        </table>
      ) : (
        <div className="sc-plot" ref={wrap} onMouseLeave={() => setHover(null)}>
          {tip}
          <svg width={w} height={height} role="img" aria-label={title}>
            {[0.5, 1].map((f) => (
              <g key={f}>
                <line x1={pad.l} x2={w - pad.r} y1={y(max * f)} y2={y(max * f)} className="sc-grid" />
                <text x={pad.l + 2} y={y(max * f) - 3} className="sc-axis">{format(max * f)}</text>
              </g>
            ))}
            <line x1={pad.l} x2={w - pad.r} y1={y(0)} y2={y(0)} className="sc-base" />
            {kind === "bar"
              ? labels.map((_, i) => {
                  let acc = 0;
                  const segs = series.map((s, si) => {
                    const v = s.values[i] ?? 0;
                    if (v <= 0) return null;
                    const top = y(acc + v), bottom = y(acc) - (acc > 0 ? 2 : 0); // 2px entre segmentos
                    acc += v;
                    const h = Math.max(1, bottom - top);
                    const isTop = series.slice(si + 1).every((o) => (o.values[i] ?? 0) <= 0);
                    const r = isTop ? Math.min(4, h / 2, bw / 2) : 0;
                    const x0 = x(i) - bw / 2;
                    // topo arredondado (4px) só no segmento de cima; base reta, presa à linha de base
                    const d = `M${x0},${bottom} V${top + r} Q${x0},${top} ${x0 + r},${top} H${x0 + bw - r} Q${x0 + bw},${top} ${x0 + bw},${top + r} V${bottom} Z`;
                    return <path key={s.key} d={d} style={{ fill: s.color }} opacity={hover === null || hover === i ? 1 : 0.55} />;
                  });
                  return <g key={i}>{segs}</g>;
                })
              : series.map((s) => (
                  <g key={s.key}>
                    <polyline fill="none" style={{ stroke: s.color }} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round"
                      points={s.values.map((v, i) => `${x(i)},${y(v)}`).join(" ")} />
                    {hover !== null && <circle cx={x(hover)} cy={y(s.values[hover] ?? 0)} r={4.5} style={{ fill: s.color }} className="sc-dot" />}
                  </g>
                ))}
            {kind === "line" && hover !== null && <line x1={x(hover)} x2={x(hover)} y1={pad.t} y2={y(0)} className="sc-cross" />}
            {labels.map((l, i) => (i % every === 0 || i === n - 1) && (
              <text key={l} x={x(i)} y={height - 6} textAnchor="middle" className="sc-axis">{l}</text>
            ))}
          </svg>
          {/* áreas de toque/foco maiores que as marcas: botões reais, alcançáveis por controle (D-pad),
              teclado (setas/Tab) e mouse — cada um mostra a dica do mês. */}
          {labels.map((l, i) => (
            <button key={`h${l}`} type="button" data-focus className="sc-hit" style={{ left: pad.l + step * i, width: step, top: pad.t, height: ih }}
              aria-label={`${l}: ${series.map((s) => `${s.label} ${format(s.values[i] ?? 0)}`).join(", ")}`}
              onMouseEnter={() => setHover(i)} onFocus={() => setHover(i)} onBlur={() => setHover(null)} />
          ))}
        </div>
      )}
    </figure>
  );
}

/** "2026-03" → "mar. 26" no idioma da interface. */
export function monthLabel(m: string): string {
  const [yy, mm] = m.split("-").map(Number);
  const d = new Date(yy, (mm || 1) - 1, 1);
  return isNaN(d.getTime()) ? m : d.toLocaleDateString(currentLang(), { month: "short", year: "2-digit" });
}

/** Lista contínua dos últimos `n` meses (inclui meses sem atividade, com zero). */
export function lastMonths(n: number): string[] {
  const out: string[] = [];
  const now = new Date();
  for (let i = n - 1; i >= 0; i--) {
    const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
    out.push(`${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`);
  }
  return out;
}
