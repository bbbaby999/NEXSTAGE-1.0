import { useCallback, useEffect, useState } from "react";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { formatBytes } from "../lib/format";
import type { Drive } from "../types";
import { currentLang, t } from "../lib/i18n";

interface Slice { key: string; label: string; bytes: number; cls: string; note?: string }

/** Unidades ocultas no painel: chave em app_settings e valor padrão quando nada foi salvo ainda. */
const HIDDEN_KEY = "hidden_drives";
const DEFAULT_HIDDEN = "D:";
/** "D:\" → "D:" (chave da unidade). */
const driveKey = (mount: string) => mount.replace(/[\\/]+$/, "").toUpperCase();

const R_OUT = 92, R_IN = 60, C = 100;
/** Fatias não vazias menores que isso ganham esse ângulo mínimo (só no desenho; valores exatos na legenda). */
const MIN_DEG = 1.5;

const pt = (r: number, deg: number) => {
  const a = ((deg - 90) * Math.PI) / 180;
  return [C + r * Math.cos(a), C + r * Math.sin(a)];
};
/** Setor de rosca de `a0` a `a1` graus (0 = topo, sentido horário). */
function arc(a0: number, a1: number) {
  if (a1 - a0 >= 359.99) a1 = a0 + 359.99; // círculo completo: o arco SVG precisa de pontos distintos
  const large = a1 - a0 > 180 ? 1 : 0;
  const [x0, y0] = pt(R_OUT, a0), [x1, y1] = pt(R_OUT, a1);
  const [x2, y2] = pt(R_IN, a1), [x3, y3] = pt(R_IN, a0);
  return `M${x0} ${y0}A${R_OUT} ${R_OUT} 0 ${large} 1 ${x1} ${y1}L${x2} ${y2}A${R_IN} ${R_IN} 0 ${large} 0 ${x3} ${y3}Z`;
}

/** Ângulos de cada fatia, com mínimo visível para fatias pequenas (o excesso sai da maior). */
function angles(slices: Slice[], total: number) {
  const deg = slices.map((s) => (total ? (s.bytes / total) * 360 : 0));
  const boosted = deg.map((d) => (d > 0 && d < MIN_DEG ? MIN_DEG : d));
  const extra = boosted.reduce((a, b) => a + b, 0) - 360;
  if (extra > 0) { const big = boosted.indexOf(Math.max(...boosted)); boosted[big] -= extra; }
  let acc = 0;
  return boosted.map((d) => { const a0 = acc; acc += d; return [a0, acc] as const; });
}

/**
 * Armazenamento das unidades detectadas pelo Windows: gráfico de rosca (NEXSTAGE, capturas, outros e
 * livre) com dica ao passar o mouse e legenda com os valores exatos. Atualiza a cada 30 s e sob demanda.
 */
export default function StoragePanel() {
  const [all, setAll] = useState<Drive[] | null>(null);
  const [hidden, setHidden] = useState<Set<string> | null>(null);
  const [sel, setSel] = useState(0);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [hover, setHover] = useState<string | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    try {
      setAll(await api.getStorage());
      setErr(null);
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    load();
    const t = setInterval(load, 30000);
    return () => clearInterval(t);
  }, [load]);
  // Unidades ocultas (app_settings.hidden_drives, ex.: "D:,E:"). Sem valor salvo: D: oculta (comportamento anterior).
  useEffect(() => {
    api.getAppSetting(HIDDEN_KEY).catch(() => null)
      .then((v) => setHidden(new Set((v ?? DEFAULT_HIDDEN).split(",").map((s) => s.trim().toUpperCase()).filter(Boolean))));
  }, []);
  const toggleHidden = (key: string) => {
    if (!hidden) return;
    const next = new Set(hidden);
    if (next.has(key)) next.delete(key); else next.add(key);
    setHidden(next);
    api.setAppSetting(HIDDEN_KEY, [...next].join(",")).catch(() => {});
  };

  if (err && !all) return <div className="setting"><div><strong>{t("Não foi possível ler as unidades")}</strong><small>{err}</small></div><Button onClick={load}>{t("Tentar novamente")}</Button></div>;
  if (!all || !hidden) return <div className="setting"><div><strong>{t("Lendo unidades…")}</strong></div></div>;
  if (!all.length) return <div className="setting"><div><strong>{t("Nenhuma unidade detectada.")}</strong></div><Button onClick={load}>{t("Atualizar")}</Button></div>;

  const drives = all.filter((x) => !hidden.has(driveKey(x.mount)));
  // Escolha de unidades exibidas: sempre disponível (inclusive com todas ocultas, para poder desfazer).
  const manage = (
    <div className="setting st-manage">
      <div><strong>{t("Unidades exibidas")}</strong><small>{t("Escolha quais unidades aparecem no gráfico. A escolha é lembrada.")}</small></div>
      <div className="st-drives" role="group" aria-label={t("Unidades exibidas")}>
        {all.map((x, i) => {
          const k = driveKey(x.mount), on = !hidden.has(k);
          return (
            <button key={x.mount + i} type="button" data-focus aria-pressed={on} className={`chip ${on ? "on" : ""}`} onClick={() => toggleHidden(k)}>
              {on ? "✓ " : ""}{k} <small>{x.name || (x.removable ? t("Removível") : x.kind || t("Disco"))}</small>
            </button>
          );
        })}
      </div>
    </div>
  );
  if (!drives.length) return (
    <>
      <div className="setting"><div><strong>{t("Todas as unidades estão ocultas.")}</strong><small>{t("Ative ao menos uma em “Unidades exibidas”.")}</small></div></div>
      {manage}
    </>
  );

  const d = drives[Math.min(sel, drives.length - 1)];
  const here = d.nexstageBytes > 0 || d.screenshotsBytes > 0;
  const others = Math.max(0, d.usedBytes - d.nexstageBytes - d.screenshotsBytes);
  const slices: Slice[] = [
    { key: "nx", label: t("Dados do NEXSTAGE"), bytes: d.nexstageBytes, cls: "st-nx", note: here ? undefined : t("não está nesta unidade") },
    { key: "shot", label: t("Capturas (galeria)"), bytes: d.screenshotsBytes, cls: "st-shot", note: here ? undefined : t("não está nesta unidade") },
    { key: "other", label: t("Outros (jogos, vídeos e demais arquivos)"), bytes: others, cls: "st-other" },
    { key: "free", label: t("Livre"), bytes: d.freeBytes, cls: "st-free" },
  ];
  const total = d.totalBytes || slices.reduce((a, s) => a + s.bytes, 0);
  const pctOf = (v: number) => (total ? (v / total) * 100 : 0);
  const fmtPct = (v: number) => { const p = pctOf(v); return p > 0 && p < 0.1 ? "<0,1%" : `${p.toLocaleString(currentLang(), { maximumFractionDigits: p < 10 ? 1 : 0 })}%`; };
  const ang = angles(slices, total);
  const hv = slices.find((s) => s.key === hover) ?? null;
  const usedPct = Math.round(pctOf(d.usedBytes));

  return (
    <>
    <div className="setting col-set storage-box">
      <div className="st-head">
        <div className="st-drives" role="radiogroup" aria-label={t("Unidade")}>
          {drives.map((x, i) => (
            <button key={x.mount + i} type="button" data-focus role="radio" aria-checked={x === d}
              className={`chip ${x === d ? "on" : ""}`} onClick={() => setSel(i)}>
              {x.mount.replace(/\\$/, "")} <small>{x.removable ? t("Removível") : x.kind || t("Disco")}</small>
            </button>
          ))}
        </div>
        <Button onClick={load} disabled={busy}>{busy ? t("Atualizando…") : t("Atualizar")}</Button>
      </div>

      <div>
        <strong>{d.name || t("Disco local")}</strong>
        <small>{d.mount}{d.fileSystem ? ` · ${d.fileSystem}` : ""}{d.accessible ? t(" · {v0} no total", { v0: formatBytes(d.totalBytes) }) : ""}
          {err && t(" · Última atualização falhou: {err}", { err })}</small>
      </div>

      {!d.accessible ? (
        <p className="muted">{t("Unidade inacessível ou sem mídia no momento.")}</p>
      ) : (
        <div className="st-body">
          <div className="st-donut" onMouseLeave={() => setHover(null)}>
            <svg viewBox="0 0 200 200" role="img"
              aria-label={t("{v0}: {usedPct}% usado, {v2} livres de {v3}", { v0: d.name || d.mount, usedPct, v2: formatBytes(d.freeBytes), v3: formatBytes(d.totalBytes) })}>
              {slices.map((s, i) => s.bytes > 0 && (
                <path key={s.key} d={arc(ang[i][0], ang[i][1])} className={`st-arc ${s.cls} ${hover && hover !== s.key ? "dim" : ""} ${hover === s.key ? "hot" : ""}`}
                  onMouseEnter={() => setHover(s.key)} />
              ))}
            </svg>
            {/* centro: destaque do item sob o mouse ou o total usado */}
            <div className="st-center" aria-hidden="true">
              {hv ? (<><b>{fmtPct(hv.bytes)}</b><span>{formatBytes(hv.bytes)}</span><small>{hv.label}</small></>)
                : (<><b>{usedPct}%</b><span>usado</span><small>{formatBytes(d.freeBytes)} livres</small></>)}
            </div>
          </div>

          <ul className="st-legend">
            {slices.map((s) => (
              <li key={s.key} className={hover === s.key ? "hot" : ""} onMouseEnter={() => setHover(s.key)} onMouseLeave={() => setHover(null)}>
                <i className={s.cls} aria-hidden="true" />
                <span>{s.label}</span>
                <b>{s.note ?? formatBytes(s.bytes)}</b>
                <em>{s.note ? "" : fmtPct(s.bytes)}</em>
              </li>
            ))}
          </ul>
        </div>
      )}
      {d.accessible && <small>{t("Jogos e vídeos não são separados porque não há como identificá-los com exatidão; ficam em “Outros”.")}</small>}
    </div>
    {manage}
    </>
  );
}
