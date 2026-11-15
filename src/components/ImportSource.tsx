import { t } from "../lib/i18n";

export type ImportSource = "steam" | "xbox";

/** Origem da importação (Biblioteca → Importar): Steam ou jogos do Xbox instalados neste PC. */
export function ImportSourceTabs({ value, onChange }: { value: ImportSource; onChange: (s: ImportSource) => void }) {
  return (
    <div className="seg import-src" role="tablist" aria-label={t("Origem da importação")} data-nav-group>
      {([["steam", "Steam"], ["xbox", "Xbox"]] as [ImportSource, string][]).map(([k, label]) => (
        <button key={k} type="button" role="tab" data-focus aria-selected={value === k} className={value === k ? "on" : ""}
          onClick={() => value !== k && onChange(k)}>{label}</button>
      ))}
    </div>
  );
}
