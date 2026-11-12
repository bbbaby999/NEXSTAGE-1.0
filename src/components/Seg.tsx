/** Controle segmentado (opções mutuamente exclusivas) navegável por foco. */
export default function Seg<T extends string | number>({ value, opts, onChange }: {
  value: T; opts: [string, T][]; onChange: (v: T) => void;
}) {
  return (
    // grupo: ao chegar de cima/baixo, o foco entra na opção escolhida (lib/nav.ts)
    <div className="seg" data-nav-group role="radiogroup">
      {opts.map(([label, v]) => (
        <button key={label} type="button" data-focus role="radio" aria-checked={value === v}
          data-nav-entry={value === v ? "" : undefined} className={value === v ? "on" : ""} onClick={() => onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}
