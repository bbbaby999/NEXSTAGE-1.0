import { useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import type { GameInfo } from "../types";
import { t } from "../lib/i18n";

interface Tile { url: string; thumb: string; label: string }

/**
 * Remover screenshots da página do jogo (Steam e internet), uma a uma. Só o registro na galeria do
 * NEXSTAGE é apagado: a imagem continua no site de origem e não volta em novas buscas.
 * Cada remoção pede confirmação no próprio item (1º toque: "Confirmar"; 2º: remove).
 */
export default function ShotManager({ profileId, gameId, info, onChange, onClose }: {
  profileId: number; gameId: number; info: GameInfo; onChange: (i: GameInfo) => void; onClose: () => void;
}) {
  const { toast } = useSession();
  const [armed, setArmed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const steam: Tile[] = info.screenshots.map((u, i) => ({ url: u, thumb: u, label: t("Steam {v0}", { v0: i + 1 }) }));
  const web: Tile[] = (info.webScreenshots ?? []).map((w) => ({ url: w.url, thumb: w.thumb, label: w.title }));
  const removed = info.removedScreenshots?.length ?? 0;

  const remove = async (tile: Tile) => {
    if (armed !== tile.url) { setArmed(tile.url); return; }
    setBusy(true);
    try {
      const next = await api.removeGameScreenshot(profileId, gameId, tile.url);
      onChange(next);
      setArmed(null);
      toast(t("Screenshot removida da galeria do NEXSTAGE."));
      // mantém o foco na grade (o item removido some)
      requestAnimationFrame(() => document.querySelector<HTMLElement>(".shotman-tile [data-focus]")?.focus());
    } catch (e) { toast(errMsg(e), "err"); }
    setBusy(false);
  };

  const restore = async () => {
    try {
      onChange(await api.restoreRemovedScreenshots(profileId, gameId));
      toast(t("Remoções desfeitas. As imagens voltam na próxima busca (Steam ou internet)."));
    } catch (e) { toast(errMsg(e), "err"); }
  };

  const group = (title: string, tiles: Tile[]) => tiles.length > 0 && (
    <>
      <h3 className="section sub">{title} <small>({tiles.length})</small></h3>
      <div className="shotman-grid">
        {tiles.map((tile) => (
          <div key={tile.url} className={`shotman-tile ${armed === tile.url ? "armed" : ""}`}>
            <img src={tile.thumb} alt="" loading="lazy" referrerPolicy="no-referrer" />
            <Button variant={armed === tile.url ? "danger" : "default"} disabled={busy} onClick={() => remove(tile)}
              onBlur={() => setArmed((a) => (a === tile.url ? null : a))} aria-label={t("Remover {label}", { label: tile.label })}>
              {armed === tile.url ? t("Confirmar remoção") : t("Remover")}
            </Button>
          </div>
        ))}
      </div>
    </>
  );

  return (
    <Modal title={t("Remover screenshots")} onClose={onClose} wide>
      <p className="muted note">{t("Remove a imagem apenas da galeria do NEXSTAGE. Nenhum arquivo de terceiros é apagado, e a imagem removida não volta em novas buscas.")}</p>
      <div className="shotman">
        {group("Steam", steam)}
        {group("Internet", web)}
        {!steam.length && !web.length && <p className="muted">{t("Nenhuma screenshot da Steam ou da internet neste jogo.")}</p>}
      </div>
      <div className="row end">
        {removed > 0 && <Button variant="ghost" onClick={restore}>{t("Desfazer remoções ({removed})", { removed })}</Button>}
        <Button variant="primary" onClick={onClose}>{t("Concluir")}</Button>
      </div>
    </Modal>
  );
}
