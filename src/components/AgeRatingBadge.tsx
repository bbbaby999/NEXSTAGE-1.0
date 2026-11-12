import { t } from "../lib/i18n";

/** Classificação etária vinda do IGDB ("PEGI: 18", "CLASS_IND: 14", "ESRB: M"). */
interface Rating { org: string; label: string; value: string; tone: string }

/** Cores das faixas (mesma convenção dos sistemas oficiais; o texto sempre acompanha a cor). */
function pegiTone(v: string) {
  return v === "3" || v === "7" ? "green" : v === "12" || v === "16" ? "amber" : v === "18" ? "red" : "neutral";
}
function classIndTone(v: string) {
  return ({ L: "green", "10": "blue", "12": "yellow", "14": "amber", "16": "red", "18": "black" } as Record<string, string>)[v] ?? "neutral";
}

/** Escolhe a classificação a exibir: PEGI → ClassInd (Brasil) → ESRB. Sem dado reconhecido: nada. */
export function pickRating(list: string[] | undefined | null): Rating | null {
  const parsed = (list ?? []).map((s) => {
    const [org, ...rest] = s.split(":");
    return { org: org.trim().toUpperCase().replace(/[\s-]/g, "_"), value: rest.join(":").trim().toUpperCase() };
  }).filter((r) => r.value);
  const pegi = parsed.find((r) => r.org === "PEGI" && /^(3|7|12|16|18)$/.test(r.value));
  if (pegi) return { org: "PEGI", label: "PEGI", value: pegi.value, tone: pegiTone(pegi.value) };
  const ci = parsed.find((r) => (r.org === "CLASS_IND" || r.org === "CLASSIND") && /^(L|10|12|14|16|18)$/.test(r.value));
  if (ci) return { org: "CLASS_IND", label: "ClassInd", value: ci.value, tone: classIndTone(ci.value) };
  const esrb = parsed.find((r) => r.org === "ESRB" && /^(EC|E|E10\+|T|M|AO|RP)$/.test(r.value));
  if (esrb) return { org: "ESRB", label: "ESRB", value: esrb.value, tone: "neutral" };
  return null;
}

/**
 * Selo discreto da classificação etária no canto inferior direito do banner do jogo.
 * Desenho próprio no estilo dos selos oficiais (quadrado com a idade e a sigla do sistema) —
 * não reproduz os logotipos registrados.
 */
export default function AgeRatingBadge({ ratings }: { ratings: string[] | undefined | null }) {
  const r = pickRating(ratings);
  if (!r) return null;
  const desc = r.value === "L" ? t("Livre para todos os públicos")
    : /^\d+$/.test(r.value) ? t("Classificação indicativa: {n} anos", { n: r.value })
    : t("Classificação indicativa: {n}", { n: r.value });
  return (
    <div className={`age-badge ${r.tone}`} role="img" aria-label={`${r.label} ${r.value} · ${desc}`} title={`${r.label} ${r.value} · ${desc} (IGDB)`}>
      <b>{r.value}</b>
      <small>{r.label}</small>
    </div>
  );
}
