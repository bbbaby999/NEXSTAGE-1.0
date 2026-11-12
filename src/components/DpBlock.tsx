import { ReactNode } from "react";

/**
 * Bloco padrão da página do jogo: título (com ações à direita) + cartão de vidro. Mesmo desenho dos
 * painéis Metacritic, Steam e RetroAchievements, para as colunas ficarem alinhadas e uniformes.
 */
export default function Block({ title, actions, children, id }: { title: string; actions?: ReactNode; children: ReactNode; id?: string }) {
  return (
    <section className="dp-block" id={id}>
      <div className="dp-block-head"><h2 className="section">{title}</h2>{actions && <div className="dp-block-actions">{actions}</div>}</div>
      <div className="dp-card">{children}</div>
    </section>
  );
}
