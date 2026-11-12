import { t } from "../lib/i18n";
/** Símbolo geométrico N + 1: o traço direito da letra N forma o número 1 (com a bandeira no topo). */
export function LogoMark({ size = 40 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 56 56" aria-hidden="true">
      <defs>
        <linearGradient id="nx-g" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#22e4ff" />
          <stop offset="1" stopColor="#2f6bff" />
        </linearGradient>
      </defs>
      <polygon points="8,8 17,8 17,48 8,48" fill="url(#nx-g)" />
      <polygon points="8,8 18,8 40,48 30,48" fill="#2f6bff" />
      <polygon points="38,8 47,8 47,48 38,48" fill="url(#nx-g)" />
      <polygon points="38,8 28,15 38,19" fill="#22e4ff" />
    </svg>
  );
}

export function Wordmark({ size = 28 }: { size?: number }) {
  return (
    <div className="wordmark" style={{ fontSize: size }}>
      <LogoMark size={size * 1.4} />
      <span>{t("NEXSTAGE")}</span>
    </div>
  );
}
