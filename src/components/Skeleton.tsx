import { t } from "../lib/i18n";
/** Esqueleto de carregamento (brilho por transform: roda no compositor, sem repintura). */
export default function Skeleton({ count = 10 }: { count?: number }) {
  return (
    <div className="grid" aria-busy="true" aria-label={t("Carregando")}>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="sk-card">
          <div className="sk sk-cover" />
          <div className="sk sk-line" />
          <div className="sk sk-line short" />
        </div>
      ))}
    </div>
  );
}
