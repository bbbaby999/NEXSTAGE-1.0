/** Temas do app. As cores ficam em variáveis CSS (`[data-theme]` em styles.css). */
import { api } from "./api";
import { autoAccent, hexToRgb, textOn, tint } from "./color";
import { tk } from "./i18n";
export const THEMES = [
  { id: "midnight", name: tk("Azul Meia-Noite"), a: "#2f6bff", b: "#22e4ff" },
  { id: "neon", name: tk("Neon"), a: "#6d28d9", b: "#ff4fd8" },
  { id: "crimson", name: tk("Carmesim"), a: "#991b1b", b: "#f87171" },
  { id: "blood", name: tk("Vermelho Sangue"), a: "#610000", b: "#dc2626" },
  { id: "emerald", name: tk("Esmeralda"), a: "#10b981", b: "#6ee7b7" },
  { id: "grass", name: tk("Verde Grama"), a: "#166534", b: "#4ade80" },
  { id: "graphite", name: tk("Branco Gelo"), a: "#7d86a3", b: "#e6ebff" },
  { id: "noon", name: tk("Meio-Dia"), a: "#facc15", b: "#fef08a" },
  // `bg`: tema com fundo próprio (a prévia mostra o fundo junto da cor de destaque)
  { id: "blackpink", name: tk("Black & Pink"), a: "#ff4fa3", b: "#ffb3d9", bg: "#0b0b0f" },
] as const satisfies readonly { id: string; name: string; a: string; b: string; bg?: string }[];

export const DEFAULT_THEME = "midnight";

// Barra de luz dos controles PlayStation (DualSense/DualShock 4). `null` = preferência ainda não lida:
// nada é enviado ao controle antes de saber se a sincronização está ligada.
let ledSync: boolean | null = null;
let ledColor: string = THEMES[0].a;

/**
 * Envia ao backend a opção geral (cor de destaque ou "off" = não mexer) e a cor de destaque em si.
 * Os ajustes individuais de cada controle Sony cadastrado (Central de Controles) vão nos `PadCfg`
 * (`led`) e têm prioridade: "Cor do tema" usa a cor de destaque mesmo com a opção geral desligada.
 */
function applyLed() {
  if (ledSync === null) return; // preferências ainda não lidas
  api.setPadLed(ledSync ? ledColor : "off", ledColor).catch(() => {});
}

/** Liga/desliga a sincronização do LED com a cor de destaque (salva em app_settings.pad_led_sync). */
export function setLedSync(on: boolean) {
  ledSync = on;
  applyLed();
}

// ---------- Tema personalizado ----------
/** Id do tema personalizado (salvo em profile_settings.theme; as cores em profile_settings.theme_custom). */
export const CUSTOM_THEME = "custom";
/** Cores escolhidas: `b` = destaque manual, ou `null` para derivar automaticamente da principal. */
export interface CustomColors { a: string; b: string | null }
export const DEFAULT_CUSTOM: CustomColors = { a: "#610000", b: null };
let custom: CustomColors = DEFAULT_CUSTOM;
const CUSTOM_PROPS = ["--a", "--b", "--ar", "--br", "--on-a", "--lab"];

/** Cores efetivas (destaque automático resolvido). */
export function resolveCustom(c: CustomColors) {
  const a = hexToRgb(c.a) ? c.a : DEFAULT_CUSTOM.a;
  const b = c.b && hexToRgb(c.b) ? c.b : autoAccent(a);
  return { a, b };
}
/** Lê o JSON salvo; valor ausente ou inválido → padrão. */
export function parseCustom(v: string | null | undefined): CustomColors {
  try {
    const o = JSON.parse(v ?? "");
    if (o && typeof o.a === "string" && hexToRgb(o.a)) return { a: o.a, b: typeof o.b === "string" && hexToRgb(o.b) ? o.b : null };
  } catch { /* padrão */ }
  return DEFAULT_CUSTOM;
}
/** Guarda as cores personalizadas (e reaplica na hora se o tema personalizado estiver ativo). */
export function setCustomColors(c: CustomColors) {
  custom = c;
  if (document.documentElement.dataset.theme === CUSTOM_THEME) applyTheme(CUSTOM_THEME);
}

export function applyTheme(id: string) {
  const root = document.documentElement;
  if (id === CUSTOM_THEME) {
    // Mesmas variáveis que os temas fixos definem em styles.css, calculadas a partir das cores escolhidas.
    const { a, b } = resolveCustom(custom);
    const vars: Record<string, string> = {
      "--a": a, "--b": b, "--ar": hexToRgb(a)!.join(","), "--br": hexToRgb(b)!.join(","),
      "--on-a": textOn(a), "--lab": tint(b, 0.35),
    };
    for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
    root.dataset.theme = CUSTOM_THEME;
    ledColor = a;
    applyLed();
    return;
  }
  CUSTOM_PROPS.forEach((p) => root.style.removeProperty(p));
  const t = THEMES.find((x) => x.id === id) ?? THEMES.find((x) => x.id === DEFAULT_THEME)!;
  root.dataset.theme = t.id;
  ledColor = t.a;
  applyLed(); // o LED acompanha a cor de destaque
}

/** Carrega e aplica o tema do perfil (inclusive as cores do personalizado). */
export async function loadProfileTheme(profileId: number): Promise<{ id: string; custom: CustomColors }> {
  const [id, c] = await Promise.all([
    api.getProfileSetting(profileId, "theme").catch(() => null),
    api.getProfileSetting(profileId, "theme_custom").catch(() => null),
  ]);
  const parsed = parseCustom(c);
  custom = parsed;
  applyTheme(id ?? DEFAULT_THEME);
  return { id: id ?? DEFAULT_THEME, custom: parsed };
}

/** Liga/desliga o fundo animado de ondas (lido pelo WaveBackground e pelo CSS). */
export function applyWaves(on: boolean) {
  document.documentElement.dataset.waves = on ? "on" : "off";
}

/** Liga/desliga as partículas cósmicas do fundo (lido pelo WaveBackground a cada quadro). */
export function applyParticles(on: boolean) {
  document.documentElement.dataset.particles = on ? "on" : "off";
}

/** Desfoque máximo (px) do plano de fundo da Home quando o ajuste está em 100%. */
const HOME_BLUR_MAX = 32;

/** Normaliza o desfoque do fundo da Home para 0–100 em passos de 5 (salvo por perfil em `home_blur`). */
export function clampHomeBlur(v: unknown): number {
  const n = Number(v);
  if (!Number.isFinite(n)) return 0;
  return Math.max(0, Math.min(100, Math.round(n / 5) * 5));
}

/** Aplica o desfoque do fundo da Home (variável CSS lida em stage.css). */
export function applyHomeBlur(pct: number) {
  const px = (clampHomeBlur(pct) / 100) * HOME_BLUR_MAX;
  document.documentElement.style.setProperty("--home-blur", `${px}px`);
}
