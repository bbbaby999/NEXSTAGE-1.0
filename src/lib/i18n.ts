/**
 * Idiomas do NEXSTAGE.
 *
 * - O texto em português (Brasil) do próprio código é a CHAVE: `t("Jogo atualizado.")`. Assim o
 *   português não precisa de dicionário e nenhum texto some se faltar uma tradução (cai no original).
 * - Variáveis: `t("Iniciando {title}…", { title })`.
 * - Os dicionários (src/locales/*.ts) carregam sob demanda; só o idioma em uso entra na memória.
 * - Ordem para decidir o idioma na primeira abertura: escolha salva → idioma escolhido no instalador →
 *   idioma do Windows → português.
 */
import { useSyncExternalStore } from "react";

export type Lang = "pt-BR" | "en" | "es" | "fr" | "de" | "it" | "ru" | "ja" | "ko" | "zh-CN";

/** Nome de cada idioma escrito nele mesmo (como os jogos e sistemas mostram). */
export const LANGS: { id: Lang; name: string }[] = [
  { id: "pt-BR", name: "Português (Brasil)" },
  { id: "en", name: "English" },
  { id: "es", name: "Español" },
  { id: "fr", name: "Français" },
  { id: "de", name: "Deutsch" },
  { id: "it", name: "Italiano" },
  { id: "ru", name: "Русский" },
  { id: "ja", name: "日本語" },
  { id: "ko", name: "한국어" },
  { id: "zh-CN", name: "简体中文" },
];

type Dict = Record<string, string>;
/**
 * Cada dicionário é uma LISTA de traduções na mesma ordem de `locales/keys.ts` (sem repetir as chaves
 * em cada idioma). `keys.ts` só cresce no fim, então as posições nunca mudam. Item vazio = sem tradução
 * (mostra o português).
 */
const loaders: Record<Exclude<Lang, "pt-BR">, () => Promise<{ default: string[] }>> = {
  en: () => import("../locales/en"),
  es: () => import("../locales/es"),
  fr: () => import("../locales/fr"),
  de: () => import("../locales/de"),
  it: () => import("../locales/it"),
  ru: () => import("../locales/ru"),
  ja: () => import("../locales/ja"),
  ko: () => import("../locales/ko"),
  "zh-CN": () => import("../locales/zh-CN"),
};

let lang: Lang = "pt-BR";
let dict: Dict = {};
const subs = new Set<() => void>();

export const currentLang = () => lang;
export const isLang = (v: unknown): v is Lang => LANGS.some((l) => l.id === v);

/** Troca o idioma (carrega o dicionário antes de avisar a interface: sem "piscar" em português). */
export async function setLang(l: Lang) {
  let next: Dict = {};
  if (l !== "pt-BR") {
    const [{ KEYS }, { default: list }] = await Promise.all([import("../locales/keys"), loaders[l]()]);
    next = Object.fromEntries(KEYS.map((k, i) => [k, list[i]]).filter(([, v]) => !!v));
  }
  lang = l;
  dict = next;
  document.documentElement.lang = l;
  subs.forEach((f) => f());
}

/** Traduz um texto (chave = português). `{nome}` é trocado pelas variáveis. */
export function t(s: string, vars?: Record<string, string | number>): string {
  let out = dict[s] ?? s;
  if (vars) out = out.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
  return out;
}

/** Re-renderiza o componente quando o idioma muda (devolve o idioma atual). */
export function useLang(): Lang {
  return useSyncExternalStore((cb) => { subs.add(cb); return () => subs.delete(cb); }, currentLang);
}

/** Idioma do Windows (ex.: "pt-BR", "en-US", "zh-Hans-CN") → um dos suportados; `null` se nenhum. */
export function matchLocale(tag: string | null | undefined): Lang | null {
  if (!tag) return null;
  const low = tag.toLowerCase();
  if (low.startsWith("pt")) return "pt-BR"; // português de Portugal também usa o pt-BR
  if (low.startsWith("zh")) return "zh-CN";
  const base = low.split(/[-_]/)[0];
  return (LANGS.find((l) => l.id === base)?.id as Lang) ?? null;
}

/** Idioma do instalador (código LCID do Windows que o NSIS grava) → idioma do app. */
export function fromLcid(lcid: string | null | undefined): Lang | null {
  const map: Record<number, Lang> = {
    1046: "pt-BR", 2070: "pt-BR", 1033: "en", 2057: "en", 1034: "es", 3082: "es", 1036: "fr", 1031: "de",
    1040: "it", 1049: "ru", 1041: "ja", 1042: "ko", 2052: "zh-CN",
  };
  const n = Number(lcid);
  return Number.isFinite(n) ? map[n] ?? null : null;
}

/**
 * Idioma na abertura do app: escolha salva (app_settings.language) → instalador → Windows → português.
 * Na primeira abertura a escolha é salva, para uma atualização (que roda o instalador de novo) nunca
 * trocar o idioma que o usuário já usa.
 */
export async function initLang(api: {
  getAppSetting: (k: string) => Promise<string | null>;
  setAppSetting: (k: string, v: string) => Promise<void>;
  installerLanguage: () => Promise<string | null>;
}) {
  let chosen: Lang = "pt-BR";
  try {
    const saved = await api.getAppSetting("language");
    if (isLang(saved)) chosen = saved;
    else {
      const fromInstaller = fromLcid(await api.installerLanguage().catch(() => null));
      chosen = fromInstaller ?? matchLocale(navigator.language) ?? "pt-BR";
      api.setAppSetting("language", chosen).catch(() => {});
    }
  } catch { chosen = matchLocale(navigator.language) ?? "pt-BR"; }
  await setLang(chosen).catch(() => setLang("pt-BR"));
}

/**
 * Marca um texto de constante (fora de componentes) como traduzível SEM traduzir agora — a tradução
 * acontece ao exibir, com `t(valor)`. Serve para a extração das chaves encontrar esses textos.
 */
export const tk = <S extends string>(s: S): S => s;

/*
 * Como adicionar um texto novo:
 *  1. Escreva-o em português dentro de t("…") (ou tk("…") em constantes fora de componentes).
 *  2. node scripts/i18n/build-keys.cjs   → acrescenta a chave no FIM de src/locales/keys.ts.
 *  3. Acrescente a tradução no FIM de cada src/locales/<idioma>.ts (mesma posição da chave).
 *  4. node scripts/i18n/check-locales.cjs → confere quantidade, {variáveis} e espaços.
 * Enquanto a tradução não existir, o texto aparece em português (nada quebra).
 */
