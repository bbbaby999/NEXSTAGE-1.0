import { CSSProperties, PointerEvent, useEffect, useRef, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import Seg from "./Seg";
import Glyph from "./Glyph";
import { onPadButton } from "../lib/nav";
import { useInputDevice } from "../lib/inputDevice";
import { play } from "../lib/sound";
import { contrast, hexToRgb, hsvToRgb, HSV, rgbToHex, rgbToHsv, RGB } from "../lib/color";
import { CustomColors, resolveCustom } from "../lib/theme";
import { t } from "../lib/i18n";

type Target = "a" | "b";
const PRESETS = ["#610000", "#b00020", "#ff6b00", "#f5c400", "#2e7d32", "#00a3a3", "#0057d9", "#5b2bd9", "#c2185b", "#5f6b7a"];
const SV_STEP = 0.02; // passo do direcional/setas no quadro (2%)

/**
 * Seletor de cor do tema personalizado: quadro saturação × brilho, barra de matiz, HEX e RGB exatos.
 * Edita a cor principal ou o destaque (automático por padrão). Pré-visualiza ao vivo pela `onChange`;
 * Cancelar desfaz, Aplicar salva. Funciona com mouse, teclado (setas no quadro) e controle (direcional).
 */
export default function ColorPicker({ initial, onChange, onCancel, onApply }: {
  initial: CustomColors; onChange: (c: CustomColors) => void; onCancel: () => void; onApply: (c: CustomColors) => void;
}) {
  const [colors, setColors] = useState<CustomColors>(initial);
  const [target, setTarget] = useState<Target>("a");
  const eff = resolveCustom(colors);
  const editingAuto = target === "b" && colors.b === null;
  const current = target === "a" ? eff.a : eff.b;

  // HSV próprio do alvo: guardar a matiz evita que ela "pule" para 0 quando a cor fica cinza/preta.
  const [hsv, setHsv] = useState<HSV>(() => rgbToHsv(hexToRgb(current)!));
  useEffect(() => { setHsv(rgbToHsv(hexToRgb(target === "a" ? eff.a : eff.b)!)); }, [target]); // eslint-disable-line react-hooks/exhaustive-deps
  const [hexText, setHexText] = useState(current);
  useEffect(() => { setHexText(current); }, [current]);

  useEffect(() => { onChange(colors); }, [colors]); // eslint-disable-line react-hooks/exhaustive-deps

  // Controle: Start / Options (☰ / +) aplica o tema — o mesmo botão que "Concluir" no teclado virtual.
  // Com o teclado virtual aberto (ex.: digitando o HEX), o botão continua sendo dele.
  const device = useInputDevice();
  const applyRef = useRef(() => onApply(colors));
  applyRef.current = () => onApply(colors);
  useEffect(() => onPadButton((b, e) => {
    if (b !== "menu" || e.defaultPrevented || document.querySelector(".kb-root, .pick-root, .ctx-root")) return;
    e.preventDefault();
    play("confirm");
    applyRef.current();
  }), []);

  const commit = (hex: string, nextHsv?: HSV) => {
    if (nextHsv) setHsv(nextHsv); else setHsv(rgbToHsv(hexToRgb(hex)!));
    setColors((c) => (target === "a" ? { ...c, a: hex } : { ...c, b: hex }));
  };
  const setFromHsv = (h: HSV) => commit(rgbToHex(hsvToRgb(h)), h);

  // ---- quadro saturação × brilho ----
  const sv = useRef<HTMLDivElement>(null);
  const pickAt = (e: PointerEvent<HTMLDivElement>) => {
    const r = sv.current!.getBoundingClientRect();
    const s = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    const v = Math.min(1, Math.max(0, 1 - (e.clientY - r.top) / r.height));
    setFromHsv([hsv[0], s, v]);
  };
  useEffect(() => {
    const el = sv.current;
    if (!el) return;
    // direcional / setas movem o ponto; na borda o foco sai normalmente
    const onCap = (e: Event) => {
      const d = (e as CustomEvent<string>).detail;
      const [h, s, v] = hsvRef.current;
      const ns = d === "left" ? s - SV_STEP : d === "right" ? s + SV_STEP : s;
      const nv = d === "down" ? v - SV_STEP : d === "up" ? v + SV_STEP : v;
      if ((d === "left" && s <= 0) || (d === "right" && s >= 1) || (d === "down" && v <= 0) || (d === "up" && v >= 1)) return;
      e.preventDefault();
      setFromHsvRef.current([h, Math.min(1, Math.max(0, ns)), Math.min(1, Math.max(0, nv))]);
    };
    el.addEventListener("navcapture", onCap);
    return () => el.removeEventListener("navcapture", onCap);
  }, []);
  const hsvRef = useRef(hsv); hsvRef.current = hsv;
  const setFromHsvRef = useRef(setFromHsv); setFromHsvRef.current = setFromHsv;

  const rgb = hexToRgb(current)!;
  const setChannel = (i: number, v: number) => {
    const next = [...rgb] as RGB;
    next[i] = Math.max(0, Math.min(255, Math.round(v || 0)));
    commit(rgbToHex(next));
  };
  const onHex = (t: string) => {
    setHexText(t);
    const c = hexToRgb(t);
    if (c) commit(rgbToHex(c));
  };

  const hueColor = rgbToHex(hsvToRgb([hsv[0], 1, 1]));
  // contraste do destaque contra o fundo escuro do app (anel de foco e realces precisam aparecer)
  const accentContrast = contrast(hexToRgb(eff.b)!, [5, 6, 10]);

  return (
    <Modal title={t("Personalizar cor do tema")} onClose={onCancel} wide>
      <div className="cp">
        <Seg<Target> value={target} onChange={setTarget} opts={[[t("Cor principal"), "a"], [t("Destaque"), "b"]]} />
        {target === "b" && (
          <div className="cp-auto">
            <Button variant={editingAuto ? "primary" : "default"}
              onClick={() => setColors((c) => ({ ...c, b: c.b === null ? eff.b : null }))}>
              {editingAuto ? t("✓ Destaque automático") : t("Destaque automático")}
            </Button>
            <small>{editingAuto ? t("Derivado da cor principal (mais claro e vivo). Mexa nos controles abaixo para escolher manualmente.") : t("Escolhido manualmente.")}</small>
          </div>
        )}

        <div className="cp-body">
          <div className="cp-pick">
            <div ref={sv} className="cp-sv" data-focus data-nav-capture tabIndex={0} role="slider"
              aria-label={t("Saturação e brilho")} aria-valuetext={t("Saturação {v0}%, brilho {v1}%", { v0: Math.round(hsv[1] * 100), v1: Math.round(hsv[2] * 100) })}
              style={{ "--hue": hueColor } as CSSProperties}
              onPointerDown={(e) => { e.currentTarget.setPointerCapture(e.pointerId); pickAt(e); }}
              onPointerMove={(e) => { if (e.buttons & 1) pickAt(e); }}>
              <i className="cp-thumb" style={{ left: `${hsv[1] * 100}%`, top: `${(1 - hsv[2]) * 100}%`, background: current }} />
            </div>
            <div className="range cp-hue">
              <input type="range" data-focus min={0} max={359} step={1} value={Math.round(hsv[0])} aria-label={t("Matiz")}
                onChange={(e) => setFromHsv([Number(e.target.value), hsv[1] || 1, hsv[2] || 1])} /* cor cinza/preta: a matiz passa a aparecer */ />
            </div>
          </div>

          <div className="cp-side">
            <div className="cp-preview">
              <div className="cp-sw" style={{ background: eff.a }}><span>{t("Principal")}</span><b>{eff.a.toUpperCase()}</b></div>
              <div className="cp-sw" style={{ background: eff.b }}><span>{t("Destaque")}</span><b>{eff.b.toUpperCase()}</b></div>
            </div>
            <label className="cp-field">
              <span>{t("HEX")}</span>
              <input className="search" type="text" value={hexText} maxLength={7} spellCheck={false}
                onChange={(e) => onHex(e.target.value)} onBlur={() => setHexText(current)} aria-label={t("Código HEX")} />
            </label>
            <div className="cp-rgb">
              {(["R", "G", "B"] as const).map((ch, i) => (
                <label key={ch} className="cp-field">
                  <span>{ch}</span>
                  <input className="search" type="number" min={0} max={255} value={rgb[i]}
                    onChange={(e) => setChannel(i, Number(e.target.value))} aria-label={t("Canal {ch}", { ch })} />
                </label>
              ))}
            </div>
            <div className="cp-presets" role="group" aria-label={t("Cores rápidas")}>
              {PRESETS.map((p) => (
                <button key={p} type="button" data-focus className="cp-preset" style={{ background: p }} title={p} aria-label={p}
                  onClick={() => commit(p)} />
              ))}
            </div>
            {accentContrast < 3 && (
              <small className="cp-warn">{t("O destaque está escuro demais para aparecer bem sobre o fundo (anel de foco e realces). Use o destaque automático ou um tom mais claro.")}</small>
            )}
          </div>
        </div>

        <div className="row cp-actions">
          <Button onClick={onCancel}>{t("Cancelar")}</Button>
          <Button variant="primary" onClick={() => onApply(colors)}>{t("Aplicar")}{device !== "keyboard" && <> <Glyph k="menu" /></>}</Button>
        </div>
      </div>
    </Modal>
  );
}
