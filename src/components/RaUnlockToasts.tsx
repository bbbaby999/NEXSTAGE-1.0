/**
 * Ícone de troféu usado nas telas de conquistas. (O aviso de desbloqueio é o cartão da janela de
 * notificação — ver `ShotToast` e `shot_toast::notify_achievements` —, que aparece por cima de
 * qualquer janela, inclusive do próprio NEXSTAGE.)
 */
export function TrophyIcon({ size = 20 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M8 4h8v5a4 4 0 0 1-8 0V4ZM8 6H5a3 3 0 0 0 3 4M16 6h3a3 3 0 0 1-3 4M12 13v4M8.5 20h7M10 17h4" />
    </svg>
  );
}
