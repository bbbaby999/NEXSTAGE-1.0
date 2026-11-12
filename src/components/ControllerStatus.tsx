import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { deviceLabels, gamepadControllers, noteNativeControllers, onNativeDevices, useControllers } from "../lib/controllers";
import type { Controller } from "../types";
import { t } from "../lib/i18n";

/** `compact`: só o ícone e o nível (barra superior com vários controles). */
export function Battery({ c, compact = false }: { c: Controller; compact?: boolean }) {
  // Com cabo USB: só o raio. Sem % nem "carregando" — o controle não informa isso com cabo.
  if (c.wired) {
    return (
      <span className="batt wired" title={t("Conectado por cabo (USB)")} aria-label={t("Conectado por cabo")}>
        <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true"><path d="M13.5 2L5 13.5h6L9.5 22 19 9.5h-6.2z" fill="currentColor" /></svg>
      </span>
    );
  }
  const pct = c.batteryPercent;
  const low = (pct !== null && pct <= 20) || c.batteryLabel === "Crítica" || c.batteryLabel === "Baixa";
  const fill = pct ?? (c.batteryLabel === "Cheia" ? 100 : c.batteryLabel === "Média" ? 55 : c.batteryLabel === "Baixa" ? 25 : c.batteryLabel === "Crítica" ? 8 : null);
  if (fill === null) return compact ? null : <span className="batt na">{t("Bateria indisponível")}</span>;
  return (
    <span className={`batt ${low ? "low" : ""}`}>
      <i className="cell"><b style={{ width: `${fill}%` }} /></i>
      {pct !== null ? `${pct}%` : c.batteryLabel ? t(c.batteryLabel) : null}
      {c.charging && " ⚡"}
      {low && !compact && t(" · baixa")}
    </span>
  );
}

const MAX_SHOWN = 2;

/** Controles conectados e bateria (somente dados reais; sem valores simulados). */
export default function ControllerStatus() {
  const [list, setList] = useState<Controller[]>([]);
  const [pads, setPads] = useState<Controller[]>([]);
  const regs = useControllers();

  useEffect(() => {
    let alive = true;
    const scan = () => setPads(gamepadControllers());
    const load = () => api.getControllers().then((v) => { if (!alive) return; noteNativeControllers(v); setList(v); scan(); }).catch(() => {});
    const both = () => { scan(); load(); };
    both();
    const t = setInterval(both, 10000);
    // controle nativo conectado/desconectado: atualiza na hora (antes esperava até 10 s)
    const offNative = onNativeDevices(load);
    window.addEventListener("gamepadconnected", both);
    window.addEventListener("gamepaddisconnected", both);
    return () => {
      alive = false;
      clearInterval(t);
      offNative();
      window.removeEventListener("gamepadconnected", both);
      window.removeEventListener("gamepaddisconnected", both);
    };
  }, []);

  const items = [...list, ...pads];
  if (!items.length) return <div className="ctl none">{t("Nenhum controle conectado")}</div>;
  const labels = deviceLabels(items, t); // dois controles do mesmo modelo: "DualSense (1)", "DualSense (2)"
  // nome personalizado na Central de Controles, quando cadastrado
  const nameOf = (c: Controller) => regs.find((r) => r.deviceKey === c.id)?.name ?? labels.get(c.id) ?? t(c.name);
  // Vários controles: modo compacto (nome mais curto, só o nível da bateria) e no máximo MAX_SHOWN
  // visíveis; os demais viram "+N" (a lista completa aparece ao passar o mouse).
  const many = items.length > 1;
  const shown = items.slice(0, MAX_SHOWN);
  const rest = items.slice(MAX_SHOWN);
  return (
    <div className={`ctl${many ? " many" : ""}`}>
      {shown.map((c, i) => (
        <div className="ctl-item" key={`${c.id}-${i}`} title={nameOf(c)}>
          <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
            <path d="M6 8h12a4 4 0 014 4v3a2 2 0 01-3.5 1.3L16 14H8l-2.5 2.3A2 2 0 012 15v-3a4 4 0 014-4z" />
          </svg>
          <span>{nameOf(c)}</span>
          <Battery c={c} compact={many} />
        </div>
      ))}
      {rest.length > 0 && <div className="ctl-item ctl-more" title={rest.map(nameOf).join(", ")}>+{rest.length}</div>}
    </div>
  );
}
